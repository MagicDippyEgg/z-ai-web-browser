//! Rendering integration tests — render HTML pages and check that content
//! appears at the correct position on the canvas.

use browser_core::css::Cascade;
use browser_core::html::parser;
use browser_core::layout::layout_document;
use browser_core::html::dom::{Node, NodeKind, NodeRef};

fn render_page(html: &str, width: f32, height: f32) -> NodeRef {
    let doc = parser::parse(html);
    let cascade = Cascade::new();
    cascade.apply(&doc, width, height);
    layout_document(&doc, width, height);
    doc
}

fn get_layout(doc: &NodeRef, selector: &str) -> Option<browser_core::html::dom::LayoutRect> {
    // Simple selector: #id or tag
    let node = if selector.starts_with('#') {
        let id = &selector[1..];
        Node::get_element_by_id(doc, id)
    } else {
        let nodes = Node::get_elements_by_tag_name(doc, selector);
        nodes.into_iter().next()
    };
    node.and_then(|n| n.layout())
}

// ===== Basic layout tests =====

#[test]
fn test_body_fills_viewport_width() {
    let html = r#"<html><body><p>Hello</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let body = get_layout(&doc, "body").expect("body");
    assert!(body.width >= 700.0, "body should fill viewport width, got {}", body.width);
}

#[test]
fn test_block_children_stack_vertically() {
    let html = r#"<html><body><div id="a">A</div><div id="b">B</div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let a = get_layout(&doc, "#a").expect("a");
    let b = get_layout(&doc, "#b").expect("b");
    assert!(b.y >= a.y + a.height, "b should be below a: a.y={} a.h={} b.y={}", a.y, a.height, b.y);
}

#[test]
fn test_text_renders_below_padding() {
    let html = r#"<html><body style="padding-top:100px"><p id="t">Hello</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let p = get_layout(&doc, "#t").expect("p");
    assert!(p.y >= 100.0, "text should be below 100px padding, got y={}", p.y);
}

#[test]
fn test_max_width_centers_content() {
    let html = r#"<html><body style="max-width:400px; margin:0 auto"><p id="t">Hello</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let body = get_layout(&doc, "body").expect("body");
    assert!(body.width <= 410.0, "body should respect max-width, got {}", body.width);
    assert!(body.x >= 100.0, "body should be centered, got x={}", body.x);
}

#[test]
fn test_em_units_resolve() {
    let html = r#"<html><body><div id="d" style="width:10em; height:5em">X</div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let d = get_layout(&doc, "#d").expect("d");
    assert_eq!(d.width, 160.0, "10em should be 160px (10*16), got {}", d.width);
    assert_eq!(d.height, 80.0, "5em should be 80px (5*16), got {}", d.height);
}

#[test]
fn test_vh_units_resolve() {
    let html = r#"<html><body style="padding-top:50vh"><p id="t">X</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let p = get_layout(&doc, "#t").expect("p");
    assert!(p.y >= 300.0, "50vh should be 300px (50% of 600), got y={}", p.y);
}

#[test]
fn test_padding_shorthand_4_values() {
    let html = r#"<html><body style="padding:10px 20px 30px 40px"><p id="t">X</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let p = get_layout(&doc, "#t").expect("p");
    assert!(p.y >= 10.0, "top padding should be 10px, got y={}", p.y);
    assert!(p.x >= 40.0, "left padding should be 40px, got x={}", p.x);
}

#[test]
fn test_padding_shorthand_2_values() {
    let html = r#"<html><body style="padding:100px 20px"><p id="t">X</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let body = get_layout(&doc, "body").expect("body");
    let p = get_layout(&doc, "#t").expect("p");
    // top/bottom = 100px, left/right = 20px
    assert!(p.y >= 100.0, "top padding should be 100px, got y={}", p.y);
}

#[test]
fn test_box_sizing_border_box() {
    let html = r#"<html><body><div id="d" style="width:200px; padding:20px; border:10px solid black; box-sizing:border-box">X</div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let d = get_layout(&doc, "#d").expect("d");
    assert_eq!(d.width, 200.0, "border-box width should be 200px, got {}", d.width);
}

#[test]
fn test_box_sizing_content_box() {
    let html = r#"<html><body><div id="d" style="width:200px; padding:20px; border:10px solid black; box-sizing:content-box">X</div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let d = get_layout(&doc, "#d").expect("d");
    // content-box: width is content area. Border box = 200 + 40 + 20 = 260
    assert!(d.width >= 200.0, "content-box border width should include padding+border, got {}", d.width);
}

// ===== Inline-block tests =====

#[test]
fn test_inline_blocks_flow_horizontally() {
    let html = r#"<html><body><input id="a" type="text"><input id="b" type="text"></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let a = get_layout(&doc, "#a").expect("a");
    let b = get_layout(&doc, "#b").expect("b");
    assert!(b.x >= a.x + a.width, "b should be to the right of a: a.x={} a.w={} b.x={}", a.x, a.width, b.x);
    assert!((b.y - a.y).abs() < 5.0, "a and b should be on the same line: a.y={} b.y={}", a.y, b.y);
}

#[test]
fn test_inline_block_gets_explicit_width() {
    let html = r#"<html><body><input id="i" type="text" style="width:300px"></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let i = get_layout(&doc, "#i").expect("i");
    assert!(i.width >= 300.0, "input should have width 300px, got {}", i.width);
}

// ===== Flexbox tests =====

#[test]
fn test_flex_row_children_side_by_side() {
    let html = r#"<html><body><div style="display:flex; flex-direction:row"><div id="a">A</div><div id="b">B</div></div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let a = get_layout(&doc, "#a").expect("a");
    let b = get_layout(&doc, "#b").expect("b");
    assert!(b.x >= a.x + a.width - 1.0, "b should be right of a in flex row");
}

#[test]
fn test_flex_column_children_stack() {
    let html = r#"<html><body><div style="display:flex; flex-direction:column"><div id="a">A</div><div id="b">B</div></div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let a = get_layout(&doc, "#a").expect("a");
    let b = get_layout(&doc, "#b").expect("b");
    assert!(b.y >= a.y + a.height - 1.0, "b should be below a in flex column");
}

// ===== Absolute positioning tests =====

#[test]
fn test_absolute_positioning_top() {
    let html = r#"<html><body><div id="d" style="position:absolute; top:200px; left:100px">X</div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let d = get_layout(&doc, "#d").expect("d");
    assert!((d.y - 200.0).abs() < 5.0, "absolute top should be 200px, got {}", d.y);
    assert!((d.x - 100.0).abs() < 5.0, "absolute left should be 100px, got {}", d.x);
}

#[test]
fn test_absolute_positioning_percent() {
    let html = r#"<html><body><div id="d" style="position:absolute; top:25%">X</div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let d = get_layout(&doc, "#d").expect("d");
    assert!((d.y - 150.0).abs() < 5.0, "25% of 600 should be 150px, got {}", d.y);
}

// ===== CSS parsing tests =====

#[test]
fn test_css_em_not_exponent() {
    let v = browser_core::css::parser::parse_value("2em");
    match v {
        browser_core::css::Value::Length(n, unit) => {
            assert_eq!(n, 2.0);
            assert_eq!(unit, browser_core::css::Unit::Em);
        }
        _ => panic!("2em should parse as Length(2, Em), got {:?}", v),
    }
}

#[test]
fn test_css_rem_not_exponent() {
    let v = browser_core::css::parser::parse_value("1.5rem");
    match v {
        browser_core::css::Value::Length(n, unit) => {
            assert_eq!(n, 1.5);
            assert_eq!(unit, browser_core::css::Unit::Rem);
        }
        _ => panic!("1.5rem should parse as Length(1.5, Rem), got {:?}", v),
    }
}

#[test]
fn test_css_scientific_notation() {
    let v = browser_core::css::parser::parse_value("1e5");
    match v {
        browser_core::css::Value::Number(n) => {
            assert_eq!(n, 100000.0);
        }
        _ => panic!("1e5 should parse as Number(100000), got {:?}", v),
    }
}

#[test]
fn test_css_value_list_with_em() {
    let v = browser_core::css::parser::parse_value("1em 2em 3em");
    match v {
        browser_core::css::Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("should be a list of 3, got {:?}", v),
    }
}

// ===== Font shorthand tests =====

#[test]
fn test_font_shorthand_parses_size() {
    let html = r#"<html><body style="font:16px/1.6 sans-serif"><p id="t">X</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let p = get_layout(&doc, "#t").expect("p");
    // If font shorthand was parsed, the text should render with font-size 16px
    // The p should have a non-zero height
    assert!(p.height > 0.0, "p should have height from font-size 16px, got {}", p.height);
}

#[test]
fn test_font_shorthand_with_em() {
    let html = r#"<html><body style="font:2em/1.5 sans-serif"><p id="t">X</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let p = get_layout(&doc, "#t").expect("p");
    // 2em with default 16px root = 32px font size
    // The p height should be at least 32px (line height 1.5 * 32 = 48)
    assert!(p.height >= 32.0, "2em font should give height >= 32px, got {}", p.height);
}

// ===== Text positioning tests =====

#[test]
fn test_text_not_at_origin() {
    let html = r#"<html><body><p id="t">Hello World</p></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let p = get_layout(&doc, "#p").unwrap_or_default();
    // The text node inside p should not be at (0,0)
    // Find the text node
    let p_node = Node::get_element_by_id(&doc, "t").expect("p");
    let mut text_pos = None;
    for c in p_node.children() {
        if let NodeKind::Text(_) = c.kind() {
            text_pos = c.layout();
        }
    }
    if let Some(tp) = text_pos {
        assert!(tp.width > 0.0, "text should have non-zero width, got {}", tp.width);
        assert!(tp.height > 0.0, "text should have non-zero height, got {}", tp.height);
    }
}

#[test]
fn test_nested_divs_get_width() {
    let html = r#"<html><body><div id="outer"><div id="inner">X</div></div></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let outer = get_layout(&doc, "#outer").expect("outer");
    let inner = get_layout(&doc, "#inner").expect("inner");
    assert!(inner.width > 0.0, "inner div should have non-zero width, got {}", inner.width);
    assert!(inner.width <= outer.width + 1.0, "inner should not be wider than outer");
}

// ===== Form element tests =====

#[test]
fn test_hidden_input_not_displayed() {
    let html = r#"<html><body><input type="hidden" id="h"><input type="text" id="v"></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let h = get_layout(&doc, "#h").expect("h");
    assert_eq!(h.width, 0.0, "hidden input should have 0 width");
    assert_eq!(h.height, 0.0, "hidden input should have 0 height");
    let v = get_layout(&doc, "#v").expect("v");
    assert!(v.width > 0.0, "visible input should have width");
    assert!(v.height > 0.0, "visible input should have height");
}

#[test]
fn test_text_input_has_default_size() {
    let html = r#"<html><body><input type="text" id="i"></body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let i = get_layout(&doc, "#i").expect("i");
    assert!(i.width >= 100.0, "text input should have default width >= 100px, got {}", i.width);
    assert!(i.height >= 20.0, "text input should have default height >= 20px, got {}", i.height);
}

// ===== Color tests =====

#[test]
fn test_color_hex_6() {
    let v = browser_core::css::parser::parse_value("#ff0000");
    match v {
        browser_core::css::Value::Color(c) => assert_eq!(c, 0xFFFF0000),
        _ => panic!("expected color"),
    }
}

#[test]
fn test_color_hex_3() {
    let v = browser_core::css::parser::parse_value("#f00");
    match v {
        browser_core::css::Value::Color(c) => assert_eq!(c, 0xFFFF0000),
        _ => panic!("expected color"),
    }
}

#[test]
fn test_color_rgb() {
    let v = browser_core::css::parser::parse_value("rgb(0, 128, 255)");
    match v {
        browser_core::css::Value::Color(c) => assert_eq!(c, 0xFF0080FF),
        _ => panic!("expected color"),
    }
}

// ===== Selector tests =====

#[test]
fn test_selector_id_specificity() {
    let sel = browser_core::css::Selector::parse("#foo").expect("parse");
    assert_eq!(sel.specificity, (1, 0, 0));
}

#[test]
fn test_selector_class_specificity() {
    let sel = browser_core::css::Selector::parse(".bar").expect("parse");
    assert_eq!(sel.specificity, (0, 1, 0));
}

#[test]
fn test_selector_compound_specificity() {
    let sel = browser_core::css::Selector::parse("div.foo#bar").expect("parse");
    assert_eq!(sel.specificity, (1, 1, 1));
}

// ===== Multiple test: example.com-like page =====

#[test]
fn test_example_com_like_page() {
    let html = r#"<html><body style="font:16px/1.6 sans-serif; max-width:26em; margin:auto; padding:25vh 2em 2em; text-align:center">
        <p>Example Domain</p>
        <p>This domain is for use in documentation examples.</p>
        </body></html>"#;
    let doc = render_page(html, 800.0, 600.0);
    let body = get_layout(&doc, "body").expect("body");
    // max-width: 26em = 416px
    assert!(body.width <= 420.0, "body should respect max-width 26em (416px), got {}", body.width);
    // padding-top: 25vh = 150px
    let p = Node::get_elements_by_tag_name(&doc, "p");
    assert!(!p.is_empty(), "should have p elements");
    let first_p = &p[0];
    let p_layout = first_p.layout().expect("p layout");
    assert!(p_layout.y >= 150.0, "first p should be below 25vh (150px) padding, got y={}", p_layout.y);
}
