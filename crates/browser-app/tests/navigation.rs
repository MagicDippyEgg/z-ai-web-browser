//! Tests for navigation and tab management.

use browser_core::html::parser;
use browser_core::net::HttpClient;
use browser_core::Page;
use std::sync::Arc;

#[test]
fn navigates_to_data_url() {
    let http = Arc::new(HttpClient::default());
    let page = Page::new(http, 800.0, 600.0);
    let url = "data:text/html,<html><body><p id=\"x\">Hello</p></body></html>";
    page.navigate(url).expect("navigate");
    let doc = page.document().expect("doc");
    let p = browser_core::html::dom::Node::get_element_by_id(&doc, "x").expect("p");
    assert_eq!(p.text_content(), "Hello");
}

#[test]
fn runs_inline_script() {
    let http = Arc::new(HttpClient::default());
    let page = Page::new(http, 800.0, 600.0);
    let html = r#"data:text/html,
        <html><body>
            <div id="x"></div>
            <script>
                document.getElementById('x').textContent = 'Set by JS';
            </script>
        </body></html>
    "#;
    page.navigate(html).expect("navigate");
    let doc = page.document().expect("doc");
    let x = browser_core::html::dom::Node::get_element_by_id(&doc, "x").expect("x");
    assert_eq!(x.text_content(), "Set by JS");
}

#[test]
fn dispatches_domcontentloaded() {
    let http = Arc::new(HttpClient::default());
    let page = Page::new(http, 800.0, 600.0);
    let html = r#"data:text/html,
        <html><body>
            <div id="x">before</div>
            <script>
                document.addEventListener('DOMContentLoaded', function() {
                    document.getElementById('x').textContent = 'after';
                });
            </script>
        </body></html>
    "#;
    page.navigate(html).expect("navigate");
    let doc = page.document().expect("doc");
    let x = browser_core::html::dom::Node::get_element_by_id(&doc, "x").expect("x");
    assert_eq!(x.text_content(), "after");
}

#[test]
fn multiple_pages_independent() {
    let http = Arc::new(HttpClient::default());
    let page1 = Page::new(http.clone(), 800.0, 600.0);
    let page2 = Page::new(http, 800.0, 600.0);
    page1
        .navigate("data:text/html,<html><body><p>A</p></body></html>")
        .expect("nav1");
    page2
        .navigate("data:text/html,<html><body><p>B</p></body></html>")
        .expect("nav2");
    let doc1 = page1.document().expect("doc1");
    let doc2 = page2.document().expect("doc2");
    let p1 = browser_core::html::dom::Node::get_elements_by_tag_name(&doc1, "p")[0].clone();
    let p2 = browser_core::html::dom::Node::get_elements_by_tag_name(&doc2, "p")[0].clone();
    assert_eq!(p1.text_content(), "A");
    assert_eq!(p2.text_content(), "B");
}
