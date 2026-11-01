//! Block layout: stack child boxes vertically, taking the parent's content
//! width. Each child can be block-level, inline-level (which we wrap into an
//! anonymous block), flex, or grid.

use super::{LayoutBox, LayoutContext, TextLine};
use crate::html::dom::*;
use std::sync::Arc;

/// Lay out `node` as a block container. `parent_box` receives the resulting
/// `LayoutBox` (pushed to `children`).
pub fn layout_block(ctx: &mut LayoutContext, node: &NodeRef, parent_box: &mut LayoutBox) {
    let style = node.style();
    if style.display == Display::None {
        node.set_layout(LayoutRect::default());
        return;
    }
    let margin = style.margin;
    let padding = style.padding;
    let border = style.border;
    // Width: explicit width, max-width, otherwise fill available.
    let font_size = style.font_size.unwrap_or(16.0);
    let mut explicit_width = style.width.and_then(|w| match w {
        Length::Px(v) => Some(v),
        Length::Percent(p) => Some(ctx.available_width * p / 100.0),
        Length::Em(v) => Some(v * font_size),
        Length::Rem(v) => Some(v * 16.0),
        Length::Vw(v) => Some(v * ctx.viewport_width / 100.0),
        Length::Vh(v) => Some(v * ctx.viewport_height / 100.0),
        _ => None,
    });
    // Apply max-width constraint
    if let Some(max_w) = style.max_width {
        let resolved_max = match max_w {
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(ctx.available_width * p / 100.0),
            Length::Em(v) => Some(v * style.font_size.unwrap_or(16.0)),
            Length::Rem(v) => Some(v * 16.0),
            Length::Vw(v) => Some(v * ctx.viewport_width / 100.0),
            Length::Vh(v) => Some(v * ctx.viewport_height / 100.0),
            _ => None,
        };
        if let Some(max_px) = resolved_max {
            if let Some(w) = explicit_width {
                explicit_width = Some(w.min(max_px));
            } else {
                explicit_width = Some(ctx.available_width.min(max_px));
            }
        }
    }
    let mut width = explicit_width.unwrap_or(ctx.available_width).max(0.0);
    // Apply min-width constraint
    if let Some(min_w) = style.min_width {
        match min_w {
            Length::Px(v) => { if width < v { width = v; } }
            Length::Percent(p) => { let min = ctx.available_width * p / 100.0; if width < min { width = min; } }
            _ => {}
        }
    }
    // margin: auto centering — if left and right margins are both "auto" (we
    // can't express auto in EdgeSizes, so we use a sentinel: if both are 0
    // and width < available, center). Real browsers use the computed value
    // 'auto'. We approximate: if width < available_width and no explicit
    // margin, center the block.
    let mut margin_left = margin.left;
    let mut margin_right = margin.right;
    if style.margin_auto_left && style.margin_auto_right && width < ctx.available_width {
        let extra = (ctx.available_width - width) / 2.0;
        margin_left = extra;
        margin_right = extra;
    } else if style.margin_auto_left {
        // Only left is auto
        if width < ctx.available_width {
            margin_left = ctx.available_width - width - margin_right;
        }
    } else if style.margin_auto_right {
        // Only right is auto
        if width < ctx.available_width {
            margin_right = ctx.available_width - width - margin_left;
        }
    }
    // X position: relative to ctx.x, plus margins.
    let x = ctx.x + margin_left;
    let y = ctx.y + margin.top;
    let border_x = x + border.left;
    let border_y = y + border.top;
    let content_x = border_x + padding.left;
    let content_y = border_y + padding.top;
    // Compute content_width (the space available for children).
    let content_width = if explicit_width.is_some() {
        // Explicit width set via CSS
        if style.box_sizing == BoxSizing::BorderBox {
            // border-box: the specified width includes padding and border,
            // so content = width - padding - border
            (width - border.left - border.right - padding.left - padding.right)
                .max(0.0)
        } else {
            // content-box: the specified width IS the content width.
            // Do NOT subtract margins/padding/border — those are outside.
            width
        }
    } else {
        // Auto width: fill the parent's available space minus margins,
        // padding, and border.
        (ctx.available_width - margin_left - margin_right - border.left - border.right
            - padding.left - padding.right)
        .max(0.0)
    };
    // For auto-width elements, the border box width fills the available
    // space minus margins. For explicit width, use the specified value.
    let box_width = if explicit_width.is_some() {
        width
    } else {
        (ctx.available_width - margin_left - margin_right).max(0.0)
    };
    let mut box_ = LayoutBox {
        node: node.clone(),
        rect: LayoutRect {
            x,
            y,
            width: box_width,
            height: 0.0, // computed after children
        },
        content: LayoutRect {
            x: content_x,
            y: content_y,
            width: content_width,
            height: 0.0,
        },
        children: Vec::new(),
        text_lines: Vec::new(),
    };
    // Lay out children. We separate them into runs of inline content and
    // individual block children. An anonymous block wraps each inline run.
    let mut inline_run: Vec<NodeRef> = Vec::new();
    let mut block_height = 0.0f32;
    let children = node.children();
    // First pass: collect absolute-positioned children to lay out after normal flow.
    let mut absolute_children: Vec<NodeRef> = Vec::new();
    for c in children {
        let cs = c.style();
        // Skip absolute-positioned children in normal flow; handle them after.
        if cs.position == Position::Absolute || cs.position == Position::Fixed {
            absolute_children.push(c.clone());
            continue;
        }
        match cs.display {
            Display::None => {
                c.set_layout(LayoutRect::default());
                continue;
            }
            Display::InlineBlock => {
                // Inline-block elements participate in inline flow (they
                // sit side-by-side on the same line, like text). But they
                // get their own layout box with explicit width/height.
                // We add them to the inline run and lay them out there.
                inline_run.push(c.clone());
            }
            Display::Block | Display::ListItem | Display::Table | Display::TableRow
            | Display::TableCell => {
                if !inline_run.is_empty() {
                    let h = layout_inline_run(ctx, &inline_run, content_x, content_y + block_height, content_width, &mut box_);
                    block_height += h;
                    inline_run.clear();
                }
                let mut sub = LayoutContext {
                    viewport_width: ctx.viewport_width,
                    viewport_height: ctx.viewport_height,
                    x: content_x,
                    y: content_y + block_height,
                    available_width: content_width,
                    available_height: ctx.available_height,
                };
                super::block::layout_block(&mut sub, &c, &mut box_);
                // The child appended itself to box_.children; grab its rect.
                if let Some(last) = box_.children.last() {
                    let h = last.rect.height + last.rect.y - (content_y + block_height);
                    let h = h.max(last.rect.height);
                    block_height += h + cs.margin.top + cs.margin.bottom;
                }
            }
            Display::Flex | Display::InlineFlex => {
                if !inline_run.is_empty() {
                    let h = layout_inline_run(ctx, &inline_run, content_x, content_y + block_height, content_width, &mut box_);
                    block_height += h;
                    inline_run.clear();
                }
                // Safety: if content_width is 0 (e.g. inside a 0-width absolute
                // container), fall back to viewport width so flex children
                // still get a usable size.
                let flex_avail = if content_width > 0.0 { content_width } else { ctx.viewport_width };
                let mut sub = LayoutContext {
                    viewport_width: ctx.viewport_width,
                    viewport_height: ctx.viewport_height,
                    x: content_x,
                    y: content_y + block_height,
                    available_width: flex_avail,
                    available_height: ctx.available_height,
                };
                super::flex::layout_flex(&mut sub, &c, &mut box_);
                if let Some(last) = box_.children.last() {
                    block_height += last.rect.height + cs.margin.top + cs.margin.bottom;
                }
            }
            Display::Grid | Display::InlineGrid => {
                if !inline_run.is_empty() {
                    let h = layout_inline_run(ctx, &inline_run, content_x, content_y + block_height, content_width, &mut box_);
                    block_height += h;
                    inline_run.clear();
                }
                let mut sub = LayoutContext {
                    viewport_width: ctx.viewport_width,
                    viewport_height: ctx.viewport_height,
                    x: content_x,
                    y: content_y + block_height,
                    available_width: content_width,
                    available_height: ctx.available_height,
                };
                super::grid::layout_grid(&mut sub, &c, &mut box_);
                if let Some(last) = box_.children.last() {
                    block_height += last.rect.height + cs.margin.top + cs.margin.bottom;
                }
            }
            _ => {
                // Inline content
                inline_run.push(c.clone());
            }
        }
    }
    if !inline_run.is_empty() {
        let h = layout_inline_run(ctx, &inline_run, content_x, content_y + block_height, content_width, &mut box_);
        block_height += h;
    }
    // Height: explicit, otherwise sum of children
    let height = match style.height {
        Some(Length::Px(v)) => v.max(block_height),
        Some(Length::Percent(p)) => (ctx.available_height * p / 100.0).max(block_height),
        Some(Length::Em(v)) => (v * font_size).max(block_height),
        Some(Length::Rem(v)) => (v * 16.0).max(block_height),
        Some(Length::Vw(v)) => (v * ctx.viewport_width / 100.0).max(block_height),
        Some(Length::Vh(v)) => (v * ctx.viewport_height / 100.0).max(block_height),
        _ => block_height,
    };
    box_.rect.height = height + border.top + border.bottom + padding.top + padding.bottom;
    box_.content.height = height;
    node.set_layout(box_.rect);
    let box_rect = box_.rect;
    parent_box.children.push(box_);
    
    // Now lay out absolute-positioned children. If this element is
    // positioned (relative/absolute/fixed), it's the containing block.
    // Otherwise, the absolute child will be handled by an ancestor or the
    // initial containing block (viewport). For simplicity, we always position
    // absolute children here using the viewport as fallback.
    let is_positioned = style.position != Position::Static;
    let containing_x = if is_positioned { box_rect.x } else { 0.0 };
    let containing_y = if is_positioned { box_rect.y } else { 0.0 };
    let containing_w = if is_positioned { box_rect.width } else { ctx.viewport_width };
    let containing_h = if is_positioned { box_rect.height } else { ctx.viewport_height };
    for abs_child in absolute_children {
        let cs = abs_child.style();
        // Default position: top-left of containing block
        let mut abs_x = containing_x;
        let mut abs_y = containing_y;
        // top/bottom override Y
        if let Some(t) = cs.top {
            match t {
                Length::Px(v) => abs_y = containing_y + v,
                Length::Percent(p) => abs_y = containing_y + containing_h * p / 100.0,
                _ => {}
            }
        }
        if let Some(b) = cs.bottom {
            match b {
                Length::Px(v) => abs_y = containing_y + containing_h - v,
                Length::Percent(p) => abs_y = containing_y + containing_h * (1.0 - p / 100.0),
                _ => {}
            }
        }
        // left/right override X
        let mut has_left = false;
        let mut has_right = false;
        if let Some(l) = cs.left {
            has_left = true;
            match l {
                Length::Px(v) => abs_x = containing_x + v,
                Length::Percent(p) => abs_x = containing_x + containing_w * p / 100.0,
                _ => {}
            }
        }
        if let Some(r) = cs.right {
            has_right = true;
            match r {
                Length::Px(v) => {
                    if !has_left {
                        abs_x = containing_x + containing_w - v;
                    }
                    // If both left and right, width = containing_w - left - right
                    // (handled by setting width on the child style)
                }
                Length::Percent(p) => {
                    if !has_left {
                        abs_x = containing_x + containing_w * (1.0 - p / 100.0);
                    }
                }
                _ => {}
            }
        }
        let _ = (has_left, has_right);
        // Lay out the absolute child
        let mut sub = LayoutContext {
            viewport_width: ctx.viewport_width,
            viewport_height: ctx.viewport_height,
            x: abs_x,
            y: abs_y,
            available_width: containing_w,
            available_height: ctx.viewport_height,
        };
        let mut tmp_parent = LayoutBox {
            node: node.clone(),
            rect: LayoutRect::default(),
            content: LayoutRect::default(),
            children: Vec::new(),
            text_lines: Vec::new(),
        };
        super::block::layout_block(&mut sub, &abs_child, &mut tmp_parent);
        // Override the position with absolute coords
        if let Some(child_box) = tmp_parent.children.last() {
            let mut new_rect = child_box.rect;
            new_rect.x = abs_x;
            new_rect.y = abs_y;
            abs_child.set_layout(new_rect);
        }
    }
}

/// Lay out a run of inline content as anonymous block. Returns the height
/// of the laid-out block.
pub fn layout_inline_run(
    _ctx: &mut LayoutContext,
    nodes: &[NodeRef],
    x: f32,
    y: f32,
    width: f32,
    parent_box: &mut LayoutBox,
) -> f32 {
    let mut state = InlineState {
        x,
        y,
        width,
        cursor_x: x,
        cursor_y: y,
        max_line_height: 16.0,
        lines: Vec::new(),
        current_line: TextLine {
            x,
            y,
            width: 0.0,
            height: 16.0,
            runs: Vec::new(),
        },
        anonymous_height: 0.0,
        viewport_width: _ctx.viewport_width,
        viewport_height: _ctx.viewport_height,
    };
    let parent_style = parent_box.node.style();
    for n in nodes {
        walk_inline(n, &parent_style, &mut state);
    }
    if !state.current_line.runs.is_empty() {
        state.current_line.width = state.cursor_x - x;
        state.current_line.height = state.max_line_height;
        state.anonymous_height += state.max_line_height;
        state.lines.push(state.current_line.clone());
    }
    parent_box.text_lines.extend(state.lines);
    state.anonymous_height
}

struct InlineState {
    x: f32,
    y: f32,
    width: f32,
    cursor_x: f32,
    cursor_y: f32,
    max_line_height: f32,
    lines: Vec<TextLine>,
    current_line: TextLine,
    anonymous_height: f32,
    viewport_width: f32,
    viewport_height: f32,
}

fn walk_inline(n: &NodeRef, _parent_style: &ComputedStyle, state: &mut InlineState) {
    let style = n.style();
    let font_size = style.font_size.unwrap_or(16.0);
    let line_h = style.line_height.unwrap_or(1.2) * font_size;
    let color = style.color.unwrap_or(0xFF000000);
    let weight = style.font_weight.unwrap_or(400);
    let italic = matches!(style.font_style, Some(FontStyle::Italic) | Some(FontStyle::Oblique));
    let underline = matches!(n.tag_name().map(|t| t.to_ascii_lowercase()).as_deref(), Some("a") | Some("u"));
    let font_family = style.font_family.clone().map(|s| s.to_string()).unwrap_or_else(|| "serif".to_string());
    let kind = n.kind();
    match &kind {
        NodeKind::Text(t) => {
            let text = t.to_string();
            let words = text.split_whitespace().collect::<Vec<_>>();
            if words.is_empty() {
                return;
            }
            // Track the bounding box of all runs for this text node,
            // so we can set its layout position for the renderer.
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = 0.0f32;
            let mut max_y = 0.0f32;
            for word in words {
                let word_w = (word.len() as f32) * font_size * 0.5;
                if state.cursor_x + word_w > state.x + state.width && state.cursor_x > state.x {
                    // Wrap
                    state.current_line.width = state.cursor_x - state.x;
                    state.current_line.height = state.max_line_height;
                    state.lines.push(state.current_line.clone());
                    state.anonymous_height += state.max_line_height;
                    state.cursor_y += state.max_line_height;
                    state.cursor_x = state.x;
                    state.max_line_height = line_h;
                    state.current_line = TextLine {
                        x: state.x,
                        y: state.cursor_y,
                        width: 0.0,
                        height: state.max_line_height,
                        runs: Vec::new(),
                    };
                }
                let run_x = state.cursor_x;
                let run_y = state.cursor_y;
                if run_x < min_x { min_x = run_x; }
                if run_y < min_y { min_y = run_y; }
                if run_x + word_w > max_x { max_x = run_x + word_w; }
                if run_y + line_h > max_y { max_y = run_y + line_h; }
                state.current_line.runs.push(crate::layout::TextRun {
                    text: word.to_string(),
                    x: run_x,
                    y: run_y,
                    width: word_w,
                    height: line_h,
                    font_size,
                    font_weight: weight,
                    color,
                    font_family: font_family.clone(),
                    italic,
                    underline,
                    node_id: n.id,
                });
                state.cursor_x += word_w + font_size * 0.3;
                state.max_line_height = state.max_line_height.max(line_h);
            }
            // Set the text node's layout to its bounding box so the
            // renderer can paint it at the correct position.
            if min_x < f32::MAX {
                n.set_layout(LayoutRect {
                    x: min_x,
                    y: min_y,
                    width: (max_x - min_x).max(0.0),
                    height: (max_y - min_y).max(0.0),
                });
            }
        }
        NodeKind::Element(e) => {
            if e.tag.eq_ignore_ascii_case("br") {
                state.current_line.width = state.cursor_x - state.x;
                state.current_line.height = state.max_line_height;
                state.lines.push(state.current_line.clone());
                state.anonymous_height += state.max_line_height;
                state.cursor_y += state.max_line_height;
                state.cursor_x = state.x;
                state.max_line_height = line_h;
                state.current_line = TextLine {
                    x: state.x,
                    y: state.cursor_y,
                    width: 0.0,
                    height: state.max_line_height,
                    runs: Vec::new(),
                };
                return;
            }
            // For inline-block elements (like inputs, buttons, inline-block divs),
            // lay out their children as a block first, then position the
            // resulting box inline.
            if style.display == Display::InlineBlock {
                // Determine the element's width
                let elem_width = style.width.and_then(|w| match w {
                    Length::Px(v) => Some(v),
                    Length::Percent(p) => Some(state.width * p / 100.0),
                    _ => None,
                });
                // Determine height
                let elem_height = style.height.and_then(|h| match h {
                    Length::Px(v) => Some(v),
                    _ => None,
                });
                // Lay out this element's children as a block to get the
                // actual content size. Use a temporary context.
                let avail_w = elem_width.unwrap_or(state.width);
                let mut sub_ctx = LayoutContext {
                    viewport_width: state.viewport_width,
                    viewport_height: state.viewport_height,
                    x: 0.0,
                    y: 0.0,
                    available_width: avail_w,
                    available_height: f32::INFINITY,
                };
                let mut sub_box = LayoutBox {
                    node: n.clone(),
                    rect: LayoutRect::default(),
                    content: LayoutRect::default(),
                    children: Vec::new(),
                    text_lines: Vec::new(),
                };
                // Save the original layout so we can restore it after
                let orig_layout = n.layout();
                super::block::layout_block(&mut sub_ctx, n, &mut sub_box);
                // Get the laid-out size
                let laid_out = n.layout().unwrap_or_default();
                let final_w = elem_width.unwrap_or(laid_out.width.max(100.0));
                let final_h = elem_height.unwrap_or(laid_out.height.max(24.0));
                // Check if we need to wrap
                if state.cursor_x + final_w > state.x + state.width && state.cursor_x > state.x {
                    state.current_line.width = state.cursor_x - state.x;
                    state.current_line.height = state.max_line_height;
                    state.lines.push(state.current_line.clone());
                    state.anonymous_height += state.max_line_height;
                    state.cursor_y += state.max_line_height;
                    state.cursor_x = state.x;
                    state.max_line_height = line_h;
                    state.current_line = TextLine {
                        x: state.x,
                        y: state.cursor_y,
                        width: 0.0,
                        height: state.max_line_height,
                        runs: Vec::new(),
                    };
                }
                // Set this element's layout position (translate all children)
                let elem_x = state.cursor_x;
                let elem_y = state.cursor_y;
                n.set_layout(LayoutRect {
                    x: elem_x,
                    y: elem_y,
                    width: final_w,
                    height: final_h,
                });
                // Translate children layouts to be relative to the new position
                translate_subtree(n, elem_x, elem_y);
                // Advance cursor
                state.cursor_x += final_w + 4.0;
                state.max_line_height = state.max_line_height.max(final_h);
                let _ = orig_layout;
                return;
            }
            // For other inline elements, recurse into children
            for c in n.children() {
                walk_inline(&c, &style, state);
            }
        }
        _ => {}
    }
}

/// Translate a node and all its descendants' layouts by (dx, dy).
/// Used to reposition inline-block subtrees after inline positioning.
fn translate_subtree(node: &NodeRef, dx: f32, dy: f32) {
    if let Some(rect) = node.layout() {
        node.set_layout(LayoutRect {
            x: rect.x + dx,
            y: rect.y + dy,
            width: rect.width,
            height: rect.height,
        });
    }
    for c in node.children() {
        translate_subtree(&c, dx, dy);
    }
}
