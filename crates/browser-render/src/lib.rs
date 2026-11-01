//! Renderer: paints a laid-out DOM tree into a pixel buffer (RGBA32).
//!
//! The renderer is intentionally simple — no anti-aliasing, no subpixel
//! positioning, no gradients. We draw:
//!
//!  * Filled rectangles for backgrounds and borders
//!  * Text (using `ab_glyph` for rasterization)
//!  * Solid borders (1px style only)
//!  * Form controls (inputs, buttons) drawn as flat rects
//!
//! The result is a `Canvas` — a width × height buffer of `u32` pixels in
//! `0xRRGGBBAA` format (which `minifb` consumes directly).

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use ahash::AHashMap;
use browser_core::html::dom::*;
use std::sync::Arc;

pub mod text;
pub mod image;

pub use image::ImageCache;

/// A simple pixel buffer. Pixels are stored as `0xRRGGBBAA` u32 values, which
/// matches `minifb::Buffer` format.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0xFFFFFFFF; width * height],
        }
    }

    pub fn clear(&mut self, color: u32) {
        for p in self.pixels.iter_mut() {
            *p = color;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w).max(0).min(self.width as i32) as usize;
        let y1 = (y + h).max(0).min(self.height as i32) as usize;
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.pixels[yy * self.width + xx] = color;
            }
        }
    }

    pub fn draw_rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    /// Alpha-bl a source color onto a destination pixel.
    fn blend(&mut self, x: usize, y: usize, src: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let dst = self.pixels[y * self.width + x];
        let sa = ((src >> 24) & 0xFF) as f32 / 255.0;
        if sa == 0.0 {
            return;
        }
        let sr = ((src >> 16) & 0xFF) as f32;
        let sg = ((src >> 8) & 0xFF) as f32;
        let sb = (src & 0xFF) as f32;
        let dr = ((dst >> 16) & 0xFF) as f32;
        let dg = ((dst >> 8) & 0xFF) as f32;
        let db = (dst & 0xFF) as f32;
        let out_r = (sr * sa + dr * (1.0 - sa)) as u32;
        let out_g = (sg * sa + dg * (1.0 - sa)) as u32;
        let out_b = (sb * sa + db * (1.0 - sa)) as u32;
        self.pixels[y * self.width + x] = (0xFF << 24) | (out_r << 16) | (out_g << 8) | out_b;
    }
}

/// The renderer. Holds a font cache and an image cache.
pub struct Renderer {
    /// The default serif font.
    pub serif: Arc<FontArc>,
    /// The default sans-serif font.
    pub sans: Arc<FontArc>,
    /// The default monospace font.
    pub mono: Arc<FontArc>,
    pub images: ImageCache,
}

impl Renderer {
    pub fn new() -> Self {
        // Try to load system fonts first; fall back to embedded DejaVu fonts
        // (which are public domain) so the binary works even on systems with
        // no fonts installed.
        let serif = load_font(&[
            // Common Linux paths
            "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSerif.ttf",
            "/usr/share/fonts/TTF/DejaVuSerif.ttf",
            "/usr/share/fonts/dejavu/DejaVuSerif.ttf",
            "/usr/local/share/fonts/DejaVuSerif.ttf",
            // macOS paths
            "/Library/Fonts/Times New Roman.ttf",
            "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
            "/System/Library/Fonts/NewYork.ttf",
        ])
        .unwrap_or_else(|| {
            // Embedded fallback — DejaVu Serif (public domain)
            Arc::new(
                FontArc::try_from_slice(include_bytes!("assets/serif.ttf"))
                    .expect("embedded serif font"),
            )
        });

        let sans = load_font(&[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/local/share/fonts/DejaVuSans.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ])
        .unwrap_or_else(|| {
            Arc::new(
                FontArc::try_from_slice(include_bytes!("assets/sans.ttf"))
                    .expect("embedded sans font"),
            )
        });

        let mono = load_font(&[
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
            "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
            "/usr/local/share/fonts/DejaVuSansMono.ttf",
            "/Library/Fonts/Courier New.ttf",
            "/System/Library/Fonts/Supplemental/Courier New.ttf",
            "/System/Library/Fonts/Menlo.ttc",
        ])
        .unwrap_or_else(|| {
            Arc::new(
                FontArc::try_from_slice(include_bytes!("assets/mono.ttf"))
                    .expect("embedded mono font"),
            )
        });

        Self {
            serif,
            sans,
            mono,
            images: ImageCache::new(),
        }
    }

    /// Pick the font matching a CSS family name.
    pub fn pick_font(&self, family: &str) -> Arc<FontArc> {
        let f = family.to_ascii_lowercase();
        if f.contains("mono") || f.contains("courier") || f.contains("consol") {
            self.mono.clone()
        } else if f.contains("sans") || f.contains("arial") || f.contains("helvet") {
            self.sans.clone()
        } else {
            self.serif.clone()
        }
    }

    /// Render the document into a canvas.
    pub fn render(&mut self, document: &NodeRef, canvas: &mut Canvas) {
        // Walk the tree, painting each element's background and borders, and
        // each text run.
        self.render_with_base(document, canvas, "");
    }

    /// Render with a base URL for resolving relative image srcs.
    pub fn render_with_base(&mut self, document: &NodeRef, canvas: &mut Canvas, base_url: &str) {
        paint_node(self, document, canvas, base_url);
    }
}

fn load_font(paths: &[&str]) -> Option<Arc<FontArc>> {
    for p in paths {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(f) = FontArc::try_from_vec(bytes) {
                return Some(Arc::new(f));
            }
        }
    }
    None
}

fn paint_node(r: &mut Renderer, node: &NodeRef, canvas: &mut Canvas, base_url: &str) {
    let style = node.style();
    if style.display == Display::None {
        return;
    }
    // Background
    if let Some(bg) = style.background_color {
        let layout = node.layout().unwrap_or_default();
        if layout.width > 0.0 && layout.height > 0.0 {
            canvas.fill_rect(
                layout.x as i32,
                layout.y as i32,
                layout.width as i32,
                layout.height as i32,
                bg,
            );
        }
    }
    // Border
    if let Some(bc) = style.border_color {
        let layout = node.layout().unwrap_or_default();
        let b = style.border;
        if b.top > 0.0 {
            canvas.fill_rect(layout.x as i32, layout.y as i32, layout.width as i32, b.top as i32, bc);
        }
        if b.bottom > 0.0 {
            canvas.fill_rect(
                layout.x as i32,
                (layout.y + layout.height - b.bottom) as i32,
                layout.width as i32,
                b.bottom as i32,
                bc,
            );
        }
        if b.left > 0.0 {
            canvas.fill_rect(layout.x as i32, layout.y as i32, b.left as i32, layout.height as i32, bc);
        }
        if b.right > 0.0 {
            canvas.fill_rect(
                (layout.x + layout.width - b.right) as i32,
                layout.y as i32,
                b.right as i32,
                layout.height as i32,
                bc,
            );
        }
    }
    // Paint text
    let kind = node.kind();
    if let NodeKind::Text(t) = &kind {
        let layout = node.layout().unwrap_or_default();
        let font = r.pick_font(style.font_family.as_deref().unwrap_or("serif"));
        let size = style.font_size.unwrap_or(16.0);
        let color = style.color.unwrap_or(0xFF000000);
        let weight = style.font_weight.unwrap_or(400);
        let italic = matches!(style.font_style, Some(FontStyle::Italic) | Some(FontStyle::Oblique));
        text::draw_text(
            canvas,
            &font,
            size,
            weight,
            italic,
            color,
            layout.x,
            layout.y + size,
            t,
        );
    }
    // Paint images
    if let NodeKind::Element(e) = &kind {
        if e.tag.eq_ignore_ascii_case("img") {
            let mut layout = node.layout().unwrap_or_default();
            if layout.width > 0.0 && layout.height > 0.0 {
                if let Some(src) = e.get_attr("src") {
                    let src = resolve_url(base_url, &src.to_string());
                    if let Some(img) = r.images.load(&src) {
                        draw_image(canvas, &img, layout);
                    } else {
                        draw_broken_image(canvas, layout);
                    }
                }
            } else if let Some(src) = e.get_attr("src") {
                // Layout gave us 0 size (no width/height attrs). Load the
                // image to get its intrinsic dimensions and draw it at the
                // current position. This is a fallback for images without
                // explicit dimensions.
                let src = resolve_url(base_url, &src.to_string());
                if let Some(img) = r.images.load(&src) {
                    // Use the image's natural size, capped to available width
                    let nat_w = img.width as f32;
                    let nat_h = img.height as f32;
                    // If height attr is set, scale proportionally
                    let (draw_w, draw_h) = if let Some(h_attr) = e.get_attr("height") {
                        if let Ok(h) = h_attr.parse::<f32>() {
                            let scale = h / nat_h;
                            (nat_w * scale, h)
                        } else {
                            (nat_w, nat_h)
                        }
                    } else if let Some(w_attr) = e.get_attr("width") {
                        if let Ok(w) = w_attr.parse::<f32>() {
                            let scale = w / nat_w;
                            (w, nat_h * scale)
                        } else {
                            (nat_w, nat_h)
                        }
                    } else {
                        (nat_w.min(800.0), nat_h * (nat_w.min(800.0) / nat_w))
                    };
                    layout.width = draw_w;
                    layout.height = draw_h;
                    // Update the node's layout so hit-testing works
                    node.set_layout(layout);
                    draw_image(canvas, &img, layout);
                }
            }
        }
    }
    // Paint form controls (input, button, textarea, select)
    if let NodeKind::Element(e) = &kind {
        let tag = e.tag.to_ascii_lowercase();
        if tag == "input" || tag == "button" || tag == "textarea" || tag == "select" {
            let layout = node.layout().unwrap_or_default();
            if layout.width > 0.0 && layout.height > 0.0 {
                // Draw background
                let bg = style.background_color.unwrap_or(0xFFFFFFFF);
                canvas.fill_rect(
                    layout.x as i32,
                    layout.y as i32,
                    layout.width as i32,
                    layout.height as i32,
                    bg,
                );
                // Draw border
                let border_color = style.border_color.unwrap_or(0xFF888888);
                canvas.draw_rect_outline(
                    layout.x as i32,
                    layout.y as i32,
                    layout.width as i32,
                    layout.height as i32,
                    border_color,
                );
                // Draw value text for inputs
                if tag == "input" || tag == "button" {
                    let val = node.input_value()
                        .or_else(|| e.get_attr("value").map(|s| s.to_string()))
                        .unwrap_or_default();
                    if !val.is_empty() {
                        let font = r.pick_font(style.font_family.as_deref().unwrap_or("sans"));
                        let size = style.font_size.unwrap_or(14.0);
                        let color = style.color.unwrap_or(0xFF000000);
                        text::draw_text(
                            canvas,
                            &font,
                            size,
                            style.font_weight.unwrap_or(400),
                            false,
                            color,
                            layout.x + 6.0,
                            layout.y + size + 3.0,
                            &val,
                        );
                    }
                }
            }
        }
    }
    // Recurse into children
    for c in node.children() {
        paint_node(r, &c, canvas, base_url);
    }
}

/// Resolve a possibly-relative URL against a base URL.
fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") || href.starts_with("file://") {
        return href.to_string();
    }
    if base.is_empty() {
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

/// Draw a decoded image scaled to fit the given layout rect.
fn draw_image(canvas: &mut Canvas, img: &image::DecodedImage, layout: LayoutRect) {
    let dst_w = layout.width as usize;
    let dst_h = layout.height as usize;
    let dst_x = layout.x as i32;
    let dst_y = layout.y as i32;
    if dst_w == 0 || dst_h == 0 {
        return;
    }
    for dy in 0..dst_h {
        for dx in 0..dst_w {
            let sx = (dx * img.width / dst_w) as usize;
            let sy = (dy * img.height / dst_h) as usize;
            let idx = (sy * img.width + sx) * 4;
            if idx + 3 < img.pixels.len() {
                let r_ = img.pixels[idx];
                let g_ = img.pixels[idx + 1];
                let b_ = img.pixels[idx + 2];
                let a_ = img.pixels[idx + 3];
                let pixel = ((a_ as u32) << 24)
                    | ((r_ as u32) << 16)
                    | ((g_ as u32) << 8)
                    | (b_ as u32);
                canvas.blend(
                    (dst_x + dx as i32) as usize,
                    (dst_y + dy as i32) as usize,
                    pixel,
                );
            }
        }
    }
}

/// Draw a placeholder for a broken image.
fn draw_broken_image(canvas: &mut Canvas, layout: LayoutRect) {
    canvas.fill_rect(
        layout.x as i32,
        layout.y as i32,
        layout.width as i32,
        layout.height as i32,
        0xFFCCCCCC,
    );
    canvas.draw_rect_outline(
        layout.x as i32,
        layout.y as i32,
        layout.width as i32,
        layout.height as i32,
        0xFF888888,
    );
}
