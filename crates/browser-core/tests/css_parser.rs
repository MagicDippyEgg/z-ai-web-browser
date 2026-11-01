//! Tests for the CSS engine.

use browser_core::css::{parse, Value};
use browser_core::css::selector::Selector;

#[test]
fn parses_simple_rule() {
    let sheet = parse("p { color: red; }");
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn parses_multiple_selectors() {
    let sheet = parse("h1, h2, h3 { font-weight: bold; }");
    if let Some(browser_core::css::Rule::Style(s)) = sheet.rules.first() {
        assert_eq!(s.selectors.len(), 3);
    } else {
        panic!("expected style rule");
    }
}

#[test]
fn parses_colors() {
    let sheet = parse("p { color: #ff0000; }");
    if let Some(browser_core::css::Rule::Style(s)) = sheet.rules.first() {
        let d = &s.declarations[0];
        assert_eq!(d.property, "color");
        match d.value {
            Value::Color(c) => assert_eq!(c, 0xFFFF0000),
            _ => panic!("expected color"),
        }
    }
}

#[test]
fn parses_rgb_function() {
    let sheet = parse("p { color: rgb(0, 128, 255); }");
    if let Some(browser_core::css::Rule::Style(s)) = sheet.rules.first() {
        let d = &s.declarations[0];
        match d.value {
            Value::Color(c) => assert_eq!(c, 0xFF0080FF),
            _ => panic!("expected color"),
        }
    }
}

#[test]
fn parses_important() {
    let sheet = parse("p { color: red !important; }");
    if let Some(browser_core::css::Rule::Style(s)) = sheet.rules.first() {
        let d = &s.declarations[0];
        assert!(d.important);
    }
}

#[test]
fn parses_length_values() {
    let sheet = parse("p { margin: 10px 20px 30px 40px; }");
    if let Some(browser_core::css::Rule::Style(s)) = sheet.rules.first() {
        let d = &s.declarations[0];
        match &d.value {
            Value::List(items) => assert_eq!(items.len(), 4),
            _ => panic!("expected list"),
        }
    }
}

#[test]
fn parses_media_query() {
    let sheet = parse("@media (max-width: 600px) { p { font-size: 14px; } }");
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn parses_type_selector() {
    let sel = Selector::parse("div").expect("parse");
    assert_eq!(sel.specificity, (0, 0, 1));
}

#[test]
fn parses_class_selector() {
    let sel = Selector::parse(".foo").expect("parse");
    assert_eq!(sel.specificity, (0, 1, 0));
}

#[test]
fn parses_id_selector() {
    let sel = Selector::parse("#bar").expect("parse");
    assert_eq!(sel.specificity, (1, 0, 0));
}

#[test]
fn parses_compound_selector() {
    let sel = Selector::parse("div.foo#bar").expect("parse");
    assert_eq!(sel.specificity, (1, 1, 1));
}

#[test]
fn parses_descendant_selector() {
    let sel = Selector::parse("div p span").expect("parse");
    assert_eq!(sel.specificity, (0, 0, 3));
}

#[test]
fn parses_child_selector() {
    let sel = Selector::parse("div > p").expect("parse");
    assert_eq!(sel.specificity, (0, 0, 2));
}

#[test]
fn parses_attribute_selector() {
    let sel = Selector::parse("[type=text]").expect("parse");
    assert_eq!(sel.specificity, (0, 1, 0));
}

#[test]
fn parses_pseudo_class() {
    let sel = Selector::parse("a:hover").expect("parse");
    assert_eq!(sel.specificity, (0, 1, 1));
}
