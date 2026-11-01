//! Tests for the JavaScript engine and DOM API.

use browser_core::html::parser;
use browser_core::js::runtime::JsRuntime;

fn runtime_with_doc(html: &str) -> JsRuntime {
    let doc = parser::parse(html);
    let rt = JsRuntime::new("https://example.com".to_string(), "https://example.com".to_string());
    rt.set_document(doc.clone());
    register_all(&rt, &doc);
    // Trigger document init by re-evaluating the doc init
    let _ = rt.evaluate("__document_init();");
    rt
}

fn register_all(rt: &JsRuntime, node: &browser_core::html::dom::NodeRef) {
    rt.register_node(node);
    for c in node.children() {
        register_all(rt, &c);
    }
}

#[test]
fn evaluates_arithmetic() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let result = rt.evaluate("1 + 2 * 3").unwrap_or_default();
    assert_eq!(result, "7");
}

#[test]
fn evaluates_string_concat() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let result = rt.evaluate("'Hello' + ', ' + 'world'").unwrap_or_default();
    assert_eq!(result, "Hello, world");
}

#[test]
fn evaluates_closures() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let code = r#"
        function makeCounter() {
            var n = 0;
            return function() { return ++n; };
        }
        var c = makeCounter();
        c(); c();
        c();
    "#;
    let _ = rt.evaluate(code);
    let result = rt.evaluate("c()").unwrap_or_default();
    assert_eq!(result, "4");
}

#[test]
fn evaluates_classes() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let code = r#"
        class Animal {
            constructor(name) { this.name = name; }
            speak() { return this.name + ' speaks'; }
        }
        class Dog extends Animal {
            speak() { return this.name + ' barks'; }
        }
        var d = new Dog('Rex');
        d.speak();
    "#;
    let result = rt.evaluate(code).unwrap_or_default();
    assert_eq!(result, "Rex barks");
}

#[test]
fn evaluates_promises() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let code = r#"
        var p = new Promise(function(resolve) { resolve(42); });
        var result = 0;
        p.then(function(v) { result = v; });
        result;
    "#;
    let _ = rt.evaluate(code);
    rt.pump();  // Run the .then microtask
    let result = rt.evaluate("result").unwrap_or_default();
    assert_eq!(result, "42");
}

#[test]
fn evaluates_array_methods() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let code = r#"
        [1, 2, 3, 4].map(function(x) { return x * 2; }).filter(function(x) { return x > 4; }).reduce(function(a, b) { return a + b; }, 0);
    "#;
    let result = rt.evaluate(code).unwrap_or_default();
    // map gives [2,4,6,8]; filter>4 gives [6,8]; reduce sum = 14
    assert_eq!(result, "14");
}

#[test]
fn dom_get_element_by_id() {
    let html = r#"<html><body><div id="main">Hello</div></body></html>"#;
    let rt = runtime_with_doc(html);
    let result = rt.evaluate(r#"document.getElementById('main').textContent"#).unwrap_or_default();
    assert_eq!(result, "Hello");
}

#[test]
fn dom_query_selector() {
    let html = r#"<html><body><div class="c"><p>One</p><p>Two</p></div></body></html>"#;
    let rt = runtime_with_doc(html);
    let result = rt.evaluate(r#"document.querySelector('div.c p').textContent"#).unwrap_or_default();
    assert_eq!(result, "One");
}

#[test]
fn dom_create_element() {
    let html = r#"<html><body><div id="main"></div></body></html>"#;
    let rt = runtime_with_doc(html);
    let code = r#"
        var el = document.createElement('span');
        el.textContent = 'Created';
        document.getElementById('main').appendChild(el);
        document.getElementById('main').textContent;
    "#;
    let result = rt.evaluate(code).unwrap_or_default();
    assert_eq!(result, "Created");
}

#[test]
fn dom_set_attribute() {
    let html = r#"<html><body><div id="x"></div></body></html>"#;
    let rt = runtime_with_doc(html);
    let code = r#"
        var el = document.getElementById('x');
        el.setAttribute('data-foo', 'bar');
        el.getAttribute('data-foo');
    "#;
    let result = rt.evaluate(code).unwrap_or_default();
    assert_eq!(result, "bar");
}

#[test]
fn dom_inner_html() {
    let html = r#"<html><body><div id="x"><p>Hello</p></div></body></html>"#;
    let rt = runtime_with_doc(html);
    let result = rt.evaluate(r#"document.getElementById('x').innerHTML"#).unwrap_or_default();
    assert!(result.contains("<p>Hello</p>"));
}

#[test]
fn dom_add_event_listener() {
    let html = r#"<html><body><button id="b">Click</button></body></html>"#;
    let rt = runtime_with_doc(html);
    let code = r#"
        var clicked = 0;
        var btn = document.getElementById('b');
        btn.addEventListener('click', function() { clicked++; });
        btn.click();
        btn.click();
        clicked;
    "#;
    let result = rt.evaluate(code).unwrap_or_default();
    assert_eq!(result, "2");
}

#[test]
fn set_timeout_runs_synchronously() {
    let rt = JsRuntime::new("null".to_string(), "about:blank".to_string());
    let code = r#"
        var x = 0;
        setTimeout(function() { x = 42; }, 0);
        x;
    "#;
    // setTimeout doesn't fire immediately — it's queued. After pump() it fires.
    let _ = rt.evaluate(code);
    rt.pump();
    let result = rt.evaluate("x").unwrap_or_default();
    assert_eq!(result, "42");
}
