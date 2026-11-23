//! Pre-AI snapshots and one-click rollback workflow helpers.
//!
//! Provides high-level convenience helpers on top of [`CvRepository`] for
//! automatically taking snapshots before AI operations, reverting to the
//! latest snapshot, and querying recent snapshots.

use anyhow::Result;

use crate::contracts::{CvDocument, CvRepository, DocumentId, SnapshotSummary};

/// Convenience re-export so callers can `use crate::db::snapshots::*;` without
/// importing from the repository module directly.
pub use crate::contracts::SnapshotSummary as Snapshot;

/// Captures a snapshot of the document before an AI tailoring or rewrite pass.
///
/// If `reason` is empty, defaults to `"pre-ai snapshot"`.
pub fn snapshot_before_ai(
    repo: &dyn CvRepository,
    cv_id: DocumentId,
    reason: &str,
) -> Result<SnapshotSummary> {
    let reason_str = if reason.trim().is_empty() {
        "pre-ai snapshot"
    } else {
        reason.trim()
    };
    repo.create_snapshot(cv_id, reason_str)
}

/// Restores the document to its most recently created snapshot.
///
/// Returns an error if no snapshots exist for the given document.
pub fn restore_latest(repo: &dyn CvRepository, cv_id: DocumentId) -> Result<CvDocument> {
    let snapshots = repo.list_snapshots(cv_id)?;
    let latest = snapshots
        .into_iter()
        .max_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.version.cmp(&b.version))
        })
        .ok_or_else(|| anyhow::anyhow!("no snapshots found for cv {cv_id}"))?;
    repo.restore_snapshot(latest.id)
}

/// Returns the most recent snapshots for a document, ordered newest first,
/// capped at `limit`.
pub fn list_recent(
    repo: &dyn CvRepository,
    cv_id: DocumentId,
    limit: usize,
) -> Result<Vec<SnapshotSummary>> {
    let mut snapshots = repo.list_snapshots(cv_id)?;
    snapshots.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.version.cmp(&a.version))
    });
    snapshots.truncate(limit);
    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{PiiContactHeader, SanitizedCvBody, SectionKind};
    use crate::db::repository::SqliteRepository;

    fn make_test_doc(repo: &SqliteRepository) -> CvDocument {
        let pii = PiiContactHeader {
            full_name: "Test Engineer".to_string(),
            ..Default::default()
        };
        let body = SanitizedCvBody {
            sections: vec![crate::contracts::CvSection {
                id: uuid::Uuid::new_v4(),
                origin_item_id: None,
                title: "SUMMARY".to_string(),
                enabled: true,
                kind: SectionKind::Summary {
                    text: "Initial summary text".to_string(),
                },
            }],
        };
        repo.create_root("Root CV", pii, body).expect("create_root")
    }

    #[test]
    fn test_snapshot_before_ai_creates_snapshot() {
        let repo = SqliteRepository::in_memory().expect("in_memory repo");
        let doc = make_test_doc(&repo);

        let snap = snapshot_before_ai(&repo, doc.id, "before skill execution").expect("snapshot");
        assert_eq!(snap.cv_id, doc.id);
        assert_eq!(snap.reason, "before skill execution");
        assert_eq!(snap.version, 1);

        // Blank reason uses default
        let snap2 = snapshot_before_ai(&repo, doc.id, "   ").expect("snapshot default");
        assert_eq!(snap2.reason, "pre-ai snapshot");
    }

    #[test]
    fn test_restore_latest_reverts_body() {
        let repo = SqliteRepository::in_memory().expect("in_memory repo");
        let mut doc = make_test_doc(&repo);

        // Snapshot original
        let snap = snapshot_before_ai(&repo, doc.id, "pre-ai pass").expect("snapshot");

        // Mutate document body
        doc.body.sections[0].kind = SectionKind::Summary {
            text: "Mutated summary by AI".to_string(),
        };
        repo.save(&doc).expect("save mutated");

        let loaded = repo.load(doc.id).expect("load");
        match &loaded.body.sections[0].kind {
            SectionKind::Summary { text } => assert_eq!(text, "Mutated summary by AI"),
            _ => panic!("unexpected section kind"),
        }

        // Restore latest snapshot
        let restored = restore_latest(&repo, doc.id).expect("restore");
        match &restored.body.sections[0].kind {
            SectionKind::Summary { text } => assert_eq!(text, "Initial summary text"),
            _ => panic!("unexpected section kind"),
        }
        // Restored doc has bumped version
        assert!(restored.version > snap.version);
    }

    #[test]
    fn test_restore_latest_errors_when_no_snapshots() {
        let repo = SqliteRepository::in_memory().expect("in_memory repo");
        let doc = make_test_doc(&repo);

        let err = restore_latest(&repo, doc.id);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("no snapshots found"));
    }

    #[test]
    fn test_list_recent_limits_and_orders_newest_first() {
        let repo = SqliteRepository::in_memory().expect("in_memory repo");
        let doc = make_test_doc(&repo);

        let s1 = snapshot_before_ai(&repo, doc.id, "snap 1").expect("s1");
        let s2 = snapshot_before_ai(&repo, doc.id, "snap 2").expect("s2");
        let s3 = snapshot_before_ai(&repo, doc.id, "snap 3").expect("s3");

        let recent = list_recent(&repo, doc.id, 2).expect("list_recent 2");
        assert_eq!(recent.len(), 2);
        // Newest first
        assert_eq!(recent[0].id, s3.id);
        assert_eq!(recent[1].id, s2.id);

        let all = list_recent(&repo, doc.id, 10).expect("list_recent 10");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, s3.id);
        assert_eq!(all[1].id, s2.id);
        assert_eq!(all[2].id, s1.id);

        let none = list_recent(&repo, doc.id, 0).expect("list_recent 0");
        assert!(none.is_empty());
    }
}
