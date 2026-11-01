//! Tests for networking (URL parsing, data: URLs, cookies).

use browser_core::net::http::HttpClient;
use browser_core::net::url::ParsedUrl;

#[test]
fn parses_http_url() {
    let p = ParsedUrl::parse("https://example.com/path?query=1").expect("parse");
    assert_eq!(p.scheme(), "https");
    assert_eq!(p.host(), "example.com");
    assert!(p.is_https());
}

#[test]
fn resolves_relative_url() {
    let base = url::Url::parse("https://example.com/foo/bar").unwrap();
    let abs = base.join("../baz").unwrap();
    assert_eq!(abs.as_str(), "https://example.com/baz");
}

#[test]
fn fetches_data_url() {
    let client = HttpClient::default();
    let resp = client.fetch("data:text/html,<h1>Hello</h1>").expect("fetch");
    assert_eq!(resp.status, 200);
    assert!(resp.is_html());
    let body = resp.body_string().unwrap();
    assert!(body.contains("Hello"));
}

#[test]
fn fetches_data_url_base64() {
    let client = HttpClient::default();
    let resp = client
        .fetch("data:text/plain;base64,SGVsbG8sIFdvcmxkIQ==")
        .expect("fetch");
    let body = resp.body_string().unwrap();
    assert_eq!(body, "Hello, World!");
}

#[test]
fn cookie_jar_returns_empty_for_unknown_host() {
    let client = HttpClient::default();
    let cookies = client.cookies().cookies_for("https://example.com/");
    assert_eq!(cookies, "");
}

#[test]
fn same_origin_check() {
    assert!(browser_core::security::is_same_origin(
        "https://example.com/a",
        "https://example.com/b",
    ));
    assert!(!browser_core::security::is_same_origin(
        "https://example.com",
        "https://other.com",
    ));
    assert!(!browser_core::security::is_same_origin(
        "http://example.com",
        "https://example.com",
    ));
}
