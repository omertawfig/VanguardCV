//! Editable hierarchical section cards for VanguardCV.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Manages interactive state for all 4 [`SectionKind`] variants:
//! - Summary (multiline edit)
//! - TimelineEntries (roles, dates, currently toggle, bullet lists)
//! - CategorizedInlineList (skill categories, separators, items)
//! - SimpleBulletList (awards, honors, training courses)

use gpui::{Context, Entity, prelude::*};

use crate::contracts::{
    BulletItem, CvSection, ItemId, SectionKind, SkillCategoryItem, TimelineEntry,
};
use crate::ui::text_input::TextInput;

pub struct SectionField {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub title_input: Entity<TextInput>,
    pub enabled: bool,
    pub kind: SectionFieldKind,
}

pub enum SectionFieldKind {
    Summary { text_input: Entity<TextInput> },
    Timeline { entries: Vec<TimelineEntryField> },
    Skills { categories: Vec<SkillCategoryField> },
    BulletList { items: Vec<BulletField> },
}

pub struct TimelineEntryField {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub primary_heading_input: Entity<TextInput>,
    pub secondary_heading_input: Entity<TextInput>,
    pub start_date_input: Entity<TextInput>,
    pub end_date_input: Entity<TextInput>,
    pub is_current: bool,
    pub enabled: bool,
    pub bullets: Vec<BulletField>,
    pub tags: Vec<String>,
}

pub struct BulletField {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub text_input: Entity<TextInput>,
    pub enabled: bool,
}

pub struct SkillCategoryField {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub label_input: Entity<TextInput>,
    pub separator_input: Entity<TextInput>,
    pub items_input: Entity<TextInput>,
    pub enabled: bool,
}

impl SectionField {
    pub fn new<V: 'static>(section: &CvSection, cx: &mut Context<V>) -> Self {
        let title_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Section Title (e.g. PROFESSIONAL EXPERIENCE)")
                .with_content(section.title.clone())
        });

        let kind = match &section.kind {
            SectionKind::Summary { text } => {
                let text_input = cx.new(|cx| {
                    TextInput::new(cx)
                        .with_placeholder("Executive summary or career objective…")
                        .with_content(text.clone())
                        .with_multiline(true)
                });
                SectionFieldKind::Summary { text_input }
            }
            SectionKind::TimelineEntries { entries } => {
                let mut entry_fields = Vec::new();
                for entry in entries {
                    entry_fields.push(Self::create_timeline_entry(entry, cx));
                }
                SectionFieldKind::Timeline {
                    entries: entry_fields,
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                let mut cat_fields = Vec::new();
                for cat in categories {
                    cat_fields.push(Self::create_skill_category(cat, cx));
                }
                SectionFieldKind::Skills {
                    categories: cat_fields,
                }
            }
            SectionKind::SimpleBulletList { items } => {
                let mut bullet_fields = Vec::new();
                for bullet in items {
                    bullet_fields.push(Self::create_bullet(bullet, cx));
                }
                SectionFieldKind::BulletList {
                    items: bullet_fields,
                }
            }
        };

        Self {
            id: section.id,
            origin_item_id: section.origin_item_id,
            title_input,
            enabled: section.enabled,
            kind,
        }
    }

    pub fn create_timeline_entry<V: 'static>(
        entry: &TimelineEntry,
        cx: &mut Context<V>,
    ) -> TimelineEntryField {
        let primary_heading_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Company / Organization / University")
                .with_content(entry.primary_heading.clone())
        });

        let secondary_heading_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Role / Degree (e.g. Senior AppSec Engineer)")
                .with_content(entry.secondary_heading.clone().unwrap_or_default())
        });

        let start_date_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Start (e.g. Apr 2020)")
                .with_content(entry.start_date.clone())
        });

        let end_date_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("End (e.g. Nov 2024)")
                .with_content(entry.end_date.clone().unwrap_or_default())
        });

        let mut bullets = Vec::new();
        for b in &entry.bullets {
            bullets.push(Self::create_bullet(b, cx));
        }

        TimelineEntryField {
            id: entry.id,
            origin_item_id: entry.origin_item_id,
            primary_heading_input,
            secondary_heading_input,
            start_date_input,
            end_date_input,
            is_current: entry.is_current,
            enabled: entry.enabled,
            bullets,
            tags: entry.tags.clone(),
        }
    }

    pub fn create_bullet<V: 'static>(bullet: &BulletItem, cx: &mut Context<V>) -> BulletField {
        let text_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Achievement bullet point…")
                .with_content(bullet.text.clone())
        });

        BulletField {
            id: bullet.id,
            origin_item_id: bullet.origin_item_id,
            text_input,
            enabled: bullet.enabled,
        }
    }

    pub fn create_skill_category<V: 'static>(
        cat: &SkillCategoryItem,
        cx: &mut Context<V>,
    ) -> SkillCategoryField {
        let label_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Category (e.g. Technical skills)")
                .with_content(cat.category_label.clone())
        });

        let separator_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Separator")
                .with_content(cat.separator.clone())
        });

        let items_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Items separated by comma or separator")
                .with_content(cat.joined_items())
        });

        SkillCategoryField {
            id: cat.id,
            origin_item_id: cat.origin_item_id,
            label_input,
            separator_input,
            items_input,
            enabled: cat.enabled,
        }
    }

    /// Reads back this section into a [`CvSection`].
    pub fn read_section(&self, cx: &gpui::App) -> CvSection {
        let title = self.title_input.read(cx).text().trim().to_string();

        let kind = match &self.kind {
            SectionFieldKind::Summary { text_input } => {
                let text = text_input.read(cx).text().trim().to_string();
                SectionKind::Summary { text }
            }
            SectionFieldKind::Timeline { entries } => {
                let mut res = Vec::new();
                for e in entries {
                    let sec_heading_raw =
                        e.secondary_heading_input.read(cx).text().trim().to_string();
                    let secondary_heading = if sec_heading_raw.is_empty() {
                        None
                    } else {
                        Some(sec_heading_raw)
                    };

                    let end_raw = e.end_date_input.read(cx).text().trim().to_string();
                    let end_date = if e.is_current || end_raw.is_empty() {
                        None
                    } else {
                        Some(end_raw)
                    };

                    let mut bullets = Vec::new();
                    for b in &e.bullets {
                        bullets.push(BulletItem {
                            id: b.id,
                            origin_item_id: b.origin_item_id,
                            enabled: b.enabled,
                            text: b.text_input.read(cx).text().trim().to_string(),
                        });
                    }

                    res.push(TimelineEntry {
                        id: e.id,
                        origin_item_id: e.origin_item_id,
                        enabled: e.enabled,
                        primary_heading: e.primary_heading_input.read(cx).text().trim().to_string(),
                        secondary_heading,
                        start_date: e.start_date_input.read(cx).text().trim().to_string(),
                        end_date,
                        is_current: e.is_current,
                        bullets,
                        tags: e.tags.clone(),
                    });
                }
                SectionKind::TimelineEntries { entries: res }
            }
            SectionFieldKind::Skills { categories } => {
                let mut res = Vec::new();
                for c in categories {
                    let separator = c.separator_input.read(cx).text().to_string();
                    let sep = if separator.is_empty() {
                        " - ".to_string()
                    } else {
                        separator
                    };
                    let items_raw = c.items_input.read(cx).text();

                    // Split on either the explicit separator or comma if separator doesn't match
                    let items = if items_raw.contains(&sep) {
                        items_raw
                            .split(&sep)
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    } else {
                        items_raw
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    };

                    res.push(SkillCategoryItem {
                        id: c.id,
                        origin_item_id: c.origin_item_id,
                        enabled: c.enabled,
                        category_label: c.label_input.read(cx).text().trim().to_string(),
                        separator: sep,
                        items,
                    });
                }
                SectionKind::CategorizedInlineList { categories: res }
            }
            SectionFieldKind::BulletList { items } => {
                let mut res = Vec::new();
                for b in items {
                    res.push(BulletItem {
                        id: b.id,
                        origin_item_id: b.origin_item_id,
                        enabled: b.enabled,
                        text: b.text_input.read(cx).text().trim().to_string(),
                    });
                }
                SectionKind::SimpleBulletList { items: res }
            }
        };

        CvSection {
            id: self.id,
            origin_item_id: self.origin_item_id,
            title,
            enabled: self.enabled,
            kind,
        }
    }
}

impl SectionField {
    /// Every text input nested in this section, used to wire change handlers.
    pub fn inputs(&self) -> Vec<Entity<TextInput>> {
        let mut inputs = vec![self.title_input.clone()];
        match &self.kind {
            SectionFieldKind::Summary { text_input } => inputs.push(text_input.clone()),
            SectionFieldKind::Timeline { entries } => {
                for entry in entries {
                    inputs.extend(entry.inputs());
                }
            }
            SectionFieldKind::Skills { categories } => {
                for category in categories {
                    inputs.extend(category.inputs());
                }
            }
            SectionFieldKind::BulletList { items } => {
                for item in items {
                    inputs.extend(item.inputs());
                }
            }
        }
        inputs
    }
}

impl TimelineEntryField {
    pub fn inputs(&self) -> Vec<Entity<TextInput>> {
        let mut inputs = vec![
            self.primary_heading_input.clone(),
            self.secondary_heading_input.clone(),
            self.start_date_input.clone(),
            self.end_date_input.clone(),
        ];
        for bullet in &self.bullets {
            inputs.extend(bullet.inputs());
        }
        inputs
    }
}

impl BulletField {
    pub fn inputs(&self) -> Vec<Entity<TextInput>> {
        vec![self.text_input.clone()]
    }
}

impl SkillCategoryField {
    pub fn inputs(&self) -> Vec<Entity<TextInput>> {
        vec![
            self.label_input.clone(),
            self.separator_input.clone(),
            self.items_input.clone(),
        ]
    }
}
