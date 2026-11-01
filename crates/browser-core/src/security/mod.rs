//! Security: same-origin policy, CORS, and content isolation helpers.

use crate::net::url::ParsedUrl;

/// Two URLs are "same origin" if they share scheme, host, and port.
pub fn is_same_origin(a: &str, b: &str) -> bool {
    let pa = match ParsedUrl::parse(a) {
        Ok(p) => p.origin(),
        Err(_) => return false,
    };
    let pb = match ParsedUrl::parse(b) {
        Ok(p) => p.origin(),
        Err(_) => return false,
    };
    pa.is_same_origin(&pb)
}

/// Check whether a fetch from `origin` to `target` is a "simple" cross-origin
/// request (no preflight required).
pub fn is_simple_request(method: &str, headers: &[(String, String)]) -> bool {
    let method_ok = matches!(method.to_ascii_uppercase().as_str(), "GET" | "HEAD" | "POST");
    if !method_ok {
        return false;
    }
    let allowed_headers = [
        "accept",
        "accept-language",
        "content-language",
        "content-type",
        "range",
    ];
    for (name, _) in headers {
        if !allowed_headers.contains(&name.to_ascii_lowercase().as_str()) {
            return false;
        }
    }
    true
}

/// Returns true if `target` is allowed to be loaded from `origin` without
/// CORS checks. Same-origin requests, `data:` URLs, and `file:` URLs are
/// always allowed.
pub fn is_cors_exempt(origin: &str, target: &str) -> bool {
    if target.starts_with("data:") || target.starts_with("file:") || target.starts_with("blob:") {
        return true;
    }
    is_same_origin(origin, target)
}

/// Returns true if the script at `script_origin` is allowed to access the
/// DOM of `page_origin`. Always true for same-origin; we never allow
/// cross-origin DOM access.
pub fn can_access_dom(page_origin: &str, script_origin: &str) -> bool {
    is_same_origin(page_origin, script_origin)
}
