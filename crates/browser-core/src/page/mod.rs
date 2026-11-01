//! A loaded web page. Owns the document, the cascade, the JS runtime, and
//! the page's origin/URL.

use crate::css::Cascade;
use crate::html::dom::*;
use crate::html::parser;
use crate::js::runtime::JsRuntime;
use crate::net::http::{HttpClient, HttpError, HttpResponse};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub struct Page {
    pub url: Mutex<String>,
    pub origin: Mutex<String>,
    pub document: Mutex<Option<NodeRef>>,
    pub cascade: Mutex<Cascade>,
    pub js: Mutex<Option<JsRuntime>>,
    pub http: Arc<HttpClient>,
    pub title: Mutex<String>,
    pub favicon_url: Mutex<Option<String>>,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub loading: Mutex<bool>,
    pub error: Mutex<Option<String>>,
}

impl Page {
    pub fn new(http: Arc<HttpClient>, viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            url: Mutex::new("about:blank".to_string()),
            origin: Mutex::new("null".to_string()),
            document: Mutex::new(None),
            cascade: Mutex::new(Cascade::new()),
            js: Mutex::new(None),
            http,
            title: Mutex::new("Blank".to_string()),
            favicon_url: Mutex::new(None),
            viewport_width,
            viewport_height,
            loading: Mutex::new(false),
            error: Mutex::new(None),
        }
    }

    /// Navigate to a URL. Blocks until the page is loaded (or an error occurs).
    pub fn navigate(&self, url: &str) -> Result<(), HttpError> {
        *self.loading.lock() = true;
        *self.error.lock() = None;
        let resp = self.http.fetch(url);
        match resp {
            Ok(r) => {
                if r.status >= 400 {
                    *self.error.lock() = Some(format!("HTTP {} for {}", r.status, url));
                    *self.loading.lock() = false;
                    return Err(HttpError::Status {
                        status: r.status,
                        url: url.to_string(),
                    });
                }
                *self.url.lock() = r.final_url.clone();
                let origin = origin_of(&r.final_url);
                *self.origin.lock() = origin.clone();
                let body = r.body_string().unwrap_or_default();
                let document = parser::parse(&body);
                // Apply HTML presentational attributes (width/height on img,
                // bgcolor on body, etc.) as inline styles so the cascade picks
                // them up.
                apply_presentational_hints(&document);
                // Register document and all descendants in the JS runtime.
                // We'll do this after creating the JS runtime.
                let cascade = extract_stylesheets(&document, &self.http, &r.final_url);
                *self.cascade.lock() = cascade;
                // Build JS runtime
                let rt = JsRuntime::new(origin.clone(), r.final_url.clone());
                rt.set_document(document.clone());
                register_all_nodes(&rt, &document);
                // Now that the document is registered, re-init document.body / head /
                // documentElement so they're not null when scripts run.
                let _ = rt.evaluate("__document_init();");
                *self.js.lock() = Some(rt);
                *self.document.lock() = Some(document.clone());
                // Apply CSS cascade
                self.cascade
                    .lock()
                    .apply(&document, self.viewport_width, self.viewport_height);
                // Run scripts
                self.run_scripts(&document, &r.final_url);
                // Extract title
                let title = extract_title(&document);
                *self.title.lock() = title;
                *self.loading.lock() = false;
                Ok(())
            }
            Err(e) => {
                *self.error.lock() = Some(format!("{}", e));
                *self.loading.lock() = false;
                Err(e)
            }
        }
    }

    /// Re-layout the page (after JS mutations or window resize). Returns the
    /// final layout rect of the root.
    pub fn layout(&self) -> Option<LayoutRect> {
        let document = self.document.lock().clone()?;
        // Re-apply CSS cascade to pick up JS mutations
        self.cascade
            .lock()
            .apply(&document, self.viewport_width, self.viewport_height);
        Some(crate::layout::layout_document(
            &document,
            self.viewport_width,
            self.viewport_height,
        ))
    }

    /// Re-render the page into a pixel buffer (in the UI layer).
    pub fn document(&self) -> Option<NodeRef> {
        self.document.lock().clone()
    }

    pub fn js(&self) -> Option<JsRuntime> {
        // We can't clone JsRuntime; return a reference via thread-local.
        // For simplicity we expose a method to run code on the runtime.
        None
    }

    /// Run a script on the page's JS runtime.
    pub fn eval(&self, code: &str) -> Result<String, crate::js::runtime::JsError> {
        let js = self.js.lock();
        match js.as_ref() {
            Some(rt) => rt.evaluate(code),
            None => Err(crate::js::runtime::JsError(
                "no JS runtime".to_string(),
            )),
        }
    }

    /// Pump the JS event loop (timers, microtasks).
    pub fn pump_js(&self) {
        if let Some(rt) = self.js.lock().as_ref() {
            rt.pump();
        }
    }

    /// Fire an event on a node and run its listeners.
    pub fn dispatch_event(&self, node: &NodeRef, event_type: &str) {
        let listeners = node.event_listeners(event_type);
        let js = self.js.lock();
        if let Some(rt) = js.as_ref() {
            for l in listeners {
                let _ = rt.call_handler(l.handler_id);
            }
        }
    }

    fn run_scripts(&self, document: &NodeRef, base_url: &str) {
        let js = self.js.lock();
        let rt = match js.as_ref() {
            Some(rt) => rt,
            None => return,
        };
        // Find all <script> elements in order and execute them.
        let scripts = find_scripts(document);
        for s in scripts {
            let src = s.get_attribute("src");
            let code = if let Some(src) = src {
                // Fetch the external script
                let abs = resolve_url(base_url, &src);
                match self.http.fetch(&abs) {
                    Ok(r) => r.body_string().unwrap_or_default(),
                    Err(e) => {
                        log::warn!("Failed to load script {}: {}", abs, e);
                        continue;
                    }
                }
            } else {
                s.text_content()
            };
            let _type = s.get_attribute("type").unwrap_or_default();
            // Skip ES modules — we don't support import/export syntax.
            if _type == "module" {
                log::debug!("Skipping ES module script (not supported)");
                continue;
            }
            if _type == "application/javascript" || _type.is_empty() || _type == "text/javascript" || _type == "application/ecmascript" || _type == "text/ecmascript" {
                // Quick check: if the code contains ES module syntax (export/import),
                // skip it to avoid syntax errors that would flood the console.
                let trimmed = code.trim_start();
                if trimmed.starts_with("export ") || trimmed.starts_with("import ") || trimmed.starts_with("export*") {
                    log::debug!("Skipping script with ES module syntax");
                    continue;
                }
                if let Err(e) = rt.evaluate(&code) {
                    // Only log as debug (not warn) to reduce console noise —
                    // many sites have minor JS issues that don't affect rendering.
                    log::debug!("JS error: {}", e.0);
                }
            }
        }
        // Fire DOMContentLoaded
        let _ = rt.evaluate("if (typeof __node_dispatchEvent === 'function') __node_dispatchEvent(0, 'DOMContentLoaded');");
        let _ = rt.evaluate("if (typeof window !== 'undefined' && window.onload) window.onload();");
    }
}

fn origin_of(url: &str) -> String {
    match ParsedUrl::parse(url) {
        Ok(p) => p.origin().serialize(),
        Err(_) => "null".to_string(),
    }
}

use crate::net::url::ParsedUrl;

fn register_all_nodes(rt: &JsRuntime, node: &NodeRef) {
    rt.register_node(node);
    for c in node.children() {
        register_all_nodes(rt, &c);
    }
}

fn find_scripts(document: &NodeRef) -> Vec<NodeRef> {
    let mut out = Vec::new();
    Node::walk_ref(document, &mut |n| {
        if let NodeKind::Element(e) = &n.kind() {
            if e.tag.eq_ignore_ascii_case("script") {
                out.push(n.clone());
            }
        }
        true
    });
    out
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

fn extract_stylesheets(document: &NodeRef, http: &HttpClient, base_url: &str) -> Cascade {
    let mut cascade = Cascade::new();
    // Find all <style> and <link rel="stylesheet"> elements
    let mut styles: Vec<(String, String)> = Vec::new(); // (css, base_url_for_resolution)
    Node::walk_ref(document, &mut |n| {
        if let NodeKind::Element(e) = &n.kind() {
            if e.tag.eq_ignore_ascii_case("style") {
                let css = n.text_content();
                styles.push((css, base_url.to_string()));
            } else if e.tag.eq_ignore_ascii_case("link") {
                let rel = e.get_attr("rel").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
                if rel == "stylesheet" {
                    if let Some(href) = e.get_attr("href") {
                        let abs = resolve_url(base_url, href);
                        match http.fetch(&abs) {
                            Ok(r) => {
                                if let Ok(text) = r.body_string() {
                                    styles.push((text, abs));
                                }
                            }
                            Err(e) => log::warn!("Failed to load stylesheet {}: {}", abs, e),
                        }
                    }
                }
            }
        }
        true
    });
    for (css, _base) in styles {
        cascade.add_text(&css);
    }
    cascade
}

fn extract_title(document: &NodeRef) -> String {
    let mut title = String::new();
    Node::walk_ref(document, &mut |n| {
        if let NodeKind::Element(e) = &n.kind() {
            if e.tag.eq_ignore_ascii_case("title") {
                title = n.text_content();
                return false;
            }
        }
        true
    });
    title.trim().to_string()
}

/// Walk the DOM and convert HTML presentational attributes to inline styles.
/// This handles:
///  - `<img width="W" height="H">` → `style="width:Wpx; height:Hpx"`
///  - `<body bgcolor="...">` / `<text="...">` → background-color / color
///  - `<font color="...">` → color
///  - `<table width="...">`, `<td width="...">` → width
///  - `align="center"` → text-align: center
///
/// Only sets the style if the attribute is present and no inline style already
/// sets the property.
fn apply_presentational_hints(document: &NodeRef) {
    Node::walk_ref(document, &mut |n| {
        if let NodeKind::Element(e) = &n.kind() {
            let tag = e.tag.to_ascii_lowercase();
            let mut extra: Vec<String> = Vec::new();
            match tag.as_str() {
                "img" => {
                    if let Some(w) = e.get_attr("width") {
                        if let Ok(px) = w.parse::<f32>() {
                            extra.push(format!("width: {}px", px));
                        }
                    }
                    if let Some(h) = e.get_attr("height") {
                        if let Ok(px) = h.parse::<f32>() {
                            extra.push(format!("height: {}px", px));
                        }
                    }
                }
                "body" => {
                    if let Some(bg) = e.get_attr("bgcolor") {
                        extra.push(format!("background-color: {}", bg));
                    }
                    if let Some(c) = e.get_attr("text") {
                        extra.push(format!("color: {}", c));
                    }
                }
                "font" => {
                    if let Some(c) = e.get_attr("color") {
                        extra.push(format!("color: {}", c));
                    }
                    if let Some(s) = e.get_attr("size") {
                        // font size 1-7, map to px
                        let size_map = ["10px", "13px", "16px", "18px", "24px", "32px", "48px"];
                        if let Ok(idx) = s.parse::<usize>() {
                            if idx >= 1 && idx <= 7 {
                                extra.push(format!("font-size: {}", size_map[idx - 1]));
                            }
                        }
                    }
                }
                "table" | "td" | "th" => {
                    if let Some(w) = e.get_attr("width") {
                        if let Ok(px) = w.parse::<f32>() {
                            extra.push(format!("width: {}px", px));
                        }
                    }
                }
                _ => {}
            }
            // align attribute on any element
            if let Some(align) = e.get_attr("align") {
                let align = align.to_ascii_lowercase();
                match align.as_str() {
                    "center" => extra.push("text-align: center".to_string()),
                    "left" => extra.push("text-align: left".to_string()),
                    "right" => extra.push("text-align: right".to_string()),
                    _ => {}
                }
            }
            if !extra.is_empty() {
                // Merge with existing inline style
                let existing = n.get_attribute("style").unwrap_or_default();
                let merged = if existing.is_empty() {
                    extra.join("; ")
                } else {
                    format!("{}; {}", existing, extra.join("; "))
                };
                n.set_attribute("style", &merged);
            }
        }
        true
    });
}
