//! Tests for the HTML parser.

use browser_core::html::dom::{Node, NodeKind, NodeRef};
use browser_core::html::parser;

fn text_of_first_text_child(node: &NodeRef) -> Option<String> {
    for c in node.children() {
        if let NodeKind::Text(t) = c.kind() {
            return Some(t.to_string());
        }
    }
    None
}

#[test]
fn parses_simple_document() {
    let html = r#"<!DOCTYPE html><html><head><title>Test</title></head><body><p>Hello</p></body></html>"#;
    let doc = parser::parse(html);
    let body = find_tag(&doc, "body").expect("body should exist");
    let p = find_tag(&body, "p").expect("p should exist");
    assert_eq!(text_of_first_text_child(&p), Some("Hello".to_string()));
}

#[test]
fn parses_attributes() {
    let html = r#"<html><body><a href="https://example.com" class="link">Click</a></body></html>"#;
    let doc = parser::parse(html);
    let a = find_tag(&doc, "a").expect("a should exist");
    assert_eq!(a.get_attribute("href"), Some("https://example.com".to_string()));
    assert_eq!(a.get_attribute("class"), Some("link".to_string()));
}

#[test]
fn handles_void_elements() {
    let html = r#"<html><body><p>Before<br>After</p><img src="x.png"></body></html>"#;
    let doc = parser::parse(html);
    let p = find_tag(&doc, "p").expect("p");
    let children = p.children();
    assert!(children.iter().any(|c| matches!(c.kind(), NodeKind::Element(e) if e.tag == "br")));
    let img = find_tag(&doc, "img").expect("img");
    assert_eq!(img.get_attribute("src"), Some("x.png".to_string()));
}

#[test]
fn decodes_entities() {
    let html = r#"<html><body><p>&amp;&lt;&gt;&quot;&#65;&#x42;</p></body></html>"#;
    let doc = parser::parse(html);
    let p = find_tag(&doc, "p").expect("p");
    let text = text_of_first_text_child(&p).unwrap_or_default();
    assert_eq!(text, "&<>\"AB");
}

#[test]
fn auto_closes_li() {
    let html = r#"<html><body><ul><li>One<li>Two<li>Three</ul></body></html>"#;
    let doc = parser::parse(html);
    let ul = find_tag(&doc, "ul").expect("ul");
    let lis: Vec<_> = ul
        .children()
        .into_iter()
        .filter(|c| matches!(c.kind(), NodeKind::Element(e) if e.tag == "li"))
        .collect();
    assert_eq!(lis.len(), 3);
    assert_eq!(text_of_first_text_child(&lis[0]), Some("One".to_string()));
    assert_eq!(text_of_first_text_child(&lis[2]), Some("Three".to_string()));
}

#[test]
fn parses_comments() {
    let html = r#"<html><body><!-- comment --><p>x</p></body></html>"#;
    let doc = parser::parse(html);
    let body = find_tag(&doc, "body").expect("body");
    assert!(body
        .children()
        .iter()
        .any(|c| matches!(c.kind(), NodeKind::Comment(_))));
}

#[test]
fn handles_raw_text_in_script() {
    let html = r#"<html><body><script>if (a < b && c > d) { x = 1; }</script></body></html>"#;
    let doc = parser::parse(html);
    let script = find_tag(&doc, "script").expect("script");
    let text = text_of_first_text_child(&script).unwrap_or_default();
    assert!(text.contains("if (a < b && c > d)"));
}

fn find_tag(node: &NodeRef, tag: &str) -> Option<NodeRef> {
    let mut found = None;
    Node::walk_ref(node, &mut |n| {
        if let NodeKind::Element(e) = n.kind() {
            if e.tag == tag {
                found = Some(n.clone());
                return false;
            }
        }
        true
    });
    found
}
