//! Flexbox layout. Implements the 2018 CSS Flexbox spec to a useful subset:
//!
//!  * `flex-direction: row | row-reverse | column | column-reverse`
//!  * `justify-content`: flex-start, flex-end, center, space-between, space-around, space-evenly
//!  * `align-items`: stretch, flex-start, flex-end, center
//!  * `flex-wrap: nowrap | wrap`
//!  * `flex-grow`, `flex-shrink`, `flex-basis`
//!
//! We do **not** implement `align-content`, `order`, or `gap`.

use super::{LayoutBox, LayoutContext};
use crate::html::dom::*;

pub fn layout_flex(ctx: &mut LayoutContext, node: &NodeRef, parent_box: &mut LayoutBox) {
    let style = node.style();
    let direction = style.flex_direction.unwrap_or_default();
    let justify = style.justify_content.unwrap_or_default();
    let align = style.align_items.unwrap_or_default();
    let wrap = style.flex_wrap.unwrap_or_default();
    let margin = style.margin;
    let padding = style.padding;
    let border = style.border;
    let width = style
        .width
        .and_then(|w| match w {
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(ctx.available_width * p / 100.0),
            _ => None,
        })
        .unwrap_or(ctx.available_width)
        .max(0.0);
    let x = ctx.x + margin.left;
    let y = ctx.y + margin.top;
    let content_x = x + border.left + padding.left;
    let content_y = y + border.top + padding.top;
    let content_width = (width - margin.left - margin.right - border.left - border.right
        - padding.left - padding.right)
        .max(0.0);
    let content_height = match style.height {
        Some(Length::Px(v)) => v,
        Some(Length::Percent(p)) => ctx.available_height * p / 100.0,
        _ => 0.0, // determined by content
    };
    let mut box_ = LayoutBox {
        node: node.clone(),
        rect: LayoutRect {
            x,
            y,
            width,
            height: 0.0,
        },
        content: LayoutRect {
            x: content_x,
            y: content_y,
            width: content_width,
            height: content_height,
        },
        children: Vec::new(),
        text_lines: Vec::new(),
    };

    // Collect children that participate in flex layout.
    let mut items: Vec<FlexItem> = Vec::new();
    for c in node.children() {
        let cs = c.style();
        if cs.display == Display::None {
            c.set_layout(LayoutRect::default());
            continue;
        }
        // Determine the item's base size.
        let (base_main, base_cross) = match direction {
            FlexDirection::Row | FlexDirection::RowReverse => {
                let basis = match cs.flex_basis.unwrap_or(Length::Auto) {
                    Length::Px(v) => Some(v),
                    Length::Percent(p) => Some(content_width * p / 100.0),
                    Length::Auto => None,
                    _ => None,
                };
                let w = basis.or_else(|| match cs.width {
                    Some(Length::Px(v)) => Some(v),
                    Some(Length::Percent(p)) => Some(content_width * p / 100.0),
                    _ => None,
                }).unwrap_or(0.0);
                let h = match cs.height {
                    Some(Length::Px(v)) => v,
                    _ => 0.0,
                };
                (w, h)
            }
            FlexDirection::Column | FlexDirection::ColumnReverse => {
                let basis = match cs.flex_basis.unwrap_or(Length::Auto) {
                    Length::Px(v) => Some(v),
                    Length::Percent(p) => Some(content_height * p / 100.0),
                    Length::Auto => None,
                    _ => None,
                };
                let h = basis.or_else(|| match cs.height {
                    Some(Length::Px(v)) => Some(v),
                    Some(Length::Percent(p)) => Some(content_height * p / 100.0),
                    _ => None,
                }).unwrap_or(0.0);
                let w = match cs.width {
                    Some(Length::Px(v)) => v,
                    _ => 0.0,
                };
                (h, w)
            }
        };
        // Measure flex item content size if needed (block layout to get height).
        let content_main = if matches!(direction, FlexDirection::Row | FlexDirection::RowReverse) {
            // For row, content main = base. Real browsers measure intrinsic
            // content size; we approximate with base.
            base_main
        } else {
            // For column, measure child's content height by laying it out as
            // a block.
            let mut sub = LayoutContext {
                viewport_width: ctx.viewport_width,
                viewport_height: ctx.viewport_height,
                x: content_x,
                y: content_y,
                available_width: content_width,
                available_height: f32::INFINITY,
            };
            let mut tmp = LayoutBox {
                node: node.clone(),
                rect: LayoutRect::default(),
                content: LayoutRect::default(),
                children: Vec::new(),
                text_lines: Vec::new(),
            };
            super::block::layout_block(&mut sub, &c, &mut tmp);
            tmp.rect.height
        };
        let main_size = if base_main > 0.0 { base_main } else { content_main };
        let cross_size = base_cross;
        let grow = cs.flex_grow.unwrap_or(0.0);
        let shrink = cs.flex_shrink.unwrap_or(1.0);
        let margin_main = match direction {
            FlexDirection::Row | FlexDirection::RowReverse => cs.margin.left + cs.margin.right,
            FlexDirection::Column | FlexDirection::ColumnReverse => cs.margin.top + cs.margin.bottom,
        };
        let margin_cross = match direction {
            FlexDirection::Row | FlexDirection::RowReverse => cs.margin.top + cs.margin.bottom,
            FlexDirection::Column | FlexDirection::ColumnReverse => cs.margin.left + cs.margin.right,
        };
        items.push(FlexItem {
            node: c.clone(),
            base_main: main_size,
            base_cross: cross_size,
            content_main,
            grow,
            shrink,
            margin_main,
            margin_cross,
            final_main: 0.0,
            final_cross: 0.0,
        });
    }
    // Determine the available main size.
    let main_size = match direction {
        FlexDirection::Row | FlexDirection::RowReverse => content_width,
        FlexDirection::Column | FlexDirection::ColumnReverse => content_height,
    };
    let cross_size = match direction {
        FlexDirection::Row | FlexDirection::RowReverse => content_height,
        FlexDirection::Column | FlexDirection::ColumnReverse => content_width,
    };
    let total_margin: f32 = items.iter().map(|i| i.margin_main).sum();
    let total_base: f32 = items.iter().map(|i| i.base_main).sum();
    let free_space = main_size - total_base - total_margin;
    let positive_free = free_space > 0.0;
    // Distribute free space via flex-grow / flex-shrink
    if positive_free {
        let total_grow: f32 = items.iter().map(|i| i.grow).sum();
        if total_grow > 0.0 {
            for it in items.iter_mut() {
                it.final_main = it.base_main + (free_space * it.grow / total_grow).max(0.0);
            }
        } else {
            for it in items.iter_mut() {
                it.final_main = it.base_main;
            }
        }
    } else {
        let total_shrink: f32 = items.iter().map(|i| i.shrink * i.base_main).sum();
        if total_shrink > 0.0 {
            for it in items.iter_mut() {
                let factor = it.shrink * it.base_main / total_shrink;
                it.final_main = (it.base_main + free_space * factor).max(0.0);
            }
        } else {
            for it in items.iter_mut() {
                it.final_main = it.base_main;
            }
        }
    }
    // Cross sizing: stretch / fit / etc.
    let mut max_cross = 0.0f32;
    for it in items.iter_mut() {
        if matches!(align, AlignItems::Stretch) && it.base_cross == 0.0 && cross_size > 0.0 {
            it.final_cross = cross_size - it.margin_cross;
        } else {
            it.final_cross = it.base_cross;
        }
        if it.final_cross > max_cross {
            max_cross = it.final_cross;
        }
    }
    // Wrap support: simple — if doesn't fit and wrap is on, wrap into multiple lines.
    let lines = if matches!(wrap, FlexWrap::Wrap | FlexWrap::WrapReverse) {
        wrap_items(&items, main_size, direction)
    } else {
        vec![items.iter().cloned().collect::<Vec<_>>()]
    };
    // Each line gets its own positioning pass.
    let mut cursor_main = 0.0f32;
    let mut cursor_cross = 0.0f32;
    let line_count = lines.len();
    let total_line_cross: f32 = lines.iter().map(|l| l.iter().map(|i| i.final_cross + i.margin_cross).sum::<f32>()).sum();
    let line_cross_size = if line_count > 0 {
        total_line_cross / line_count as f32
    } else {
        0.0
    };
    let _ = line_cross_size;
    let mut final_height = 0.0f32;
    for line in &lines {
        let line_main_used: f32 = line.iter().map(|i| i.final_main + i.margin_main).sum();
        let line_free = main_size - line_main_used;
        let start_main = match justify {
            JustifyContent::FlexStart | JustifyContent::SpaceBetween | JustifyContent::SpaceAround | JustifyContent::SpaceEvenly => 0.0,
            JustifyContent::FlexEnd => line_free,
            JustifyContent::Center => line_free / 2.0,
        };
        let (gap_before, gap_between) = match justify {
            JustifyContent::SpaceBetween => (0.0, if line.len() > 1 { line_free / (line.len() - 1) as f32 } else { 0.0 }),
            JustifyContent::SpaceAround => (if line.len() > 0 { line_free / line.len() as f32 / 2.0 } else { 0.0 }, if line.len() > 0 { line_free / line.len() as f32 } else { 0.0 }),
            JustifyContent::SpaceEvenly => (if line.len() > 0 { line_free / (line.len() + 1) as f32 } else { 0.0 }, if line.len() > 0 { line_free / (line.len() + 1) as f32 } else { 0.0 }),
            _ => (0.0, 0.0),
        };
        cursor_main = start_main + gap_before;
        for it in line {
            // Compute cross position within line
            let cross_start = match align {
                AlignItems::Stretch | AlignItems::FlexStart => it.margin_cross / 2.0,
                AlignItems::Center => (line_cross_size - it.final_cross) / 2.0,
                AlignItems::FlexEnd => line_cross_size - it.final_cross - it.margin_cross / 2.0,
                AlignItems::Baseline => it.margin_cross / 2.0,
            };
            let (mx, my, mw, mh);
            match direction {
                FlexDirection::Row => {
                    mx = content_x + cursor_main + it.margin_main / 2.0;
                    my = content_y + cursor_cross + cross_start;
                    mw = it.final_main;
                    mh = it.final_cross;
                }
                FlexDirection::RowReverse => {
                    mx = content_x + (main_size - cursor_main - it.final_main) - it.margin_main / 2.0;
                    my = content_y + cursor_cross + cross_start;
                    mw = it.final_main;
                    mh = it.final_cross;
                }
                FlexDirection::Column => {
                    mx = content_x + cursor_cross + cross_start;
                    my = content_y + cursor_main + it.margin_main / 2.0;
                    mw = it.final_cross;
                    mh = it.final_main;
                }
                FlexDirection::ColumnReverse => {
                    mx = content_x + cursor_cross + cross_start;
                    my = content_y + (main_size - cursor_main - it.final_main) - it.margin_main / 2.0;
                    mw = it.final_cross;
                    mh = it.final_main;
                }
            }
            let child_box = LayoutBox {
                node: it.node.clone(),
                rect: LayoutRect { x: mx, y: my, width: mw, height: mh },
                content: LayoutRect { x: mx, y: my, width: mw, height: mh },
                children: Vec::new(),
                text_lines: Vec::new(),
            };
            it.node.set_layout(child_box.rect);
            // Lay out the child's content into its own box.
            let mut sub = LayoutContext {
                viewport_width: ctx.viewport_width,
                viewport_height: ctx.viewport_height,
                x: mx,
                y: my,
                available_width: mw,
                available_height: mh,
            };
            let mut sub_box = child_box;
            super::block::layout_block(&mut sub, &it.node, &mut sub_box);
            box_.children.push(sub_box);
            cursor_main += it.final_main + it.margin_main + gap_between;
        }
        cursor_cross += line_cross_size;
        final_height = final_height.max(cursor_cross);
    }
    let final_main_extent = match direction {
        FlexDirection::Row | FlexDirection::RowReverse => final_height.max(max_cross),
        FlexDirection::Column | FlexDirection::ColumnReverse => {
            let total_main: f32 = lines.iter().map(|l| l.iter().map(|i| i.final_main + i.margin_main).sum::<f32>()).sum();
            total_main
        }
    };
    let _ = final_main_extent;
    let final_height = if content_height > 0.0 {
        content_height
    } else {
        match direction {
            FlexDirection::Row | FlexDirection::RowReverse => max_cross,
            FlexDirection::Column | FlexDirection::ColumnReverse => {
                let total: f32 = lines.iter().map(|l| l.iter().map(|i| i.final_main + i.margin_main).sum::<f32>()).sum();
                total
            }
        }
    };
    box_.rect.height = final_height + border.top + border.bottom + padding.top + padding.bottom;
    box_.content.height = final_height;
    node.set_layout(box_.rect);
    parent_box.children.push(box_);
}

#[derive(Clone)]
struct FlexItem {
    node: NodeRef,
    base_main: f32,
    base_cross: f32,
    content_main: f32,
    grow: f32,
    shrink: f32,
    margin_main: f32,
    margin_cross: f32,
    final_main: f32,
    final_cross: f32,
}

fn wrap_items(items: &[FlexItem], main_size: f32, _direction: FlexDirection) -> Vec<Vec<FlexItem>> {
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut cursor = 0.0f32;
    for it in items.iter() {
        let needed = it.final_main + it.margin_main;
        if cursor + needed > main_size && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            cursor = 0.0;
        }
        cursor += needed;
        current.push(it.clone());
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
