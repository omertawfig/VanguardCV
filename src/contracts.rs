//! Shared contracts for every VanguardCV subsystem.
//!
//! **Ownership:** the Main Agent (Lead Architect & Orchestrator) exclusively
//! writes this file. Sub-agents implement against these types and traits and
//! must not modify them; request a contract change through the orchestrator.
//!
//! The contract deliberately keeps the domain model, service traits, and
//! streaming event types together so that all three workstreams (data/privacy,
//! AI/PDF, and GPUI) can be developed in parallel against a single source of
//! truth.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Identifiers & storage keys
// ---------------------------------------------------------------------------

pub type DocumentId = Uuid;
pub type ItemId = Uuid;

/// Keys used with [`CvRepository::save_setting`] / [`CvRepository::load_setting`].
pub mod settings_keys {
    /// JSON-serialized [`super::AiConfig`].
    pub const AI_CONFIG: &str = "ai.config";
    /// JSON-serialized UI preferences (theme, pane widths, ...).
    pub const UI_PREFERENCES: &str = "ui.preferences";
    /// JSON-serialized list of skill ids enabled by default.
    pub const DEFAULT_SKILL_IDS: &str = "skills.default_enabled";
    /// UUID of the CV marked as Default in the CV library.
    pub const DEFAULT_CV_ID: &str = "workspace.default_cv_id";
    /// Persisted per-CV Job Discovery search profiles keyed by CV source + content hash.
    pub const JOB_PROFILES_CACHE: &str = "jobs.saved_profiles_v1";
    /// Preferred ISO 3166-1 alpha-2 country code for Job Discovery searches (empty = Worldwide).
    pub const JOB_COUNTRY: &str = "jobs.country";
    /// Whether automatic background update checks and downloads are enabled ("true"/"false").
    pub const AUTO_UPDATE_ENABLED: &str = "app.auto_update_enabled";
}

// ---------------------------------------------------------------------------
// Core domain model
// ---------------------------------------------------------------------------

/// A complete CV document: either a Root Master CV or a Child Sub-Variation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvDocument {
    pub id: DocumentId,
    /// `None` = Root Master CV, `Some(id)` = Child Sub-Variation.
    pub parent_id: Option<DocumentId>,
    /// Tracks which revision of the parent this child branched from.
    pub parent_version_snapshot: u32,
    /// e.g. "Offensive Security Master" or "CrowdStrike - Red Team".
    pub title: String,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Populated on Child Sub-Variations only.
    pub job_context: Option<JobContext>,
    /// STRICTLY LOCAL. Never serialized into an AI request.
    pub pii_header: PiiContactHeader,
    /// Safe for AI once it has passed through [`PrivacyGuard`].
    pub body: SanitizedCvBody,
}

impl CvDocument {
    /// Creates a new empty Root CV.
    pub fn new_root(title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            parent_id: None,
            parent_version_snapshot: 1,
            title: title.into(),
            version: 1,
            created_at: now,
            updated_at: now,
            job_context: None,
            pii_header: PiiContactHeader::default(),
            body: SanitizedCvBody::default(),
        }
    }

    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    pub fn is_child(&self) -> bool {
        self.parent_id.is_some()
    }
}

/// Lightweight document descriptor used by the sidebar tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvSummary {
    pub id: DocumentId,
    pub parent_id: Option<DocumentId>,
    pub title: String,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub job_context: Option<JobContext>,
    /// Number of Child Sub-Variations (always `0` for children).
    pub child_count: usize,
}

impl CvSummary {
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    pub fn display_title(&self) -> String {
        match &self.job_context {
            Some(ctx) => format!("{} — {}", self.title, ctx.target_role),
            None => self.title.clone(),
        }
    }
}

impl From<&CvDocument> for CvSummary {
    fn from(doc: &CvDocument) -> Self {
        Self {
            id: doc.id,
            parent_id: doc.parent_id,
            title: doc.title.clone(),
            version: doc.version,
            created_at: doc.created_at,
            updated_at: doc.updated_at,
            job_context: doc.job_context.clone(),
            child_count: 0,
        }
    }
}

/// Context describing a specific job application (Child Sub-Variations only).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JobContext {
    pub company_name: String,
    pub target_role: String,
    pub job_description_raw: String,
    pub application_url: Option<String>,
    pub applied_skill_ids: Vec<String>,
    pub ats_match_score: Option<u8>,
}

impl JobContext {
    pub fn label(&self) -> String {
        if self.company_name.is_empty() {
            self.target_role.clone()
        } else if self.target_role.is_empty() {
            self.company_name.clone()
        } else {
            format!("{} — {}", self.company_name, self.target_role)
        }
    }
}

// ---------------------------------------------------------------------------
// Strictly-local PII header
// ---------------------------------------------------------------------------

/// Strictly-local personally identifiable information.
///
/// This type intentionally lives outside [`SanitizedCvBody`] and must never be
/// handed to the AI orchestrator. Only [`PrivacyGuard`] may read it in order to
/// build a redaction dictionary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PiiContactHeader {
    pub full_name: String,
    pub headline_job_title: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub location: Option<String>,
    pub social_links: Vec<SocialLink>,
    pub custom_fields: Vec<CustomHeaderField>,
}

impl PiiContactHeader {
    pub fn is_empty(&self) -> bool {
        self.full_name.trim().is_empty()
            && self
                .headline_job_title
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            && self.email.as_deref().unwrap_or("").trim().is_empty()
            && self.phone.as_deref().unwrap_or("").trim().is_empty()
            && self.location.as_deref().unwrap_or("").trim().is_empty()
            && self.social_links.is_empty()
            && self.custom_fields.is_empty()
    }

    /// Contact line items used in the centered PDF header, in display order.
    pub fn contact_items(&self) -> Vec<String> {
        let mut items = Vec::new();
        if let Some(email) = self.email.as_deref().filter(|v| !v.trim().is_empty()) {
            items.push(email.trim().to_string());
        }
        if let Some(phone) = self.phone.as_deref().filter(|v| !v.trim().is_empty()) {
            items.push(phone.trim().to_string());
        }
        if let Some(location) = self.location.as_deref().filter(|v| !v.trim().is_empty()) {
            items.push(location.trim().to_string());
        }
        for field in self.custom_fields.iter().filter(|f| f.enabled) {
            let value = field.value.trim();
            if !value.is_empty() {
                let label = field.label.trim();
                if label.is_empty() {
                    items.push(value.to_string());
                } else {
                    items.push(format!("{label}: {value}"));
                }
            }
        }
        items
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialLink {
    pub id: Uuid,
    /// e.g. "LinkedIn", "GitHub", "Portfolio".
    pub platform: String,
    /// e.g. "linkedin.com/in/handle".
    pub display_text: String,
    /// e.g. "https://linkedin.com/in/handle".
    pub url: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomHeaderField {
    pub id: Uuid,
    pub label: String,
    pub value: String,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Hierarchical CV body
// ---------------------------------------------------------------------------

/// The AI-visible CV body. Contains no contact PII by construction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SanitizedCvBody {
    pub sections: Vec<CvSection>,
}

impl SanitizedCvBody {
    pub fn section(&self, id: ItemId) -> Option<&CvSection> {
        self.sections.iter().find(|s| s.id == id)
    }

    pub fn section_mut(&mut self, id: ItemId) -> Option<&mut CvSection> {
        self.sections.iter_mut().find(|s| s.id == id)
    }

    pub fn enabled_sections(&self) -> impl Iterator<Item = &CvSection> {
        self.sections.iter().filter(|s| s.enabled)
    }

    /// Every bullet in the body, in document order.
    pub fn bullets(&self) -> Vec<&BulletItem> {
        let mut out = Vec::new();
        for section in &self.sections {
            match &section.kind {
                SectionKind::TimelineEntries { entries } => {
                    for entry in entries.iter().filter(|e| e.enabled) {
                        out.extend(entry.bullets.iter());
                    }
                }
                SectionKind::SimpleBulletList { items } => out.extend(items.iter()),
                SectionKind::Summary { .. } | SectionKind::CategorizedInlineList { .. } => {}
            }
        }
        out
    }

    pub fn find_bullet(&self, id: ItemId) -> Option<&BulletItem> {
        for section in &self.sections {
            match &section.kind {
                SectionKind::TimelineEntries { entries } => {
                    for entry in entries {
                        if let Some(bullet) = entry.bullets.iter().find(|b| b.id == id) {
                            return Some(bullet);
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if let Some(bullet) = items.iter().find(|b| b.id == id) {
                        return Some(bullet);
                    }
                }
                SectionKind::Summary { .. } | SectionKind::CategorizedInlineList { .. } => {}
            }
        }
        None
    }

    pub fn find_bullet_mut(&mut self, id: ItemId) -> Option<&mut BulletItem> {
        for section in &mut self.sections {
            match &mut section.kind {
                SectionKind::TimelineEntries { entries } => {
                    for entry in &mut *entries {
                        if let Some(bullet) = entry.bullets.iter_mut().find(|b| b.id == id) {
                            return Some(bullet);
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if let Some(bullet) = items.iter_mut().find(|b| b.id == id) {
                        return Some(bullet);
                    }
                }
                SectionKind::Summary { .. } | SectionKind::CategorizedInlineList { .. } => {}
            }
        }
        None
    }

    /// Finds the section that owns a given bullet.
    pub fn section_of_bullet(&self, bullet_id: ItemId) -> Option<&CvSection> {
        self.sections.iter().find(|section| match &section.kind {
            SectionKind::TimelineEntries { entries } => entries
                .iter()
                .any(|e| e.bullets.iter().any(|b| b.id == bullet_id)),
            SectionKind::SimpleBulletList { items } => items.iter().any(|b| b.id == bullet_id),
            SectionKind::Summary { .. } | SectionKind::CategorizedInlineList { .. } => false,
        })
    }

    /// Finds any item (section, entry, bullet, skill category) by id.
    pub fn contains_item(&self, id: ItemId) -> bool {
        for section in &self.sections {
            if section.id == id {
                return true;
            }
            match &section.kind {
                SectionKind::TimelineEntries { entries } => {
                    if entries
                        .iter()
                        .any(|e| e.id == id || e.bullets.iter().any(|b| b.id == id))
                    {
                        return true;
                    }
                }
                SectionKind::CategorizedInlineList { categories } => {
                    if categories.iter().any(|c| c.id == id) {
                        return true;
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if items.iter().any(|b| b.id == id) {
                        return true;
                    }
                }
                SectionKind::Summary { .. } => {}
            }
        }
        false
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CvSection {
    pub id: ItemId,
    /// Links this section back to the Root CV item it was cloned from.
    pub origin_item_id: Option<ItemId>,
    /// Rendered uppercase in the PDF, e.g. "PROFESSIONAL EXPERIENCE".
    pub title: String,
    /// Can be toggled off per Child Sub-Variation.
    pub enabled: bool,
    pub kind: SectionKind,
}

impl CvSection {
    pub fn kind_name(&self) -> &'static str {
        match &self.kind {
            SectionKind::Summary { .. } => "Summary",
            SectionKind::TimelineEntries { .. } => "Timeline",
            SectionKind::CategorizedInlineList { .. } => "Skills",
            SectionKind::SimpleBulletList { .. } => "Bullet list",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SectionKind {
    /// Paragraph summary or objective.
    Summary { text: String },
    /// Chronological roles (PROFESSIONAL EXPERIENCE, EDUCATION, ...).
    TimelineEntries { entries: Vec<TimelineEntry> },
    /// Inline bold category label + separated list (SKILLS AND OTHERS).
    CategorizedInlineList { categories: Vec<SkillCategoryItem> },
    /// Flat bulleted list (HONORS AND AWARDS, TRAINING COURSES).
    SimpleBulletList { items: Vec<BulletItem> },
}

impl SectionKind {
    pub fn entry_count(&self) -> usize {
        match self {
            SectionKind::TimelineEntries { entries } => entries.len(),
            SectionKind::CategorizedInlineList { categories } => categories.len(),
            SectionKind::SimpleBulletList { items } => items.len(),
            SectionKind::Summary { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub enabled: bool,
    /// Line 1 left: company / organization / university.
    pub primary_heading: String,
    /// Line 2 left: job title / degree (bold).
    pub secondary_heading: Option<String>,
    /// e.g. "Apr 2020".
    pub start_date: String,
    /// `None` with `is_current = true` renders "(Currently)".
    pub end_date: Option<String>,
    pub is_current: bool,
    pub bullets: Vec<BulletItem>,
    pub tags: Vec<String>,
}

impl TimelineEntry {
    pub fn new(primary_heading: impl Into<String>, start_date: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            origin_item_id: None,
            enabled: true,
            primary_heading: primary_heading.into(),
            secondary_heading: None,
            start_date: start_date.into(),
            end_date: None,
            is_current: false,
            bullets: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Human-readable date range used by the editor and the PDF template.
    pub fn date_range(&self) -> String {
        let start = self.start_date.trim();
        if self.is_current {
            if start.is_empty() {
                return "(Currently)".to_string();
            }
            return format!("{} - (Currently)", start);
        }
        match self.end_date.as_deref().map(str::trim) {
            Some(end) if !end.is_empty() && !start.is_empty() => format!("{} - {}", start, end),
            Some(end) if !end.is_empty() => end.to_string(),
            _ => start.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillCategoryItem {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub enabled: bool,
    /// e.g. "Soft skills", "Technical skills", "Languages".
    pub category_label: String,
    /// Default: " - " or ", ".
    pub separator: String,
    pub items: Vec<String>,
}

impl SkillCategoryItem {
    /// Joins the items with the category separator, skipping empties.
    pub fn joined_items(&self) -> String {
        self.items
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(&self.separator)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BulletItem {
    pub id: ItemId,
    pub origin_item_id: Option<ItemId>,
    pub enabled: bool,
    pub text: String,
}

impl BulletItem {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            origin_item_id: None,
            enabled: true,
            text: text.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshots, lineage & promotion
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotSummary {
    pub id: Uuid,
    pub cv_id: DocumentId,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromoteOutcome {
    /// The item id inside the parent document.
    pub parent_item_id: ItemId,
    pub parent_document_id: DocumentId,
}

// ---------------------------------------------------------------------------
// Privacy air-gap
// ---------------------------------------------------------------------------

/// Opaque proof that a payload went through [`PrivacyGuard`].
///
/// Fields are private and the only constructor is `pub(crate)`, so the AI
/// layer cannot fabricate a token by hand; it can only pass one along.
#[derive(Debug, Clone)]
pub struct ScrubbedPayloadToken {
    nonce: Uuid,
    issued_at: DateTime<Utc>,
    payload_digest: u64,
}

impl ScrubbedPayloadToken {
    /// Intended for [`PrivacyGuard`] implementations only.
    pub(crate) fn mint(payload_json: &str) -> Self {
        // FNV-1a: cheap, deterministic fingerprint to bind the token to the
        // exact serialized payload. This is an integration guard, not crypto.
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in payload_json.as_bytes() {
            digest ^= u64::from(*byte);
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self {
            nonce: Uuid::new_v4(),
            issued_at: Utc::now(),
            payload_digest: digest,
        }
    }

    pub fn nonce(&self) -> Uuid {
        self.nonce
    }

    pub fn issued_at(&self) -> DateTime<Utc> {
        self.issued_at
    }

    pub fn payload_digest(&self) -> u64 {
        self.payload_digest
    }
}

/// A body that has passed through the scrubber, plus the outbound JSON that a
/// user can audit in the "Inspect AI Payload" modal.
#[derive(Debug, Clone)]
pub struct ScrubbedPayload {
    pub body: SanitizedCvBody,
    pub json: String,
    pub token: ScrubbedPayloadToken,
    /// Number of PII matches replaced by the scrubber (for the UI badge).
    pub redaction_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiKind {
    FullName,
    NameToken,
    Email,
    Phone,
    SocialUrl,
    SocialHandle,
    CustomField,
    RegexEmail,
    RegexPhone,
    RegexUrl,
    IpAddress,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PiiMatch {
    pub start: usize,
    pub end: usize,
    pub token: String,
    pub kind: PiiKind,
}

// ---------------------------------------------------------------------------
// Service traits
//
// Every service is `Send + Sync` and is stored as an `Arc<dyn ...>` in
// [`Services`]. Backends may use blocking I/O internally; the integration
// layer is responsible for running them on a blocking worker when needed.
// ---------------------------------------------------------------------------

pub trait CvRepository: Send + Sync {
    fn list_roots(&self) -> Result<Vec<CvSummary>>;
    fn list_children(&self, root_id: DocumentId) -> Result<Vec<CvSummary>>;
    fn list_all(&self) -> Result<Vec<CvSummary>>;

    fn load(&self, id: DocumentId) -> Result<CvDocument>;

    fn create_root(
        &self,
        title: &str,
        pii: PiiContactHeader,
        body: SanitizedCvBody,
    ) -> Result<CvDocument>;

    /// Persists manual edits. Allowed on roots and children.
    fn save(&self, doc: &CvDocument) -> Result<()>;

    /// Deletes a document. Roots with children must fail rather than cascade.
    fn delete(&self, id: DocumentId) -> Result<()>;

    /// Deep-clones `root_id` into a new Child Sub-Variation.
    ///
    /// Every cloned section / entry / bullet / skill category must keep an
    /// `origin_item_id` pointing at the Root item it came from.
    fn branch_sub_variation(
        &self,
        root_id: DocumentId,
        title: &str,
        job_ctx: JobContext,
    ) -> Result<CvDocument>;

    /// AI-originated batch updates. MUST return an error when
    /// `doc.parent_id.is_none()` (the Root write-guard).
    fn save_ai_batch(&self, doc: &CvDocument) -> Result<()>;

    /// Push a new bullet / skill / course added in a child up to its Root CV.
    fn promote_item(&self, child_id: DocumentId, item_id: ItemId) -> Result<PromoteOutcome>;

    fn create_snapshot(&self, cv_id: DocumentId, reason: &str) -> Result<SnapshotSummary>;
    fn list_snapshots(&self, cv_id: DocumentId) -> Result<Vec<SnapshotSummary>>;
    fn restore_snapshot(&self, snapshot_id: Uuid) -> Result<CvDocument>;

    fn save_setting(&self, key: &str, value: &str) -> Result<()>;
    fn load_setting(&self, key: &str) -> Result<Option<String>>;

    /// Portable JSON export of one document (PII included; explicit user action).
    fn export_json(&self, id: DocumentId) -> Result<String>;
    fn import_json(&self, json: &str) -> Result<CvDocument>;

    /// Consistent single-file backup of the database.
    fn backup_to(&self, path: &std::path::Path) -> Result<()>;

    /// Wipes all CV documents, snapshots, and user settings, leaving an empty workspace.
    fn reset_all(&self) -> Result<()> {
        anyhow::bail!("this repository does not support factory reset")
    }
}

/// Zero-trust PII scrubber contract.
pub trait PrivacyGuard: Send + Sync {
    /// Scans `body` against a dictionary built from `pii` plus regex patterns
    /// and returns the sanitized body and the exact JSON that may be sent to
    /// the AI provider, bound to a [`ScrubbedPayloadToken`].
    fn scrub(&self, body: &SanitizedCvBody, pii: &PiiContactHeader) -> Result<ScrubbedPayload>;

    /// Read-only rendering of the outbound JSON for the payload inspector.
    fn inspect_outbound_payload(
        &self,
        body: &SanitizedCvBody,
        pii: &PiiContactHeader,
    ) -> Result<String>;

    /// Finds PII matches in arbitrary text (used by tests and the UI scanner).
    fn scan(&self, text: &str, pii: &PiiContactHeader) -> Vec<PiiMatch>;
}

/// One rasterized PDF page, row-major RGBA8.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    pub page_index: usize,
    pub width_px: u32,
    pub height_px: u32,
    pub rgba: Vec<u8>,
}

/// Embedded Typst compiler contract.
pub trait PdfCompiler: Send + Sync {
    /// Compiles the PII header + sanitized body into PDF bytes.
    fn compile_to_pdf(&self, pii: &PiiContactHeader, body: &SanitizedCvBody) -> Result<Vec<u8>>;

    /// Rasterizes pages for the GPUI preview viewport.
    fn render_preview_pages(
        &self,
        pii: &PiiContactHeader,
        body: &SanitizedCvBody,
        max_pages: usize,
    ) -> Result<Vec<RenderedPage>>;
}

// ---------------------------------------------------------------------------
// Skills engine
// ---------------------------------------------------------------------------

/// Serde default for [`SkillManifest::enabled`].
fn default_skill_enabled() -> bool {
    true
}

/// A parsed `omp`-style skill manifest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default_enabled: bool,
    /// Whether the skill is currently available to the agent. Disabled skills
    /// are still listed so they can be re-enabled, but are never included in a
    /// system prompt.
    #[serde(default = "default_skill_enabled")]
    pub enabled: bool,
    /// Free-form rule table (`[rules]` in the TOML manifest).
    #[serde(default)]
    pub rules: toml::Table,
    #[serde(default)]
    pub prompt_injection: SkillPromptInjection,
    /// Path the manifest was loaded from; not serialized.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillPromptInjection {
    #[serde(default)]
    pub system_directives: String,
}

pub trait SkillRegistry: Send + Sync {
    fn list(&self) -> Vec<SkillManifest>;
    fn get(&self, id: &str) -> Option<SkillManifest>;
    /// Re-reads the skills directory; the watcher calls this on change.
    fn reload(&self) -> Result<Vec<SkillManifest>>;
    /// Concatenates system directives for the given skill ids.
    fn compose_system_prompt(&self, skill_ids: &[String]) -> Result<String>;
    /// `~/.config/vanguard-cv/skills` (created on demand).
    fn skills_dir(&self) -> PathBuf;

    /// Enables or disables a skill without deleting its files.
    fn set_skill_enabled(&self, _id: &str, _enabled: bool) -> Result<()> {
        anyhow::bail!("this skill registry does not support enable/disable")
    }

    /// Imports a Markdown `SKILL.md` (or TOML manifest) into the user skills
    /// directory and returns the parsed manifest.
    fn import_skill(&self, _source: &std::path::Path) -> Result<SkillManifest> {
        anyhow::bail!("this skill registry does not support importing skills")
    }

    /// Deletes a skill (both user skills and default/bundled skills).
    fn delete_skill(&self, _id: &str) -> Result<()> {
        anyhow::bail!("this skill registry does not support deleting skills")
    }

    /// Restores any previously deleted bundled default skills and returns the
    /// number of restored skill IDs.
    fn restore_bundled_skills(&self) -> Result<usize> {
        Ok(0)
    }

    /// Returns `true` when at least one bundled default skill has been deleted.
    fn has_deleted_bundled_skills(&self) -> bool {
        false
    }

    /// Deletes all custom user skills and restores every bundled default skill to enabled.
    fn reset_to_defaults(&self) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AI orchestrator (Official OpenAI Sign in with ChatGPT + Google Gemini API)
// ---------------------------------------------------------------------------

/// Official OpenAI OIDC / OAuth 2.0 issuer for Sign in with ChatGPT.
pub const CHATGPT_AUTH_ISSUER: &str = "https://auth.openai.com";

/// Official OpenAI API v1 base URL (supports `GET /v1/models` and `POST /v1/responses`).
pub const CHATGPT_API_BASE_URL: &str = "https://api.openai.com/v1";

/// Official ChatGPT usage dashboard where users review plan and app limits.
pub const CHATGPT_USAGE_SETTINGS_URL: &str = "https://chatgpt.com/settings/usage";

/// Human-readable application name sent via `agent_name_hint` on initial registration.
pub const CHATGPT_AGENT_NAME_HINT: &str = "VanguardCV";

/// OAuth scope required for direct ChatGPT plan usage on OpenAI's Responses API.
pub const CHATGPT_PLAN_USAGE_SCOPE: &str = "chatgpt.tokens.use.direct";

/// Fallback default ChatGPT model slug used before `GET /v1/models` is fetched.
pub const DEFAULT_CHATGPT_MODEL: &str = "gpt-5";

/// Official Google Gemini Generative Language API base URL.
pub const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Google AI Studio key management page.
pub const GEMINI_API_KEY_URL: &str = "https://aistudio.google.com/apikey";

/// Default Gemini model slug used before `GET /models` is fetched.
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";

/// Active AI backend selected by the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    #[default]
    ChatGpt,
    GeminiApi,
}

impl AiProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AiProviderKind::ChatGpt => "chatgpt",
            AiProviderKind::GeminiApi => "gemini",
        }
    }

    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "gemini" | "gemini_api" | "google" => AiProviderKind::GeminiApi,
            _ => AiProviderKind::ChatGpt,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AiProviderKind::ChatGpt => "ChatGPT plan",
            AiProviderKind::GeminiApi => "Google Gemini API",
        }
    }
}

/// UI-safe summary of the user's Sign in with ChatGPT authentication state.
///
/// Never carries raw access, refresh, or ID tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGptAuthStatus {
    /// `true` when an active ChatGPT sign-in session is present.
    pub signed_in: bool,
    /// `true` when granted scopes include `chatgpt.tokens.use.direct`.
    pub plan_usage_enabled: bool,
    /// User email from the verified OIDC ID token, if present.
    pub email: Option<String>,
    /// User display name from the verified OIDC ID token, if present.
    pub name: Option<String>,
    /// Stable OpenAI account subject (`sub`) claim.
    pub subject: Option<String>,
    /// Dynamically issued OAuth client ID (`oaiapp_...`), if registered.
    pub client_id: Option<String>,
    /// Expiration timestamp of the current access token, if known.
    pub access_token_expires_at: Option<DateTime<Utc>>,
    /// `true` when a previously connected session needs reauthorization.
    pub needs_reauth: bool,
}

impl ChatGptAuthStatus {
    /// Returns `true` when VanguardCV can run AI requests using the user's ChatGPT plan.
    pub fn is_ready_for_inference(&self) -> bool {
        self.signed_in && self.plan_usage_enabled && !self.needs_reauth
    }

    /// Best human-readable account label for Settings and status banners.
    pub fn account_label(&self) -> String {
        match (
            self.name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            self.email
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            (Some(name), Some(email)) if !name.eq_ignore_ascii_case(email) => {
                format!("{name} ({email})")
            }
            (_, Some(email)) => email.to_string(),
            (Some(name), None) => name.to_string(),
            (None, None) => "ChatGPT account".to_string(),
        }
    }
}

/// Metadata for an available AI model returned by the active provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model slug passed to the provider API (e.g. `"gpt-5.4"` or `"gemini-2.5-flash"`).
    pub id: String,
    /// Human-readable display name (e.g. `"GPT-5.4"` or `"Gemini 2.5 Flash"`).
    pub display_name: String,
    /// Short provider description of the model.
    pub description: String,
    /// Maximum supported input context tokens, if reported.
    pub input_token_limit: Option<u64>,
    /// Maximum supported output tokens, if reported.
    pub output_token_limit: Option<u64>,
    /// Whether the model supports reasoning/thinking mode.
    pub supports_thinking: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub provider: AiProviderKind,
    pub base_url: String,
    pub auth_issuer: String,
    pub model: String,
    #[serde(default = "default_gemini_base_url")]
    pub gemini_base_url: String,
    #[serde(default)]
    pub gemini_api_key: Option<String>,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
    pub request_timeout_secs: u64,
}

fn default_gemini_base_url() -> String {
    GEMINI_API_BASE_URL.to_string()
}

fn default_gemini_model() -> String {
    DEFAULT_GEMINI_MODEL.to_string()
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: AiProviderKind::ChatGpt,
            base_url: CHATGPT_API_BASE_URL.to_string(),
            auth_issuer: CHATGPT_AUTH_ISSUER.to_string(),
            model: DEFAULT_CHATGPT_MODEL.to_string(),
            gemini_base_url: GEMINI_API_BASE_URL.to_string(),
            gemini_api_key: None,
            gemini_model: DEFAULT_GEMINI_MODEL.to_string(),
            request_timeout_secs: 120,
        }
    }
}

impl AiConfig {
    pub fn active_model(&self) -> &str {
        match self.provider {
            AiProviderKind::ChatGpt => self.model.as_str(),
            AiProviderKind::GeminiApi => self.gemini_model.as_str(),
        }
    }

    pub fn is_gemini_configured(&self) -> bool {
        self.gemini_api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
            && !self.gemini_model.trim().is_empty()
    }

    pub fn is_configured(&self) -> bool {
        match self.provider {
            AiProviderKind::ChatGpt => !self.model.trim().is_empty(),
            AiProviderKind::GeminiApi => self.is_gemini_configured(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtsScore {
    pub matched_keywords: Vec<String>,
    pub missing_keywords: Vec<String>,
    pub score: u8,
}

impl AtsScore {
    pub fn normalized_score(&self) -> f32 {
        f32::from(self.score.min(100)) / 100.0
    }
}

/// Request for the full "Generate Job Variation" agent run.
#[derive(Debug, Clone)]
pub struct VariationRequest {
    /// The already-branched Child Sub-Variation being tailored.
    pub child_cv_id: DocumentId,
    pub job_context: JobContext,
    /// `omp` skill ids selected in the wizard.
    pub skill_ids: Vec<String>,
}

/// Request for the granular inline copilot.
#[derive(Debug, Clone)]
pub struct InlineRewriteRequest {
    pub cv_id: DocumentId,
    pub bullet_id: ItemId,
    /// User instruction, e.g. "make it one line and quantified".
    pub instruction: String,
    pub skill_ids: Vec<String>,
}

/// A deterministic, reviewable change proposed by the AI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiProposal {
    ToggleVisibility {
        item_id: ItemId,
        enabled: bool,
        reason: String,
    },
    BulletRewrite {
        bullet_id: ItemId,
        original: String,
        proposed: String,
        rationale: String,
    },
    SummaryRewrite {
        section_id: ItemId,
        original: String,
        proposed: String,
    },
    SkillReorder {
        category_id: ItemId,
        items: Vec<String>,
    },
}

impl AiProposal {
    /// The document item this proposal mutates.
    pub fn target_item_id(&self) -> ItemId {
        match self {
            AiProposal::ToggleVisibility { item_id, .. } => *item_id,
            AiProposal::BulletRewrite { bullet_id, .. } => *bullet_id,
            AiProposal::SummaryRewrite { section_id, .. } => *section_id,
            AiProposal::SkillReorder { category_id, .. } => *category_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariationOutcome {
    pub child_cv_id: DocumentId,
    pub proposals: Vec<AiProposal>,
    pub ats_score: Option<AtsScore>,
    pub summary: String,
}

/// Streaming events emitted by the agent loop into the UI.
#[derive(Debug, Clone)]
pub enum AiEvent {
    RunStarted {
        run_id: Uuid,
    },
    Status {
        message: String,
    },
    TextDelta {
        delta: String,
    },
    ToolCall {
        name: String,
        arguments: serde_json::Value,
    },
    Proposal {
        proposal: AiProposal,
    },
    AtsScore {
        score: AtsScore,
    },
    RunCompleted {
        run_id: Uuid,
        summary: String,
    },
    RunFailed {
        run_id: Uuid,
        message: String,
    },
}

pub type AiEventSink = UnboundedSender<AiEvent>;

/// Multi-provider streaming AI orchestrator contract (Official OpenAI Sign in
/// with ChatGPT + Official Google Gemini API).
#[async_trait]
pub trait AiOrchestrator: Send + Sync {
    /// Streams a full tailoring pass against a scrubbed payload.
    ///
    /// The implementation MUST reject requests whose `payload.token` was not
    /// minted by a [`PrivacyGuard`] implementation.
    async fn generate_variation(
        &self,
        request: VariationRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<VariationOutcome>;

    /// Runs a single-bullet rewrite and emits [`AiProposal::BulletRewrite`].
    async fn rewrite_bullet(
        &self,
        request: InlineRewriteRequest,
        payload: ScrubbedPayload,
        events: AiEventSink,
    ) -> Result<Option<AiProposal>>;

    /// Runs a lightweight, non-CV-mutating structured completion (e.g., PII-stripped
    /// job search profile generation) using the active AI provider and model.
    async fn complete_structured_prompt(
        &self,
        _system_prompt: String,
        _user_prompt: String,
    ) -> Result<String> {
        anyhow::bail!("Structured prompt completion is not supported by this orchestrator")
    }

    /// Fetches available models for the currently active provider.
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(Vec::new())
    }

    /// Returns the current Sign in with ChatGPT session status (UI-safe, no tokens).
    fn auth_status(&self) -> ChatGptAuthStatus {
        ChatGptAuthStatus::default()
    }

    /// Launches the official Sign in with ChatGPT browser OAuth + PKCE loopback flow.
    async fn sign_in_with_chatgpt(
        &self,
        force_consent: bool,
        fresh_registration: bool,
    ) -> Result<ChatGptAuthStatus> {
        let _ = (force_consent, fresh_registration);
        anyhow::bail!("Sign in with ChatGPT is not supported by this orchestrator")
    }

    /// Revokes the active refresh token at OpenAI's OIDC revocation endpoint and
    /// clears local tokens while preserving the persistent `ext_agent_host_id`.
    async fn sign_out(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn configure(&self, config: AiConfig) -> Result<()>;
    fn config(&self) -> AiConfig;
    fn is_configured(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Service registry passed to the UI
// ---------------------------------------------------------------------------

/// All backends the GPUI layer is allowed to touch. Cheap to clone.
#[derive(Clone)]
pub struct Services {
    pub repository: Arc<dyn CvRepository>,
    pub privacy: Arc<dyn PrivacyGuard>,
    pub pdf: Arc<dyn PdfCompiler>,
    pub ai: Arc<dyn AiOrchestrator>,
    pub skills: Arc<dyn SkillRegistry>,
}

impl Services {
    pub fn new(
        repository: Arc<dyn CvRepository>,
        privacy: Arc<dyn PrivacyGuard>,
        pdf: Arc<dyn PdfCompiler>,
        ai: Arc<dyn AiOrchestrator>,
        skills: Arc<dyn SkillRegistry>,
    ) -> Self {
        Self {
            repository,
            privacy,
            pdf,
            ai,
            skills,
        }
    }
}
