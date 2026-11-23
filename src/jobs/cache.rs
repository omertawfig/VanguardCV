//! Caching for Job Discovery profiles and live listings.
//!
//! - **Per-CV search profiles** can be loaded from / saved to SQLite `settings`
//!   keyed by `(CvSearchSource, content_hash)` so restarting the app reuses
//!   extracted keywords with 0 AI tokens until a CV is edited.
//! - **Live job listings** remain strictly in-memory with a bounded TTL so
//!   fetched opportunities are always cleared when closing the application, and
//!   revisiting already-fetched pages requires 0 network requests.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::contracts::{CvDocument, DocumentId};
use crate::jobs::models::{CvSearchSource, JobSearchResult, PersistedProfileEntry, SearchProfile};

/// Default time-to-live for cached job search responses (10 minutes).
pub const DEFAULT_LISTINGS_TTL: Duration = Duration::from_secs(600);
/// Minimum interval between consecutive live network searches to protect
/// upstream APIs from accidental rapid-click bursts.
pub const MIN_FETCH_COOLDOWN: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone)]
struct CachedListings {
    result: JobSearchResult,
    fetched_at: Instant,
}

/// Cache for per-CV search profiles (backed by SQLite `settings` via
/// import/export helpers) and strictly in-memory paginated job listings.
#[derive(Debug)]
pub struct JobDiscoveryCache {
    /// Keyed by `CvSearchSource::cache_key_prefix()` storing the latest
    /// `content_hash` + `SearchProfile`s for that CV source.
    profiles: HashMap<String, PersistedProfileEntry>,
    /// Keyed by query + filter + page signature with a bounded TTL (in-memory only).
    listings: HashMap<String, CachedListings>,
    listings_ttl: Duration,
    last_live_fetch_at: Option<Instant>,
}

impl Default for JobDiscoveryCache {
    fn default() -> Self {
        Self::new(DEFAULT_LISTINGS_TTL)
    }
}

impl JobDiscoveryCache {
    pub fn new(listings_ttl: Duration) -> Self {
        Self {
            profiles: HashMap::new(),
            listings: HashMap::new(),
            listings_ttl,
            last_live_fetch_at: None,
        }
    }

    /// Hydrates saved per-CV search profiles from a JSON string loaded out of SQLite `settings`.
    pub fn load_persisted_profiles(&mut self, json: &str) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, PersistedProfileEntry>>(json) {
            self.profiles = map;
        }
    }

    /// Serializes all saved per-CV search profiles into compact JSON for SQLite `settings`.
    pub fn serialize_persisted_profiles(&self) -> Option<String> {
        serde_json::to_string(&self.profiles).ok()
    }

    /// Returns cached profiles ONLY if the stored `content_hash` still matches
    /// the current CV content (edits automatically cause a cache miss).
    pub fn get_profiles(
        &self,
        source: &CvSearchSource,
        content_hash: &str,
    ) -> Option<(&[SearchProfile], bool)> {
        let key = source.cache_key_prefix();
        let entry = self.profiles.get(&key)?;
        if entry.content_hash == content_hash && !entry.profiles.is_empty() {
            Some((entry.profiles.as_slice(), entry.used_fallback))
        } else {
            None
        }
    }

    /// Returns `true` if a persisted entry exists for this CV source, even if its
    /// `content_hash` is now stale due to recent edits.
    pub fn has_stale_profiles(&self, source: &CvSearchSource, content_hash: &str) -> bool {
        let key = source.cache_key_prefix();
        self.profiles
            .get(&key)
            .is_some_and(|e| e.content_hash != content_hash)
    }

    pub fn put_profiles(
        &mut self,
        source: &CvSearchSource,
        content_hash: &str,
        profiles: Vec<SearchProfile>,
        used_fallback: bool,
    ) {
        let key = source.cache_key_prefix();
        self.profiles.insert(
            key,
            PersistedProfileEntry {
                content_hash: content_hash.to_string(),
                profiles,
                used_fallback,
            },
        );
    }

    /// Removes persisted search profiles for a deleted CV (plus `AllCvs` since its set changed).
    pub fn remove_cv_profiles(&mut self, doc_id: DocumentId) {
        let single_key = CvSearchSource::Single(doc_id).cache_key_prefix();
        self.profiles.remove(&single_key);
        self.profiles
            .remove(&CvSearchSource::AllCvs.cache_key_prefix());
    }

    pub fn get_listings(&self, signature: &str) -> Option<JobSearchResult> {
        let entry = self.listings.get(signature)?;
        if entry.fetched_at.elapsed() <= self.listings_ttl {
            let mut res = entry.result.clone();
            res.from_cache = true;
            Some(res)
        } else {
            None
        }
    }

    pub fn put_listings(&mut self, signature: impl Into<String>, result: JobSearchResult) {
        self.listings.insert(
            signature.into(),
            CachedListings {
                result,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Returns remaining cooldown duration if a live network request was fired very recently.
    pub fn cooldown_remaining(&self) -> Option<Duration> {
        let last = self.last_live_fetch_at?;
        let elapsed = last.elapsed();
        if elapsed < MIN_FETCH_COOLDOWN {
            Some(MIN_FETCH_COOLDOWN - elapsed)
        } else {
            None
        }
    }

    pub fn record_live_fetch(&mut self) {
        self.last_live_fetch_at = Some(Instant::now());
    }

    /// Clears only cached job listings (used by the manual Refresh button).
    pub fn invalidate_listings(&mut self) {
        self.listings.clear();
    }

    /// Clears both profile and listing caches.
    pub fn clear_all(&mut self) {
        self.profiles.clear();
        self.listings.clear();
        self.last_live_fetch_at = None;
    }
}

/// Computes a deterministic content hash over the non-PII body of one or more
/// CVs so profile caching keys cleanly on `(source, content_hash)`.
pub fn compute_cv_content_hash(docs: &[&CvDocument]) -> String {
    let mut hasher = Sha256::new();
    let mut sorted: Vec<&CvDocument> = docs.to_vec();
    sorted.sort_by_key(|d| d.id);
    for doc in sorted {
        hasher.update(doc.id.as_bytes());
        hasher.update(b":");
        if let Ok(body_json) = serde_json::to_vec(&doc.body) {
            hasher.update(&body_json);
        }
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}
