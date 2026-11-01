//! Browser chrome: the top area of the window containing tabs, address bar,
//! navigation buttons, and a menu. Drawn directly to the canvas.

use super::{Browser, CHROME_HEIGHT};
use browser_render::Canvas;
use browser_render::text;
use minifb::Key;

const TAB_BAR_HEIGHT: usize = 32;
const TOOLBAR_HEIGHT: usize = 40;
const BUTTON_SIZE: usize = 28;

/// Layout regions for chrome elements. Recomputed each frame.
struct Layout {
    tabs_rect: (i32, i32, i32, i32),
    address_rect: (i32, i32, i32, i32),
    back_btn: (i32, i32, i32, i32),
    fwd_btn: (i32, i32, i32, i32),
    reload_btn: (i32, i32, i32, i32),
    new_tab_btn: (i32, i32, i32, i32),
}

fn compute_layout(b: &Browser) -> Layout {
    let w = b.width as i32;
    let tabs_rect = (0, 0, w, TAB_BAR_HEIGHT as i32);
    let toolbar_y = TAB_BAR_HEIGHT as i32;
    let mut x = 12;
    let back_btn = (x, toolbar_y + 6, BUTTON_SIZE as i32, BUTTON_SIZE as i32);
    x += BUTTON_SIZE as i32 + 4;
    let fwd_btn = (x, toolbar_y + 6, BUTTON_SIZE as i32, BUTTON_SIZE as i32);
    x += BUTTON_SIZE as i32 + 4;
    let reload_btn = (x, toolbar_y + 6, BUTTON_SIZE as i32, BUTTON_SIZE as i32);
    x += BUTTON_SIZE as i32 + 12;
    let address_rect = (
        x,
        toolbar_y + 4,
        w - x - 12,
        TOOLBAR_HEIGHT as i32 - 8,
    );
    let new_tab_btn = (w - 32, 6, 24, 20);
    Layout {
        tabs_rect,
        address_rect,
        back_btn,
        fwd_btn,
        reload_btn,
        new_tab_btn,
    }
}

// Modern color palette
const COLOR_TAB_BAR_BG: u32 = 0xFF3A3A3A;
const COLOR_TAB_INACTIVE: u32 = 0xFF525252;
const COLOR_TAB_ACTIVE: u32 = 0xFFE8E8E8;
const COLOR_TAB_TEXT_INACTIVE: u32 = 0xFFCCCCCC;
const COLOR_TAB_TEXT_ACTIVE: u32 = 0xFF1A1A1A;
const COLOR_TOOLBAR_BG: u32 = 0xFFE8E8E8;
const COLOR_BTN_BG: u32 = 0xFFD8D8D8;
const COLOR_BTN_HOVER: u32 = 0xFFC8C8C8;
const COLOR_BTN_TEXT: u32 = 0xFF333333;
const COLOR_ADDRESS_BG: u32 = 0xFFFFFFFF;
const COLOR_ADDRESS_BG_FOCUSED: u32 = 0xFFFFFFFF;
const COLOR_ADDRESS_BORDER: u32 = 0xFFBBBBBB;
const COLOR_ADDRESS_BORDER_FOCUSED: u32 = 0xFF1A73E8;
const COLOR_ADDRESS_TEXT: u32 = 0xFF202020;
const COLOR_NEW_TAB_BTN: u32 = 0xFF707070;

pub fn render(b: &Browser, canvas: &mut Canvas) {
    let layout = compute_layout(b);
    // Tab bar background (dark)
    canvas.fill_rect(
        layout.tabs_rect.0,
        layout.tabs_rect.1,
        layout.tabs_rect.2,
        layout.tabs_rect.3,
        COLOR_TAB_BAR_BG,
    );
    // Toolbar background (light)
    canvas.fill_rect(
        0,
        TAB_BAR_HEIGHT as i32,
        b.width as i32,
        TOOLBAR_HEIGHT as i32,
        COLOR_TOOLBAR_BG,
    );

    // Mouse position for hover detection
    let (mx, my) = b
        .window
        .get_mouse_pos(minifb::MouseMode::Clamp)
        .map(|p| (p.0 as i32, p.1 as i32))
        .unwrap_or((-1, -1));

    // Draw each tab
    let tab_width = 180i32;
    for (i, tab) in b.tabs.iter().enumerate() {
        let x = i as i32 * tab_width;
        let active = i == b.active_tab;
        let bg = if active { COLOR_TAB_ACTIVE } else { COLOR_TAB_INACTIVE };
        let text_color = if active { COLOR_TAB_TEXT_ACTIVE } else { COLOR_TAB_TEXT_INACTIVE };
        // Tab shape: rounded look via slightly inset rect
        canvas.fill_rect(x + 2, 3, tab_width - 4, TAB_BAR_HEIGHT as i32 - 6, bg);
        // Active tab gets a subtle accent line at top
        if active {
            canvas.fill_rect(x + 2, 3, tab_width - 4, 2, COLOR_ADDRESS_BORDER_FOCUSED);
        }
        // Title
        let title = tab.page.title.lock().clone();
        let title = if title.is_empty() {
            "New Tab".to_string()
        } else {
            title
        };
        // Truncate title to fit
        let max_chars = ((tab_width - 30) / 7) as usize;
        let display: String = title.chars().take(max_chars).collect();
        let display = if title.chars().count() > max_chars {
            format!("{}…", display)
        } else {
            display
        };
        text::draw_text(
            canvas,
            &b.renderer.sans,
            12.0,
            400,
            false,
            text_color,
            (x + 10) as f32,
            20.0,
            &display,
        );
        // Close button on tab (only show on hover or if active)
        if active || point_in_rect(mx, my, (x + tab_width - 22, 7, 16, 16)) {
            let cb_x = x + tab_width - 22;
            let cb_y = 7;
            let cb_w = 16;
            let cb_h = 16;
            canvas.fill_rect(cb_x, cb_y, cb_w, cb_h, 0x33808080);
            // Draw "x" with two diagonal lines
            for d in 0..8 {
                canvas.fill_rect(cb_x + 4 + d, cb_y + 4 + d, 1, 1, text_color);
                canvas.fill_rect(cb_x + 4 + d, cb_y + 11 - d, 1, 1, text_color);
            }
        }
    }

    // New tab button (+)
    let ntb_hover = point_in_rect(mx, my, layout.new_tab_btn);
    let ntb_color = if ntb_hover { 0xFFFFFFFF } else { 0xFFB0B0B0 };
    let cx = layout.new_tab_btn.0 + layout.new_tab_btn.2 / 2;
    let cy = layout.new_tab_btn.1 + layout.new_tab_btn.3 / 2;
    // Horizontal bar
    canvas.fill_rect(cx - 6, cy - 1, 12, 2, ntb_color);
    // Vertical bar
    canvas.fill_rect(cx - 1, cy - 6, 2, 12, ntb_color);

    // Navigation buttons
    draw_icon_button(canvas, &b.renderer, layout.back_btn, "◀", mx, my);
    draw_icon_button(canvas, &b.renderer, layout.fwd_btn, "▶", mx, my);
    draw_icon_button(canvas, &b.renderer, layout.reload_btn, "⟳", mx, my);

    // Address bar
    let focused = *b.address_focused.lock();
    let address_bg = if focused { COLOR_ADDRESS_BG_FOCUSED } else { COLOR_ADDRESS_BG };
    let address_border = if focused { COLOR_ADDRESS_BORDER_FOCUSED } else { COLOR_ADDRESS_BORDER };
    canvas.fill_rect(
        layout.address_rect.0,
        layout.address_rect.1,
        layout.address_rect.2,
        layout.address_rect.3,
        address_bg,
    );
    // Border (2px when focused for emphasis)
    let border_w = if focused { 2 } else { 1 };
    canvas.draw_rect_outline(
        layout.address_rect.0,
        layout.address_rect.1,
        layout.address_rect.2,
        layout.address_rect.3,
        address_border,
    );
    if focused {
        // Draw a second outline for 2px effect
        canvas.draw_rect_outline(
            layout.address_rect.0 + 1,
            layout.address_rect.1 + 1,
            layout.address_rect.2 - 2,
            layout.address_rect.3 - 2,
            address_border,
        );
    }
    let _ = border_w;

    let address_text = b.address_input.lock().clone();
    let cursor_pos = *b.address_cursor.lock();
    // Draw text before cursor
    let text_before: String = address_text.chars().take(cursor_pos).collect();
    let text_after: String = address_text.chars().skip(cursor_pos).collect();
    let text_x = (layout.address_rect.0 + 8) as f32;
    let text_y = (layout.address_rect.1 + 21) as f32;
    text::draw_text(
        canvas,
        &b.renderer.sans,
        14.0,
        400,
        false,
        COLOR_ADDRESS_TEXT,
        text_x,
        text_y,
        &address_text,
    );
    // Blinking cursor when focused
    if focused {
        let cursor_x = layout.address_rect.0 + 8
            + (text::measure_width(&b.renderer.sans, 14.0, &text_before) as i32);
        // Blink at ~1Hz (show for 500ms, hide for 500ms)
        let blink = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0) % 1000) < 600;
        if blink {
            canvas.fill_rect(cursor_x, layout.address_rect.1 + 6, 1, layout.address_rect.3 - 12, COLOR_ADDRESS_TEXT);
        }
    }

    // Loading / error / ready indicator (small dot in address bar right side)
    let loading = *b.tabs[b.active_tab].page.loading.lock();
    let err = b.tabs[b.active_tab].page.error.lock().clone();
    let indicator_color = if err.is_some() {
        0xFFCC0000
    } else if loading {
        0xFF1A73E8
    } else {
        0xFF00AA00
    };
    let ix = layout.address_rect.0 + layout.address_rect.2 - 14;
    let iy = layout.address_rect.1 + layout.address_rect.3 / 2 - 3;
    canvas.fill_rect(ix, iy, 6, 6, indicator_color);

    // Status bar at bottom of chrome (shows URL or error)
    if let Some(e) = err {
        let bar_y = TAB_BAR_HEIGHT as i32 + TOOLBAR_HEIGHT as i32 - 16;
        canvas.fill_rect(0, bar_y, b.width as i32, 16, 0xFFFFE0E0);
        text::draw_text(
            canvas,
            &b.renderer.sans,
            11.0,
            400,
            false,
            0xFFCC0000,
            8.0,
            bar_y as f32 + 12.0,
            &e,
        );
    }
}

fn draw_icon_button(
    canvas: &mut Canvas,
    r: &browser_render::Renderer,
    rect: (i32, i32, i32, i32),
    label: &str,
    mx: i32,
    my: i32,
) {
    let hover = point_in_rect(mx, my, rect);
    let bg = if hover { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    canvas.fill_rect(rect.0, rect.1, rect.2, rect.3, bg);
    text::draw_text(
        canvas,
        &r.sans,
        14.0,
        400,
        false,
        COLOR_BTN_TEXT,
        (rect.0 + rect.2 / 2 - 6) as f32,
        (rect.1 + rect.3 / 2 + 5) as f32,
        label,
    );
}

/// Handle a mouse click in the chrome area (y < CHROME_HEIGHT).
pub fn handle_click(b: &mut Browser, x: i32, y: i32) {
    let layout = compute_layout(b);
    let tab_width = 180i32;
    // Tab bar
    if y < TAB_BAR_HEIGHT as i32 {
        // Click on a tab?
        let tab_idx = x / tab_width;
        if tab_idx >= 0 && (tab_idx as usize) < b.tabs.len() {
            // Check if click was on the tab's close button
            let tab_x = tab_idx * tab_width;
            let close_btn = (tab_x + tab_width - 22, 7, 16, 16);
            if point_in_rect(x, y, close_btn) && b.tabs.len() > 1 {
                b.active_tab = tab_idx as usize;
                b.close_tab();
                return;
            }
            b.active_tab = tab_idx as usize;
            return;
        }
        // New tab button
        if point_in_rect(x, y, layout.new_tab_btn) {
            b.new_tab("about:blank");
            return;
        }
        return;
    }
    // Address bar click
    if point_in_rect(x, y, layout.address_rect) {
        let focused = *b.address_focused.lock();
        if !focused {
            // First click: focus and select all (pre-fill with current URL)
            *b.address_focused.lock() = true;
            let url = b.tabs[b.active_tab].page.url.lock().clone();
            *b.address_input.lock() = url.clone();
            *b.address_cursor.lock() = url.chars().count();
        } else {
            // Already focused — move cursor to click position
            let click_x = x - layout.address_rect.0 - 8;
            if click_x >= 0 {
                let text = b.address_input.lock().clone();
                let mut acc = 0.0f32;
                let mut best_pos = 0;
                for (i, c) in text.chars().enumerate() {
                    let w = text::measure_width(&b.renderer.sans, 14.0, &c.to_string());
                    if acc + w / 2.0 > click_x as f32 {
                        best_pos = i;
                        break;
                    }
                    acc += w;
                    best_pos = i + 1;
                }
                *b.address_cursor.lock() = best_pos;
            }
        }
        return;
    } else {
        *b.address_focused.lock() = false;
    }
    // Navigation buttons
    if point_in_rect(x, y, layout.back_btn) {
        b.back();
        return;
    }
    if point_in_rect(x, y, layout.fwd_btn) {
        b.forward();
        return;
    }
    if point_in_rect(x, y, layout.reload_btn) {
        b.reload();
        return;
    }
}

fn point_in_rect(x: i32, y: i32, rect: (i32, i32, i32, i32)) -> bool {
    x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
}

/// Handle keyboard input for the chrome (called from main event loop).
/// This handles both special keys (Enter, Backspace, arrows) and character
/// input (via key_to_char in lib.rs).
pub fn handle_key(b: &mut Browser, key: Key, shift: bool, ctrl: bool) {
    if *b.address_focused.lock() {
        let mut buf = b.address_input.lock();
        let mut cursor = b.address_cursor.lock();
        match key {
            Key::Enter => {
                let url = buf.clone();
                buf.clear();
                *cursor = 0;
                drop(buf);
                drop(cursor);
                *b.address_focused.lock() = false;
                b.navigate_active(&url);
            }
            Key::Backspace => {
                if *cursor > 0 {
                    // Delete the character before the cursor
                    let pos = *cursor;
                    let before: String = buf.chars().take(pos - 1).collect();
                    let after: String = buf.chars().skip(pos).collect();
                    *buf = format!("{}{}", before, after);
                    *cursor = pos - 1;
                }
            }
            Key::Delete => {
                // Delete the character after the cursor
                if *cursor < buf.chars().count() {
                    let pos = *cursor;
                    let before: String = buf.chars().take(pos).collect();
                    let after: String = buf.chars().skip(pos + 1).collect();
                    *buf = format!("{}{}", before, after);
                    // Cursor stays in place
                }
            }
            Key::Left => {
                if ctrl {
                    // Ctrl+Left: move to start of previous word
                    let mut pos = *cursor;
                    if pos > 0 { pos -= 1; }
                    while pos > 0 && buf.chars().nth(pos - 1) == Some(' ') { pos -= 1; }
                    while pos > 0 && buf.chars().nth(pos - 1) != Some(' ') { pos -= 1; }
                    *cursor = pos;
                } else if *cursor > 0 {
                    *cursor -= 1;
                }
            }
            Key::Right => {
                let len = buf.chars().count();
                if ctrl {
                    // Ctrl+Right: move to end of next word
                    let mut pos = *cursor;
                    while pos < len && buf.chars().nth(pos) == Some(' ') { pos += 1; }
                    while pos < len && buf.chars().nth(pos) != Some(' ') { pos += 1; }
                    *cursor = pos;
                } else if *cursor < len {
                    *cursor += 1;
                }
            }
            Key::Home => {
                *cursor = 0;
            }
            Key::End => {
                *cursor = buf.chars().count();
            }
            Key::Escape => {
                buf.clear();
                *cursor = 0;
                *b.address_focused.lock() = false;
            }
            _ => {
                // Try to map to a character and insert at cursor
                if let Some(c) = super::key_to_char(key, shift) {
                    let pos = *cursor;
                    let before: String = buf.chars().take(pos).collect();
                    let after: String = buf.chars().skip(pos).collect();
                    *buf = format!("{}{}{}", before, c, after);
                    *cursor = pos + 1;
                }
            }
        }
    } else {
        // Not focused on address bar — handle tab shortcuts
        if !ctrl {
            match key {
                Key::T => { b.new_tab("about:blank"); }
                Key::W => { b.close_tab(); }
                _ => {}
            }
        }
    }
    let _ = shift;
}

/// Handle a character input for the address bar. (Legacy — now handled in
/// handle_key via key_to_char. Kept for compatibility.)
pub fn handle_char(b: &mut Browser, c: char) {
    if *b.address_focused.lock() && (c.is_ascii_graphic() || c == ' ') {
        let mut buf = b.address_input.lock();
        let mut cursor = b.address_cursor.lock();
        let pos = *cursor;
        let before: String = buf.chars().take(pos).collect();
        let after: String = buf.chars().skip(pos).collect();
        *buf = format!("{}{}{}", before, c, after);
        *cursor = pos + 1;
    }
}
