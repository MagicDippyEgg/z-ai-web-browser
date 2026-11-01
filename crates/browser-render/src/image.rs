//! Image cache: fetches and decodes images, caches by URL.

use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct ImageCache {
    cache: Arc<RwLock<AHashMap<String, Arc<DecodedImage>>>>,
}

pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    /// RGBA8 pixels.
    pub pixels: Vec<u8>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(AHashMap::new())),
        }
    }

    pub fn get(&self, url: &str) -> Option<Arc<DecodedImage>> {
        self.cache.read().get(url).cloned()
    }

    pub fn insert(&self, url: String, image: Arc<DecodedImage>) {
        self.cache.write().insert(url, image);
    }

    /// Try to load and decode an image from a URL. Returns the decoded image
    /// or `None` on failure.
    pub fn load(&self, url: &str) -> Option<Arc<DecodedImage>> {
        if let Some(img) = self.get(url) {
            return Some(img);
        }
        // Fetch and decode synchronously.
        let client = browser_core::net::HttpClient::default();
        let resp = client.fetch(url).ok()?;
        if !resp.is_image() {
            return None;
        }
        let img = image::load_from_memory(&resp.body).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let decoded = Arc::new(DecodedImage {
            width: w as usize,
            height: h as usize,
            pixels: rgba.into_raw(),
        });
        self.insert(url.to_string(), decoded.clone());
        Some(decoded)
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}
