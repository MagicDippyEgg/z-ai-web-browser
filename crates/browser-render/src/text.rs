//! Text rasterization using `ab_glyph`. We rasterize each glyph into an
//! alpha bitmap and alpha-blend it onto the canvas.

use super::Canvas;
use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::sync::Arc;

/// Draw a single line of text at `(x, y)` (baseline y).
pub fn draw_text(
    canvas: &mut Canvas,
    font: &Arc<FontArc>,
    size: f32,
    weight: u16,
    italic: bool,
    color: u32,
    x: f32,
    y: f32,
    text: &str,
) {
    let _ = weight;
    let _ = italic;
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();
    let baseline = y - (ascent - size).max(0.0) / 2.0;
    let mut pen_x = x;
    let mut last_glyph: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        if let Some(prev) = last_glyph {
            pen_x += scaled.kern(prev, glyph_id);
        }
        let mut glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(pen_x, baseline));
        if let Some(outlined) = font.outline_glyph(glyph.clone()) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                let alpha = (coverage * 255.0) as u32;
                if alpha == 0 {
                    return;
                }
                let src = (alpha << 24) | (color & 0x00FFFFFF);
                canvas.blend(px as usize, py as usize, src);
            });
        }
        // Advance pen
        pen_x += scaled.h_advance(glyph_id);
        last_glyph = Some(glyph_id);
        let _ = &mut glyph;
    }
}

/// Measure the width of a string at the given font size.
pub fn measure_width(font: &Arc<FontArc>, size: f32, text: &str) -> f32 {
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let mut w = 0.0;
    let mut last: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        if let Some(p) = last {
            w += scaled.kern(p, gid);
        }
        w += scaled.h_advance(gid);
        last = Some(gid);
    }
    w
}
