//! Parent↔Child structural and word-level diff calculation.
//!
//! Provides fine-grained diffing between a Child Sub-Variation and its Root
//! Master CV using `origin_item_id` pairing and the `similar` crate for
//! word-level inline diffs.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::contracts::{
    BulletItem, CvDocument, CvSection, DocumentId, ItemId, SectionKind, SkillCategoryItem,
    TimelineEntry,
};

/// Type of text span in an inline diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSpanKind {
    Added,
    Removed,
    Unchanged,
}

/// Convenience alias for [`DiffSpanKind`].
pub type SpanKind = DiffSpanKind;

/// An inline span of text indicating whether it was added, removed, or unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextDiffSpan {
    pub kind: DiffSpanKind,
    pub text: String,
}

impl TextDiffSpan {
    pub fn added(text: impl Into<String>) -> Self {
        Self {
            kind: DiffSpanKind::Added,
            text: text.into(),
        }
    }

    pub fn removed(text: impl Into<String>) -> Self {
        Self {
            kind: DiffSpanKind::Removed,
            text: text.into(),
        }
    }

    pub fn unchanged(text: impl Into<String>) -> Self {
        Self {
            kind: DiffSpanKind::Unchanged,
            text: text.into(),
        }
    }
}

/// The nature of change detected between child and parent items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Item exists in child but not in parent.
    Added,
    /// Item exists in parent but absent from child.
    Removed,
    /// Content, text, or metadata changed.
    Modified,
    /// Visibility enabled flag changed from true (parent) to false (child).
    Disabled,
    /// Visibility enabled flag changed from false (parent) to true (child).
    Enabled,
    /// Relative position or order changed within its container.
    Reordered,
}

/// Kind of CV item participating in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Section,
    TimelineEntry,
    Bullet,
    SkillCategory,
    Summary,
}

/// Granular diff details for a specific item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDiff {
    /// The item ID in child (or parent item ID if removed).
    pub item_id: ItemId,
    /// The origin item ID in parent, if paired or tracked.
    pub origin_item_id: Option<ItemId>,
    /// The type of item (Section, TimelineEntry, Bullet, SkillCategory, Summary).
    pub item_kind: ItemKind,
    /// The type of change (Added, Removed, Modified, Disabled, Enabled, Reordered).
    pub change_kind: ChangeKind,
    /// Human-readable location / path (e.g. "PROFESSIONAL EXPERIENCE > Senior Engineer > Bullet").
    pub location: String,
    /// Word-level diff spans for text changes.
    pub text_diff: Vec<TextDiffSpan>,
    /// Original text in parent, if applicable.
    pub parent_text: Option<String>,
    /// New text in child, if applicable.
    pub child_text: Option<String>,
    /// Enabled state in parent, if applicable.
    pub enabled_in_parent: Option<bool>,
    /// Enabled state in child, if applicable.
    pub enabled_in_child: Option<bool>,
}

/// Complete structural and word-level diff between a Child Sub-Variation
/// and its Root Master CV.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineageDiff {
    pub child_id: DocumentId,
    pub parent_id: DocumentId,
    /// All item IDs that were added, removed, modified, disabled/enabled, or reordered.
    pub changed_item_ids: Vec<Uuid>,
    /// Detailed list of all item-level diffs.
    pub item_diffs: Vec<ItemDiff>,
}

impl LineageDiff {
    /// Returns `true` if any differences exist between child and parent.
    pub fn has_changes(&self) -> bool {
        !self.changed_item_ids.is_empty()
    }

    /// Returns all items that were added in the child.
    pub fn added_items(&self) -> impl Iterator<Item = &ItemDiff> {
        self.item_diffs
            .iter()
            .filter(|d| d.change_kind == ChangeKind::Added)
    }

    /// Returns all items that were removed from the parent.
    pub fn removed_items(&self) -> impl Iterator<Item = &ItemDiff> {
        self.item_diffs
            .iter()
            .filter(|d| d.change_kind == ChangeKind::Removed)
    }

    /// Returns all items whose text or metadata was modified.
    pub fn modified_items(&self) -> impl Iterator<Item = &ItemDiff> {
        self.item_diffs
            .iter()
            .filter(|d| d.change_kind == ChangeKind::Modified)
    }

    /// Returns all items whose visibility was disabled in the child.
    pub fn disabled_items(&self) -> impl Iterator<Item = &ItemDiff> {
        self.item_diffs
            .iter()
            .filter(|d| d.change_kind == ChangeKind::Disabled)
    }

    /// Returns all items whose order was changed.
    pub fn reordered_items(&self) -> impl Iterator<Item = &ItemDiff> {
        self.item_diffs
            .iter()
            .filter(|d| d.change_kind == ChangeKind::Reordered)
    }
}

/// Computes word-level diff spans between `old_text` and `new_text` using `similar`.
///
/// Merges adjacent spans of the same kind so that multi-word additions or
/// deletions are grouped together cleanly.
pub fn compute_word_diff(old_text: &str, new_text: &str) -> Vec<TextDiffSpan> {
    if old_text == new_text {
        if old_text.is_empty() {
            return Vec::new();
        }
        return vec![TextDiffSpan::unchanged(old_text)];
    }

    if old_text.is_empty() {
        return vec![TextDiffSpan::added(new_text)];
    }

    if new_text.is_empty() {
        return vec![TextDiffSpan::removed(old_text)];
    }

    let diff = TextDiff::from_words(old_text, new_text);
    let mut raw_spans = Vec::new();

    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Equal => DiffSpanKind::Unchanged,
            ChangeTag::Delete => DiffSpanKind::Removed,
            ChangeTag::Insert => DiffSpanKind::Added,
        };
        raw_spans.push(TextDiffSpan {
            kind,
            text: change.value().to_string(),
        });
    }

    let mut merged: Vec<TextDiffSpan> = Vec::new();
    for span in raw_spans {
        if span.text.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut()
            && last.kind == span.kind
        {
            last.text.push_str(&span.text);
            continue;
        }
        merged.push(span);
    }
    merged
}

/// Computes a structural and word-level diff between a Child Sub-Variation
/// and its parent Root CV.
///
/// Validates that `child.parent_id == Some(parent.id)` and returns a descriptive
/// error otherwise.
pub fn compute_diff(child: &CvDocument, parent: &CvDocument) -> Result<LineageDiff> {
    if child.parent_id != Some(parent.id) {
        bail!(
            "compute_diff: child document '{}' (id={}) has parent_id={:?}, which does not match parent document '{}' (id={})",
            child.title,
            child.id,
            child.parent_id,
            parent.title,
            parent.id,
        );
    }

    let mut item_diffs: Vec<ItemDiff> = Vec::new();
    let mut changed_ids: HashSet<Uuid> = HashSet::new();

    let parent_sections: HashMap<ItemId, &CvSection> =
        parent.body.sections.iter().map(|s| (s.id, s)).collect();

    let mut matched_parent_section_ids: HashSet<ItemId> = HashSet::new();
    let mut matched_parent_entry_ids: HashSet<ItemId> = HashSet::new();
    let mut matched_parent_bullet_ids: HashSet<ItemId> = HashSet::new();
    let mut matched_parent_category_ids: HashSet<ItemId> = HashSet::new();

    // 1. Diff each section in child against parent
    for child_section in &child.body.sections {
        let parent_section = child_section
            .origin_item_id
            .and_then(|orig| parent_sections.get(&orig).copied())
            .or_else(|| parent_sections.get(&child_section.id).copied());

        match parent_section {
            None => {
                // Whole section is added in child
                changed_ids.insert(child_section.id);
                item_diffs.push(ItemDiff {
                    item_id: child_section.id,
                    origin_item_id: child_section.origin_item_id,
                    item_kind: ItemKind::Section,
                    change_kind: ChangeKind::Added,
                    location: child_section.title.clone(),
                    text_diff: vec![TextDiffSpan::added(&child_section.title)],
                    parent_text: None,
                    child_text: Some(child_section.title.clone()),
                    enabled_in_parent: None,
                    enabled_in_child: Some(child_section.enabled),
                });

                // Record nested items as added
                record_all_added_in_section(child_section, &mut item_diffs, &mut changed_ids);
            }
            Some(parent_sec) => {
                matched_parent_section_ids.insert(parent_sec.id);

                // Section visibility change
                if child_section.enabled != parent_sec.enabled {
                    changed_ids.insert(child_section.id);
                    item_diffs.push(ItemDiff {
                        item_id: child_section.id,
                        origin_item_id: Some(parent_sec.id),
                        item_kind: ItemKind::Section,
                        change_kind: if child_section.enabled {
                            ChangeKind::Enabled
                        } else {
                            ChangeKind::Disabled
                        },
                        location: child_section.title.clone(),
                        text_diff: Vec::new(),
                        parent_text: Some(parent_sec.title.clone()),
                        child_text: Some(child_section.title.clone()),
                        enabled_in_parent: Some(parent_sec.enabled),
                        enabled_in_child: Some(child_section.enabled),
                    });
                }

                // Section title change
                if child_section.title != parent_sec.title {
                    changed_ids.insert(child_section.id);
                    item_diffs.push(ItemDiff {
                        item_id: child_section.id,
                        origin_item_id: Some(parent_sec.id),
                        item_kind: ItemKind::Section,
                        change_kind: ChangeKind::Modified,
                        location: child_section.title.clone(),
                        text_diff: compute_word_diff(&parent_sec.title, &child_section.title),
                        parent_text: Some(parent_sec.title.clone()),
                        child_text: Some(child_section.title.clone()),
                        enabled_in_parent: Some(parent_sec.enabled),
                        enabled_in_child: Some(child_section.enabled),
                    });
                }

                // Diff contents based on SectionKind
                match (&child_section.kind, &parent_sec.kind) {
                    (
                        SectionKind::Summary { text: child_text },
                        SectionKind::Summary { text: parent_text },
                    ) => {
                        if child_text != parent_text {
                            changed_ids.insert(child_section.id);
                            item_diffs.push(ItemDiff {
                                item_id: child_section.id,
                                origin_item_id: child_section
                                    .origin_item_id
                                    .or(Some(parent_sec.id)),
                                item_kind: ItemKind::Summary,
                                change_kind: ChangeKind::Modified,
                                location: format!("{} > Summary", child_section.title),
                                text_diff: compute_word_diff(parent_text, child_text),
                                parent_text: Some(parent_text.clone()),
                                child_text: Some(child_text.clone()),
                                enabled_in_parent: Some(parent_sec.enabled),
                                enabled_in_child: Some(child_section.enabled),
                            });
                        }
                    }
                    (
                        SectionKind::TimelineEntries {
                            entries: child_entries,
                        },
                        SectionKind::TimelineEntries {
                            entries: parent_entries,
                        },
                    ) => {
                        diff_timeline_entries(
                            &child_section.title,
                            parent_sec,
                            child_entries,
                            parent_entries,
                            &mut item_diffs,
                            &mut changed_ids,
                            &mut matched_parent_entry_ids,
                            &mut matched_parent_bullet_ids,
                        );
                    }
                    (
                        SectionKind::SimpleBulletList {
                            items: child_bullets,
                        },
                        SectionKind::SimpleBulletList {
                            items: parent_bullets,
                        },
                    ) => {
                        diff_bullets(
                            &child_section.title,
                            None,
                            child_bullets,
                            parent_bullets,
                            &mut item_diffs,
                            &mut changed_ids,
                            &mut matched_parent_bullet_ids,
                        );
                    }
                    (
                        SectionKind::CategorizedInlineList {
                            categories: child_cats,
                        },
                        SectionKind::CategorizedInlineList {
                            categories: parent_cats,
                        },
                    ) => {
                        diff_skill_categories(
                            &child_section.title,
                            parent_sec,
                            child_cats,
                            parent_cats,
                            &mut item_diffs,
                            &mut changed_ids,
                            &mut matched_parent_category_ids,
                        );
                    }
                    _ => {
                        // Section kind changed entirely
                        changed_ids.insert(child_section.id);
                        item_diffs.push(ItemDiff {
                            item_id: child_section.id,
                            origin_item_id: Some(parent_sec.id),
                            item_kind: ItemKind::Section,
                            change_kind: ChangeKind::Modified,
                            location: child_section.title.clone(),
                            text_diff: compute_word_diff(
                                parent_sec.kind_name(),
                                child_section.kind_name(),
                            ),
                            parent_text: Some(parent_sec.kind_name().to_string()),
                            child_text: Some(child_section.kind_name().to_string()),
                            enabled_in_parent: Some(parent_sec.enabled),
                            enabled_in_child: Some(child_section.enabled),
                        });
                    }
                }
            }
        }
    }

    // 2. Detect removed parent sections
    for parent_sec in &parent.body.sections {
        if !matched_parent_section_ids.contains(&parent_sec.id) {
            changed_ids.insert(parent_sec.id);
            item_diffs.push(ItemDiff {
                item_id: parent_sec.id,
                origin_item_id: Some(parent_sec.id),
                item_kind: ItemKind::Section,
                change_kind: ChangeKind::Removed,
                location: parent_sec.title.clone(),
                text_diff: vec![TextDiffSpan::removed(&parent_sec.title)],
                parent_text: Some(parent_sec.title.clone()),
                child_text: None,
                enabled_in_parent: Some(parent_sec.enabled),
                enabled_in_child: None,
            });
            record_all_removed_in_section(parent_sec, &mut item_diffs, &mut changed_ids);
        }
    }

    // 3. Detect reordered sections
    let mut paired_parent_sections = Vec::new();
    let mut paired_child_sections = Vec::new();
    for child_sec in &child.body.sections {
        if let Some(orig) = child_sec.origin_item_id {
            if parent_sections.contains_key(&orig) {
                paired_parent_sections.push(orig);
                paired_child_sections.push(orig);
            }
        } else if parent_sections.contains_key(&child_sec.id) {
            paired_parent_sections.push(child_sec.id);
            paired_child_sections.push(child_sec.id);
        }
    }

    paired_parent_sections.sort_by_key(|id| {
        parent
            .body
            .sections
            .iter()
            .position(|s| s.id == *id)
            .unwrap_or(0)
    });

    if paired_child_sections != paired_parent_sections {
        for (idx, child_orig_id) in paired_child_sections.iter().enumerate() {
            if paired_parent_sections.get(idx) != Some(child_orig_id)
                && let Some(sec) =
                    child.body.sections.iter().find(|s| {
                        s.origin_item_id == Some(*child_orig_id) || s.id == *child_orig_id
                    })
            {
                changed_ids.insert(sec.id);
                item_diffs.push(ItemDiff {
                    item_id: sec.id,
                    origin_item_id: Some(*child_orig_id),
                    item_kind: ItemKind::Section,
                    change_kind: ChangeKind::Reordered,
                    location: sec.title.clone(),
                    text_diff: Vec::new(),
                    parent_text: Some(sec.title.clone()),
                    child_text: Some(sec.title.clone()),
                    enabled_in_parent: Some(sec.enabled),
                    enabled_in_child: Some(sec.enabled),
                });
            }
        }
    }

    let mut changed_item_ids: Vec<Uuid> = changed_ids.into_iter().collect();
    changed_item_ids.sort();

    Ok(LineageDiff {
        child_id: child.id,
        parent_id: parent.id,
        changed_item_ids,
        item_diffs,
    })
}

#[allow(clippy::too_many_arguments)]
fn diff_timeline_entries(
    section_title: &str,
    parent_sec: &CvSection,
    child_entries: &[TimelineEntry],
    parent_entries: &[TimelineEntry],
    item_diffs: &mut Vec<ItemDiff>,
    changed_ids: &mut HashSet<Uuid>,
    matched_parent_entry_ids: &mut HashSet<ItemId>,
    matched_parent_bullet_ids: &mut HashSet<ItemId>,
) {
    let parent_entries_map: HashMap<ItemId, &TimelineEntry> =
        parent_entries.iter().map(|e| (e.id, e)).collect();
    let mut paired_parent_order = Vec::new();
    let mut paired_child_order = Vec::new();

    for child_entry in child_entries {
        let parent_entry = child_entry
            .origin_item_id
            .and_then(|orig| parent_entries_map.get(&orig).copied())
            .or_else(|| parent_entries_map.get(&child_entry.id).copied());

        if let Some(pe) = parent_entry {
            matched_parent_entry_ids.insert(pe.id);
            paired_parent_order.push(pe.id);
            paired_child_order.push(pe.id);

            // Enabled changed
            if child_entry.enabled != pe.enabled {
                changed_ids.insert(child_entry.id);
                item_diffs.push(ItemDiff {
                    item_id: child_entry.id,
                    origin_item_id: Some(pe.id),
                    item_kind: ItemKind::TimelineEntry,
                    change_kind: if child_entry.enabled {
                        ChangeKind::Enabled
                    } else {
                        ChangeKind::Disabled
                    },
                    location: format!("{section_title} > {}", child_entry.primary_heading),
                    text_diff: Vec::new(),
                    parent_text: Some(pe.primary_heading.clone()),
                    child_text: Some(child_entry.primary_heading.clone()),
                    enabled_in_parent: Some(pe.enabled),
                    enabled_in_child: Some(child_entry.enabled),
                });
            }

            // Headings / dates / tags changed
            let headings_differ = child_entry.primary_heading != pe.primary_heading
                || child_entry.secondary_heading != pe.secondary_heading
                || child_entry.start_date != pe.start_date
                || child_entry.end_date != pe.end_date
                || child_entry.is_current != pe.is_current
                || child_entry.tags != pe.tags;

            if headings_differ {
                changed_ids.insert(child_entry.id);
                let p_str = format!(
                    "{} | {} | {}",
                    pe.primary_heading,
                    pe.secondary_heading.as_deref().unwrap_or(""),
                    pe.date_range()
                );
                let c_str = format!(
                    "{} | {} | {}",
                    child_entry.primary_heading,
                    child_entry.secondary_heading.as_deref().unwrap_or(""),
                    child_entry.date_range()
                );
                item_diffs.push(ItemDiff {
                    item_id: child_entry.id,
                    origin_item_id: Some(pe.id),
                    item_kind: ItemKind::TimelineEntry,
                    change_kind: ChangeKind::Modified,
                    location: format!("{section_title} > {}", child_entry.primary_heading),
                    text_diff: compute_word_diff(&p_str, &c_str),
                    parent_text: Some(p_str),
                    child_text: Some(c_str),
                    enabled_in_parent: Some(pe.enabled),
                    enabled_in_child: Some(child_entry.enabled),
                });
            }

            // Diff bullets inside this entry
            diff_bullets(
                section_title,
                Some(&child_entry.primary_heading),
                &child_entry.bullets,
                &pe.bullets,
                item_diffs,
                changed_ids,
                matched_parent_bullet_ids,
            );
        } else {
            // Child entry added
            changed_ids.insert(child_entry.id);
            item_diffs.push(ItemDiff {
                item_id: child_entry.id,
                origin_item_id: child_entry.origin_item_id,
                item_kind: ItemKind::TimelineEntry,
                change_kind: ChangeKind::Added,
                location: format!("{section_title} > {}", child_entry.primary_heading),
                text_diff: vec![TextDiffSpan::added(&child_entry.primary_heading)],
                parent_text: None,
                child_text: Some(child_entry.primary_heading.clone()),
                enabled_in_parent: None,
                enabled_in_child: Some(child_entry.enabled),
            });
            for b in &child_entry.bullets {
                changed_ids.insert(b.id);
                item_diffs.push(ItemDiff {
                    item_id: b.id,
                    origin_item_id: b.origin_item_id,
                    item_kind: ItemKind::Bullet,
                    change_kind: ChangeKind::Added,
                    location: format!("{section_title} > {} > Bullet", child_entry.primary_heading),
                    text_diff: vec![TextDiffSpan::added(&b.text)],
                    parent_text: None,
                    child_text: Some(b.text.clone()),
                    enabled_in_parent: None,
                    enabled_in_child: Some(b.enabled),
                });
            }
        }
    }

    // Check entry reordering
    paired_parent_order
        .sort_by_key(|id| parent_entries.iter().position(|e| e.id == *id).unwrap_or(0));
    if paired_child_order != paired_parent_order {
        for (idx, child_orig_id) in paired_child_order.iter().enumerate() {
            if paired_parent_order.get(idx) != Some(child_orig_id)
                && let Some(entry) = child_entries
                    .iter()
                    .find(|e| e.origin_item_id == Some(*child_orig_id) || e.id == *child_orig_id)
            {
                changed_ids.insert(entry.id);
                item_diffs.push(ItemDiff {
                    item_id: entry.id,
                    origin_item_id: Some(*child_orig_id),
                    item_kind: ItemKind::TimelineEntry,
                    change_kind: ChangeKind::Reordered,
                    location: format!("{section_title} > {}", entry.primary_heading),
                    text_diff: Vec::new(),
                    parent_text: Some(entry.primary_heading.clone()),
                    child_text: Some(entry.primary_heading.clone()),
                    enabled_in_parent: Some(entry.enabled),
                    enabled_in_child: Some(entry.enabled),
                });
            }
        }
    }

    // Check removed parent entries
    for pe in parent_entries {
        if !matched_parent_entry_ids.contains(&pe.id) {
            changed_ids.insert(pe.id);
            item_diffs.push(ItemDiff {
                item_id: pe.id,
                origin_item_id: Some(pe.id),
                item_kind: ItemKind::TimelineEntry,
                change_kind: ChangeKind::Removed,
                location: format!("{} > {}", parent_sec.title, pe.primary_heading),
                text_diff: vec![TextDiffSpan::removed(&pe.primary_heading)],
                parent_text: Some(pe.primary_heading.clone()),
                child_text: None,
                enabled_in_parent: Some(pe.enabled),
                enabled_in_child: None,
            });
            for b in &pe.bullets {
                changed_ids.insert(b.id);
                item_diffs.push(ItemDiff {
                    item_id: b.id,
                    origin_item_id: Some(b.id),
                    item_kind: ItemKind::Bullet,
                    change_kind: ChangeKind::Removed,
                    location: format!("{} > {} > Bullet", parent_sec.title, pe.primary_heading),
                    text_diff: vec![TextDiffSpan::removed(&b.text)],
                    parent_text: Some(b.text.clone()),
                    child_text: None,
                    enabled_in_parent: Some(b.enabled),
                    enabled_in_child: None,
                });
            }
        }
    }
}

fn diff_bullets(
    section_title: &str,
    entry_heading: Option<&str>,
    child_bullets: &[BulletItem],
    parent_bullets: &[BulletItem],
    diffs: &mut Vec<ItemDiff>,
    changed_ids: &mut HashSet<Uuid>,
    matched_parent_bullet_ids: &mut HashSet<ItemId>,
) {
    let parent_map: HashMap<ItemId, &BulletItem> =
        parent_bullets.iter().map(|b| (b.id, b)).collect();
    let mut paired_parent_order = Vec::new();
    let mut paired_child_order = Vec::new();

    let loc_prefix = match entry_heading {
        Some(heading) => format!("{section_title} > {heading}"),
        None => section_title.to_string(),
    };

    for child_bullet in child_bullets {
        let parent_bullet = child_bullet
            .origin_item_id
            .and_then(|orig| parent_map.get(&orig).copied())
            .or_else(|| parent_map.get(&child_bullet.id).copied());

        if let Some(pb) = parent_bullet {
            matched_parent_bullet_ids.insert(pb.id);
            paired_parent_order.push(pb.id);
            paired_child_order.push(pb.id);

            // Enabled changed
            if child_bullet.enabled != pb.enabled {
                changed_ids.insert(child_bullet.id);
                diffs.push(ItemDiff {
                    item_id: child_bullet.id,
                    origin_item_id: Some(pb.id),
                    item_kind: ItemKind::Bullet,
                    change_kind: if child_bullet.enabled {
                        ChangeKind::Enabled
                    } else {
                        ChangeKind::Disabled
                    },
                    location: format!("{loc_prefix} > Bullet"),
                    text_diff: Vec::new(),
                    parent_text: Some(pb.text.clone()),
                    child_text: Some(child_bullet.text.clone()),
                    enabled_in_parent: Some(pb.enabled),
                    enabled_in_child: Some(child_bullet.enabled),
                });
            }

            // Text changed
            if child_bullet.text != pb.text {
                changed_ids.insert(child_bullet.id);
                diffs.push(ItemDiff {
                    item_id: child_bullet.id,
                    origin_item_id: Some(pb.id),
                    item_kind: ItemKind::Bullet,
                    change_kind: ChangeKind::Modified,
                    location: format!("{loc_prefix} > Bullet"),
                    text_diff: compute_word_diff(&pb.text, &child_bullet.text),
                    parent_text: Some(pb.text.clone()),
                    child_text: Some(child_bullet.text.clone()),
                    enabled_in_parent: Some(pb.enabled),
                    enabled_in_child: Some(child_bullet.enabled),
                });
            }
        } else {
            // Child bullet added (origin points to nothing in parent or is None)
            changed_ids.insert(child_bullet.id);
            diffs.push(ItemDiff {
                item_id: child_bullet.id,
                origin_item_id: child_bullet.origin_item_id,
                item_kind: ItemKind::Bullet,
                change_kind: ChangeKind::Added,
                location: format!("{loc_prefix} > Bullet"),
                text_diff: vec![TextDiffSpan::added(&child_bullet.text)],
                parent_text: None,
                child_text: Some(child_bullet.text.clone()),
                enabled_in_parent: None,
                enabled_in_child: Some(child_bullet.enabled),
            });
        }
    }

    // Reordered bullets
    paired_parent_order
        .sort_by_key(|id| parent_bullets.iter().position(|b| b.id == *id).unwrap_or(0));
    if paired_child_order != paired_parent_order {
        for (idx, child_orig_id) in paired_child_order.iter().enumerate() {
            if paired_parent_order.get(idx) != Some(child_orig_id)
                && let Some(bullet) = child_bullets
                    .iter()
                    .find(|b| b.origin_item_id == Some(*child_orig_id) || b.id == *child_orig_id)
            {
                changed_ids.insert(bullet.id);
                diffs.push(ItemDiff {
                    item_id: bullet.id,
                    origin_item_id: Some(*child_orig_id),
                    item_kind: ItemKind::Bullet,
                    change_kind: ChangeKind::Reordered,
                    location: format!("{loc_prefix} > Bullet"),
                    text_diff: Vec::new(),
                    parent_text: Some(bullet.text.clone()),
                    child_text: Some(bullet.text.clone()),
                    enabled_in_parent: Some(bullet.enabled),
                    enabled_in_child: Some(bullet.enabled),
                });
            }
        }
    }

    // Removed parent bullets
    for pb in parent_bullets {
        if !matched_parent_bullet_ids.contains(&pb.id) {
            changed_ids.insert(pb.id);
            diffs.push(ItemDiff {
                item_id: pb.id,
                origin_item_id: Some(pb.id),
                item_kind: ItemKind::Bullet,
                change_kind: ChangeKind::Removed,
                location: format!("{loc_prefix} > Bullet"),
                text_diff: vec![TextDiffSpan::removed(&pb.text)],
                parent_text: Some(pb.text.clone()),
                child_text: None,
                enabled_in_parent: Some(pb.enabled),
                enabled_in_child: None,
            });
        }
    }
}

fn diff_skill_categories(
    section_title: &str,
    parent_sec: &CvSection,
    child_cats: &[SkillCategoryItem],
    parent_cats: &[SkillCategoryItem],
    item_diffs: &mut Vec<ItemDiff>,
    changed_ids: &mut HashSet<Uuid>,
    matched_parent_category_ids: &mut HashSet<ItemId>,
) {
    let parent_cats_map: HashMap<ItemId, &SkillCategoryItem> =
        parent_cats.iter().map(|c| (c.id, c)).collect();
    let mut paired_parent_order = Vec::new();
    let mut paired_child_order = Vec::new();

    for child_cat in child_cats {
        let parent_cat = child_cat
            .origin_item_id
            .and_then(|orig| parent_cats_map.get(&orig).copied())
            .or_else(|| parent_cats_map.get(&child_cat.id).copied());

        if let Some(pc) = parent_cat {
            matched_parent_category_ids.insert(pc.id);
            paired_parent_order.push(pc.id);
            paired_child_order.push(pc.id);

            // Enabled changed
            if child_cat.enabled != pc.enabled {
                changed_ids.insert(child_cat.id);
                item_diffs.push(ItemDiff {
                    item_id: child_cat.id,
                    origin_item_id: Some(pc.id),
                    item_kind: ItemKind::SkillCategory,
                    change_kind: if child_cat.enabled {
                        ChangeKind::Enabled
                    } else {
                        ChangeKind::Disabled
                    },
                    location: format!("{section_title} > {}", child_cat.category_label),
                    text_diff: Vec::new(),
                    parent_text: Some(pc.category_label.clone()),
                    child_text: Some(child_cat.category_label.clone()),
                    enabled_in_parent: Some(pc.enabled),
                    enabled_in_child: Some(child_cat.enabled),
                });
            }

            // Content or items changed
            let content_differ = child_cat.category_label != pc.category_label
                || child_cat.separator != pc.separator
                || child_cat.items != pc.items;

            if content_differ {
                changed_ids.insert(child_cat.id);
                let p_text = format!("{}: {}", pc.category_label, pc.joined_items());
                let c_text = format!("{}: {}", child_cat.category_label, child_cat.joined_items());
                item_diffs.push(ItemDiff {
                    item_id: child_cat.id,
                    origin_item_id: Some(pc.id),
                    item_kind: ItemKind::SkillCategory,
                    change_kind: ChangeKind::Modified,
                    location: format!("{section_title} > {}", child_cat.category_label),
                    text_diff: compute_word_diff(&p_text, &c_text),
                    parent_text: Some(p_text),
                    child_text: Some(c_text),
                    enabled_in_parent: Some(pc.enabled),
                    enabled_in_child: Some(child_cat.enabled),
                });
            }
        } else {
            // Child category added
            changed_ids.insert(child_cat.id);
            let c_text = format!("{}: {}", child_cat.category_label, child_cat.joined_items());
            item_diffs.push(ItemDiff {
                item_id: child_cat.id,
                origin_item_id: child_cat.origin_item_id,
                item_kind: ItemKind::SkillCategory,
                change_kind: ChangeKind::Added,
                location: format!("{section_title} > {}", child_cat.category_label),
                text_diff: vec![TextDiffSpan::added(&c_text)],
                parent_text: None,
                child_text: Some(c_text),
                enabled_in_parent: None,
                enabled_in_child: Some(child_cat.enabled),
            });
        }
    }

    // Reordered categories
    paired_parent_order.sort_by_key(|id| parent_cats.iter().position(|c| c.id == *id).unwrap_or(0));
    if paired_child_order != paired_parent_order {
        for (idx, child_orig_id) in paired_child_order.iter().enumerate() {
            if paired_parent_order.get(idx) != Some(child_orig_id)
                && let Some(cat) = child_cats
                    .iter()
                    .find(|c| c.origin_item_id == Some(*child_orig_id) || c.id == *child_orig_id)
            {
                changed_ids.insert(cat.id);
                item_diffs.push(ItemDiff {
                    item_id: cat.id,
                    origin_item_id: Some(*child_orig_id),
                    item_kind: ItemKind::SkillCategory,
                    change_kind: ChangeKind::Reordered,
                    location: format!("{section_title} > {}", cat.category_label),
                    text_diff: Vec::new(),
                    parent_text: Some(cat.category_label.clone()),
                    child_text: Some(cat.category_label.clone()),
                    enabled_in_parent: Some(cat.enabled),
                    enabled_in_child: Some(cat.enabled),
                });
            }
        }
    }

    // Removed parent categories
    for pc in parent_cats {
        if !matched_parent_category_ids.contains(&pc.id) {
            changed_ids.insert(pc.id);
            let p_text = format!("{}: {}", pc.category_label, pc.joined_items());
            item_diffs.push(ItemDiff {
                item_id: pc.id,
                origin_item_id: Some(pc.id),
                item_kind: ItemKind::SkillCategory,
                change_kind: ChangeKind::Removed,
                location: format!("{} > {}", parent_sec.title, pc.category_label),
                text_diff: vec![TextDiffSpan::removed(&p_text)],
                parent_text: Some(p_text),
                child_text: None,
                enabled_in_parent: Some(pc.enabled),
                enabled_in_child: None,
            });
        }
    }
}

fn record_all_added_in_section(
    section: &CvSection,
    item_diffs: &mut Vec<ItemDiff>,
    changed_ids: &mut HashSet<Uuid>,
) {
    match &section.kind {
        SectionKind::Summary { text } => {
            changed_ids.insert(section.id);
            item_diffs.push(ItemDiff {
                item_id: section.id,
                origin_item_id: section.origin_item_id,
                item_kind: ItemKind::Summary,
                change_kind: ChangeKind::Added,
                location: format!("{} > Summary", section.title),
                text_diff: vec![TextDiffSpan::added(text)],
                parent_text: None,
                child_text: Some(text.clone()),
                enabled_in_parent: None,
                enabled_in_child: Some(section.enabled),
            });
        }
        SectionKind::TimelineEntries { entries } => {
            for entry in entries {
                changed_ids.insert(entry.id);
                item_diffs.push(ItemDiff {
                    item_id: entry.id,
                    origin_item_id: entry.origin_item_id,
                    item_kind: ItemKind::TimelineEntry,
                    change_kind: ChangeKind::Added,
                    location: format!("{} > {}", section.title, entry.primary_heading),
                    text_diff: vec![TextDiffSpan::added(&entry.primary_heading)],
                    parent_text: None,
                    child_text: Some(entry.primary_heading.clone()),
                    enabled_in_parent: None,
                    enabled_in_child: Some(entry.enabled),
                });
                for b in &entry.bullets {
                    changed_ids.insert(b.id);
                    item_diffs.push(ItemDiff {
                        item_id: b.id,
                        origin_item_id: b.origin_item_id,
                        item_kind: ItemKind::Bullet,
                        change_kind: ChangeKind::Added,
                        location: format!("{} > {} > Bullet", section.title, entry.primary_heading),
                        text_diff: vec![TextDiffSpan::added(&b.text)],
                        parent_text: None,
                        child_text: Some(b.text.clone()),
                        enabled_in_parent: None,
                        enabled_in_child: Some(b.enabled),
                    });
                }
            }
        }
        SectionKind::CategorizedInlineList { categories } => {
            for cat in categories {
                changed_ids.insert(cat.id);
                let text = format!("{}: {}", cat.category_label, cat.joined_items());
                item_diffs.push(ItemDiff {
                    item_id: cat.id,
                    origin_item_id: cat.origin_item_id,
                    item_kind: ItemKind::SkillCategory,
                    change_kind: ChangeKind::Added,
                    location: format!("{} > {}", section.title, cat.category_label),
                    text_diff: vec![TextDiffSpan::added(&text)],
                    parent_text: None,
                    child_text: Some(text),
                    enabled_in_parent: None,
                    enabled_in_child: Some(cat.enabled),
                });
            }
        }
        SectionKind::SimpleBulletList { items } => {
            for b in items {
                changed_ids.insert(b.id);
                item_diffs.push(ItemDiff {
                    item_id: b.id,
                    origin_item_id: b.origin_item_id,
                    item_kind: ItemKind::Bullet,
                    change_kind: ChangeKind::Added,
                    location: format!("{} > Bullet", section.title),
                    text_diff: vec![TextDiffSpan::added(&b.text)],
                    parent_text: None,
                    child_text: Some(b.text.clone()),
                    enabled_in_parent: None,
                    enabled_in_child: Some(b.enabled),
                });
            }
        }
    }
}

fn record_all_removed_in_section(
    section: &CvSection,
    item_diffs: &mut Vec<ItemDiff>,
    changed_ids: &mut HashSet<Uuid>,
) {
    match &section.kind {
        SectionKind::Summary { text } => {
            changed_ids.insert(section.id);
            item_diffs.push(ItemDiff {
                item_id: section.id,
                origin_item_id: Some(section.id),
                item_kind: ItemKind::Summary,
                change_kind: ChangeKind::Removed,
                location: format!("{} > Summary", section.title),
                text_diff: vec![TextDiffSpan::removed(text)],
                parent_text: Some(text.clone()),
                child_text: None,
                enabled_in_parent: Some(section.enabled),
                enabled_in_child: None,
            });
        }
        SectionKind::TimelineEntries { entries } => {
            for entry in entries {
                changed_ids.insert(entry.id);
                item_diffs.push(ItemDiff {
                    item_id: entry.id,
                    origin_item_id: Some(entry.id),
                    item_kind: ItemKind::TimelineEntry,
                    change_kind: ChangeKind::Removed,
                    location: format!("{} > {}", section.title, entry.primary_heading),
                    text_diff: vec![TextDiffSpan::removed(&entry.primary_heading)],
                    parent_text: Some(entry.primary_heading.clone()),
                    child_text: None,
                    enabled_in_parent: Some(entry.enabled),
                    enabled_in_child: None,
                });
                for b in &entry.bullets {
                    changed_ids.insert(b.id);
                    item_diffs.push(ItemDiff {
                        item_id: b.id,
                        origin_item_id: Some(b.id),
                        item_kind: ItemKind::Bullet,
                        change_kind: ChangeKind::Removed,
                        location: format!("{} > {} > Bullet", section.title, entry.primary_heading),
                        text_diff: vec![TextDiffSpan::removed(&b.text)],
                        parent_text: Some(b.text.clone()),
                        child_text: None,
                        enabled_in_parent: Some(b.enabled),
                        enabled_in_child: None,
                    });
                }
            }
        }
        SectionKind::CategorizedInlineList { categories } => {
            for cat in categories {
                changed_ids.insert(cat.id);
                let text = format!("{}: {}", cat.category_label, cat.joined_items());
                item_diffs.push(ItemDiff {
                    item_id: cat.id,
                    origin_item_id: Some(cat.id),
                    item_kind: ItemKind::SkillCategory,
                    change_kind: ChangeKind::Removed,
                    location: format!("{} > {}", section.title, cat.category_label),
                    text_diff: vec![TextDiffSpan::removed(&text)],
                    parent_text: Some(text),
                    child_text: None,
                    enabled_in_parent: Some(cat.enabled),
                    enabled_in_child: None,
                });
            }
        }
        SectionKind::SimpleBulletList { items } => {
            for b in items {
                changed_ids.insert(b.id);
                item_diffs.push(ItemDiff {
                    item_id: b.id,
                    origin_item_id: Some(b.id),
                    item_kind: ItemKind::Bullet,
                    change_kind: ChangeKind::Removed,
                    location: format!("{} > Bullet", section.title),
                    text_diff: vec![TextDiffSpan::removed(&b.text)],
                    parent_text: Some(b.text.clone()),
                    child_text: None,
                    enabled_in_parent: Some(b.enabled),
                    enabled_in_child: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{CvRepository, JobContext};
    use crate::db::repository::SqliteRepository;

    fn build_sample_root_and_child() -> (CvDocument, CvDocument) {
        let repo = SqliteRepository::in_memory().expect("in_memory repo");
        repo.seed_if_empty().expect("seed");
        let roots = repo.list_roots().expect("list roots");
        let root = repo.load(roots[0].id).expect("load root");

        let job_ctx = JobContext {
            company_name: "CrowdStrike".to_string(),
            target_role: "Senior Detection Engineer".to_string(),
            job_description_raw: "Sample JD".to_string(),
            ..Default::default()
        };

        let child = repo
            .branch_sub_variation(root.id, "CrowdStrike Sub-Variation", job_ctx)
            .expect("branch");

        (root, child)
    }

    #[test]
    fn test_unchanged_clone_yields_no_changes() {
        let (root, child) = build_sample_root_and_child();

        let diff = compute_diff(&child, &root).expect("compute_diff");
        assert_eq!(diff.child_id, child.id);
        assert_eq!(diff.parent_id, root.id);
        assert!(!diff.has_changes());
        assert!(diff.changed_item_ids.is_empty());
        assert!(diff.item_diffs.is_empty());
        assert_eq!(diff.added_items().count(), 0);
        assert_eq!(diff.removed_items().count(), 0);
        assert_eq!(diff.modified_items().count(), 0);
        assert_eq!(diff.disabled_items().count(), 0);
        assert_eq!(diff.reordered_items().count(), 0);
    }

    #[test]
    fn test_edited_bullet_text_produces_word_level_added_removed_spans() {
        let (root, mut child) = build_sample_root_and_child();

        // Find a bullet in the child and modify its text
        let bullet = child.body.bullets()[0];
        let bullet_id = bullet.id;
        let original_text = bullet.text.clone();

        let modified_text = format!("Enhanced and expanded {original_text} with AI automation");
        let b_mut = child.body.find_bullet_mut(bullet_id).expect("find bullet");
        b_mut.text = modified_text.clone();

        let diff = compute_diff(&child, &root).expect("compute_diff");
        assert!(diff.has_changes());
        assert!(diff.changed_item_ids.contains(&bullet_id));

        let mod_item = diff
            .modified_items()
            .find(|d| d.item_id == bullet_id)
            .expect("bullet modified diff");

        assert_eq!(mod_item.item_kind, ItemKind::Bullet);
        assert_eq!(
            mod_item.parent_text.as_deref(),
            Some(original_text.as_str())
        );
        assert_eq!(mod_item.child_text.as_deref(), Some(modified_text.as_str()));

        // Check word-level spans
        assert!(
            mod_item
                .text_diff
                .iter()
                .any(|s| s.kind == DiffSpanKind::Added)
        );
        assert!(
            mod_item
                .text_diff
                .iter()
                .any(|s| s.kind == DiffSpanKind::Unchanged)
        );
    }

    #[test]
    fn test_disabled_bullet_is_detected() {
        let (root, mut child) = build_sample_root_and_child();

        let bullet = child.body.bullets()[0];
        let bullet_id = bullet.id;

        let b_mut = child.body.find_bullet_mut(bullet_id).expect("find bullet");
        b_mut.enabled = false;

        let diff = compute_diff(&child, &root).expect("compute_diff");
        assert!(diff.has_changes());
        assert!(diff.changed_item_ids.contains(&bullet_id));

        let dis_item = diff
            .disabled_items()
            .find(|d| d.item_id == bullet_id)
            .expect("disabled diff");

        assert_eq!(dis_item.item_kind, ItemKind::Bullet);
        assert_eq!(dis_item.enabled_in_parent, Some(true));
        assert_eq!(dis_item.enabled_in_child, Some(false));
    }

    #[test]
    fn test_added_child_bullet_with_origin_item_id_is_reported() {
        let (root, mut child) = build_sample_root_and_child();

        let fake_parent_id = Uuid::new_v4();
        let new_bullet = BulletItem {
            id: Uuid::new_v4(),
            origin_item_id: Some(fake_parent_id), // points to nothing in parent
            enabled: true,
            text: "Engineered high-throughput streaming pipeline in Rust".to_string(),
        };
        let new_bullet_id = new_bullet.id;

        // Append to first SimpleBulletList or TimelineEntry
        let mut added = false;
        for section in &mut child.body.sections {
            if let SectionKind::TimelineEntries { entries } = &mut section.kind
                && let Some(entry) = entries.first_mut()
            {
                entry.bullets.push(new_bullet.clone());
                added = true;
                break;
            }
        }
        assert!(added);

        let diff = compute_diff(&child, &root).expect("compute_diff");
        assert!(diff.has_changes());
        assert!(diff.changed_item_ids.contains(&new_bullet_id));

        let add_item = diff
            .added_items()
            .find(|d| d.item_id == new_bullet_id)
            .expect("added bullet diff");

        assert_eq!(add_item.item_kind, ItemKind::Bullet);
        assert_eq!(add_item.origin_item_id, Some(fake_parent_id));
        assert_eq!(
            add_item.child_text.as_deref(),
            Some("Engineered high-throughput streaming pipeline in Rust")
        );
        assert!(
            add_item
                .text_diff
                .iter()
                .all(|s| s.kind == DiffSpanKind::Added)
        );
    }

    #[test]
    fn test_removed_parent_item_is_reported() {
        let (root, mut child) = build_sample_root_and_child();

        // Remove the first bullet from child
        let mut removed_bullet_id = None;
        for section in &mut child.body.sections {
            if let SectionKind::TimelineEntries { entries } = &mut section.kind
                && let Some(entry) = entries.first_mut()
            {
                let removed = entry.bullets.remove(0);
                removed_bullet_id = removed.origin_item_id;
                break;
            }
        }
        let parent_bullet_id = removed_bullet_id.expect("removed origin id");

        let diff = compute_diff(&child, &root).expect("compute_diff");
        assert!(diff.has_changes());
        assert!(diff.changed_item_ids.contains(&parent_bullet_id));

        let rem_item = diff
            .removed_items()
            .find(|d| d.item_id == parent_bullet_id)
            .expect("removed bullet diff");

        assert_eq!(rem_item.item_kind, ItemKind::Bullet);
        assert!(
            rem_item
                .text_diff
                .iter()
                .all(|s| s.kind == DiffSpanKind::Removed)
        );
    }

    #[test]
    fn test_root_vs_root_diff_is_rejected() {
        let repo = SqliteRepository::in_memory().expect("in_memory repo");
        repo.seed_if_empty().expect("seed");
        let roots = repo.list_roots().expect("list roots");
        let root1 = repo.load(roots[0].id).expect("load root");
        let root2 = root1.clone();

        let err = compute_diff(&root1, &root2);
        assert!(err.is_err());
        let err_msg = err.unwrap_err().to_string();
        assert!(err_msg.contains("does not match parent document"));
    }

    #[test]
    fn test_summary_and_skills_diff() {
        let (mut root, mut child) = build_sample_root_and_child();

        // Add a Summary section to root and child (paired by origin_item_id)
        let summary_root_id = Uuid::new_v4();
        let summary_child_id = Uuid::new_v4();
        root.body.sections.insert(
            0,
            CvSection {
                id: summary_root_id,
                origin_item_id: None,
                title: "SUMMARY".to_string(),
                enabled: true,
                kind: SectionKind::Summary {
                    text: "Original senior leadership summary".to_string(),
                },
            },
        );
        child.body.sections.insert(
            0,
            CvSection {
                id: summary_child_id,
                origin_item_id: Some(summary_root_id),
                title: "SUMMARY".to_string(),
                enabled: true,
                kind: SectionKind::Summary {
                    text: "Rewritten senior leadership summary".to_string(),
                },
            },
        );

        // Mutate skills in child
        for section in &mut child.body.sections {
            if let SectionKind::CategorizedInlineList { categories } = &mut section.kind
                && let Some(cat) = categories.first_mut()
            {
                cat.items.push("Kernel Exploitation".to_string());
            }
        }

        let diff = compute_diff(&child, &root).expect("diff");
        assert!(diff.has_changes());

        let summary_diff = diff
            .modified_items()
            .find(|d| d.item_kind == ItemKind::Summary)
            .expect("summary modified");
        assert_eq!(
            summary_diff.child_text.as_deref(),
            Some("Rewritten senior leadership summary")
        );
        assert_eq!(
            summary_diff.parent_text.as_deref(),
            Some("Original senior leadership summary")
        );

        let skill_diff = diff
            .modified_items()
            .find(|d| d.item_kind == ItemKind::SkillCategory)
            .expect("skill category modified");
        assert!(
            skill_diff
                .child_text
                .as_deref()
                .unwrap()
                .contains("Kernel Exploitation")
        );
    }

    #[test]
    fn test_reordered_bullets_are_detected() {
        let (root, mut child) = build_sample_root_and_child();

        // Reorder bullets in first timeline entry
        let mut reordered_ids = Vec::new();
        for section in &mut child.body.sections {
            if let SectionKind::TimelineEntries { entries } = &mut section.kind
                && let Some(entry) = entries.first_mut()
                && entry.bullets.len() >= 2
            {
                entry.bullets.swap(0, 1);
                reordered_ids.push(entry.bullets[0].id);
                reordered_ids.push(entry.bullets[1].id);
                break;
            }
        }

        let diff = compute_diff(&child, &root).expect("diff");
        assert!(diff.has_changes());
        let reordered = diff.reordered_items().collect::<Vec<_>>();
        assert!(!reordered.is_empty());
        assert!(
            reordered
                .iter()
                .any(|r| r.item_kind == ItemKind::Bullet && reordered_ids.contains(&r.item_id))
        );
    }
}
