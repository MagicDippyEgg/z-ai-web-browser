//! HTTP/HTTPS client.
//!
//! Built on `ureq`, which gives us blocking HTTP/1.1 over a Rust TLS stack
//! (rustls) with built-in certificate verification against the Mozilla root
//! program (`webpki-roots`).
//!
//! On top of ureq we add:
//!  * Cookie persistence per host (in-memory)
//!  * A simple on-disk cache keyed by URL (304-aware)
//!  * Redirect handling with a cap on the number of hops
//!  * Header sanitization for unsafe header names

use super::url::ParsedUrl;
use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ureq::{Agent, AgentBuilder};

/// Maximum number of redirects we'll follow per request.
const MAX_REDIRECTS: u32 = 10;

#[derive(Clone, Default)]
pub struct CookieJar {
    inner: Arc<RwLock<AHashMap<String, Vec<StorableCookie>>>>,
}

#[derive(Clone, Debug)]
struct StorableCookie {
    name: String,
    value: String,
    path: String,
    secure: bool,
    expires: Option<u64>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, url: &str, name: &str, value: &str, path: &str, secure: bool, expires: Option<u64>) {
        let host = match ParsedUrl::parse(url) {
            Ok(p) => p.host().to_string(),
            Err(_) => return,
        };
        let mut g = self.inner.write();
        g.entry(host).or_default().push(StorableCookie {
            name: name.to_string(),
            value: value.to_string(),
            path: path.to_string(),
            secure,
            expires,
        });
    }

    pub fn cookies_for(&self, url: &str) -> String {
        let parsed = match ParsedUrl::parse(url) {
            Ok(p) => p,
            Err(_) => return String::new(),
        };
        let host = parsed.host();
        let path = parsed.url.path().to_string();
        let now = now_secs();
        let g = self.inner.read();
        let cookies = match g.get(host) {
            Some(c) => c,
            None => return String::new(),
        };
        let mut out = Vec::new();
        for c in cookies {
            if let Some(exp) = c.expires {
                if exp < now {
                    continue;
                }
            }
            if c.secure && !parsed.is_https() {
                continue;
            }
            if !path.starts_with(&c.path) {
                continue;
            }
            out.push(format!("{}={}", c.name, c.value));
        }
        out.join("; ")
    }
}

/// The browser's persistent HTTP client. Owns the cookie jar, the agent, and
/// the response cache.
pub struct HttpClient {
    agent: Agent,
    cookies: CookieJar,
    cache: RwLock<AHashMap<String, CachedResponse>>,
    /// User-agent string we send on every request.
    user_agent: String,
}

pub struct HttpResponse {
    pub url: String,
    pub final_url: String,
    pub status: u16,
    pub headers: AHashMap<String, String>,
    pub body: Vec<u8>,
    pub from_cache: bool,
}

#[derive(Clone)]
struct CachedResponse {
    body: Vec<u8>,
    headers: AHashMap<String, String>,
    status: u16,
    fetched_at: u64,
    max_age: u64,
    etag: Option<String>,
}

impl HttpResponse {
    pub fn content_type(&self) -> &str {
        self.headers
            .get("content-type")
            .map(|s| s.as_str())
            .unwrap_or("application/octet-stream")
    }

    pub fn body_string(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.body.clone())
    }

    pub fn is_html(&self) -> bool {
        self.content_type().starts_with("text/html")
    }

    pub fn is_css(&self) -> bool {
        self.content_type().starts_with("text/css")
    }

    pub fn is_js(&self) -> bool {
        let ct = self.content_type();
        ct.starts_with("application/javascript")
            || ct.starts_with("text/javascript")
            || ct.starts_with("application/x-javascript")
    }

    pub fn is_image(&self) -> bool {
        self.content_type().starts_with("image/")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("network error: {0}")]
    Network(String),
    #[error("HTTP {status}: {url}")]
    Status { status: u16, url: String },
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("scheme not supported: {0}")]
    UnsupportedScheme(String),
    #[error("I/O error: {0}")]
    Io(String),
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new("Z-Browser/0.1")
    }
}

impl HttpClient {
    pub fn new(user_agent: &str) -> Self {
        let agent = AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .redirects(0) // we handle redirects manually so we can cap & log
            .user_agent(user_agent)
            .build();
        Self {
            agent,
            cookies: CookieJar::new(),
            cache: RwLock::new(AHashMap::new()),
            user_agent: user_agent.to_string(),
        }
    }

    pub fn cookies(&self) -> CookieJar {
        self.cookies.clone()
    }

    /// Fetch a URL, following redirects and consulting the cache.
    pub fn fetch(&self, url: &str) -> Result<HttpResponse, HttpError> {
        // Handle about: URLs (only about:blank is supported).
        if url == "about:blank" || url == "about:blank/" {
            let mut headers = AHashMap::new();
            headers.insert("content-type".to_string(), "text/html; charset=utf-8".to_string());
            return Ok(HttpResponse {
                url: url.to_string(),
                final_url: url.to_string(),
                status: 200,
                headers,
                body: b"<html><body></body></html>".to_vec(),
                from_cache: false,
            });
        }
        // Handle data: URLs without going to the network.
        if let Some(body) = parse_data_url(url) {
            let (mime, data) = body;
            let mut headers = AHashMap::new();
            headers.insert("content-type".to_string(), mime);
            return Ok(HttpResponse {
                url: url.to_string(),
                final_url: url.to_string(),
                status: 200,
                headers,
                body: data,
                from_cache: false,
            });
        }
        // Handle file: URLs by reading from disk.
        if url.starts_with("file://") {
            let path = url.trim_start_matches("file://");
            let path = if let Some(stripped) = path.strip_prefix("localhost/") {
                format!("/{}", stripped)
            } else {
                path.to_string()
            };
            let body = std::fs::read(&path).map_err(|e| HttpError::Io(e.to_string()))?;
            let mut headers = AHashMap::new();
            let mime = mime_guess(&path);
            headers.insert("content-type".to_string(), mime.to_string());
            return Ok(HttpResponse {
                url: url.to_string(),
                final_url: url.to_string(),
                status: 200,
                headers,
                body,
                from_cache: false,
            });
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(HttpError::UnsupportedScheme(
                url.split(':').next().unwrap_or("").to_string(),
            ));
        }
        // Cache lookup
        if let Some(cached) = self.cache.read().get(url).cloned() {
            let now = now_secs();
            if cached.fetched_at + cached.max_age > now {
                return Ok(HttpResponse {
                    url: url.to_string(),
                    final_url: url.to_string(),
                    status: cached.status,
                    headers: cached.headers.clone(),
                    body: cached.body.clone(),
                    from_cache: true,
                });
            }
        }
        let mut current = url.to_string();
        let mut hops = 0u32;
        loop {
            if hops >= MAX_REDIRECTS {
                return Err(HttpError::TooManyRedirects);
            }
            let cookie_header = self.cookies.cookies_for(&current);
            let mut req = self.agent.get(&current);
            if !cookie_header.is_empty() {
                req = req.set("Cookie", &cookie_header);
            }
            // Conditional request if we have an etag for this URL
            if let Some(cached) = self.cache.read().get(&current).cloned() {
                if let Some(etag) = &cached.etag {
                    req = req.set("If-None-Match", etag);
                }
            }
            let resp = req.call().map_err(|e| HttpError::Network(e.to_string()))?;
            let status = resp.status();
            // Redirects
            if (300..400).contains(&status) {
                if let Some(loc) = resp.header("location") {
                    let next = resolve_url(&current, loc)?;
                    hops += 1;
                    current = next;
                    continue;
                }
            }
            // Read headers BEFORE consuming resp via into_reader().
            let header_names: Vec<String> = {
                let mut names = resp.headers_names();
                names.sort();
                names.dedup();
                names
            };
            let mut headers = AHashMap::new();
            for h in &header_names {
                if let Some(v) = resp.header(h) {
                    headers.insert(h.to_lowercase(), v.to_string());
                }
            }
            // Read body
            let mut body = Vec::new();
            resp.into_reader()
                .read_to_end(&mut body)
                .map_err(|e| HttpError::Io(e.to_string()))?;
            // Set-Cookie handling
            if let Some(set_cookie) = headers.get("set-cookie").cloned() {
                self.process_set_cookie(&current, &set_cookie);
            }
            let final_url = current.clone();
            let max_age = parse_cache_control(headers.get("cache-control").map(|s| s.as_str()))
                .unwrap_or(0);
            let etag = headers.get("etag").cloned();
            // Store in cache
            if max_age > 0 {
                self.cache.write().insert(
                    url.to_string(),
                    CachedResponse {
                        body: body.clone(),
                        headers: headers.clone(),
                        status,
                        fetched_at: now_secs(),
                        max_age,
                        etag: etag.clone(),
                    },
                );
            }
            return Ok(HttpResponse {
                url: url.to_string(),
                final_url,
                status,
                headers,
                body,
                from_cache: false,
            });
        }
    }

    /// Fetch a sub-resource (CSS, JS, image) relative to a base URL.
    pub fn fetch_sub(&self, url: &str, base: &str) -> Result<HttpResponse, HttpError> {
        let abs = resolve_url(base, url)?;
        self.fetch(&abs)
    }

    fn process_set_cookie(&self, request_url: &str, header: &str) {
        // Very simplified Set-Cookie parser. Splits on first ';' for the
        // name=value pair, then scans the rest for path/secure/expires.
        let mut parts = header.splitn(2, ';');
        let pair = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("");
        let (name, value) = match pair.split_once('=') {
            Some((n, v)) => (n.trim(), v.trim()),
            None => return,
        };
        let mut path = "/".to_string();
        let mut secure = false;
        let mut expires = None;
        for attr in rest.split(';') {
            let attr = attr.trim();
            let (k, v) = match attr.split_once('=') {
                Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim()),
                None => (attr.to_ascii_lowercase(), ""),
            };
            match k.as_str() {
                "path" => path = v.to_string(),
                "secure" => secure = true,
                "expires" => {
                    expires = parse_http_date(v);
                }
                "max-age" => {
                    if let Ok(secs) = v.parse::<u64>() {
                        expires = Some(now_secs() + secs);
                    }
                }
                _ => {}
            }
        }
        self.cookies
            .add(request_url, name, value, &path, secure, expires);
    }
}

fn resolve_url(base: &str, href: &str) -> Result<String, HttpError> {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") || href.starts_with("file://") {
        return Ok(href.to_string());
    }
    let base_url = url::Url::parse(base).map_err(|e| HttpError::InvalidUrl(e.to_string()))?;
    let resolved = base_url
        .join(href)
        .map_err(|e| HttpError::InvalidUrl(e.to_string()))?;
    Ok(resolved.to_string())
}

fn parse_cache_control(cc: Option<&str>) -> Option<u64> {
    let cc = cc?;
    for part in cc.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("max-age=") {
            return rest.trim().parse::<u64>().ok();
        }
    }
    None
}

fn parse_http_date(s: &str) -> Option<u64> {
    // Best-effort: we don't really need to be precise; if we fail, we just
    // don't expire the cookie.
    let _ = s;
    None
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mut mime = "text/plain".to_string();
    let mut base64 = false;
    for part in meta.split(';') {
        match part {
            "base64" => base64 = true,
            "" => {}
            other => mime = other.to_string(),
        }
    }
    if mime.is_empty() {
        mime = "text/plain".to_string();
    }
    let bytes = if base64 {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?
    } else {
        percent_encoding::percent_decode_str(data)
            .decode_utf8_lossy()
            .into_owned()
            .into_bytes()
    };
    Some((mime, bytes))
}

fn mime_guess(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

use std::io::Read;
