//! Debounced saving and Root↔Child promotion helpers.
//!
//! [`DebouncedSaver`] accumulates rapid document mutations (e.g. every
//! keystroke in the editor) and writes the latest version of each modified
//! document to the repository only after a configurable quiet period.  It is
//! designed to be held inside an `Arc` so it can be shared between the UI
//! thread and a background save task.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;

use crate::contracts::{CvDocument, CvRepository, DocumentId};

// ---------------------------------------------------------------------------
// DebouncedSaver
// ---------------------------------------------------------------------------

/// Internal state protected by a mutex.
struct SaverState {
    /// Latest pending version per document ID waiting to be flushed.
    pending: HashMap<DocumentId, CvDocument>,
}

/// Accumulates document mutations and flushes them to the repository after
/// `delay` milliseconds of inactivity.
///
/// Tracks pending edits per [`DocumentId`] so switching documents before a
/// flush never drops unsaved edits to the previous document.
pub struct DebouncedSaver {
    repo: Arc<dyn CvRepository>,
    delay: Duration,
    state: Mutex<SaverState>,
}

impl DebouncedSaver {
    /// Creates a new `DebouncedSaver` backed by `repo` with the given debounce
    /// window.
    pub fn new(repo: Arc<dyn CvRepository>, delay: Duration) -> Self {
        Self {
            repo,
            delay,
            state: Mutex::new(SaverState {
                pending: HashMap::new(),
            }),
        }
    }

    /// Schedules `doc` to be saved after the debounce period.
    ///
    /// If called repeatedly for the same document before the delay expires,
    /// only the **last** document version will be written. Pending edits for
    /// different documents are preserved independently.
    pub fn schedule(&self, doc: CvDocument) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pending.insert(doc.id, doc);
    }

    /// Immediately writes all pending documents to the repository.
    /// Returns `Ok(false)` if there was nothing to flush, `Ok(true)` if at
    /// least one document was written.
    ///
    /// If saving any document fails, it is re-queued (unless a newer version
    /// was scheduled while the save was in flight) and the error is returned.
    pub fn flush(&self) -> Result<bool> {
        let docs: Vec<CvDocument> = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.pending.is_empty() {
                return Ok(false);
            }
            state.pending.drain().map(|(_, doc)| doc).collect()
        };

        let mut saved_any = false;
        let mut first_error: Option<anyhow::Error> = None;

        for doc in docs {
            match self.repo.save(&doc) {
                Ok(()) => {
                    saved_any = true;
                }
                Err(err) => {
                    // Re-queue failed document unless a newer edit arrived concurrently.
                    let mut state = self
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    state.pending.entry(doc.id).or_insert(doc);
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }
        Ok(saved_any)
    }

    /// Returns the debounce delay.
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// Returns `true` if there is at least one unsaved document pending.
    pub fn is_dirty(&self) -> bool {
        self.state
            .lock()
            .map(|s| !s.pending.is_empty())
            .unwrap_or(false)
    }

    /// Clears all pending documents without saving them. Use with care.
    pub fn discard(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{CvRepository, PiiContactHeader, SanitizedCvBody};
    use crate::db::repository::SqliteRepository;

    fn make_saver() -> (Arc<SqliteRepository>, DebouncedSaver) {
        let repo = Arc::new(SqliteRepository::in_memory().unwrap());
        let saver = DebouncedSaver::new(repo.clone(), Duration::from_millis(100));
        (repo, saver)
    }

    #[test]
    fn flush_nothing_returns_false() {
        let (_, saver) = make_saver();
        assert!(!saver.flush().unwrap());
    }

    #[test]
    fn schedule_then_flush_saves_document() {
        let (repo, saver) = make_saver();
        let doc = repo
            .create_root(
                "Test",
                PiiContactHeader::default(),
                SanitizedCvBody::default(),
            )
            .unwrap();

        // Schedule an update
        let mut updated = doc.clone();
        updated.title = "Updated Title".to_string();
        saver.schedule(updated);
        assert!(saver.is_dirty());

        saver.flush().unwrap();
        assert!(!saver.is_dirty());

        let loaded = repo.load(doc.id).unwrap();
        assert_eq!(loaded.title, "Updated Title");
    }

    #[test]
    fn schedule_multiple_keeps_latest() {
        let (repo, saver) = make_saver();
        let doc = repo
            .create_root(
                "Base",
                PiiContactHeader::default(),
                SanitizedCvBody::default(),
            )
            .unwrap();

        for i in 0..5u32 {
            let mut d = doc.clone();
            d.title = format!("Title {i}");
            saver.schedule(d);
        }

        saver.flush().unwrap();
        let loaded = repo.load(doc.id).unwrap();
        assert_eq!(loaded.title, "Title 4");
    }

    #[test]
    fn schedule_multiple_documents_preserves_all() {
        let (repo, saver) = make_saver();
        let doc_a = repo
            .create_root(
                "Doc A",
                PiiContactHeader::default(),
                SanitizedCvBody::default(),
            )
            .unwrap();
        let doc_b = repo
            .create_root(
                "Doc B",
                PiiContactHeader::default(),
                SanitizedCvBody::default(),
            )
            .unwrap();

        let mut upd_a = doc_a.clone();
        upd_a.title = "Doc A Edited".to_string();
        let mut upd_b = doc_b.clone();
        upd_b.title = "Doc B Edited".to_string();

        saver.schedule(upd_a);
        saver.schedule(upd_b);
        assert!(saver.is_dirty());

        assert!(saver.flush().unwrap());
        assert!(!saver.is_dirty());

        assert_eq!(repo.load(doc_a.id).unwrap().title, "Doc A Edited");
        assert_eq!(repo.load(doc_b.id).unwrap().title, "Doc B Edited");
    }

    #[test]
    fn discard_clears_pending() {
        let (repo, saver) = make_saver();
        let doc = repo
            .create_root(
                "Discard Me",
                PiiContactHeader::default(),
                SanitizedCvBody::default(),
            )
            .unwrap();
        saver.schedule(doc);
        assert!(saver.is_dirty());
        saver.discard();
        assert!(!saver.is_dirty());
        assert!(!saver.flush().unwrap());
    }
}
