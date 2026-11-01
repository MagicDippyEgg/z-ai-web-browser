//! Browser UI chrome: tabs, address bar, navigation buttons, bookmarks, history.
//!
//! The UI runs on the main thread, owns a `minifb::Window`, and pumps events
//! into the active `Page`. Each tab owns its own `Page`. Sub-resources are
//! fetched on the main thread (synchronously) which is fine for our purposes.

use browser_core::html::dom::{LayoutRect, Node, NodeKind, NodeRef};
use browser_core::net::HttpClient;
use browser_core::Page;
use browser_render::{Canvas, Renderer};
use minifb::{Key, MouseButton, MouseMode, ScaleMode, Window, WindowOptions};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod chrome;
mod history;

pub use history::History;

const DEFAULT_WIDTH: usize = 1280;
const DEFAULT_HEIGHT: usize = 800;
const CHROME_HEIGHT: usize = 72; // tab bar (32) + toolbar (40)

/// The browser application. Owns the window, the page tabs, and the renderer.
pub struct Browser {
    pub window: Window,
    pub renderer: Renderer,
    pub http: Arc<HttpClient>,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub bookmarks: Mutex<Vec<Bookmark>>,
    pub history: Mutex<History>,
    pub width: usize,
    pub height: usize,
    /// Address bar text being typed.
    pub address_input: Mutex<String>,
    /// Cursor position in the address bar (character offset, not bytes).
    pub address_cursor: Mutex<usize>,
    /// Whether the address bar is focused.
    pub address_focused: Mutex<bool>,
    /// Node id of the focused input element on the page (0 = none).
    pub focused_input: Mutex<usize>,
    pub running: bool,
    pub last_render: Instant,
    pub last_nav_target: Option<String>,
    /// True if the left mouse button was down on the previous frame.
    /// Used to detect click edges (rising edge = a real click).
    mouse_was_down: bool,
    /// Per-tab scroll offset (pixels scrolled down). Positive = content
    /// moved up.
    pub scroll: Vec<i32>,
    /// Track whether the window was just resized so we can re-layout.
    pub need_relayout: bool,
}

pub struct Tab {
    pub page: Arc<Page>,
    /// Navigation history for this tab.
    pub history: Mutex<History>,
}

#[derive(Clone)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

impl Browser {
    pub fn new() -> Self {
        let window = Window::new(
            "Z Browser",
            DEFAULT_WIDTH,
            DEFAULT_HEIGHT,
            WindowOptions {
                resize: true,
                scale_mode: ScaleMode::Stretch,
                ..Default::default()
            },
        )
        .expect("failed to create window");
        let http = Arc::new(HttpClient::new("Z-Browser/0.1"));
        let renderer = Renderer::new();
        let bookmarks = Mutex::new(load_bookmarks());
        let history = Mutex::new(History::new());
        let mut tabs = Vec::new();
        // Initial tab loads about:blank
        let initial_page = Arc::new(Page::new(http.clone(), DEFAULT_WIDTH as f32, (DEFAULT_HEIGHT - CHROME_HEIGHT) as f32));
        tabs.push(Tab {
            page: initial_page,
            history: Mutex::new(History::new()),
        });
        Self {
            window,
            renderer,
            http,
            tabs,
            active_tab: 0,
            bookmarks,
            history,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            address_input: Mutex::new(String::new()),
            address_cursor: Mutex::new(0),
            address_focused: Mutex::new(false),
            focused_input: Mutex::new(0),
            running: true,
            last_render: Instant::now(),
            last_nav_target: None,
            mouse_was_down: false,
            scroll: vec![0],
            need_relayout: false,
        }
    }

    /// Open a URL in the active tab. Adds the URL to the tab's history.
    pub fn navigate_active(&mut self, url: &str) {
        let normalized = normalize_url(url);
        let tab = &mut self.tabs[self.active_tab];
        // Snapshot the previous URL into the tab's history.
        let current_url = tab.page.url.lock().clone();
        if current_url != normalized && current_url != "about:blank" {
            tab.history.lock().push_back(&current_url);
        }
        tab.history.lock().clear_forward();
        tab.history.lock().push_back(&normalized);
        // Reset scroll to top on navigation.
        if self.active_tab < self.scroll.len() {
            self.scroll[self.active_tab] = 0;
        }
        // Block on the navigation.
        if let Err(e) = tab.page.navigate(&normalized) {
            log::warn!("Navigation error: {}", e);
        }
        // Update global history
        let title = tab.page.title.lock().clone();
        self.history.lock().visit(&normalized, &title);
        // Update address bar
        *self.address_input.lock() = normalized.clone();
        // Move cursor to end of address
        *self.address_cursor.lock() = normalized.chars().count();
        self.last_nav_target = Some(normalized);
        // CRITICAL: mark for re-layout so the page actually gets laid out
        // and rendered. Without this, the page loads but has no layout rects,
        // so the renderer draws nothing.
        self.need_relayout = true;
    }

    /// Add a new tab and navigate to a URL.
    pub fn new_tab(&mut self, url: &str) {
        let page = Arc::new(Page::new(self.http.clone(), self.width as f32, (self.height - CHROME_HEIGHT) as f32));
        let mut history = History::new();
        history.push_back(url);
        self.tabs.push(Tab {
            page,
            history: Mutex::new(history),
        });
        self.scroll.push(0);
        self.active_tab = self.tabs.len() - 1;
        if !url.is_empty() && url != "about:blank" {
            self.navigate_active(url);
        }
    }

    /// Close the active tab.
    pub fn close_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs.remove(self.active_tab);
        self.scroll.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
    }

    /// Navigate back in the active tab's history.
    pub fn back(&mut self) {
        let url = self.tabs[self.active_tab].history.lock().back();
        if let Some(u) = url {
            let _ = self.tabs[self.active_tab].page.navigate(&u);
            *self.address_input.lock() = u.clone();
            self.last_nav_target = Some(u);
        }
    }

    /// Navigate forward in the active tab's history.
    pub fn forward(&mut self) {
        let url = self.tabs[self.active_tab].history.lock().forward();
        if let Some(u) = url {
            let _ = self.tabs[self.active_tab].page.navigate(&u);
            *self.address_input.lock() = u.clone();
            self.last_nav_target = Some(u);
        }
    }

    /// Reload the active tab.
    pub fn reload(&mut self) {
        let url = self.tabs[self.active_tab].page.url.lock().clone();
        if url != "about:blank" {
            let _ = self.tabs[self.active_tab].page.navigate(&url);
        }
    }

    /// Main event loop. Returns when the user closes the window.
    pub fn run(&mut self) {
        // Load a default page so the user sees something.
        self.navigate_active("about:blank");

        while self.running && self.window.is_open() {
            // Check for window resize.
            let (w, h) = self.window.get_size();
            if w != self.width || h != self.height {
                if w > 0 && h > 0 {
                    self.width = w;
                    self.height = h;
                    self.need_relayout = true;
                }
            }

            // Pump JS event loop
            self.tabs[self.active_tab].page.pump_js();

            // Re-layout if needed (after navigation or resize).
            if self.need_relayout {
                self.tabs[self.active_tab].page.layout();
                self.need_relayout = false;
            }

            // Render
            let mut canvas = Canvas::new(self.width, self.height);
            canvas.clear(0xFFE8E8E8);

            // Render the active page into the canvas below the chrome.
            // Apply scroll offset by translating content up.
            if let Some(doc) = self.tabs[self.active_tab].page.document() {
                let scroll = self.scroll.get(self.active_tab).copied().unwrap_or(0);
                // Temporarily translate the document's layout by the scroll offset.
                // We do this by walking the tree and adjusting each node's layout,
                // then restoring after render. (A real browser would use a
                // transform stack; we keep it simple.)
                if scroll != 0 {
                    translate_layout(&doc, 0.0, -(scroll as f32));
                }
                let base_url = self.tabs[self.active_tab].page.url.lock().clone();
                self.renderer.render_with_base(&doc, &mut canvas, &base_url);
                if scroll != 0 {
                    translate_layout(&doc, 0.0, scroll as f32);
                }
            }

            // Render chrome (tabs, address bar, buttons) on top
            chrome::render(self, &mut canvas);

            // Blit to window
            self.window
                .update_with_buffer(&canvas.pixels, canvas.width, canvas.height)
                .expect("update_with_buffer failed");

            // Handle events
            self.handle_events();

            // Throttle to ~60fps
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    fn handle_events(&mut self) {
        let mut clicked_url: Option<String> = None;
        let mut clicked_submit: Option<NodeRef> = None;
        let mut clicked_input: Option<NodeRef> = None;
        // Modifier key state
        let shift = self.window.is_key_down(Key::LeftShift) || self.window.is_key_down(Key::RightShift);
        let ctrl = self.window.is_key_down(Key::LeftCtrl) || self.window.is_key_down(Key::RightCtrl);
        
        let addr_focused = *self.address_focused.lock();
        let focused_input_id = *self.focused_input.lock();
        let page_input_focused = focused_input_id > 0;
        
        // All key input: use KeyRepeat::Yes so holding a key auto-repeats
        let pressed: Vec<Key> = self.window.get_keys_pressed(minifb::KeyRepeat::Yes);
        for k in pressed {
            // Handle special keys first
            match k {
                Key::Escape => {
                    if addr_focused {
                        *self.address_focused.lock() = false;
                    } else if page_input_focused {
                        *self.focused_input.lock() = 0;
                    } else {
                        self.running = false;
                    }
                    continue;
                }
                Key::F5 => { self.reload(); continue; }
                _ => {}
            }
            // Ctrl shortcuts
            if ctrl {
                match k {
                    Key::L => {
                        *self.address_focused.lock() = true;
                        let url = self.tabs[self.active_tab].page.url.lock().clone();
                        let len = url.chars().count();
                        *self.address_input.lock() = url;
                        *self.address_cursor.lock() = len;
                        *self.focused_input.lock() = 0;
                        continue;
                    }
                    Key::T => { self.new_tab("about:blank"); continue; }
                    Key::W => { self.close_tab(); continue; }
                    Key::R => { self.reload(); continue; }
                    _ => {}
                }
            }
            // Route input based on focus
            if addr_focused {
                // Address bar gets the key
                chrome::handle_key(self, k, shift, ctrl);
            } else if page_input_focused {
                // Page input gets the key
                self.handle_page_input_key(k, shift);
            } else {
                // Non-input shortcuts
                chrome::handle_key(self, k, shift, ctrl);
            }
        }
        
        // Page Up/Down and arrow keys for scrolling (when not typing)
        if !addr_focused && !page_input_focused {
            let keys: Vec<Key> = self.window.get_keys();
            for k in keys {
                let scroll_amount = match k {
                    Key::PageDown => (self.height - CHROME_HEIGHT) as i32,
                    Key::PageUp => -((self.height - CHROME_HEIGHT) as i32),
                    Key::Down => 40,
                    Key::Up => -40,
                    Key::Space => (self.height - CHROME_HEIGHT) as i32 / 2,
                    _ => 0,
                };
                if scroll_amount != 0 && self.active_tab < self.scroll.len() {
                    let s = &mut self.scroll[self.active_tab];
                    *s += scroll_amount;
                    if *s < 0 { *s = 0; }
                    if let Some(doc) = self.tabs[self.active_tab].page.document() {
                        let max = compute_max_scroll(&doc, (self.height - CHROME_HEIGHT) as f32);
                        if *s > max { *s = max; }
                    }
                }
            }
        }
        // Mouse — detect click EDGES
        let mouse_down = self.window.get_mouse_down(MouseButton::Left);
        let click = mouse_down && !self.mouse_was_down;
        self.mouse_was_down = mouse_down;

        if click {
            *self.focused_input.lock() = 0; // Clear input focus on any click
            if let Some(mouse) = self.window.get_mouse_pos(MouseMode::Clamp) {
                let (mx, my) = (mouse.0 as i32, mouse.1 as i32);
                if my < CHROME_HEIGHT as i32 {
                    chrome::handle_click(self, mx, my);
                } else {
                    let scroll = self.scroll.get(self.active_tab).copied().unwrap_or(0);
                    let py = my - CHROME_HEIGHT as i32 + scroll;
                    let doc = self.tabs[self.active_tab].page.document();
                    if let Some(doc) = doc {
                        if let Some(target) = find_element_at(&doc, mx as f32, py as f32) {
                            // Walk up from the clicked element to find the nearest
                            // link, input, or button (the click might be on a
                            // child element like <span> inside an <a>)
                            let mut node = Some(target.clone());
                            while let Some(n) = &node {
                                if let NodeKind::Element(e) = &n.kind() {
                                    let tag = e.tag.to_ascii_lowercase();
                                    // Check for link
                                    if tag == "a" {
                                        if let Some(href) = e.get_attr("href") {
                                            let base = self.tabs[self.active_tab].page.url.lock().clone();
                                            let abs = resolve_url(&base, &href);
                                            clicked_url = Some(abs);
                                        }
                                        break;
                                    }
                                    // Check for input/textarea (focus it)
                                    if tag == "input" || tag == "textarea" {
                                        let input_type = e.get_attr("type").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
                                        if input_type != "hidden" && input_type != "submit" && input_type != "button" {
                                            clicked_input = Some(n.clone());
                                        } else if input_type == "submit" || input_type == "button" {
                                            clicked_submit = n.parent();
                                        }
                                        break;
                                    }
                                    // Check for submit button
                                    if tag == "button" {
                                        let type_attr = e.get_attr("type").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
                                        if type_attr != "button" {
                                            clicked_submit = n.parent();
                                        }
                                        break;
                                    }
                                }
                                node = n.parent();
                            }
                            // Dispatch click event on the actual target
                            self.tabs[self.active_tab].page.dispatch_event(&target, "click");
                        }
                    }
                }
            }
        }

        // Mouse wheel scrolling.
        // minifb: scroll up (wheel away from user) → dy > 0 on X11, dy > 0 on Wayland.
        // We want: scroll up → content moves down → scroll offset decreases.
        let scroll_delta = self.window.get_scroll_wheel();
        if let Some((_dx, dy)) = scroll_delta {
            // dy > 0 means scroll up. We want scroll up to decrease the
            // scroll offset (content moves down). So delta = dy * speed.
            let delta = (dy * 40.0) as i32;
            if self.active_tab < self.scroll.len() {
                let s = &mut self.scroll[self.active_tab];
                *s -= delta;  // scroll up (dy>0) decreases offset
                if *s < 0 { *s = 0; }
                if let Some(doc) = self.tabs[self.active_tab].page.document() {
                    let max = compute_max_scroll(&doc, (self.height - CHROME_HEIGHT) as f32);
                    if *s > max { *s = max; }
                }
            }
        }

        if let Some(url) = clicked_url {
            self.navigate_active(&url);
        }
        if let Some(form) = clicked_submit {
            self.submit_form(&form);
        }
        if let Some(input) = clicked_input {
            // Focus this input element
            *self.focused_input.lock() = input.id;
            // Initialize value if not already set
            if input.input_value().is_none() {
                let initial = input.get_attribute("value").unwrap_or_default();
                input.set_input_value(Some(initial));
            }
            self.tabs[self.active_tab].page.dispatch_event(&input, "focus");
            self.need_relayout = true;
        }
    }

    /// Handle a key press when a page input element has focus.
    fn handle_page_input_key(&mut self, key: Key, shift: bool) {
        let focused_id = *self.focused_input.lock();
        if focused_id == 0 { return; }
        
        let doc = self.tabs[self.active_tab].page.document();
        let doc = match doc { Some(d) => d, None => return };
        
        // Find the focused input node by id
        let mut input_node = None;
        Node::walk_ref(&doc, &mut |n| {
            if n.id == focused_id {
                input_node = Some(n.clone());
                false
            } else {
                true
            }
        });
        let input = match input_node { Some(n) => n, None => return };
        
        match key {
            Key::Enter => {
                // Submit the form (if any)
                let form = input.parent();
                if let Some(f) = &form {
                    let is_form = f.tag_name().map(|t| t.eq_ignore_ascii_case("form")).unwrap_or(false);
                    if is_form {
                        self.submit_form(f);
                    }
                }
                *self.focused_input.lock() = 0;
            }
            Key::Backspace => {
                let mut val = input.input_value().unwrap_or_default();
                if !val.is_empty() {
                    val.pop();
                    input.set_input_value(Some(val));
                    self.need_relayout = true;
                }
            }
            Key::Escape => {
                *self.focused_input.lock() = 0;
            }
            _ => {
                // Map key to character and append to input value
                if let Some(c) = key_to_char(key, shift) {
                    let mut val = input.input_value().unwrap_or_default();
                    val.push(c);
                    input.set_input_value(Some(val));
                    self.need_relayout = true;
                }
            }
        }
    }

    fn submit_form(&mut self, form: &NodeRef) {
        let action = form.get_attribute("action").unwrap_or_default();
        let method = form.get_attribute("method").map(|s| s.to_ascii_lowercase()).unwrap_or_else(|| "get".to_string());
        let base = self.tabs[self.active_tab].page.url.lock().clone();
        let target_url = if action.is_empty() {
            base.clone()
        } else {
            resolve_url(&base, &action)
        };
        // Collect form data
        let mut params = Vec::new();
        for c in form.descendants() {
            if let NodeKind::Element(e) = &c.kind() {
                if e.tag.eq_ignore_ascii_case("input") || e.tag.eq_ignore_ascii_case("textarea") {
                    if let Some(name) = e.get_attr("name") {
                        let value = c.input_value().or_else(|| c.get_attribute("value")).unwrap_or_default();
                        params.push(format!("{}={}", url_encode(&name), url_encode(&value)));
                    }
                }
            }
        }
        let query = params.join("&");
        let final_url = if method == "get" {
            if target_url.contains('?') {
                format!("{}&{}", target_url, query)
            } else {
                format!("{}?{}", target_url, query)
            }
        } else {
            // For POST we just navigate to the action URL — we don't actually
            // POST the data, which is a known limitation. (We could but it
            // would require an HttpClient method that accepts a body.)
            target_url
        };
        self.navigate_active(&final_url);
    }
}

/// Find the topmost element at a point in the page (page-local coords).
fn find_element_at(doc: &NodeRef, x: f32, y: f32) -> Option<NodeRef> {
    let mut best: Option<NodeRef> = None;
    let mut best_area: f32 = f32::MAX;
    Node::walk_ref(doc, &mut |n| {
        let kind = n.kind();
        if !matches!(kind, NodeKind::Element(_) | NodeKind::Text(_)) {
            return true;
        }
        let layout = n.layout().unwrap_or_default();
        if layout.width <= 0.0 || layout.height <= 0.0 {
            return true;
        }
        if x >= layout.x
            && y >= layout.y
            && x < layout.x + layout.width
            && y < layout.y + layout.height
        {
            // Prefer the smallest (deepest) element that contains the point.
            let area = layout.width * layout.height;
            if area < best_area {
                best_area = area;
                best = Some(n.clone());
            }
        }
        true
    });
    if let Some(b) = &best {
        if matches!(b.kind(), NodeKind::Text(_)) {
            if let Some(p) = b.parent() {
                return Some(p);
            }
        }
    }
    best
}

/// Translate every node's layout rect by (dx, dy). Used to apply scroll
/// offset before rendering and undo it afterwards. We mutate the cached
/// layout in place — this is a bit hacky but avoids a full layout redraw.
fn translate_layout(node: &NodeRef, dx: f32, dy: f32) {
    if let Some(rect) = node.layout() {
        node.set_layout(LayoutRect {
            x: rect.x + dx,
            y: rect.y + dy,
            width: rect.width,
            height: rect.height,
        });
    }
    for c in node.children() {
        translate_layout(&c, dx, dy);
    }
}

/// Compute the maximum scroll offset (so the bottom of the content is visible
/// at the bottom of the viewport). Returns 0 if the content fits.
fn compute_max_scroll(doc: &NodeRef, viewport_height: f32) -> i32 {
    let mut max_bottom = 0.0f32;
    Node::walk_ref(doc, &mut |n| {
        if let Some(rect) = n.layout() {
            let bottom = rect.y + rect.height;
            if bottom > max_bottom {
                max_bottom = bottom;
            }
        }
        true
    });
    let max = (max_bottom - viewport_height).max(0.0) as i32;
    max
}

fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") {
        return href.to_string();
    }
    match url::Url::parse(base) {
        Ok(b) => match b.join(href) {
            Ok(u) => u.to_string(),
            Err(_) => href.to_string(),
        },
        Err(_) => href.to_string(),
    }
}

fn url_encode(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

fn normalize_url(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return "about:blank".to_string();
    }
    if input.starts_with("about:") || input.starts_with("http://") || input.starts_with("https://") || input.starts_with("file://") || input.starts_with("data:") {
        return input.to_string();
    }
    // If it looks like a domain (has a dot, no spaces), prepend https://
    if input.contains('.') && !input.contains(' ') {
        return format!("https://{}", input);
    }
    // Otherwise treat as a search query (use DuckDuckGo as the search engine)
    let encoded = url_encode(input);
    format!("https://duckduckgo.com/?q={}", encoded)
}

fn load_bookmarks() -> Vec<Bookmark> {
    // Default bookmarks
    vec![
        Bookmark {
            title: "Example Domain".to_string(),
            url: "https://example.com".to_string(),
        },
        Bookmark {
            title: "Wikipedia".to_string(),
            url: "https://en.wikipedia.org".to_string(),
        },
    ]
}

/// Map a minifb Key to a character, applying Shift for the US keyboard layout.
/// Returns None for non-character keys.
pub fn key_to_char(k: Key, shift: bool) -> Option<char> {
    // Letters
    let c = match k {
        Key::A => 'a',
        Key::B => 'b',
        Key::C => 'c',
        Key::D => 'd',
        Key::E => 'e',
        Key::F => 'f',
        Key::G => 'g',
        Key::H => 'h',
        Key::I => 'i',
        Key::J => 'j',
        Key::K => 'k',
        Key::L => 'l',
        Key::M => 'm',
        Key::N => 'n',
        Key::O => 'o',
        Key::P => 'p',
        Key::Q => 'q',
        Key::R => 'r',
        Key::S => 's',
        Key::T => 't',
        Key::U => 'u',
        Key::V => 'v',
        Key::W => 'w',
        Key::X => 'x',
        Key::Y => 'y',
        Key::Z => 'z',
        // Digits (top row)
        Key::Key0 => if shift { ')' } else { '0' },
        Key::Key1 => if shift { '!' } else { '1' },
        Key::Key2 => if shift { '@' } else { '2' },
        Key::Key3 => if shift { '#' } else { '3' },
        Key::Key4 => if shift { '$' } else { '4' },
        Key::Key5 => if shift { '%' } else { '5' },
        Key::Key6 => if shift { '^' } else { '6' },
        Key::Key7 => if shift { '&' } else { '7' },
        Key::Key8 => if shift { '*' } else { '8' },
        Key::Key9 => if shift { '(' } else { '9' },
        // Punctuation
        Key::Minus => if shift { '_' } else { '-' },
        Key::Equal => if shift { '+' } else { '=' },
        Key::LeftBracket => if shift { '{' } else { '[' },
        Key::RightBracket => if shift { '}' } else { ']' },
        Key::Backslash => if shift { '|' } else { '\\' },
        Key::Semicolon => if shift { ':' } else { ';' },
        Key::Apostrophe => if shift { '"' } else { '\'' },
        Key::Backquote => if shift { '~' } else { '`' },
        Key::Comma => if shift { '<' } else { ',' },
        Key::Period => if shift { '>' } else { '.' },
        Key::Slash => if shift { '?' } else { '/' },
        Key::Space => ' ',
        // Numpad digits (treat as regular digits)
        Key::NumPad0 => '0',
        Key::NumPad1 => '1',
        Key::NumPad2 => '2',
        Key::NumPad3 => '3',
        Key::NumPad4 => '4',
        Key::NumPad5 => '5',
        Key::NumPad6 => '6',
        Key::NumPad7 => '7',
        Key::NumPad8 => '8',
        Key::NumPad9 => '9',
        Key::NumPadPlus => '+',
        Key::NumPadMinus => '-',
        Key::NumPadAsterisk => '*',
        Key::NumPadSlash => '/',
        Key::NumPadDot => '.',
        _ => return None,
    };
    // Apply shift to letters
    if shift && c.is_ascii_alphabetic() {
        Some(c.to_ascii_uppercase())
    } else {
        Some(c)
    }
}
