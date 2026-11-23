//! Word-level inline diff engine built on the `similar` crate.
//!
//! Provides inline word-level diffing between original CV text and AI-proposed
//! rewrites or parent/child variations.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use crate::contracts::{AiProposal, ItemId};

/// The semantic operation of a diff span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffOp {
    Equal,
    Insert,
    Delete,
}

/// A contiguous segment of text within an inline diff.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum DiffSpan {
    /// Text present in both original and proposed content without changes.
    Unchanged(String),
    /// Text newly added in the proposed content.
    Added(String),
    /// Text removed from the original content.
    Removed(String),
}

impl DiffSpan {
    pub fn unchanged(text: impl Into<String>) -> Self {
        Self::Unchanged(text.into())
    }

    pub fn added(text: impl Into<String>) -> Self {
        Self::Added(text.into())
    }

    pub fn removed(text: impl Into<String>) -> Self {
        Self::Removed(text.into())
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Unchanged(s) | Self::Added(s) | Self::Removed(s) => s,
        }
    }

    pub fn op(&self) -> DiffOp {
        match self {
            Self::Unchanged(_) => DiffOp::Equal,
            Self::Added(_) => DiffOp::Insert,
            Self::Removed(_) => DiffOp::Delete,
        }
    }

    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added(_))
    }

    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed(_))
    }

    pub fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged(_))
    }
}

/// Summary counts of diff operations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStats {
    pub additions: usize,
    pub deletions: usize,
    pub unchanged: usize,
}

/// Inline word-level diff for a single bullet point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BulletDiff {
    pub bullet_id: ItemId,
    pub original: String,
    pub proposed: String,
    pub spans: Vec<DiffSpan>,
}

impl BulletDiff {
    pub fn has_changes(&self) -> bool {
        has_changes(&self.spans)
    }

    pub fn stats(&self) -> DiffStats {
        diff_stats(&self.spans)
    }
}

/// Inline word-level diff for a section summary paragraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryDiff {
    pub section_id: ItemId,
    pub original: String,
    pub proposed: String,
    pub spans: Vec<DiffSpan>,
}

impl SummaryDiff {
    pub fn has_changes(&self) -> bool {
        has_changes(&self.spans)
    }

    pub fn stats(&self) -> DiffStats {
        diff_stats(&self.spans)
    }
}

/// Unified container for an item diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemDiff {
    Bullet(BulletDiff),
    Summary(SummaryDiff),
}

impl ItemDiff {
    pub fn target_item_id(&self) -> ItemId {
        match self {
            Self::Bullet(b) => b.bullet_id,
            Self::Summary(s) => s.section_id,
        }
    }

    pub fn spans(&self) -> &[DiffSpan] {
        match self {
            Self::Bullet(b) => &b.spans,
            Self::Summary(s) => &s.spans,
        }
    }

    pub fn has_changes(&self) -> bool {
        has_changes(self.spans())
    }

    pub fn stats(&self) -> DiffStats {
        diff_stats(self.spans())
    }
}

/// Computes word-level diff spans between an original string and a proposed string.
///
/// Adjacent changes with the same operation are merged together to keep the
/// resulting span list compact and easy to render in GPUI or terminal views.
pub fn diff_words(original: &str, proposed: &str) -> Vec<DiffSpan> {
    if original.is_empty() && proposed.is_empty() {
        return Vec::new();
    }
    if original.is_empty() {
        return vec![DiffSpan::Added(proposed.to_string())];
    }
    if proposed.is_empty() {
        return vec![DiffSpan::Removed(original.to_string())];
    }
    if original == proposed {
        return vec![DiffSpan::Unchanged(original.to_string())];
    }

    let diff = TextDiff::from_words(original, proposed);
    let mut spans: Vec<DiffSpan> = Vec::new();

    for change in diff.iter_all_changes() {
        let tag = change.tag();
        let value = change.value();

        match (spans.last_mut(), tag) {
            (Some(DiffSpan::Unchanged(s)), ChangeTag::Equal) => {
                s.push_str(value);
            }
            (Some(DiffSpan::Added(s)), ChangeTag::Insert) => {
                s.push_str(value);
            }
            (Some(DiffSpan::Removed(s)), ChangeTag::Delete) => {
                s.push_str(value);
            }
            (_, ChangeTag::Equal) => {
                spans.push(DiffSpan::Unchanged(value.to_string()));
            }
            (_, ChangeTag::Insert) => {
                spans.push(DiffSpan::Added(value.to_string()));
            }
            (_, ChangeTag::Delete) => {
                spans.push(DiffSpan::Removed(value.to_string()));
            }
        }
    }

    spans
}

/// Produces an inline word diff for a bullet rewrite.
pub fn diff_bullet(bullet_id: ItemId, original: &str, proposed: &str) -> BulletDiff {
    BulletDiff {
        bullet_id,
        original: original.to_string(),
        proposed: proposed.to_string(),
        spans: diff_words(original, proposed),
    }
}

/// Produces an inline word diff for a summary rewrite.
pub fn diff_summary(section_id: ItemId, original: &str, proposed: &str) -> SummaryDiff {
    SummaryDiff {
        section_id,
        original: original.to_string(),
        proposed: proposed.to_string(),
        spans: diff_words(original, proposed),
    }
}

/// Computes an [`ItemDiff`] from an [`AiProposal`] if the proposal is a bullet or summary rewrite.
pub fn diff_proposal(proposal: &AiProposal) -> Option<ItemDiff> {
    match proposal {
        AiProposal::BulletRewrite {
            bullet_id,
            original,
            proposed,
            ..
        } => Some(ItemDiff::Bullet(diff_bullet(
            *bullet_id, original, proposed,
        ))),
        AiProposal::SummaryRewrite {
            section_id,
            original,
            proposed,
        } => Some(ItemDiff::Summary(diff_summary(
            *section_id,
            original,
            proposed,
        ))),
        AiProposal::ToggleVisibility { .. } | AiProposal::SkillReorder { .. } => None,
    }
}

/// Returns `true` if any span is added or removed.
pub fn has_changes(spans: &[DiffSpan]) -> bool {
    spans.iter().any(|s| !s.is_unchanged())
}

/// Computes summary statistics over diff spans.
pub fn diff_stats(spans: &[DiffSpan]) -> DiffStats {
    let mut stats = DiffStats::default();
    for span in spans {
        match span {
            DiffSpan::Added(_) => stats.additions += 1,
            DiffSpan::Removed(_) => stats.deletions += 1,
            DiffSpan::Unchanged(_) => stats.unchanged += 1,
        }
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_empty_strings() {
        assert_eq!(diff_words("", ""), vec![]);

        let added = diff_words("", "New content");
        assert_eq!(added, vec![DiffSpan::Added("New content".to_string())]);

        let removed = diff_words("Old content", "");
        assert_eq!(removed, vec![DiffSpan::Removed("Old content".to_string())]);
    }

    #[test]
    fn test_identical_strings() {
        let diff = diff_words("Same text exactly", "Same text exactly");
        assert_eq!(
            diff,
            vec![DiffSpan::Unchanged("Same text exactly".to_string())]
        );
        assert!(!has_changes(&diff));
    }

    #[test]
    fn test_insertions() {
        let original = "Developed web applications";
        let proposed = "Developed scalable web applications in Rust";
        let diff = diff_words(original, proposed);

        assert!(has_changes(&diff));
        assert!(
            diff.iter()
                .any(|s| s.is_added() && s.text().contains("scalable"))
        );
        assert!(
            diff.iter()
                .any(|s| s.is_added() && s.text().contains("Rust"))
        );
        assert!(diff.iter().all(|s| !s.is_removed()));
    }

    #[test]
    fn test_deletions() {
        let original = "Responsible for managing and maintaining legacy codebases";
        let proposed = "Maintained legacy codebases";
        let diff = diff_words(original, proposed);

        assert!(has_changes(&diff));
        assert!(diff.iter().any(|s| s.is_removed()));
    }

    #[test]
    fn test_replacements() {
        let original = "Helped team with deployment issues";
        let proposed = "Automated continuous deployment pipeline";
        let diff = diff_words(original, proposed);

        assert!(has_changes(&diff));
        assert!(diff.iter().any(|s| s.is_removed()));
        assert!(diff.iter().any(|s| s.is_added()));

        let stats = diff_stats(&diff);
        assert!(stats.additions > 0);
        assert!(stats.deletions > 0);
    }

    #[test]
    fn test_unicode_and_special_characters() {
        let original = "Conception d'applications sécurisées à Paris — 100% testées";
        let proposed =
            "Conception d'applications ultra-sécurisées à Paris — 100% testées avec Rust 🦀";
        let diff = diff_words(original, proposed);

        assert!(has_changes(&diff));
        let added_text: String = diff
            .iter()
            .filter(|s| s.is_added())
            .map(|s| s.text())
            .collect();
        assert!(added_text.contains("🦀") || added_text.contains("Rust"));

        let diff_cjk = diff_words("開発者として働いた", "シニア開発者として働いた");
        assert!(has_changes(&diff_cjk));
    }

    #[test]
    fn test_per_bullet_diff() {
        let id = Uuid::new_v4();
        let orig = "Fixed bugs in backend";
        let prop = "Resolved 15 critical vulnerabilities in backend auth service";
        let bdiff = diff_bullet(id, orig, prop);

        assert_eq!(bdiff.bullet_id, id);
        assert_eq!(bdiff.original, orig);
        assert_eq!(bdiff.proposed, prop);
        assert!(bdiff.has_changes());
    }

    #[test]
    fn test_per_summary_diff() {
        let id = Uuid::new_v4();
        let orig = "Software engineer looking for security roles.";
        let prop = "Offensive security engineer with 5+ years of penetration testing experience.";
        let sdiff = diff_summary(id, orig, prop);

        assert_eq!(sdiff.section_id, id);
        assert_eq!(sdiff.original, orig);
        assert_eq!(sdiff.proposed, prop);
        assert!(sdiff.has_changes());
    }

    #[test]
    fn test_diff_from_proposal() {
        let bullet_id = Uuid::new_v4();
        let prop = AiProposal::BulletRewrite {
            bullet_id,
            original: "Old bullet".to_string(),
            proposed: "New bullet".to_string(),
            rationale: "Better impact".to_string(),
        };

        let item_diff = diff_proposal(&prop).expect("should create bullet diff");
        assert_eq!(item_diff.target_item_id(), bullet_id);
        assert!(item_diff.has_changes());

        let visibility_prop = AiProposal::ToggleVisibility {
            item_id: Uuid::new_v4(),
            enabled: false,
            reason: "Not relevant".to_string(),
        };
        assert!(diff_proposal(&visibility_prop).is_none());
    }
}
