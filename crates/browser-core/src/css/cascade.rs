//! CSS cascade: collect declarations, sort by origin/importance/specificity/
//! order, then compute the `ComputedStyle` for every node.

use super::parser::{parse, Declaration, Rule, Stylesheet, Value};
use super::properties;
use crate::html::dom::{self, *};
use ahash::AHashMap;
use smol_str::SmolStr;
use std::sync::Arc;

/// Aggregated stylesheet set for a document. Add stylesheets in source order
/// (lower index = earlier).
#[derive(Default, Clone)]
pub struct Cascade {
    sheets: Vec<Stylesheet>,
}

impl Cascade {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_stylesheet(&mut self, sheet: Stylesheet) {
        self.sheets.push(sheet);
    }

    pub fn add_text(&mut self, css: &str) {
        let sheet = parse(css);
        self.sheets.push(sheet);
    }

    /// Compute and apply the style for every node in the document.
    pub fn apply(&self, document: &NodeRef, viewport_width: f32, viewport_height: f32) {
        // 1. Collect declarations per node, plus inheritance.
        let user_agent = user_agent_stylesheet();
        // Apply to the whole tree.
        apply_recursive(
            document,
            &[user_agent],
            &self.sheets,
            viewport_width,
            viewport_height,
            ComputedStyle::default_initial(),
        );
    }
}

fn apply_recursive(
    node: &NodeRef,
    ua: &[Stylesheet],
    user: &[Stylesheet],
    vw: f32,
    vh: f32,
    parent_style: ComputedStyle,
) {
    // Collect matching declarations in order: UA, user, author.
    let mut matches: Vec<(MatchedDecl, usize)> = Vec::new();
    let mut order = 0usize;
    // Walk all stylesheets; for each rule, if any selector matches, record.
    let collect = |sheets: &[Stylesheet], origin: Origin| -> Vec<(MatchedDecl, usize)> {
        let mut out = Vec::new();
        for sheet in sheets {
            collect_rules(&sheet.rules, node, origin, &mut out, &mut order.clone());
        }
        out
    };
    let mut all = Vec::new();
    let mut o = 0usize;
    for sheet in ua {
        collect_rules(&sheet.rules, node, Origin::UserAgent, &mut all, &mut o);
    }
    o = 1_000_000;
    for sheet in user {
        collect_rules(&sheet.rules, node, Origin::Author, &mut all, &mut o);
    }
    // Inline style — highest specificity.
    let inline_decls = inline_style_declarations(node);
    let mut order_inline = 100_000_000usize;
    for d in inline_decls {
        all.push((
            MatchedDecl {
                decl: d,
                specificity: (1, 0, 0, 0),
                origin: Origin::Author,
                order: order_inline,
            },
            order_inline,
        ));
        order_inline += 1;
    }
    let _ = matches;
    let _ = collect;

    // Sort by: origin (author normal > ua normal > author !important > ua !important reversed), then specificity, then order.
    all.sort_by(|a, b| {
        let ra = rank(a.0.origin, a.0.decl.important);
        let rb = rank(b.0.origin, b.0.decl.important);
        ra.cmp(&rb)
            .then(a.0.specificity.cmp(&b.0.specificity))
            .then(a.0.order.cmp(&b.0.order))
    });

    // Build a style map. Last-write-wins after sort.
    let mut style = parent_style.clone_for_inherit();
    for (md, _) in &all {
        properties::apply_declaration(node, &mut style, &md.decl, vw, vh, &parent_style);
    }
    // Resolve missing defaults.
    properties::finalize(node, &mut style, &parent_style, vw, vh);
    node.set_style(style.clone());

    // Recurse.
    let children = node.children();
    for c in children {
        apply_recursive(&c, ua, user, vw, vh, style.clone());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Origin {
    UserAgent,
    Author,
}

struct MatchedDecl {
    decl: Declaration,
    /// (id, class, type, _unused)
    specificity: (u32, u32, u32, u32),
    origin: Origin,
    order: usize,
}

fn collect_rules(rules: &[Rule], node: &NodeRef, origin: Origin, out: &mut Vec<(MatchedDecl, usize)>, order: &mut usize) {
    for r in rules {
        match r {
            Rule::Style(s) => {
                let mut best_spec = None;
                for sel in &s.selectors {
                    if sel.matches(node) {
                        let spec = (sel.specificity.0, sel.specificity.1, sel.specificity.2, 0);
                        best_spec = Some(match best_spec {
                            None => spec,
                            Some(prev) => if spec > prev { spec } else { prev },
                        });
                    }
                }
                if let Some(spec) = best_spec {
                    for d in &s.declarations {
                        out.push((
                            MatchedDecl {
                                decl: d.clone(),
                                specificity: spec,
                                origin,
                                order: *order,
                            },
                            *order,
                        ));
                        *order += 1;
                    }
                }
            }
            Rule::Media(m) => {
                // We treat all media queries as matching (we don't evaluate them).
                // In a real browser we'd check the media query against the
                // viewport; for now we apply all media rules.
                collect_rules(&m.rules, node, origin, out, order);
            }
            _ => {}
        }
    }
}

fn inline_style_declarations(node: &NodeRef) -> Vec<Declaration> {
    let kind = node.kind();
    let elem = match &kind {
        NodeKind::Element(e) => e,
        _ => return Vec::new(),
    };
    let style_str = match elem.get_attr("style") {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    // Parse a series of `prop: value;` pairs.
    let mut out = Vec::new();
    for part in style_str.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((p, v)) = part.split_once(':') {
            let p = p.trim().to_ascii_lowercase();
            let v = v.trim();
            let mut important = false;
            let v = if let Some(stripped) = v.strip_suffix("!important").or_else(|| v.strip_suffix("! important")) {
                important = true;
                stripped.trim()
            } else {
                v
            };
            out.push(Declaration {
                property: SmolStr::new(p),
                value: super::parser::parse_value(v),
                important,
            });
        }
    }
    out
}

/// Sort rank: !important author > !important ua > author > ua.
fn rank(origin: Origin, important: bool) -> u32 {
    match (origin, important) {
        (Origin::Author, true) => 4,
        (Origin::UserAgent, true) => 3,
        (Origin::Author, false) => 2,
        (Origin::UserAgent, false) => 1,
    }
}

impl ComputedStyle {
    /// Construct an "initial" style — the defaults inherited by the document
    /// element before any CSS is applied.
    pub fn default_initial() -> Self {
        let mut s = Self::default();
        s.color = Some(0xFF000000);
        s.background_color = None;
        s.font_size = Some(16.0);
        s.font_weight = Some(400);
        s.font_style = Some(FontStyle::Normal);
        s.font_family = Some(SmolStr::new("serif"));
        s.line_height = Some(1.2);
        s.opacity = Some(1.0);
        s
    }

    /// Make a copy suitable for inheriting into a child: keep inherited
    /// properties, reset non-inherited ones to defaults.
    pub fn clone_for_inherit(&self) -> Self {
        Self {
            // Inherited
            color: self.color,
            font_size: self.font_size,
            font_weight: self.font_weight,
            font_style: self.font_style,
            font_family: self.font_family.clone(),
            line_height: self.line_height,
            text_align: self.text_align,
            visibility: self.visibility,
            opacity: self.opacity,
            // Non-inherited — reset
            display: Display::default(),
            position: Position::default(),
            background_color: None,
            margin: EdgeSizes::default(),
            margin_auto_left: false,
            margin_auto_right: false,
            padding: EdgeSizes::default(),
            border: EdgeSizes::default(),
            border_color: None,
            width: None,
            height: None,
            min_width: None,
            max_width: None,
            box_sizing: BoxSizing::default(),
            flex_direction: None,
            justify_content: None,
            align_items: None,
            flex_wrap: None,
            flex_grow: None,
            flex_shrink: None,
            flex_basis: None,
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_column: None,
            grid_row: None,
            top: None,
            right: None,
            bottom: None,
            left: None,
            z_index: None,
            overflow: None,
            raw: AHashMap::new(),
        }
    }
}

/// The user-agent stylesheet. A small but useful default style for HTML.
fn user_agent_stylesheet() -> Stylesheet {
    let css = r#"
        html, body { display: block; margin: 8px; }
        head, title, meta, link, base, script, style { display: none; }
        div, section, article, header, footer, main, nav, aside, p, ul, ol, li,
        table, thead, tbody, tfoot, tr, form, fieldset, blockquote, pre, figure,
        figcaption, details, summary, address, hr, dl, dt, dd, h1, h2, h3, h4, h5, h6 {
            display: block;
        }
        h1 { font-size: 2em; font-weight: bold; margin: 0.67em 0; }
        h2 { font-size: 1.5em; font-weight: bold; margin: 0.83em 0; }
        h3 { font-size: 1.17em; font-weight: bold; margin: 1em 0; }
        h4 { font-size: 1em; font-weight: bold; margin: 1.33em 0; }
        h5 { font-size: 0.83em; font-weight: bold; margin: 1.67em 0; }
        h6 { font-size: 0.67em; font-weight: bold; margin: 2.33em 0; }
        p { margin: 1em 0; }
        ul, ol { margin: 1em 0; padding-left: 40px; }
        li { display: list-item; }
        a { color: blue; text-decoration: underline; cursor: pointer; }
        img { display: inline-block; }
        br { display: inline; }
        span, em, strong, b, i, u, code, small, sub, sup, label, abbr, cite, q,
        mark, time, var, kbd, samp { display: inline; }
        b, strong { font-weight: bold; }
        i, em { font-style: italic; }
        u { text-decoration: underline; }
        code, pre { font-family: monospace; }
        pre { white-space: pre; }
        hr { display: block; border: 1px inset; height: 0; margin: 0.5em auto; }
        table { display: table; border-collapse: collapse; }
        tr { display: table-row; }
        td, th { display: table-cell; padding: 1px; }
        th { font-weight: bold; }
        input, button, textarea, select { display: inline-block; font-family: inherit; font-size: inherit; box-sizing: border-box; }
        input[type="hidden"] { display: none; }
        input[type="checkbox"], input[type="radio"] { width: 13px; height: 13px; }
        input[type="text"], input[type="search"], input[type="email"], input[type="url"], input[type="password"], input:not([type]) { width: 200px; height: 24px; padding: 3px 6px; border: 1px solid #888; }
        textarea { width: 300px; height: 80px; padding: 4px; border: 1px solid #888; }
        button { padding: 4px 12px; border: 1px solid #888; background: #eee; cursor: pointer; min-width: 40px; height: 28px; }
        input { padding: 3px 6px; border: 1px solid #888; height: 24px; }
        input[type="submit"], input[type="button"] {
            padding: 4px 12px; border: 1px solid #888; background: #eee; cursor: pointer; min-width: 40px; height: 28px;
        }
        label { cursor: default; }
        form { display: block; }
        fieldset { display: block; border: 1px solid #888; padding: 0.5em; }
        legend { display: block; padding: 0 0.5em; }
        blockquote { display: block; margin: 1em 40px; }
        ul, ol { padding-left: 40px; }
        ul li { list-style: disc; }
        ol li { list-style: decimal; }
        html { background: white; }
        body { color: black; background: white; margin: 8px; }
        hr { border: 1px solid #888; border-bottom: none; }
        q::before { content: open-quote; }
        q::after { content: close-quote; }
        mark { background: yellow; color: black; }
        small { font-size: 0.83em; }
        sub { vertical-align: sub; font-size: 0.83em; }
        sup { vertical-align: super; font-size: 0.83em; }
    "#;
    parse(css)
}
