//! Incremental compilation cache for raster previews.
//!
//! The key is a content hash of the *entire* rendered input (PII header,
//! sanitized body, and page limit), so a cached page list can never be shown
//! for a different document revision.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use crate::contracts::{PiiContactHeader, RenderedPage, SanitizedCvBody};

/// Content-addressed cache of rasterized preview pages.
#[derive(Default)]
pub struct PreviewCache {
    entries: Mutex<HashMap<u64, Vec<RenderedPage>>>,
}

impl PreviewCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes the content key for a preview request.
    pub fn key(
        pii: &PiiContactHeader,
        body: &SanitizedCvBody,
        max_pages: usize,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        max_pages.hash(&mut hasher);
        if let Ok(bytes) = serde_json::to_vec(pii) {
            hasher.write(&bytes);
        }
        if let Ok(bytes) = serde_json::to_vec(body) {
            hasher.write(&bytes);
        }
        hasher.finish()
    }

    /// Returns cached pages for `key`, if present.
    pub fn get(&self, key: u64) -> Option<Vec<RenderedPage>> {
        self.entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(&key).cloned())
    }

    /// Stores pages for `key`.
    pub fn insert(&self, key: u64, pages: Vec<RenderedPage>) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(key, pages);
        }
    }

    /// Removes every cached entry.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or_default()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::sample_root_document;

    #[test]
    fn keys_change_when_content_changes() {
        let doc = sample_root_document();
        let key = PreviewCache::key(&doc.pii_header, &doc.body, 3);
        assert_eq!(key, PreviewCache::key(&doc.pii_header, &doc.body, 3));
        assert_ne!(key, PreviewCache::key(&doc.pii_header, &doc.body, 4));

        let mut changed = doc.clone();
        changed.body.sections[0].title.push('!');
        assert_ne!(key, PreviewCache::key(&changed.pii_header, &changed.body, 3));
    }

    #[test]
    fn insert_get_clear() {
        let cache = PreviewCache::new();
        assert!(cache.is_empty());
        cache.insert(7, Vec::new());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(7), Some(Vec::new()));
        cache.clear();
        assert!(cache.get(7).is_none());
    }
}
