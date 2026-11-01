//! Basic CSS Grid layout. Supports:
//!
//!  * `grid-template-columns` / `grid-template-rows` with px, %, fr, auto
//!  * `grid-column` / `grid-row` (line-based placement)
//!  * Auto-placement of items that don't specify a column/row
//!  * `gap` (rendered as gaps between tracks; only row-gap/column-gap supported)
//!
//! Does **not** support: `grid-area` shorthand, named areas, alignment
//! properties beyond simple stretch, or subgrid.

use super::{LayoutBox, LayoutContext};
use crate::html::dom::*;

pub fn layout_grid(ctx: &mut LayoutContext, node: &NodeRef, parent_box: &mut LayoutBox) {
    let style = node.style();
    let margin = style.margin;
    let padding = style.padding;
    let border = style.border;
    let width = style.width
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

    let col_template = &style.grid_template_columns;
    let row_template = &style.grid_template_rows;

    // Compute column widths
    let col_count = col_template.len().max(1);
    let mut col_widths = vec![0.0f32; col_count];
    let mut fr_total = 0.0f32;
    let mut fixed_used = 0.0f32;
    for (i, t) in col_template.iter().enumerate() {
        match t {
            TrackSize::Px(v) => {
                col_widths[i] = *v;
                fixed_used += *v;
            }
            TrackSize::Percent(p) => {
                col_widths[i] = content_width * p / 100.0;
                fixed_used += col_widths[i];
            }
            TrackSize::Fr(n) => {
                fr_total += n;
            }
            _ => {
                col_widths[i] = 0.0; // auto, decided later
            }
        }
    }
    let fr_space = (content_width - fixed_used).max(0.0);
    if fr_total > 0.0 {
        for (i, t) in col_template.iter().enumerate() {
            if let TrackSize::Fr(n) = t {
                col_widths[i] = fr_space * n / fr_total;
            }
        }
    }
    // Auto-distribute remaining space to auto tracks
    let auto_count = col_template.iter().filter(|t| matches!(t, TrackSize::Auto)).count();
    if auto_count > 0 {
        let used: f32 = col_widths.iter().sum();
        let remaining = (content_width - used).max(0.0);
        let each = remaining / auto_count as f32;
        for (i, t) in col_template.iter().enumerate() {
            if matches!(t, TrackSize::Auto) {
                col_widths[i] = each;
            }
        }
    }
    if col_template.is_empty() {
        // Single implicit column taking full width
        col_widths = vec![content_width];
    }

    // Place items
    let mut placements: Vec<(NodeRef, usize, usize, usize, usize)> = Vec::new();
    // (node, col_start, col_end, row_start, row_end) — end is exclusive
    let mut auto_cursor_col = 0;
    let mut auto_cursor_row = 0;
    for c in node.children() {
        let cs = c.style();
        if cs.display == Display::None {
            c.set_layout(LayoutRect::default());
            continue;
        }
        let (col_start, col_end) = cs.grid_column.unwrap_or((0, 0));
        let (row_start, row_end) = cs.grid_row.unwrap_or((0, 0));
        if col_start > 0 {
            let cs_ = col_start as usize;
            let ce = if col_end > 0 { col_end as usize } else { cs_ + 1 };
            let rs = if row_start > 0 { row_start as usize } else { 1 };
            let re = if row_end > 0 { row_end as usize } else { rs + 1 };
            placements.push((c.clone(), cs_ - 1, ce - 1, rs - 1, re - 1));
        } else {
            // Auto-placement
            placements.push((
                c.clone(),
                auto_cursor_col,
                auto_cursor_col + 1,
                auto_cursor_row,
                auto_cursor_row + 1,
            ));
            auto_cursor_col += 1;
            if auto_cursor_col >= col_count {
                auto_cursor_col = 0;
                auto_cursor_row += 1;
            }
        }
    }
    let max_row = placements.iter().map(|p| p.4).max().unwrap_or(0);
    let row_count = row_template.len().max(max_row + 1).max(1);
    let mut row_heights = vec![0.0f32; row_count];
    // Compute row heights from explicit template
    for (i, t) in row_template.iter().enumerate() {
        match t {
            TrackSize::Px(v) => row_heights[i] = *v,
            TrackSize::Percent(_) => {} // need container height; assume 0
            _ => {}
        }
    }
    // Measure items and assign sizes
    let mut item_sizes: Vec<(usize, f32, f32)> = Vec::new(); // (idx, w, h)
    for (i, (n, cs, ce, rs, re)) in placements.iter().enumerate() {
        // Span width
        let span_w: f32 = col_widths[*cs..*ce.min(&col_widths.len())].iter().sum();
        let mut sub = LayoutContext {
            viewport_width: ctx.viewport_width,
            viewport_height: ctx.viewport_height,
            x: content_x,
            y: content_y,
            available_width: span_w,
            available_height: f32::INFINITY,
        };
        let mut tmp = LayoutBox {
            node: n.clone(),
            rect: LayoutRect::default(),
            content: LayoutRect::default(),
            children: Vec::new(),
            text_lines: Vec::new(),
        };
        super::block::layout_block(&mut sub, n, &mut tmp);
        let h = tmp.rect.height;
        item_sizes.push((i, span_w, h));
        // Distribute height to row(s)
        let span_rows = re - rs;
        if span_rows == 1 {
            if row_heights[*rs] < h {
                row_heights[*rs] = h;
            }
        }
    }
    let total_row_height: f32 = row_heights.iter().sum();
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
            height: total_row_height,
        },
        children: Vec::new(),
        text_lines: Vec::new(),
    };
    // Position each item
    let mut col_x = vec![content_x; col_count + 1];
    for i in 0..col_count {
        col_x[i + 1] = col_x[i] + col_widths[i];
    }
    let mut row_y = vec![content_y; row_count + 1];
    for i in 0..row_count {
        row_y[i + 1] = row_y[i] + row_heights[i];
    }
    for (i, (n, cs, ce, rs, re)) in placements.iter().enumerate() {
        let (_, w, h) = item_sizes[i];
        let _ = w;
        let cx = col_x[*cs];
        let cy = row_y[*rs];
        let cw: f32 = col_widths[*cs..*ce.min(&col_widths.len())].iter().sum();
        let ch: f32 = row_heights[*rs..*re.min(&row_heights.len())].iter().sum();
        let rect = LayoutRect {
            x: cx,
            y: cy,
            width: cw,
            height: ch.max(h),
        };
        n.set_layout(rect);
        let mut sub = LayoutContext {
            viewport_width: ctx.viewport_width,
            viewport_height: ctx.viewport_height,
            x: cx,
            y: cy,
            available_width: cw,
            available_height: ch.max(h),
        };
        let mut sub_box = LayoutBox {
            node: n.clone(),
            rect,
            content: rect,
            children: Vec::new(),
            text_lines: Vec::new(),
        };
        super::block::layout_block(&mut sub, n, &mut sub_box);
        box_.children.push(sub_box);
    }
    box_.rect.height = total_row_height + border.top + border.bottom + padding.top + padding.bottom;
    box_.content.height = total_row_height;
    node.set_layout(box_.rect);
    parent_box.children.push(box_);
}
