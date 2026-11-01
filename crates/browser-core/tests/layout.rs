//! Tests for the layout engine.

use browser_core::css::Cascade;
use browser_core::html::parser;
use browser_core::layout::layout_document;

#[test]
fn lays_out_simple_document() {
    let html = r#"<html><body><p>Hello</p></body></html>"#;
    let doc = parser::parse(html);
    let cascade = Cascade::new();
    cascade.apply(&doc, 800.0, 600.0);
    let _rect = layout_document(&doc, 800.0, 600.0);
    // The document should have laid out without panicking.
}

#[test]
fn lays_out_block_elements_vertically() {
    let html = r#"<html><body><div id="a">A</div><div id="b">B</div></body></html>"#;
    let doc = parser::parse(html);
    let cascade = Cascade::new();
    cascade.apply(&doc, 800.0, 600.0);
    layout_document(&doc, 800.0, 600.0);
    let a = browser_core::html::dom::Node::get_element_by_id(&doc, "a").expect("a");
    let b = browser_core::html::dom::Node::get_element_by_id(&doc, "b").expect("b");
    let a_layout = a.layout().unwrap_or_default();
    let b_layout = b.layout().unwrap_or_default();
    assert!(b_layout.y >= a_layout.y + a_layout.height, "b should be below a");
}

#[test]
fn lays_out_flex_row() {
    let html = r#"<html><body><div style="display:flex; flex-direction:row;"><div id="a">A</div><div id="b">B</div></div></body></html>"#;
    let doc = parser::parse(html);
    let cascade = Cascade::new();
    cascade.apply(&doc, 800.0, 600.0);
    layout_document(&doc, 800.0, 600.0);
    let a = browser_core::html::dom::Node::get_element_by_id(&doc, "a").expect("a");
    let b = browser_core::html::dom::Node::get_element_by_id(&doc, "b").expect("b");
    let a_layout = a.layout().unwrap_or_default();
    let b_layout = b.layout().unwrap_or_default();
    // In a flex row, b should be to the right of a
    assert!(b_layout.x >= a_layout.x + a_layout.width - 1.0, "b should be right of a (a.x={}, a.w={}, b.x={})", a_layout.x, a_layout.width, b_layout.x);
}
