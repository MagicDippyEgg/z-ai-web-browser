//! Maps parsed CSS declarations onto `ComputedStyle` fields, and resolves
//! `Length::Em` / `Length::Rem` / `Length::Percent` against the parent style
//! and viewport.

use super::parser::{Value, Unit};
use crate::html::dom::*;
use smol_str::SmolStr;
use std::sync::Arc;

pub fn apply_declaration(
    _node: &NodeRef,
    style: &mut ComputedStyle,
    decl: &super::parser::Declaration,
    vw: f32,
    vh: f32,
    parent: &ComputedStyle,
) {
    // Stash raw value for debugging / round-trip
    style.raw.insert(decl.property.clone(), SmolStr::new(decl.value.to_string_raw()));

    let prop = decl.property.as_str();
    let v = &decl.value;

    match prop {
        "display" => {
            if let Some(s) = v.as_ident() {
                style.display = match s {
                    "inline" => Display::Inline,
                    "block" => Display::Block,
                    "inline-block" => Display::InlineBlock,
                    "flex" => Display::Flex,
                    "inline-flex" => Display::InlineFlex,
                    "grid" => Display::Grid,
                    "inline-grid" => Display::InlineGrid,
                    "none" => Display::None,
                    "list-item" => Display::ListItem,
                    "table" => Display::Table,
                    "table-row" => Display::TableRow,
                    "table-cell" => Display::TableCell,
                    _ => style.display,
                };
            } else if let Value::None = v {
                style.display = Display::None;
            }
        }
        "position" => {
            if let Some(s) = v.as_ident() {
                style.position = match s {
                    "static" => Position::Static,
                    "relative" => Position::Relative,
                    "absolute" => Position::Absolute,
                    "fixed" => Position::Fixed,
                    "sticky" => Position::Sticky,
                    _ => style.position,
                };
            }
        }
        "color" => {
            if let Some(c) = resolve_color(v) {
                style.color = Some(c);
            } else if let Value::Inherit = v {
                style.color = parent.color;
            }
        }
        "background" | "background-color" => {
            if let Value::None = v {
                style.background_color = None;
            } else if let Some(c) = resolve_color(v) {
                style.background_color = Some(c);
            }
        }
        "font-size" => {
            if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.font_size = Some(px);
            } else if let Value::Ident(s) = v {
                let s_lower = s.to_ascii_lowercase();
                if let Some(stripped) = s_lower.strip_prefix("x-") {
                    let m = match stripped {
                        "small" => 0.6,
                        "large" => 1.5,
                        _ => 1.0,
                    };
                    style.font_size = Some(parent.font_size.unwrap_or(16.0) * m);
                } else {
                    let m = match s_lower.as_str() {
                        "xx-small" => 0.6,
                        "x-small" => 0.75,
                        "small" => 0.89,
                        "medium" => 1.0,
                        "large" => 1.2,
                        "x-large" => 1.5,
                        "xx-large" => 2.0,
                        "smaller" => 0.83,
                        "larger" => 1.2,
                        _ => 1.0,
                    };
                    style.font_size = Some(parent.font_size.unwrap_or(16.0) * m);
                }
            }
        }
        "font-weight" => match v {
            Value::Number(n) => style.font_weight = Some(*n as u16),
            Value::Ident(s) => {
                style.font_weight = Some(match s.as_str() {
                    "normal" => 400,
                    "bold" => 700,
                    "lighter" => (parent.font_weight.unwrap_or(400) as i32 - 100).max(100) as u16,
                    "bolder" => (parent.font_weight.unwrap_or(400) as i32 + 100).min(900) as u16,
                    _ => style.font_weight.unwrap_or(400),
                });
            }
            _ => {}
        },
        "font-style" => {
            if let Some(s) = v.as_ident() {
                style.font_style = Some(match s {
                    "normal" => FontStyle::Normal,
                    "italic" => FontStyle::Italic,
                    "oblique" => FontStyle::Oblique,
                    _ => style.font_style.unwrap_or(FontStyle::Normal),
                });
            }
        }
        "font-family" => {
            if let Some(s) = v.as_string() {
                // Take the first family, strip quotes
                let first = s.split(',').next().unwrap_or(s).trim();
                let first = first.trim_matches(|c: char| c == '"' || c == '\'');
                style.font_family = Some(SmolStr::new(first));
            }
        }
        "line-height" => {
            if let Value::Number(n) = v {
                style.line_height = Some(*n as f32);
            } else if let Some(n) = v.as_number() {
                style.line_height = Some(n as f32);
            }
        }
        "margin" => {
            // Check for "auto" in the shorthand
            let v_str = v.to_string_raw();
            if v_str.contains("auto") {
                // Parse like "0 auto" or "auto" etc.
                let parts: Vec<&str> = v_str.split_whitespace().collect();
                match parts.len() {
                    1 => {
                        // margin: auto → all auto
                        if parts[0] == "auto" {
                            style.margin = EdgeSizes::default();
                            style.margin_auto_left = true;
                            style.margin_auto_right = true;
                        }
                    }
                    2 => {
                        // margin: 0 auto → top/bottom=0, left/right=auto
                        if parts[0] != "auto" {
                            let v0_parsed = super::parser::parse_value(parts[0]);
                            let v0 = resolve_length(&v0_parsed, parent.font_size.unwrap_or(16.0), 16.0, vw, vh).unwrap_or(0.0);
                            style.margin.top = v0;
                            style.margin.bottom = v0;
                        }
                        if parts[1] == "auto" {
                            style.margin_auto_left = true;
                            style.margin_auto_right = true;
                        } else {
                            let v1_parsed = super::parser::parse_value(parts[1]);
                            let v1 = resolve_length(&v1_parsed, parent.font_size.unwrap_or(16.0), 16.0, vw, vh).unwrap_or(0.0);
                            style.margin.left = v1;
                            style.margin.right = v1;
                        }
                    }
                    _ => {
                        let edges = resolve_edge_shorthand(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
                        style.margin = edges;
                    }
                }
            } else {
                let edges = resolve_edge_shorthand(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
                style.margin = edges;
            }
        }
        "margin-top" => {
            if let Value::Ident(s) = v {
                if s == "auto" { /* auto top margin not centered */ }
            } else if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.margin.top = px;
            }
        }
        "margin-bottom" => {
            if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.margin.bottom = px;
            }
        }
        "margin-left" => {
            if let Value::Ident(s) = v {
                if s == "auto" { style.margin_auto_left = true; }
            } else if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.margin.left = px;
                style.margin_auto_left = false;
            }
        }
        "margin-right" => {
            if let Value::Ident(s) = v {
                if s == "auto" { style.margin_auto_right = true; }
            } else if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.margin.right = px;
                style.margin_auto_right = false;
            }
        }
        "padding" => {
            let edges = resolve_edge_shorthand(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
            style.padding = edges;
        }
        "padding-top" => {
            if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.padding.top = px;
            }
        }
        "padding-bottom" => {
            if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.padding.bottom = px;
            }
        }
        "padding-left" => {
            if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.padding.left = px;
            }
        }
        "padding-right" => {
            if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                style.padding.right = px;
            }
        }
        "border" => {
            // border: <width> <style> <color>
            let mut width = 0.0;
            let mut color = None;
            if let Value::List(items) = v {
                for item in items {
                    if let Some(px) = resolve_length(item, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                        width = px;
                    } else if let Some(c) = resolve_color(item) {
                        color = Some(c);
                    }
                }
            } else if let Some(px) = resolve_length(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                width = px;
            }
            style.border = EdgeSizes {
                top: width,
                right: width,
                bottom: width,
                left: width,
            };
            if let Some(c) = color {
                style.border_color = Some(c);
            }
        }
        "border-width" => {
            let edges = resolve_edge_shorthand(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
            style.border = edges;
        }
        "border-color" => {
            if let Some(c) = resolve_color(v) {
                style.border_color = Some(c);
            }
        }
        "width" => {
            style.width = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "height" => {
            style.height = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "min-width" => {
            style.min_width = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "max-width" => {
            style.max_width = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "font" => {
            // font shorthand: [style] [weight] [size]/[line-height] [family]
            // e.g. "16px/1.6 system-ui, sans-serif" or "italic bold 14px Arial"
            let v_str = v.to_string_raw();
            // Find the size (first number with a unit)
            let parts: Vec<&str> = v_str.split_whitespace().collect();
            let mut found_size = false;
            for part in parts {
                if !found_size {
                    // Check if this part contains a size (number + px/em/etc)
                    let val = super::parser::parse_value(part);
                    if let Value::Length(_, _) = val {
                        if let Some(px) = resolve_length(&val, parent.font_size.unwrap_or(16.0), 16.0, vw, vh) {
                            style.font_size = Some(px);
                            found_size = true;
                            continue;
                        }
                    }
                    // Check for font-style
                    if let Some(s) = val.as_ident() {
                        let s_lower = s.to_ascii_lowercase();
                        match s_lower.as_str() {
                            "italic" => { style.font_style = Some(FontStyle::Italic); continue; }
                            "oblique" => { style.font_style = Some(FontStyle::Oblique); continue; }
                            "normal" => { /* could be style or weight */ continue; }
                            _ => {}
                        }
                    }
                    // Check for font-weight
                    if let Value::Number(n) = val {
                        style.font_weight = Some(n as u16);
                        continue;
                    }
                    if let Value::Ident(s) = val {
                        let s_lower = s.to_ascii_lowercase();
                        match s_lower.as_str() {
                            "bold" => { style.font_weight = Some(700); continue; }
                            "bolder" => { style.font_weight = Some((parent.font_weight.unwrap_or(400) + 100).min(900) as u16); continue; }
                            "lighter" => { style.font_weight = Some((parent.font_weight.unwrap_or(400) - 100).max(100) as u16); continue; }
                            _ => {}
                        }
                    }
                }
                // After size, check for line-height (starts with /)
                if found_size && part.starts_with('/') {
                    let lh_str = &part[1..];
                    if let Ok(lh) = lh_str.parse::<f32>() {
                        style.line_height = Some(lh);
                        continue;
                    }
                    // Could be /1.6 without space
                    let lh_val = super::parser::parse_value(lh_str);
                    if let Some(n) = lh_val.as_number() {
                        style.line_height = Some(n as f32);
                    }
                    continue;
                }
                // After size, everything else is font-family
                if found_size {
                    // Take the rest as font-family
                    let family = part;
                    let family = family.trim_matches(|c: char| c == '"' || c == '\'');
                    style.font_family = Some(SmolStr::new(family));
                }
            }
        }
        "flex-direction" => {
            if let Some(s) = v.as_ident() {
                style.flex_direction = Some(match s {
                    "row" => FlexDirection::Row,
                    "row-reverse" => FlexDirection::RowReverse,
                    "column" => FlexDirection::Column,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    _ => style.flex_direction.unwrap_or_default(),
                });
            }
        }
        "justify-content" => {
            if let Some(s) = v.as_ident() {
                style.justify_content = Some(match s {
                    "flex-start" => JustifyContent::FlexStart,
                    "flex-end" => JustifyContent::FlexEnd,
                    "center" => JustifyContent::Center,
                    "space-between" => JustifyContent::SpaceBetween,
                    "space-around" => JustifyContent::SpaceAround,
                    "space-evenly" => JustifyContent::SpaceEvenly,
                    _ => style.justify_content.unwrap_or_default(),
                });
            }
        }
        "align-items" => {
            if let Some(s) = v.as_ident() {
                style.align_items = Some(match s {
                    "stretch" => AlignItems::Stretch,
                    "flex-start" => AlignItems::FlexStart,
                    "flex-end" => AlignItems::FlexEnd,
                    "center" => AlignItems::Center,
                    "baseline" => AlignItems::Baseline,
                    _ => style.align_items.unwrap_or_default(),
                });
            }
        }
        "flex-wrap" => {
            if let Some(s) = v.as_ident() {
                style.flex_wrap = Some(match s {
                    "nowrap" => FlexWrap::NoWrap,
                    "wrap" => FlexWrap::Wrap,
                    "wrap-reverse" => FlexWrap::WrapReverse,
                    _ => style.flex_wrap.unwrap_or_default(),
                });
            }
        }
        "flex-grow" => {
            if let Value::Number(n) = v {
                style.flex_grow = Some(*n as f32);
            }
        }
        "flex-shrink" => {
            if let Value::Number(n) = v {
                style.flex_shrink = Some(*n as f32);
            }
        }
        "flex-basis" => {
            style.flex_basis = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "flex" => {
            // flex: <grow> <shrink> <basis>
            let mut grow = None;
            let mut shrink = None;
            let mut basis = None;
            if let Value::List(items) = v {
                for item in items {
                    match item {
                        Value::Number(n) => {
                            if grow.is_none() {
                                grow = Some(*n as f32);
                            } else if shrink.is_none() {
                                shrink = Some(*n as f32);
                            }
                        }
                        _ => {
                            basis = resolve_length_value(item, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
                        }
                    }
                }
            }
            style.flex_grow = grow.or(Some(0.0));
            style.flex_shrink = shrink.or(Some(1.0));
            style.flex_basis = basis.or(Some(Length::Auto));
        }
        "grid-template-columns" => {
            style.grid_template_columns = resolve_track_list(v);
        }
        "grid-template-rows" => {
            style.grid_template_rows = resolve_track_list(v);
        }
        "grid-column" => {
            if let Some((a, b)) = resolve_grid_placement(v) {
                style.grid_column = Some((a, b));
            }
        }
        "grid-row" => {
            if let Some((a, b)) = resolve_grid_placement(v) {
                style.grid_row = Some((a, b));
            }
        }
        "top" => {
            style.top = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "right" => {
            style.right = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "bottom" => {
            style.bottom = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "left" => {
            style.left = resolve_length_value(v, parent.font_size.unwrap_or(16.0), 16.0, vw, vh);
        }
        "z-index" => {
            if let Value::Number(n) = v {
                style.z_index = Some(*n as i32);
            } else if let Value::Auto = v {
                style.z_index = None;
            }
        }
        "overflow" => {
            if let Some(s) = v.as_ident() {
                style.overflow = Some(match s {
                    "visible" => Overflow::Visible,
                    "hidden" => Overflow::Hidden,
                    "scroll" => Overflow::Scroll,
                    "auto" => Overflow::Auto,
                    _ => style.overflow.unwrap_or_default(),
                });
            }
        }
        "box-sizing" => {
            if let Some(s) = v.as_ident() {
                style.box_sizing = match s {
                    "border-box" => BoxSizing::BorderBox,
                    _ => BoxSizing::ContentBox,
                };
            }
        }
        "text-align" => {
            if let Some(s) = v.as_ident() {
                style.text_align = Some(match s {
                    "left" => TextAlign::Left,
                    "right" => TextAlign::Right,
                    "center" => TextAlign::Center,
                    "justify" => TextAlign::Justify,
                    _ => style.text_align.unwrap_or_default(),
                });
            }
        }
        "visibility" => {
            if let Some(s) = v.as_ident() {
                style.visibility = Some(match s {
                    "visible" => Visibility::Visible,
                    "hidden" => Visibility::Hidden,
                    "collapse" => Visibility::Collapse,
                    _ => style.visibility.unwrap_or_default(),
                });
            }
        }
        "opacity" => {
            if let Value::Number(n) = v {
                style.opacity = Some(*n as f32);
            }
        }
        _ => {
            // Unknown property — we still kept the raw value above.
        }
    }
}

/// Resolve loose ends after all declarations have been applied.
pub fn finalize(
    node: &NodeRef,
    style: &mut ComputedStyle,
    parent: &ComputedStyle,
    vw: f32,
    vh: f32,
) {
    // Default display from tag if not set.
    // The cascade starts with Display::Inline (the enum default). If no CSS
    // rule set display, we apply the tag's default (e.g. div→block, img→inline-block).
    if !style.raw.contains_key("display") {
        if let Some(tag) = node.tag_name() {
            let d = default_display(&tag);
            style.display = d;
        }
    }
    // If display is none for head/title/etc., that's fine.
    if style.color.is_none() {
        style.color = parent.color.or(Some(0xFF000000));
    }
    if style.font_size.is_none() {
        style.font_size = parent.font_size.or(Some(16.0));
    }
    if style.font_weight.is_none() {
        style.font_weight = parent.font_weight.or(Some(400));
    }
    if style.font_family.is_none() {
        style.font_family = parent.font_family.clone().or_else(|| Some(SmolStr::new("serif")));
    }
    if style.line_height.is_none() {
        style.line_height = parent.line_height.or(Some(1.2));
    }
    if style.opacity.is_none() {
        style.opacity = Some(1.0);
    }
    let _ = (vw, vh);
}

fn resolve_color(v: &Value) -> Option<u32> {
    match v {
        Value::Color(c) => Some(*c),
        Value::Ident(s) => super::parser::named_color(s).map(|c| Value::Color(c)).and_then(|c| c.as_color()),
        Value::Function(_, _) => {
            // Re-tokenize the function's string form.
            let s = v.to_string_raw();
            let parsed = super::parser::parse_value(&s);
            parsed.as_color()
        }
        Value::List(items) => {
            for item in items {
                if let Some(c) = resolve_color(item) {
                    return Some(c);
                }
            }
            None
        }
        _ => None,
    }
}

fn resolve_length(v: &Value, em: f32, rem: f32, vw: f32, vh: f32) -> Option<f32> {
    match v {
        Value::Length(n, unit) => Some(match unit {
            Unit::Px => *n as f32,
            Unit::Em => *n as f32 * em,
            Unit::Rem => *n as f32 * rem,
            Unit::Vw => *n as f32 * vw / 100.0,
            Unit::Vh => *n as f32 * vh / 100.0,
            Unit::Vmin => *n as f32 * vw.min(vh) / 100.0,
            Unit::Vmax => *n as f32 * vw.max(vh) / 100.0,
            Unit::Pt => *n as f32 * 4.0 / 3.0,
            Unit::Pc => *n as f32 * 16.0,
            Unit::In => *n as f32 * 96.0,
            Unit::Cm => *n as f32 * 96.0 / 2.54,
            Unit::Mm => *n as f32 * 96.0 / 25.4,
            _ => *n as f32,
        }),
        Value::Number(n) => Some(*n as f32),
        Value::Percentage(p) => Some(*p as f32),
        Value::List(items) => items
            .iter()
            .find_map(|i| resolve_length(i, em, rem, vw, vh)),
        _ => None,
    }
}

fn resolve_length_value(v: &Value, em: f32, rem: f32, vw: f32, vh: f32) -> Option<Length> {
    match v {
        Value::Auto => Some(Length::Auto),
        Value::None => None,
        Value::Length(n, unit) => Some(match unit {
            Unit::Px => Length::Px(*n as f32),
            Unit::Em => Length::Em(*n as f32),
            Unit::Rem => Length::Rem(*n as f32),
            Unit::Vw => Length::Vw(*n as f32),
            Unit::Vh => Length::Vh(*n as f32),
            Unit::Percent => Length::Percent(*n as f32),
            Unit::Fr => Length::Fr(*n as f32),
            _ => Length::Px(*n as f32),
        }),
        Value::Percentage(p) => Some(Length::Percent(*p as f32)),
        Value::Number(n) => Some(Length::Px(*n as f32)),
        Value::List(items) => items
            .iter()
            .find_map(|i| resolve_length_value(i, em, rem, vw, vh)),
        _ => None,
    }
}

fn resolve_edge_shorthand(v: &Value, em: f32, rem: f32, vw: f32, vh: f32) -> EdgeSizes {
    let nums: Vec<f32> = match v {
        Value::List(items) => items
            .iter()
            .filter_map(|i| resolve_length(i, em, rem, vw, vh))
            .collect(),
        Value::None => vec![0.0],
        _ => resolve_length(v, em, rem, vw, vh).map(|n| vec![n]).unwrap_or_default(),
    };
    match nums.len() {
        0 => EdgeSizes::default(),
        1 => EdgeSizes {
            top: nums[0],
            right: nums[0],
            bottom: nums[0],
            left: nums[0],
        },
        2 => EdgeSizes {
            top: nums[0],
            right: nums[1],
            bottom: nums[0],
            left: nums[1],
        },
        3 => EdgeSizes {
            top: nums[0],
            right: nums[1],
            bottom: nums[2],
            left: nums[1],
        },
        _ => EdgeSizes {
            top: nums[0],
            right: nums[1],
            bottom: nums[2],
            left: nums[3],
        },
    }
}

fn resolve_track_list(v: &Value) -> Vec<TrackSize> {
    let items = match v {
        Value::List(items) => items.clone(),
        _ => vec![v.clone()],
    };
    items
        .iter()
        .filter_map(|i| match i {
            Value::Auto => Some(TrackSize::Auto),
            Value::Ident(s) if s == "auto" => Some(TrackSize::Auto),
            Value::Ident(s) if s == "min-content" => Some(TrackSize::MinContent),
            Value::Ident(s) if s == "max-content" => Some(TrackSize::MaxContent),
            Value::Length(n, Unit::Px) => Some(TrackSize::Px(*n as f32)),
            Value::Length(n, Unit::Fr) => Some(TrackSize::Fr(*n as f32)),
            Value::Percentage(p) => Some(TrackSize::Percent(*p as f32)),
            Value::Function(name, args) if name == "minmax" => {
                // Just take the max as the track size for simplicity.
                args.last().and_then(|a| match a {
                    Value::Auto => Some(TrackSize::Auto),
                    Value::Length(n, Unit::Px) => Some(TrackSize::Px(*n as f32)),
                    Value::Length(n, Unit::Fr) => Some(TrackSize::Fr(*n as f32)),
                    Value::Percentage(p) => Some(TrackSize::Percent(*p as f32)),
                    _ => Some(TrackSize::Auto),
                })
            }
            _ => None,
        })
        .collect()
}

fn resolve_grid_placement(v: &Value) -> Option<(i32, i32)> {
    match v {
        Value::Number(n) => Some((*n as i32, *n as i32 + 1)),
        Value::Auto => Some((1, -1)),
        Value::List(items) | Value::Slash(items) => {
            let mut nums = Vec::new();
            for i in items {
                if let Value::Number(n) = i {
                    nums.push(*n as i32);
                }
            }
            if nums.len() == 1 {
                Some((nums[0], nums[0] + 1))
            } else if nums.len() >= 2 {
                Some((nums[0], nums[1]))
            } else {
                None
            }
        }
        _ => None,
    }
}
