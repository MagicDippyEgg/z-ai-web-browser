//! Networking layer.
//!
//! Wraps `ureq` for HTTP/HTTPS, with TLS validation, redirects, cookies,
//! caching, and `data:`/`file:` URL handling on top.

pub mod http;
pub mod url;

pub use http::{HttpClient, HttpResponse, HttpError, CookieJar};
pub use url::ParsedUrl;
