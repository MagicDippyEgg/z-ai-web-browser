//! URL parsing and resolution.
//!
//! Thin wrapper around the `url` crate, with a few helpers we use elsewhere.

use url::Url;

#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub raw: String,
    pub url: Url,
}

#[derive(Debug, Clone, Default)]
pub struct UrlOrigin {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

impl ParsedUrl {
    pub fn parse(input: &str) -> Result<Self, url::ParseError> {
        let url = Url::parse(input)?;
        Ok(Self {
            raw: url.to_string(),
            url,
        })
    }

    pub fn parse_with_base(input: &str, base: &Url) -> Result<Self, url::ParseError> {
        let url = if input.starts_with("javascript:") {
            // Don't try to parse javascript: URLs
            return Err(url::ParseError::RelativeUrlWithoutBase);
        } else {
            Url::options().base_url(Some(base)).parse(input)?
        };
        Ok(Self {
            raw: url.to_string(),
            url,
        })
    }

    pub fn origin(&self) -> UrlOrigin {
        UrlOrigin {
            scheme: self.url.scheme().to_string(),
            host: self.url.host_str().unwrap_or("").to_string(),
            port: self.url.port(),
        }
    }

    pub fn scheme(&self) -> &str {
        self.url.scheme()
    }

    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or("")
    }

    pub fn is_https(&self) -> bool {
        self.url.scheme() == "https"
    }

    pub fn is_http(&self) -> bool {
        self.url.scheme() == "http"
    }

    pub fn is_data(&self) -> bool {
        self.url.scheme() == "data"
    }

    pub fn is_file(&self) -> bool {
        self.url.scheme() == "file"
    }
}

impl UrlOrigin {
    pub fn is_same_origin(&self, other: &UrlOrigin) -> bool {
        self.scheme == other.scheme && self.host == other.host && self.port == other.port
    }

    pub fn serialize(&self) -> String {
        match self.port {
            Some(p) => format!("{}://{}:{}", self.scheme, self.host, p),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }
}
