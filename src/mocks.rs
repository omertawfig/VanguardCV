//! In-memory mock backends used by the GPUI workstream while the real SQLite,
//! Typst, and Sign in with ChatGPT backends are being built.
//!
//! **Ownership:** Main Agent. These implementations are intentionally simple
//! and are not shipped as production backends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use regex::RegexBuilder;
use uuid::Uuid;

use crate::contracts::*;

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RepoState {
    docs: HashMap<DocumentId, CvDocument>,
    order: Vec<DocumentId>,
    snapshots: Vec<StoredSnapshot>,
    settings: HashMap<String, String>,
}

struct StoredSnapshot {
    summary: SnapshotSummary,
    body: SanitizedCvBody,
}

#[derive(Default)]
pub struct InMemoryRepository {
    state: Mutex<RepoState>,
}

impl InMemoryRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Populates a realistic Root CV so the UI can be developed without the
    /// SQLite seed generator.
    pub fn with_sample_data() -> Self {
        let repo = Self::new();
        let doc = sample_root_document();
        let mut state = repo.state.lock().unwrap();
        state.order.push(doc.id);
        state.docs.insert(doc.id, doc);
        drop(state);
        repo
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut RepoState) -> Result<T>) -> Result<T> {
        let mut state = self.state.lock().map_err(|_| anyhow!("repo lock poisoned"))?;
        f(&mut state)
    }
}

impl CvRepository for InMemoryRepository {
    fn list_roots(&self) -> Result<Vec<CvSummary>> {
        self.with_state(|state| Ok(summaries(state, true)))
    }

    fn list_children(&self, root_id: DocumentId) -> Result<Vec<CvSummary>> {
        self.with_state(|state| {
            let mut out = Vec::new();
            for id in &state.order {
                if let Some(doc) = state.docs.get(id)
                    && doc.parent_id == Some(root_id)
                {
                    out.push(CvSummary::from(doc));
                }
            }
            Ok(out)
        })
    }

    fn list_all(&self) -> Result<Vec<CvSummary>> {
        self.with_state(|state| Ok(summaries(state, false)))
    }

    fn load(&self, id: DocumentId) -> Result<CvDocument> {
        self.with_state(|state| {
            state
                .docs
                .get(&id)
                .cloned()
                .ok_or_else(|| anyhow!("CV document {id} not found"))
        })
    }

    fn create_root(
        &self,
        title: &str,
        pii: PiiContactHeader,
        body: SanitizedCvBody,
    ) -> Result<CvDocument> {
        let mut doc = CvDocument::new_root(title);
        doc.pii_header = pii;
        doc.body = body;
        self.with_state(|state| {
            state.order.push(doc.id);
            state.docs.insert(doc.id, doc.clone());
            Ok(doc)
        })
    }

    fn save(&self, doc: &CvDocument) -> Result<()> {
        self.with_state(|state| {
            if !state.docs.contains_key(&doc.id) {
                bail!("CV document {} not found", doc.id);
            }
            let mut next = doc.clone();
            next.version = next.version.saturating_add(1);
            next.updated_at = Utc::now();
            state.docs.insert(next.id, next);
            Ok(())
        })
    }

    fn delete(&self, id: DocumentId) -> Result<()> {
        self.with_state(|state| {
            if state
                .docs
                .values()
                .any(|doc| doc.parent_id == Some(id))
            {
                bail!("cannot delete a Root CV that still has child variations");
            }
            state.docs.remove(&id);
            state.order.retain(|existing| *existing != id);
            Ok(())
        })
    }

    fn branch_sub_variation(
        &self,
        root_id: DocumentId,
        title: &str,
        job_ctx: JobContext,
    ) -> Result<CvDocument> {
        self.with_state(|state| {
            let root = state
                .docs
                .get(&root_id)
                .cloned()
                .ok_or_else(|| anyhow!("root CV {root_id} not found"))?;
            if root.parent_id.is_some() {
                bail!("cannot branch a Child Sub-Variation; select a Root CV");
            }
            let now = Utc::now();
            let child = CvDocument {
                id: Uuid::new_v4(),
                parent_id: Some(root_id),
                parent_version_snapshot: root.version,
                title: title.to_string(),
                version: 1,
                created_at: now,
                updated_at: now,
                job_context: Some(job_ctx),
                pii_header: root.pii_header.clone(),
                body: deep_clone_body(&root.body),
            };
            state.order.push(child.id);
            state.docs.insert(child.id, child.clone());
            Ok(child)
        })
    }

    fn save_ai_batch(&self, doc: &CvDocument) -> Result<()> {
        if doc.parent_id.is_none() {
            bail!("write-guard: AI batch updates are not allowed on a Root CV");
        }
        self.save(doc)
    }

    fn promote_item(&self, child_id: DocumentId, item_id: ItemId) -> Result<PromoteOutcome> {
        self.with_state(|state| {
            let child = state
                .docs
                .get(&child_id)
                .cloned()
                .ok_or_else(|| anyhow!("child CV {child_id} not found"))?;
            let root_id = child
                .parent_id
                .ok_or_else(|| anyhow!("promote requires a Child Sub-Variation"))?;
            let mut root = state
                .docs
                .get(&root_id)
                .cloned()
                .ok_or_else(|| anyhow!("parent root {root_id} not found"))?;

            let origin = child
                .body
                .find_bullet(item_id)
                .map(|bullet| (bullet.text.clone(), bullet.origin_item_id))
                .or_else(|| {
                    child.body.sections.iter().find_map(|section| match &section.kind {
                        SectionKind::CategorizedInlineList { categories } => categories
                            .iter()
                            .find(|category| category.id == item_id)
                            .map(|category| (category.items.join(", "), category.origin_item_id)),
                        _ => None,
                    })
                })
                .ok_or_else(|| anyhow!("item {item_id} not found in child CV"))?;

            let parent_item_id = match origin.1 {
                Some(parent_item_id) if root.body.find_bullet_mut(parent_item_id).is_some() => {
                    if let Some(bullet) = root.body.find_bullet_mut(parent_item_id) {
                        bullet.text = origin.0.clone();
                    }
                    parent_item_id
                }
                _ => {
                    let new_item = BulletItem::new(origin.0.clone());
                    let new_id = new_item.id;
                    if let Some(section) = root.body.sections.iter_mut().find(|section| {
                        matches!(section.kind, SectionKind::SimpleBulletList { .. })
                    }) {
                        if let SectionKind::SimpleBulletList { items } = &mut section.kind {
                            items.push(new_item);
                        }
                    } else {
                        let section = CvSection {
                            id: Uuid::new_v4(),
                            origin_item_id: None,
                            title: "ADDITIONAL ITEMS".to_string(),
                            enabled: true,
                            kind: SectionKind::SimpleBulletList {
                                items: vec![new_item],
                            },
                        };
                        root.body.sections.push(section);
                    }
                    new_id
                }
            };

            root.version += 1;
            root.updated_at = Utc::now();
            state.docs.insert(root.id, root);
            Ok(PromoteOutcome {
                parent_item_id,
                parent_document_id: root_id,
            })
        })
    }

    fn create_snapshot(&self, cv_id: DocumentId, reason: &str) -> Result<SnapshotSummary> {
        self.with_state(|state| {
            let doc = state
                .docs
                .get(&cv_id)
                .ok_or_else(|| anyhow!("CV document {cv_id} not found"))?;
            let summary = SnapshotSummary {
                id: Uuid::new_v4(),
                cv_id,
                version: doc.version,
                created_at: Utc::now(),
                reason: reason.to_string(),
            };
            state.snapshots.push(StoredSnapshot {
                summary: summary.clone(),
                body: doc.body.clone(),
            });
            Ok(summary)
        })
    }

    fn list_snapshots(&self, cv_id: DocumentId) -> Result<Vec<SnapshotSummary>> {
        self.with_state(|state| {
            Ok(state
                .snapshots
                .iter()
                .filter(|snapshot| snapshot.summary.cv_id == cv_id)
                .map(|snapshot| snapshot.summary.clone())
                .collect())
        })
    }

    fn restore_snapshot(&self, snapshot_id: Uuid) -> Result<CvDocument> {
        self.with_state(|state| {
            let snapshot = state
                .snapshots
                .iter()
                .find(|snapshot| snapshot.summary.id == snapshot_id)
                .ok_or_else(|| anyhow!("snapshot {snapshot_id} not found"))?
                .summary
                .clone();
            let mut doc = state
                .docs
                .get(&snapshot.cv_id)
                .cloned()
                .ok_or_else(|| anyhow!("CV document {} not found", snapshot.cv_id))?;
            doc.body = state
                .snapshots
                .iter()
                .find(|snapshot| snapshot.summary.id == snapshot_id)
                .expect("checked above")
                .body
                .clone();
            doc.version = snapshot.version;
            doc.updated_at = Utc::now();
            state.docs.insert(doc.id, doc.clone());
            Ok(doc)
        })
    }

    fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_state(|state| {
            state.settings.insert(key.to_string(), value.to_string());
            Ok(())
        })
    }

    fn load_setting(&self, key: &str) -> Result<Option<String>> {
        self.with_state(|state| Ok(state.settings.get(key).cloned()))
    }

    fn export_json(&self, id: DocumentId) -> Result<String> {
        let doc = self.load(id)?;
        Ok(serde_json::to_string_pretty(&doc)?)
    }

    fn import_json(&self, json: &str) -> Result<CvDocument> {
        let mut doc: CvDocument = serde_json::from_str(json)?;
        doc.id = Uuid::new_v4();
        doc.parent_id = None;
        doc.job_context = None;
        doc.created_at = Utc::now();
        doc.updated_at = doc.created_at;
        self.with_state(|state| {
            state.order.push(doc.id);
            state.docs.insert(doc.id, doc.clone());
            Ok(doc)
        })
    }

    fn backup_to(&self, path: &Path) -> Result<()> {
        let state = self.state.lock().map_err(|_| anyhow!("repo lock poisoned"))?;
        let docs: Vec<&CvDocument> = state.order.iter().filter_map(|id| state.docs.get(id)).collect();
        std::fs::write(path, serde_json::to_vec_pretty(&docs)?)?;
        Ok(())
    }
}

fn summaries(state: &RepoState, roots_only: bool) -> Vec<CvSummary> {
    state
        .order
        .iter()
        .filter_map(|id| state.docs.get(id))
        .filter(|doc| !roots_only || doc.parent_id.is_none())
        .map(|doc| {
            let mut summary = CvSummary::from(doc);
            summary.child_count = state
                .docs
                .values()
                .filter(|child| child.parent_id == Some(doc.id))
                .count();
            summary
        })
        .collect()
}

fn deep_clone_body(body: &SanitizedCvBody) -> SanitizedCvBody {
    let sections = body
        .sections
        .iter()
        .map(|section| {
            let kind = match &section.kind {
                SectionKind::Summary { text } => SectionKind::Summary { text: text.clone() },
                SectionKind::TimelineEntries { entries } => SectionKind::TimelineEntries {
                    entries: entries
                        .iter()
                        .map(|entry| TimelineEntry {
                            id: Uuid::new_v4(),
                            origin_item_id: Some(entry.id),
                            enabled: entry.enabled,
                            primary_heading: entry.primary_heading.clone(),
                            secondary_heading: entry.secondary_heading.clone(),
                            start_date: entry.start_date.clone(),
                            end_date: entry.end_date.clone(),
                            is_current: entry.is_current,
                            bullets: entry
                                .bullets
                                .iter()
                                .map(|bullet| BulletItem {
                                    id: Uuid::new_v4(),
                                    origin_item_id: Some(bullet.id),
                                    enabled: bullet.enabled,
                                    text: bullet.text.clone(),
                                })
                                .collect(),
                            tags: entry.tags.clone(),
                        })
                        .collect(),
                },
                SectionKind::CategorizedInlineList { categories } => {
                    SectionKind::CategorizedInlineList {
                        categories: categories
                            .iter()
                            .map(|category| SkillCategoryItem {
                                id: Uuid::new_v4(),
                                origin_item_id: Some(category.id),
                                enabled: category.enabled,
                                category_label: category.category_label.clone(),
                                separator: category.separator.clone(),
                                items: category.items.clone(),
                            })
                            .collect(),
                    }
                }
                SectionKind::SimpleBulletList { items } => SectionKind::SimpleBulletList {
                    items: items
                        .iter()
                        .map(|bullet| BulletItem {
                            id: Uuid::new_v4(),
                            origin_item_id: Some(bullet.id),
                            enabled: bullet.enabled,
                            text: bullet.text.clone(),
                        })
                        .collect(),
                },
            };
            CvSection {
                id: Uuid::new_v4(),
                origin_item_id: Some(section.id),
                title: section.title.clone(),
                enabled: section.enabled,
                kind,
            }
        })
        .collect();
    SanitizedCvBody { sections }
}

/// A representative Root CV mirroring the reference photo structure.
pub fn sample_root_document() -> CvDocument {
    let mut doc = CvDocument::new_root("Cybersecurity & Pentesting Master");
    doc.pii_header = PiiContactHeader {
        full_name: "Alex Morgan".to_string(),
        headline_job_title: Some("Offensive Security Engineer".to_string()),
        email: Some("alex.morgan@example.com".to_string()),
        phone: Some("+1 555 010 4242".to_string()),
        location: Some("Remote".to_string()),
        social_links: vec![SocialLink {
            id: Uuid::new_v4(),
            platform: "GitHub".to_string(),
            display_text: "github.com/alexmorgan".to_string(),
            url: "https://github.com/alexmorgan".to_string(),
            enabled: true,
        }],
        custom_fields: Vec::new(),
    };
    doc.body.sections = vec![
        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "PROFESSIONAL EXPERIENCE".to_string(),
            enabled: true,
            kind: SectionKind::TimelineEntries {
                entries: vec![
                    {
                        let mut entry =
                            TimelineEntry::new("Independent Bug Bounty Research", "Mar 2021");
                        entry.is_current = true;
                        entry.end_date = None;
                        entry.bullets = vec![
                            BulletItem::new(
                                "Reported over 40 critical and high-severity vulnerabilities across \
                                 HackerOne and Bugcrowd programs.",
                            ),
                            BulletItem::new(
                                "Built reproducible PoCs and remediation guidance for web and \
                                 cloud attack surfaces.",
                            ),
                        ];
                        entry
                    },
                    {
                        let mut entry = TimelineEntry::new("CrowdStrike", "Apr 2020");
                        entry.secondary_heading = Some("Security Analyst".to_string());
                        entry.end_date = Some("Feb 2021".to_string());
                        entry.bullets = vec![BulletItem::new(
                            "Automated detection triage in Python and reduced mean time to \
                             respond by 30%.",
                        )];
                        entry
                    },
                ],
            },
        },
        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "EDUCATION".to_string(),
            enabled: true,
            kind: SectionKind::TimelineEntries {
                entries: vec![{
                    let mut entry = TimelineEntry::new("University of Applied Sciences", "Sep 2015");
                    entry.secondary_heading = Some("B.Sc. Computer Science".to_string());
                    entry.end_date = Some("Jun 2019".to_string());
                    entry.bullets = vec![BulletItem::new(
                        "Focus on network security and distributed systems.",
                    )];
                    entry
                }],
            },
        },
        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "SKILLS AND OTHERS".to_string(),
            enabled: true,
            kind: SectionKind::CategorizedInlineList {
                categories: vec![
                    SkillCategoryItem {
                        id: Uuid::new_v4(),
                        origin_item_id: None,
                        enabled: true,
                        category_label: "Soft skills".to_string(),
                        separator: " - ".to_string(),
                        items: vec![
                            "Clear communication".to_string(),
                            "Ownership".to_string(),
                            "Mentoring".to_string(),
                        ],
                    },
                    SkillCategoryItem {
                        id: Uuid::new_v4(),
                        origin_item_id: None,
                        enabled: true,
                        category_label: "Technical skills".to_string(),
                        separator: " - ".to_string(),
                        items: vec![
                            "Rust".to_string(),
                            "Python".to_string(),
                            "Burp Suite".to_string(),
                        ],
                    },
                ],
            },
        },
        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "HONORS AND AWARDS:".to_string(),
            enabled: true,
            kind: SectionKind::SimpleBulletList {
                items: vec![BulletItem::new("Top 1% HackerOne signal, 2024")],
            },
        },
        CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: "TRAINING COURSES:".to_string(),
            enabled: true,
            kind: SectionKind::SimpleBulletList {
                items: vec![BulletItem::new("Offensive Security Certified Professional (OSCP)")],
            },
        },
    ];
    doc
}

// ---------------------------------------------------------------------------
// Privacy
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MockPrivacy;

impl PrivacyGuard for MockPrivacy {
    fn scrub(&self, body: &SanitizedCvBody, pii: &PiiContactHeader) -> Result<ScrubbedPayload> {
        let mut count = 0usize;
        let body = redact_body(body, pii, &mut count);
        let json = serde_json::to_string(&body)?;
        let token = ScrubbedPayloadToken::mint(&json);
        Ok(ScrubbedPayload {
            body,
            json,
            token,
            redaction_count: count,
        })
    }

    fn inspect_outbound_payload(
        &self,
        body: &SanitizedCvBody,
        pii: &PiiContactHeader,
    ) -> Result<String> {
        Ok(self.scrub(body, pii)?.json)
    }

    fn scan(&self, text: &str, pii: &PiiContactHeader) -> Vec<PiiMatch> {
        let mut out = Vec::new();
        if let Some((regex, kinds)) = mock_dictionary(pii) {
            for capture in regex.find_iter(text) {
                let kind = kinds
                    .iter()
                    .find(|(value, _)| value.eq_ignore_ascii_case(capture.as_str()))
                    .map(|(_, kind)| *kind)
                    .unwrap_or(PiiKind::CustomField);
                out.push(PiiMatch {
                    start: capture.start(),
                    end: capture.end(),
                    token: format!("[REDACTED_PII_{}]", out.len() + 1),
                    kind,
                });
            }
        }
        out
    }
}

fn mock_dictionary(pii: &PiiContactHeader) -> Option<(regex::Regex, Vec<(String, PiiKind)>)> {
    let mut values: Vec<(String, PiiKind)> = Vec::new();
    let mut push = |value: &str, kind: PiiKind| {
        let value = value.trim();
        if value.len() >= 3 {
            values.push((value.to_string(), kind));
        }
    };
    push(&pii.full_name, PiiKind::FullName);
    for part in pii.full_name.split_whitespace() {
        push(part, PiiKind::NameToken);
    }
    if let Some(email) = &pii.email {
        push(email, PiiKind::Email);
    }
    if let Some(phone) = &pii.phone {
        push(phone, PiiKind::Phone);
    }
    for link in &pii.social_links {
        push(&link.display_text, PiiKind::SocialHandle);
        push(&link.url, PiiKind::SocialUrl);
    }
    for field in &pii.custom_fields {
        push(&field.value, PiiKind::CustomField);
    }
    if values.is_empty() {
        return None;
    }
    let pattern = values
        .iter()
        .map(|(value, _)| regex::escape(value))
        .collect::<Vec<_>>()
        .join("|");
    let regex = RegexBuilder::new(&pattern).case_insensitive(true).build().ok()?;
    Some((regex, values))
}

fn redact_body(body: &SanitizedCvBody, pii: &PiiContactHeader, count: &mut usize) -> SanitizedCvBody {
    let mut scrubbed = body.clone();
    for section in &mut scrubbed.sections {
        section.title = redact_text(&section.title, pii, count);
        match &mut section.kind {
            SectionKind::Summary { text } => {
                *text = redact_text(text, pii, count);
            }
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    entry.primary_heading = redact_text(&entry.primary_heading, pii, count);
                    if let Some(heading) = &mut entry.secondary_heading {
                        *heading = redact_text(heading, pii, count);
                    }
                    for tag in &mut entry.tags {
                        *tag = redact_text(tag, pii, count);
                    }
                    for bullet in &mut entry.bullets {
                        bullet.text = redact_text(&bullet.text, pii, count);
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for category in categories {
                    category.category_label = redact_text(&category.category_label, pii, count);
                    for item in &mut category.items {
                        *item = redact_text(item, pii, count);
                    }
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for item in items {
                    item.text = redact_text(&item.text, pii, count);
                }
            }
        }
    }
    scrubbed
}

fn redact_text(text: &str, pii: &PiiContactHeader, count: &mut usize) -> String {
    let mut out = text.to_string();
    if let Some((dictionary, _)) = mock_dictionary(pii) {
        let mut next = *count;
        out = dictionary
            .replace_all(&out, |_: &regex::Captures<'_>| {
                next += 1;
                format!("[REDACTED_PII_{next}]")
            })
            .into_owned();
        *count = next;
    }
    for pattern in [
        r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
        r#"https?://[^\s"']+"#,
        r"\+?[0-9][0-9 ()\-]{9,}[0-9]",
    ] {
        let regex = regex::Regex::new(pattern).expect("static redaction pattern");
        let mut next = *count;
        out = regex
            .replace_all(&out, |_: &regex::Captures<'_>| {
                next += 1;
                format!("[REDACTED_PII_{next}]")
            })
            .into_owned();
        *count = next;
    }
    out
}

// ---------------------------------------------------------------------------
// PDF compiler
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MockPdfCompiler;

impl PdfCompiler for MockPdfCompiler {
    fn compile_to_pdf(&self, _pii: &PiiContactHeader, _body: &SanitizedCvBody) -> Result<Vec<u8>> {
        Ok(mock_pdf_bytes())
    }

    fn render_preview_pages(
        &self,
        _pii: &PiiContactHeader,
        _body: &SanitizedCvBody,
        max_pages: usize,
    ) -> Result<Vec<RenderedPage>> {
        let pages = max_pages.clamp(1, 3);
        Ok((0..pages)
            .map(|page_index| {
                let width = 794u32;
                let height = 1123u32;
                let mut rgba = vec![255u8; (width * height * 4) as usize];
                for y in 0..140u32 {
                    for x in 0..width {
                        let idx = ((y * width + x) * 4) as usize;
                        rgba[idx] = 240;
                        rgba[idx + 1] = 240;
                        rgba[idx + 2] = 240;
                    }
                }
                for y in 140..150u32 {
                    for x in 60..width - 60 {
                        let idx = ((y * width + x) * 4) as usize;
                        rgba[idx] = 132;
                        rgba[idx + 1] = 0;
                        rgba[idx + 2] = 16;
                    }
                }
                RenderedPage {
                    page_index,
                    width_px: width,
                    height_px: height,
                    rgba,
                }
            })
            .collect())
    }
}

fn mock_pdf_bytes() -> Vec<u8> {
    let content = "BT /F1 18 Tf 72 770 Td (VanguardCV Mock PDF) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref_offset = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref_offset
    ));
    pdf.into_bytes()
}

// ---------------------------------------------------------------------------
// AI orchestrator
// ---------------------------------------------------------------------------

pub struct MockAiOrchestrator {
    config: Mutex<AiConfig>,
    auth: Mutex<ChatGptAuthStatus>,
}

impl Default for MockAiOrchestrator {
    fn default() -> Self {
        Self {
            config: Mutex::new(AiConfig {
                base_url: "mock://chatgpt".to_string(),
                auth_issuer: "mock://auth.openai.com".to_string(),
                model: DEFAULT_CHATGPT_MODEL.to_string(),
                request_timeout_secs: 60,
                ..AiConfig::default()
            }),
            auth: Mutex::new(ChatGptAuthStatus {
                signed_in: true,
                plan_usage_enabled: true,
                email: Some("alex.morgan@example.com".to_string()),
                name: Some("Alex Morgan".to_string()),
                subject: Some("mock-sub-1".to_string()),
                client_id: Some("oaiapp_mock".to_string()),
                access_token_expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
                needs_reauth: false,
            }),
        }
    }
}

#[async_trait::async_trait]
impl AiOrchestrator for MockAiOrchestrator {
    async fn generate_variation(
        &self,
        request: VariationRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<VariationOutcome> {
        let run_id = Uuid::new_v4();
        let _ = events.send(AiEvent::RunStarted { run_id });
        let _ = events.send(AiEvent::Status {
            message: format!("Reviewing {} against the job description", request.child_cv_id),
        });

        let mut proposals = Vec::new();
        for section in &payload.body.sections {
            if let SectionKind::Summary { text } = &section.kind {
                let proposal = AiProposal::SummaryRewrite {
                    section_id: section.id,
                    original: text.clone(),
                    proposed: format!(
                        "{text} Focused on {} at {}.",
                        request.job_context.target_role, request.job_context.company_name
                    ),
                };
                let _ = events.send(AiEvent::Proposal {
                    proposal: proposal.clone(),
                });
                proposals.push(proposal);
            }
        }
        if let Some(bullet) = payload.body.bullets().first() {
            let proposal = AiProposal::BulletRewrite {
                bullet_id: bullet.id,
                original: bullet.text.clone(),
                proposed: format!("{} (aligned to the target role)", bullet.text),
                rationale: "Mock proposal: fronts the job's core terminology.".to_string(),
            };
            let _ = events.send(AiEvent::Proposal {
                proposal: proposal.clone(),
            });
            proposals.push(proposal);
        }
        let score = AtsScore {
            matched_keywords: vec!["security".to_string(), "python".to_string()],
            missing_keywords: vec!["kubernetes".to_string()],
            score: 78,
        };
        let _ = events.send(AiEvent::AtsScore { score: score.clone() });
        let _ = events.send(AiEvent::TextDelta {
            delta: "Applied selected skills.".to_string(),
        });
        let summary = "Mock variation generated.".to_string();
        let _ = events.send(AiEvent::RunCompleted {
            run_id,
            summary: summary.clone(),
        });
        Ok(VariationOutcome {
            child_cv_id: request.child_cv_id,
            proposals,
            ats_score: Some(score),
            summary,
        })
    }

    async fn rewrite_bullet(
        &self,
        request: InlineRewriteRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<Option<AiProposal>> {
        let bullet = payload
            .body
            .find_bullet(request.bullet_id)
            .ok_or_else(|| anyhow!("bullet not found in sanitized payload"))?;
        let proposal = AiProposal::BulletRewrite {
            bullet_id: request.bullet_id,
            original: bullet.text.clone(),
            proposed: format!("{} — {}", bullet.text, request.instruction),
            rationale: "Mock inline rewrite.".to_string(),
        };
        let _ = events.send(AiEvent::Proposal {
            proposal: proposal.clone(),
        });
        Ok(Some(proposal))
    }

    fn auth_status(&self) -> ChatGptAuthStatus {
        self.auth.lock().map(|a| a.clone()).unwrap_or_default()
    }

    async fn sign_in_with_chatgpt(
        &self,
        _force_consent: bool,
        _fresh_registration: bool,
    ) -> Result<ChatGptAuthStatus> {
        let status = ChatGptAuthStatus {
            signed_in: true,
            plan_usage_enabled: true,
            email: Some("alex.morgan@example.com".to_string()),
            name: Some("Alex Morgan".to_string()),
            subject: Some("mock-sub-1".to_string()),
            client_id: Some("oaiapp_mock".to_string()),
            access_token_expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            needs_reauth: false,
        };
        if let Ok(mut lock) = self.auth.lock() {
            *lock = status.clone();
        }
        Ok(status)
    }

    async fn sign_out(&self) -> Result<Option<String>> {
        if let Ok(mut lock) = self.auth.lock() {
            *lock = ChatGptAuthStatus::default();
        }
        Ok(None)
    }

    fn configure(&self, config: AiConfig) -> Result<()> {
        *self.config.lock().map_err(|_| anyhow!("config lock poisoned"))? = config;
        Ok(())
    }

    fn config(&self) -> AiConfig {
        self.config.lock().map(|config| config.clone()).unwrap_or_default()
    }

    fn is_configured(&self) -> bool {
        self.auth_status().is_ready_for_inference() && self.config().is_configured()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![
            ModelInfo {
                id: "gpt-5.4".to_string(),
                display_name: "GPT-5.4".to_string(),
                description: "Flagship ChatGPT model for deep CV tailoring and ATS analysis.".to_string(),
                input_token_limit: Some(272_000),
                output_token_limit: None,
                supports_thinking: true,
            },
            ModelInfo {
                id: "gpt-5".to_string(),
                display_name: "GPT-5".to_string(),
                description: "Balanced ChatGPT model for structured career writing.".to_string(),
                input_token_limit: Some(272_000),
                output_token_limit: None,
                supports_thinking: true,
            },
            ModelInfo {
                id: "gpt-5.4-mini".to_string(),
                display_name: "GPT-5.4 Mini".to_string(),
                description: "Fast everyday model for quick inline bullet rewrites.".to_string(),
                input_token_limit: Some(272_000),
                output_token_limit: None,
                supports_thinking: false,
            },
        ])
    }
}
// ---------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------

pub struct MockSkillRegistry {
    skills: Mutex<Vec<SkillManifest>>,
}

impl Default for MockSkillRegistry {
    fn default() -> Self {
        let skills = vec![
            SkillManifest {
                id: "ats_classic_optimizer".to_string(),
                name: "ATS Classic Optimizer".to_string(),
                category: "Formatting & ATS".to_string(),
                version: "1.0.0".to_string(),
                description: Some("Keeps the CV parseable by classic ATS engines.".to_string()),
                default_enabled: true,
                enabled: true,
                rules: toml::Table::new(),
                prompt_injection: SkillPromptInjection {
                    system_directives: "Front-load exact job-description keywords that are \
                        grounded in the candidate's real experience."
                        .to_string(),
                },
                source_path: None,
            },
            SkillManifest {
                id: "xyz_bullet_enhancer".to_string(),
                name: "XYZ Bullet Enhancer".to_string(),
                category: "Writing Quality".to_string(),
                version: "1.0.0".to_string(),
                description: Some("Rewrites bullets as Accomplished X by doing Y.".to_string()),
                default_enabled: true,
                enabled: true,
                rules: toml::Table::new(),
                prompt_injection: SkillPromptInjection {
                    system_directives: "Rewrite bullets as Accomplished X by doing Y, measured \
                        by Z. Never invent metrics."
                        .to_string(),
                },
                source_path: None,
            },
        ];
        Self {
            skills: Mutex::new(skills),
        }
    }
}

impl SkillRegistry for MockSkillRegistry {
    fn list(&self) -> Vec<SkillManifest> {
        self.skills.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn get(&self, id: &str) -> Option<SkillManifest> {
        self.list().into_iter().find(|skill| skill.id == id)
    }

    fn reload(&self) -> Result<Vec<SkillManifest>> {
        Ok(self.list())
    }

    fn compose_system_prompt(&self, skill_ids: &[String]) -> Result<String> {
        let mut prompt = String::new();
        for id in skill_ids {
            if let Some(skill) = self.get(id) {
                prompt.push_str(&skill.prompt_injection.system_directives);
                prompt.push_str("\n\n");
            }
        }
        Ok(prompt)
    }

    fn skills_dir(&self) -> PathBuf {
        PathBuf::from("skills")
    }
}

// ---------------------------------------------------------------------------
// Convenience bundle
// ---------------------------------------------------------------------------

/// Builds a complete mock [`Services`] bundle for UI development.
pub fn mock_services() -> Services {
    Services::new(
        Arc::new(InMemoryRepository::with_sample_data()),
        Arc::new(MockPrivacy),
        Arc::new(MockPdfCompiler),
        Arc::new(MockAiOrchestrator::default()),
        Arc::new(MockSkillRegistry::default()),
    )
}
