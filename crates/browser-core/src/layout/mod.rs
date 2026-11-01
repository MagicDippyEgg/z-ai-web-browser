//! Layout engine: turns a styled DOM into positioned boxes.
//!
//! Layout is split into:
//!  * `block` — normal block flow (children stack vertically)
//!  * `inline` — inline formatting context (text wraps into lines)
//!  * `flex` — flexbox layout
//!  * `grid` — basic grid layout
//!
//! Layout produces a `LayoutBox` for every node that participates in layout
//! (text nodes are absorbed into their parent's inline context). Each box has
//! a `LayoutRect` (its border box) and a list of child boxes.

use crate::html::dom::*;
use std::sync::Arc;

pub mod block;
pub mod flex;
pub mod grid;
pub mod inline;

pub use block::layout_block;
pub use flex::layout_flex;
pub use grid::layout_grid;

/// Run layout on the document. Returns the document's content rect.
pub fn layout_document(document: &NodeRef, viewport_width: f32, viewport_height: f32) -> LayoutRect {
    let style = document.style();
    let mut root_box = LayoutBox {
        node: document.clone(),
        rect: LayoutRect {
            x: 0.0,
            y: 0.0,
            width: viewport_width,
            height: viewport_height,
        },
        content: LayoutRect {
            x: 0.0,
            y: 0.0,
            width: viewport_width,
            height: viewport_height,
        },
        children: Vec::new(),
        text_lines: Vec::new(),
    };
    let _ = style;
    // Find the <html> child (or first element child) and lay it out.
    let mut html_node = None;
    let mut body_node = None;
    for c in document.children() {
        if c.tag_name().map(|t| t.eq_ignore_ascii_case("html")).unwrap_or(false) {
            html_node = Some(c.clone());
            for cc in c.children() {
                if cc.tag_name().map(|t| t.eq_ignore_ascii_case("body")).unwrap_or(false) {
                    body_node = Some(cc.clone());
                    break;
                }
            }
            break;
        }
    }
    let root = body_node.or(html_node).unwrap_or_else(|| document.clone());
    let root_style = root.style();
    let padding = root_style.padding;
    let border = root_style.border;
    let margin = root_style.margin;
    let content_width = (viewport_width - margin.left - margin.right - padding.left - padding.right
        - border.left - border.right)
        .max(0.0);
    let content_x = margin.left + border.left + padding.left;
    let content_y = margin.top + border.top + padding.top;
    let mut ctx = LayoutContext {
        viewport_width,
        viewport_height,
        x: content_x,
        y: content_y,
        available_width: content_width,
        available_height: viewport_height - content_y,
    };
    layout_block(&mut ctx, &root, &mut root_box);
    // Ensure root fills viewport height so background colors cover the window
    let mut root_layout = root.layout().unwrap_or_default();
    if root_layout.height < viewport_height {
        root_layout.height = viewport_height;
        root.set_layout(root_layout);
    }
    let _ = root_box;
    // After layout, walk the tree and stamp `LayoutRect` onto each node so
    // hit-testing and JS APIs like getBoundingClientRect work.
    stamp_layout(&root);
    let rect = root.layout().unwrap_or_default();
    rect
}

fn stamp_layout(node: &NodeRef) {
    // The layout was already set on the node by `layout_block`/etc.
    // Walk children and recurse.
    let _ = node.layout();
    for c in node.children() {
        stamp_layout(&c);
    }
}

/// A layout box. We keep `text_lines` separately for inline content.
#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub node: NodeRef,
    pub rect: LayoutRect,
    /// Content box (inside padding).
    pub content: LayoutRect,
    pub children: Vec<LayoutBox>,
    pub text_lines: Vec<TextLine>,
}

#[derive(Clone, Debug, Default)]
pub struct TextLine {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub runs: Vec<TextRun>,
}

#[derive(Clone, Debug)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub font_weight: u16,
    pub color: u32,
    pub font_family: String,
    pub italic: bool,
    pub underline: bool,
    pub node_id: usize,
}

/// Per-call layout state. We pass this around so children can read the
/// available width / height set by their parent.
pub struct LayoutContext {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub x: f32,
    pub y: f32,
    pub available_width: f32,
    pub available_height: f32,
}

impl LayoutContext {
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            viewport_width,
            viewport_height,
            x: 0.0,
            y: 0.0,
            available_width: viewport_width,
            available_height: viewport_height,
        }
    }
}

/// Resolve a length value against parent size and font size.
pub fn resolve_length(
    length: Length,
    parent_size: f32,
    font_size: f32,
    root_font_size: f32,
    viewport: Option<(f32, f32)>,
) -> f32 {
    match length {
        Length::Auto => 0.0, // caller decides
        Length::Px(v) => v,
        Length::Percent(p) => parent_size * p / 100.0,
        Length::Em(v) => v * font_size,
        Length::Rem(v) => v * root_font_size,
        Length::Vw(v) => {
            let vw = viewport.map(|(w, _)| w).unwrap_or(parent_size);
            v * vw / 100.0
        }
        Length::Vh(v) => {
            let vh = viewport.map(|(_, h)| h).unwrap_or(parent_size);
            v * vh / 100.0
        }
        Length::Fr(_) => 0.0, // handled by flex/grid
    }
}

/// Whether a length is "auto".
pub fn is_auto(length: &Option<Length>) -> bool {
    matches!(length, Some(Length::Auto) | None)
}
