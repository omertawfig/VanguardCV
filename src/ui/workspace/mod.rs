//! Main Workspace View adapted from the Figma Make design and wired to
//! VanguardCV's real SQLite, Zero-Trust PII scrubber, Typst PDF, official OpenAI
//! Sign in with ChatGPT, and `omp` FileSkillRegistry services.
//!
//! Integrates the 6 primary workspaces:
//! 1. CV library (Searchable, filterable tree of masters and variations)
//! 2. CV editor (Hierarchical cards, PII header, inline AI review & ATS score)
//! 3. PDF preview (Live embedded Typst raster preview, page overflow warning, export)
//! 4. AI skills (`omp` skill registry management: enable/disable, import, create, delete)
//! 5. Personal info (Segregated Zero-Trust local PII contact header & payload inspector)
//! 6. Settings (Dark/Light theme, Sign in with ChatGPT & model picker, DB backup/JSON/snapshots)

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use gpui::prelude::FluentBuilder;
use gpui::*;
use image::Frame;
use tokio::sync::mpsc::unbounded_channel;
use uuid::Uuid;

use crate::contracts::{
    AiConfig, AiEvent, AiProposal, AiProviderKind, AtsScore, BulletItem, CHATGPT_API_BASE_URL,
    CHATGPT_USAGE_SETTINGS_URL, ChatGptAuthStatus, CustomHeaderField, SanitizedCvBody, CvDocument, CvSection,
    DEFAULT_CHATGPT_MODEL, DEFAULT_GEMINI_MODEL, DocumentId, GEMINI_API_BASE_URL,
    GEMINI_API_KEY_URL, InlineRewriteRequest, ItemId, JobContext, ModelInfo, RenderedPage,
    SectionKind, Services, SkillCategoryItem, SkillManifest, SnapshotSummary, SocialLink,
    TimelineEntry, VariationRequest, settings_keys,
};
use crate::credentials;
use crate::db::archive;
use crate::db::lineage_diff::{self, ItemDiff};
use crate::db::snapshots;
use crate::db::sync::DebouncedSaver;
use crate::importer::{ImportDraft, ImportLineRole, ImportSectionKind};
use crate::pdf::exporter;
use crate::ui::components::ui::{tooltip_view,
    ButtonVariant, IconName, el_id, render_brand_logo_badge, render_button, render_icon, render_icon_button,
};
use crate::ui::text_input::{TextInput, TextInputEvent};
use crate::ui::theme::{self, Theme, ThemeMode};
use crate::jobs::{
    CvSearchSource, FreehireProvider, JobDiscoveryCache, JobFilters, JobListing, JobProvider,
    MAX_PAGE_NUMBER, MAX_PAGE_SIZE, SearchProfile, compute_cv_content_hash, extract_fallback_profiles,
    generate_search_profiles_with_ai, normalize_country_code,
};
use crate::ui::views::editor::render_editor_view;
use crate::ui::views::jobs::render_jobs_view;
use crate::ui::views::library::render_library_view;
use crate::ui::views::modals::render_modal_overlay;
use crate::ui::views::navigation::render_sidebar;
use crate::ui::views::preview::{render_live_preview_pane, render_preview_view};
use crate::ui::views::privacy::render_privacy_view;
use crate::ui::views::settings::render_settings_view;
use crate::ui::views::skills::render_skills_view;
use crate::updater::{
    self, CURRENT_VERSION, DownloadedUpdate, LATEST_RELEASE_API_URL, RELEASES_PAGE_URL,
    UpdateCheckOutcome, UpdateReleaseInfo, UpdateStatus,
};

gpui::actions!(
    vanguard_cv,
    [
        NewVariation,
        ExportPdfShortcut,
        ToggleAiPanel,
        AcceptAllProposals,
        CloseModal,
    ]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceViewPage {
    Library,
    Jobs,
    Editor,
    Preview,
    Skills,
    Privacy,
    Settings,
}

impl WorkspaceViewPage {
    pub fn display_name(&self) -> &'static str {
        match self {
            WorkspaceViewPage::Library => "CV library",
            WorkspaceViewPage::Jobs => "Job discovery",
            WorkspaceViewPage::Editor => "CV editor",
            WorkspaceViewPage::Preview => "PDF preview",
            WorkspaceViewPage::Skills => "AI skills",
            WorkspaceViewPage::Privacy => "Personal info",
            WorkspaceViewPage::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryFilter {
    All,
    Masters,
    Variations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySort {
    LastEdited,
    Name,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    NewMaster,
    ImportCv,
    NewVariation {
        root_id: DocumentId,
    },
    Compare,
    PayloadPreview,
    ExportPdf,
    PromoteItem,
    EditSummary {
        section_id: ItemId,
    },
    RenameSection {
        section_id: ItemId,
    },
    EditEntry {
        section_id: ItemId,
        is_new: bool,
    },
    EditBullet {
        section_id: ItemId,
        entry_id: Option<ItemId>,
        is_new: bool,
    },
    EditCategory {
        section_id: ItemId,
        is_new: bool,
    },
    AddSection,
    RenameCv,
    EditSkill {
        is_new: bool,
        read_only: bool,
    },
    EditContact,
    EditSocialLink {
        link_id: Option<ItemId>,
    },
    EditCustomField {
        field_id: Option<ItemId>,
    },
    DocumentMenu,
    ConfirmDeleteCv {
        doc_id: DocumentId,
    },
    ConfirmResetApp,
    RewriteBullet {
        section_id: ItemId,
        bullet_id: ItemId,
    },
}

/// UI-ready wrapper around any of the 4 [`AiProposal`] variants.
#[derive(Debug, Clone, PartialEq)]
pub struct UiProposal {
    pub id: String,
    pub target_item_id: ItemId,
    pub original: String,
    pub proposed: String,
    pub rationale: String,
    pub proposal: AiProposal,
    pub pii_warning: Option<String>,
    pub is_edited: bool,
}

impl UiProposal {
    pub fn from_ai_proposal(p: &AiProposal, doc: &CvDocument) -> Self {
        let pii_warning = proposal_pii_warning(p);
        match p {
            AiProposal::BulletRewrite {
                bullet_id,
                original,
                proposed,
                rationale,
            } => Self {
                id: Uuid::new_v4().to_string(),
                target_item_id: *bullet_id,
                original: original.clone(),
                proposed: proposed.clone(),
                rationale: rationale.clone(),
                proposal: p.clone(),
                pii_warning,
                is_edited: false,
            },
            AiProposal::SummaryRewrite {
                section_id,
                original,
                proposed,
            } => Self {
                id: Uuid::new_v4().to_string(),
                target_item_id: *section_id,
                original: original.clone(),
                proposed: proposed.clone(),
                rationale: "Tailored executive summary alignment".to_string(),
                proposal: p.clone(),
                pii_warning,
                is_edited: false,
            },
            AiProposal::ToggleVisibility {
                item_id,
                enabled,
                reason,
            } => {
                let label = describe_item_in_doc(doc, *item_id);
                Self {
                    id: Uuid::new_v4().to_string(),
                    target_item_id: *item_id,
                    original: format!(
                        "{} ({})",
                        label,
                        if *enabled { "Hidden from PDF" } else { "Included in PDF" }
                    ),
                    proposed: format!(
                        "{} ({})",
                        label,
                        if *enabled { "Included in PDF" } else { "Hidden from PDF" }
                    ),
                    rationale: reason.clone(),
                    proposal: p.clone(),
                    pii_warning,
                    is_edited: false,
                }
            }
            AiProposal::SkillReorder {
                category_id,
                items,
            } => {
                let current_items = find_skill_category(doc, *category_id)
                    .map(|c| c.joined_items())
                    .unwrap_or_default();
                let sep = find_skill_category(doc, *category_id)
                    .map(|c| c.separator.as_str())
                    .unwrap_or(", ");
                Self {
                    id: Uuid::new_v4().to_string(),
                    target_item_id: *category_id,
                    original: current_items,
                    proposed: items.join(sep),
                    rationale: "Front-load job-relevant skills".to_string(),
                    proposal: p.clone(),
                    pii_warning,
                    is_edited: false,
                }
            }
        }
    }

    pub fn is_text_editable(&self) -> bool {
        matches!(
            self.proposal,
            AiProposal::BulletRewrite { .. }
                | AiProposal::SummaryRewrite { .. }
                | AiProposal::SkillReorder { .. }
        )
    }
}

pub fn update_proposal_text(
    prop: &mut UiProposal,
    edited_text: &str,
    doc: Option<&CvDocument>,
) -> Result<(), &'static str> {
    let trimmed = edited_text.trim();
    if trimmed.is_empty() {
        return Err("Proposed text cannot be empty.");
    }
    match &mut prop.proposal {
        AiProposal::BulletRewrite { proposed, .. }
        | AiProposal::SummaryRewrite { proposed, .. } => {
            *proposed = trimmed.to_string();
            prop.proposed = trimmed.to_string();
        }
        AiProposal::SkillReorder { category_id, items } => {
            let sep = doc
                .and_then(|d| find_skill_category(d, *category_id))
                .map(|c| c.separator.as_str())
                .unwrap_or(", ");
            let sep_trim = sep.trim();
            let parsed: Vec<String> = if !sep_trim.is_empty() && trimmed.contains(sep_trim) {
                trimmed
                    .split(sep_trim)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                trimmed
                    .split([',', '\n', ';'])
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };
            if parsed.is_empty() {
                return Err("Enter at least one skill.");
            }
            *items = parsed;
            prop.proposed = items.join(sep);
        }
        AiProposal::ToggleVisibility { .. } => {
            return Err("Visibility proposals cannot be edited as text.");
        }
    }
    prop.pii_warning = proposal_pii_warning(&prop.proposal);
    prop.is_edited = true;
    Ok(())
}

fn proposal_pii_warning(p: &AiProposal) -> Option<String> {
    let contains_token = match p {
        AiProposal::BulletRewrite { proposed, .. } => proposed.contains("[REDACTED_PII_"),
        AiProposal::SummaryRewrite { proposed, .. } => proposed.contains("[REDACTED_PII_"),
        AiProposal::SkillReorder { items, .. } => {
            items.iter().any(|s| s.contains("[REDACTED_PII_"))
        }
        AiProposal::ToggleVisibility { .. } => false,
    };
    if contains_token {
        Some(
            "Blocked: proposal contains a redaction placeholder ([REDACTED_PII_...]). Edit manually or revert."
                .to_string(),
        )
    } else {
        None
    }
}

fn find_skill_category(doc: &CvDocument, category_id: ItemId) -> Option<&SkillCategoryItem> {
    for sec in &doc.body.sections {
        if let SectionKind::CategorizedInlineList { categories } = &sec.kind {
            if let Some(cat) = categories.iter().find(|c| c.id == category_id) {
                return Some(cat);
            }
        }
    }
    None
}

fn describe_item_in_doc(doc: &CvDocument, item_id: ItemId) -> String {
    for sec in &doc.body.sections {
        if sec.id == item_id {
            return format!("Section \"{}\"", sec.title);
        }
        match &sec.kind {
            SectionKind::TimelineEntries { entries } => {
                for entry in entries {
                    if entry.id == item_id {
                        return format!("Entry \"{}\"", entry.primary_heading);
                    }
                    for bullet in &entry.bullets {
                        if bullet.id == item_id {
                            return truncate_preview(&bullet.text, 72);
                        }
                    }
                }
            }
            SectionKind::SimpleBulletList { items } => {
                for bullet in items {
                    if bullet.id == item_id {
                        return truncate_preview(&bullet.text, 72);
                    }
                }
            }
            SectionKind::CategorizedInlineList { categories } => {
                for cat in categories {
                    if cat.id == item_id {
                        return format!("Category \"{}\"", cat.category_label);
                    }
                }
            }
            SectionKind::Summary { .. } => {}
        }
    }
    format!("Item {item_id}")
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(max_chars).collect();
        s.push('…');
        s
    }
}

#[derive(Clone, Debug, Default)]
pub struct AiAgentState {
    pub is_running: bool,
    pub target_doc_id: Option<DocumentId>,
    pub step: String,
    pub streamed_text: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorDragKind {
    Section {
        section_id: ItemId,
    },
    TimelineEntry {
        section_id: ItemId,
        entry_id: ItemId,
    },
    Bullet {
        section_id: ItemId,
        entry_id: Option<ItemId>,
        bullet_id: ItemId,
    },
    SkillCategory {
        section_id: ItemId,
        category_id: ItemId,
    },
    SocialLink {
        link_id: ItemId,
    },
    CustomHeaderField {
        field_id: ItemId,
    },
}

#[derive(Clone, Debug)]
pub struct EditorDragPayload {
    pub kind: EditorDragKind,
    pub label: String,
    pub sublabel: Option<String>,
}

pub struct DragGhostView {
    label: String,
    sublabel: Option<String>,
    theme: Theme,
}

impl DragGhostView {
    pub fn new(payload: &EditorDragPayload, theme: Theme) -> Self {
        Self {
            label: payload.label.clone(),
            sublabel: payload.sublabel.clone(),
            theme,
        }
    }
}

impl Render for DragGhostView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;
        div()
            .max_w(px(340.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(6.0))
            .bg(t.surface)
            .border_1()
            .border_color(t.accent)
            .shadow_lg()
            .opacity(0.95)
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .flex_shrink_0()
                    .child(render_icon(IconName::Grip, 14.0, t.accent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(self.label.clone()),
                    )
                    .when_some(self.sublabel.clone(), |el, sub| {
                        el.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(t.secondary)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(sub),
                        )
                    }),
            )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewPanState {
    pub is_split: bool,
    pub start_mouse: Point<Pixels>,
    pub start_offset: Point<Pixels>,
}

pub struct WorkspaceView {
    pub focus_handle: FocusHandle,
    pub services: Services,
    pub debounced_saver: Arc<DebouncedSaver>,
    pub theme: Theme,
    pub current_view: WorkspaceViewPage,
    pub documents: Vec<CvDocument>,
    pub active_doc_id: Option<DocumentId>,
    pub default_cv_id: Option<DocumentId>,
    pub proposals: HashMap<DocumentId, Vec<UiProposal>>,
    pub ats_scores: HashMap<DocumentId, AtsScore>,
    pub ai_summaries: HashMap<DocumentId, String>,
    pub snapshots: Vec<SnapshotSummary>,
    pub ai_state: AiAgentState,
    pub active_dialog: Option<Dialog>,
    pub notice: Option<String>,
    pub notice_is_error: bool,
    pub notice_generation: u64,
    pub scheduled_notice_generation: u64,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub sidebar_user_toggle: Option<bool>,
    pub library_query: String,
    pub library_sort: LibrarySort,
    pub library_filter: LibraryFilter,
    pub collapsed_masters: HashSet<DocumentId>,
    pub expanded_sections: HashMap<ItemId, bool>,
    pub expanded_entries: HashMap<ItemId, bool>,
    pub preview_zoom: u32,
    pub split_preview_open: bool,
    pub split_preview_zoom: u32,
    pub split_preview_scroll: ScrollHandle,
    pub full_preview_scroll: ScrollHandle,
    pub models_list_scroll: ScrollHandle,
    pub preview_pan_state: Option<PreviewPanState>,
    pub active_drag_item: Option<EditorDragKind>,
    pub drag_did_reorder: bool,
    pub rendered_images: Vec<Arc<RenderImage>>,
    pub preview_error: Option<String>,
    pub is_rendering_pdf: bool,
    pub is_exporting_pdf: bool,
    pub save_generation: u64,
    pub pdf_render_generation: u64,

    // Form inputs
    pub master_title_input: Entity<TextInput>,
    pub variation_company_input: Entity<TextInput>,
    pub variation_role_input: Entity<TextInput>,
    pub variation_jd_input: Entity<TextInput>,
    pub variation_skills: HashSet<String>,
    pub rename_title_input: Entity<TextInput>,
    pub rename_section_input: Entity<TextInput>,
    pub summary_text_input: Entity<TextInput>,
    pub entry_org_input: Entity<TextInput>,
    pub entry_role_input: Entity<TextInput>,
    pub entry_start_input: Entity<TextInput>,
    pub entry_end_input: Entity<TextInput>,
    pub entry_is_current: bool,
    pub bullet_text_input: Entity<TextInput>,
    pub proposal_edit_input: Entity<TextInput>,
    pub rewrite_instruction_input: Entity<TextInput>,
    pub rewrite_selected_skills: HashSet<String>,
    pub cat_label_input: Entity<TextInput>,
    pub cat_items_input: Entity<TextInput>,
    pub cat_separator: String,
    pub sec_title_input: Entity<TextInput>,
    pub sec_type_selection: String,
    pub skill_name_input: Entity<TextInput>,
    pub skill_desc_input: Entity<TextInput>,
    pub skill_category_input: Entity<TextInput>,
    pub skill_inst_input: Entity<TextInput>,
    pub pii_name_input: Entity<TextInput>,
    pub pii_headline_input: Entity<TextInput>,
    pub pii_email_input: Entity<TextInput>,
    pub pii_phone_input: Entity<TextInput>,
    pub pii_location_input: Entity<TextInput>,
    pub social_platform_input: Entity<TextInput>,
    pub social_display_input: Entity<TextInput>,
    pub social_url_input: Entity<TextInput>,
    pub custom_field_label_input: Entity<TextInput>,
    pub custom_field_value_input: Entity<TextInput>,
    pub search_input: Entity<TextInput>,
    pub active_provider: AiProviderKind,
    pub chatgpt_auth: ChatGptAuthStatus,
    pub auth_in_progress: bool,
    pub auth_error: Option<String>,
    pub show_plan_welcome_banner: bool,
    pub gemini_api_key_input: Entity<TextInput>,
    pub gemini_key_visible: bool,
    pub gemini_key_configured: bool,
    pub selected_model: String,
    pub selected_gemini_model: String,
    pub available_models: Vec<ModelInfo>,
    pub models_loading: bool,
    pub models_error: Option<String>,
    pub models_fetch_generation: u64,
    pub model_filter_input: Entity<TextInput>,
    pub model_filter_query: String,
    pub promote_target: Option<(ItemId, ItemId, Option<ItemId>)>,
    pub editing_entry_id: Option<ItemId>,
    pub editing_bullet_id: Option<ItemId>,
    pub editing_category_id: Option<ItemId>,
    pub editing_skill_id: Option<String>,
    pub editing_proposal_id: Option<String>,
    pub job_source: Option<CvSearchSource>,
    pub job_profiles: Vec<SearchProfile>,
    pub job_profiles_from_cache: bool,
    pub job_profiles_used_fallback: bool,
    pub job_filters: JobFilters,
    pub job_country_dropdown_open: bool,
    pub job_country_scroll: UniformListScrollHandle,
    pub job_query_input: Entity<TextInput>,
    pub job_listings: Vec<JobListing>,
    pub job_total_estimated: u64,
    pub job_has_next_page: bool,
    pub job_listings_from_cache: bool,
    pub job_profiles_stale: bool,
    pub job_warnings: Vec<String>,
    pub job_expanded_slugs: HashSet<String>,
    pub job_profiles_loading: bool,
    pub job_listings_loading: bool,
    pub job_error: Option<String>,
    pub job_cache: JobDiscoveryCache,
    pub job_provider: Arc<dyn JobProvider>,
    pub job_profile_generation: u64,
    pub job_search_generation: u64,
    pub job_last_searched_signature: Option<String>,
    pub(super) active_job_search_task: Option<(String, tokio::task::AbortHandle)>,
    pub auto_update_enabled: bool,
    pub update_status: UpdateStatus,
    pub update_release: Option<UpdateReleaseInfo>,
    pub update_downloaded: Option<DownloadedUpdate>,
    pub update_error: Option<String>,
    pub update_generation: u64,
    pub import_draft: Option<ImportDraft>,
    pub import_title_input: Entity<TextInput>,
    pub import_loading: bool,
    pub import_lines_scroll: ScrollHandle,
    pub import_preview_scroll: ScrollHandle,
    pub _search_sub: Subscription,
    pub _model_filter_sub: Subscription,
    pub _job_query_sub: Subscription,
}

impl WorkspaceView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let debounced_saver = Arc::new(DebouncedSaver::new(
            services.repository.clone(),
            Duration::from_millis(400),
        ));

        // Restore persisted theme preference
        let initial_mode = services
            .repository
            .load_setting(settings_keys::UI_PREFERENCES)
            .ok()
            .flatten()
            .and_then(|raw| {
                serde_json::from_str::<serde_json::Value>(&raw)
                    .ok()
                    .and_then(|v| v.get("theme").and_then(|t| t.as_str()).map(str::to_string))
                    .or(Some(raw))
            })
            .map(|val| {
                if val.eq_ignore_ascii_case("light") {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                }
            })
            .unwrap_or(ThemeMode::Dark);
        theme::set_active_theme_mode(initial_mode);
        let initial_theme = match initial_mode {
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::Light => Theme::light(),
        };

        // Load all documents from repository; start with no CV loaded in Current Document
        // so the user explicitly chooses a CV from CV library or the sidebar switcher.
        let docs = load_all_documents(&services);
        let active_doc_id: Option<DocumentId> = None;
        let default_cv_id: Option<DocumentId> = services
            .repository
            .load_setting(settings_keys::DEFAULT_CV_ID)
            .ok()
            .flatten()
            .and_then(|raw| Uuid::parse_str(raw.trim()).ok())
            .filter(|id| docs.iter().any(|d| d.id == *id));
        let mut job_cache = JobDiscoveryCache::default();
        if let Ok(Some(saved_profiles_json)) =
            services.repository.load_setting(settings_keys::JOB_PROFILES_CACHE)
        {
            job_cache.load_persisted_profiles(&saved_profiles_json);
        }
        let initial_snapshots: Vec<SnapshotSummary> = Vec::new();

        // Create text inputs
        let master_title_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Principal Security Engineer"));
        let import_title_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("Imported Master CV title"));
        let variation_company_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., CrowdStrike"));
        let variation_role_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Senior Detection Engineer"));
        let variation_jd_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Paste the target job description or key requirements…")
        });
        let rename_title_input = cx.new(|cx| TextInput::new(cx).with_placeholder("CV name"));
        let rename_section_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("Section title"));
        let summary_text_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Write a concise, factual introduction to your experience…")
        });
        let entry_org_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("Organization or university"));
        let entry_role_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("Role or qualification"));
        let entry_start_input = cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Apr 2022"));
        let entry_end_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Present or Dec 2024"));
        let bullet_text_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Describe what you accomplished and the measurable impact…")
        });
        let proposal_edit_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Edit the proposed text before keeping it…")
        });
        let rewrite_instruction_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Optional instruction, e.g., Quantify impact and tighten to one line…")
        });
        let cat_label_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Programming Languages"));
        let cat_items_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Comma-separated skills, e.g., Rust, Python, Go, Kubernetes")
        });
        let sec_title_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Selected Projects"));
        let skill_name_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Executive Conciseness"));
        let skill_desc_input = cx.new(|cx| {
            TextInput::new(cx).with_placeholder("Short summary of what this skill enforces")
        });
        let skill_category_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., Formatting, Writing, ATS"));
        let skill_inst_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_multiline(true)
                .with_placeholder("Detailed directives injected into the AI system prompt…")
        });
        let pii_name_input = cx.new(|cx| TextInput::new(cx).with_placeholder("Full name"));
        let pii_headline_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("Professional headline"));
        let pii_email_input = cx.new(|cx| TextInput::new(cx).with_placeholder("Email address"));
        let pii_phone_input = cx.new(|cx| TextInput::new(cx).with_placeholder("Phone number"));
        let pii_location_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("City, Country or Remote"));
        let social_platform_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., GitHub, LinkedIn, Portfolio"));
        let social_display_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., github.com/alexmorgan"));
        let social_url_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., https://github.com/alexmorgan"));
        let custom_field_label_input = cx.new(|cx| {
            TextInput::new(cx).with_placeholder("e.g., Security Clearance, Work Authorization")
        });
        let custom_field_value_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("e.g., UK SC Cleared / EU Citizen"));
        let search_input = cx.new(|cx| TextInput::new(cx).with_placeholder("Search CVs…"));

        let ai_cfg = services.ai.config();
        let active_provider = ai_cfg.provider;
        let chatgpt_auth = services.ai.auth_status();
        let selected_model = if ai_cfg.model.trim().is_empty() {
            DEFAULT_CHATGPT_MODEL.to_string()
        } else {
            ai_cfg.model.clone()
        };
        let selected_gemini_model = if ai_cfg.gemini_model.trim().is_empty() {
            DEFAULT_GEMINI_MODEL.to_string()
        } else {
            ai_cfg.gemini_model.clone()
        };
        let initial_gemini_key = ai_cfg.gemini_api_key.clone().unwrap_or_default();
        let gemini_key_configured = !initial_gemini_key.trim().is_empty();
        let gemini_api_key_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Paste your Google Gemini API key (AIza…)")
                .with_masked(true)
                .with_value(initial_gemini_key)
        });
        let model_filter_input =
            cx.new(|cx| TextInput::new(cx).with_placeholder("Filter available models…"));
        let job_query_input = cx.new(|cx| {
            TextInput::new(cx).with_placeholder(
                "Optional custom search override (e.g., Rust Security Engineer)…",
            )
        });

        let _search_sub = cx.subscribe(&search_input, |this, _, event: &TextInputEvent, cx| {
            let TextInputEvent::Change(val) = event;
            this.library_query = val.clone();
            cx.notify();
        });
        let _model_filter_sub =
            cx.subscribe(&model_filter_input, |this, _, event: &TextInputEvent, cx| {
                let TextInputEvent::Change(val) = event;
                this.model_filter_query = val.clone();
                this.models_list_scroll.set_offset(point(px(0.0), px(0.0)));
                cx.notify();
            });
        let _job_query_sub =
            cx.subscribe(&job_query_input, |this, _, event: &TextInputEvent, cx| {
                let TextInputEvent::Change(val) = event;
                this.job_filters.manual_query = val.clone();
                this.job_filters.page = 1;
                cx.notify();
            });

        let saved_job_country = services
            .repository
            .load_setting(settings_keys::JOB_COUNTRY)
            .ok()
            .flatten()
            .and_then(|raw| normalize_country_code(&raw));
        let initial_job_filters = JobFilters {
            country: saved_job_country,
            ..JobFilters::default()
        };
        let auto_update_enabled = services
            .repository
            .load_setting(settings_keys::AUTO_UPDATE_ENABLED)
            .ok()
            .flatten()
            .map(|raw| {
                let norm = raw.trim().to_ascii_lowercase();
                !matches!(norm.as_str(), "false" | "0" | "off" | "no")
            })
            .unwrap_or(true);

        let mut view = Self {
            focus_handle: cx.focus_handle(),
            services,
            debounced_saver,
            theme: initial_theme,
            current_view: WorkspaceViewPage::Library,
            documents: docs,
            active_doc_id,
            default_cv_id,
            proposals: HashMap::new(),
            ats_scores: HashMap::new(),
            ai_summaries: HashMap::new(),
            snapshots: initial_snapshots,
            ai_state: AiAgentState::default(),
            active_dialog: None,
            notice: None,
            notice_is_error: false,
            notice_generation: 0,
            scheduled_notice_generation: 0,
            viewport_width: 1280.0,
            viewport_height: 820.0,
            sidebar_user_toggle: None,
            library_query: String::new(),
            library_sort: LibrarySort::LastEdited,
            library_filter: LibraryFilter::All,
            collapsed_masters: HashSet::new(),
            expanded_sections: HashMap::new(),
            expanded_entries: HashMap::new(),
            preview_zoom: 100,
            split_preview_open: true,
            split_preview_zoom: 100,
            split_preview_scroll: ScrollHandle::new(),
            full_preview_scroll: ScrollHandle::new(),
            models_list_scroll: ScrollHandle::new(),
            preview_pan_state: None,
            active_drag_item: None,
            drag_did_reorder: false,
            rendered_images: Vec::new(),
            preview_error: None,
            is_rendering_pdf: false,
            is_exporting_pdf: false,
            save_generation: 0,
            pdf_render_generation: 0,
            master_title_input,
            variation_company_input,
            variation_role_input,
            variation_jd_input,
            variation_skills: HashSet::new(),
            rename_title_input,
            rename_section_input,
            summary_text_input,
            entry_org_input,
            entry_role_input,
            entry_start_input,
            entry_end_input,
            entry_is_current: false,
            bullet_text_input,
            proposal_edit_input,
            rewrite_instruction_input,
            rewrite_selected_skills: HashSet::new(),
            cat_label_input,
            cat_items_input,
            cat_separator: ", ".to_string(),
            sec_title_input,
            sec_type_selection: "Timeline entries".to_string(),
            skill_name_input,
            skill_desc_input,
            skill_category_input,
            skill_inst_input,
            pii_name_input,
            pii_headline_input,
            pii_email_input,
            pii_phone_input,
            pii_location_input,
            social_platform_input,
            social_display_input,
            social_url_input,
            custom_field_label_input,
            custom_field_value_input,
            search_input,
            active_provider,
            chatgpt_auth,
            auth_in_progress: false,
            auth_error: None,
            show_plan_welcome_banner: false,
            gemini_api_key_input,
            gemini_key_visible: false,
            gemini_key_configured,
            selected_model,
            selected_gemini_model,
            available_models: Vec::new(),
            models_loading: false,
            models_error: None,
            models_fetch_generation: 0,
            model_filter_input,
            model_filter_query: String::new(),
            promote_target: None,
            editing_entry_id: None,
            editing_bullet_id: None,
            editing_category_id: None,
            editing_skill_id: None,
            editing_proposal_id: None,
            job_source: None,
            job_profiles: Vec::new(),
            job_profiles_from_cache: false,
            job_profiles_used_fallback: false,
            job_filters: initial_job_filters,
            job_country_dropdown_open: false,
            job_country_scroll: UniformListScrollHandle::new(),
            job_query_input,
            job_listings: Vec::new(),
            job_total_estimated: 0,
            job_has_next_page: false,
            job_listings_from_cache: false,
            job_profiles_stale: false,
            job_warnings: Vec::new(),
            job_expanded_slugs: HashSet::new(),
            job_profiles_loading: false,
            job_listings_loading: false,
            job_error: None,
            job_cache,
            job_provider: Arc::new(FreehireProvider::default()),
            job_profile_generation: 0,
            job_search_generation: 0,
            job_last_searched_signature: None,
            active_job_search_task: None,
            auto_update_enabled,
            update_status: UpdateStatus::Idle,
            update_release: None,
            update_downloaded: None,
            update_error: None,
            update_generation: 0,
            import_draft: None,
            import_title_input,
            import_loading: false,
            import_lines_scroll: ScrollHandle::new(),
            import_preview_scroll: ScrollHandle::new(),
            _search_sub,
            _model_filter_sub,
            _job_query_sub,
        };

        if view.is_ai_configured() {
            view.fetch_available_models(false, cx);
        }
        view.bootstrap_default_cv_job_search(cx);
        if view.auto_update_enabled {
            view.schedule_startup_update_check(cx);
        }
        view
    }

    // -----------------------------------------------------------------------
    // Cross-Platform Native File Dialogs & External Browser Launchers
    // -----------------------------------------------------------------------

    /// Opens a native single-file open dialog. On Windows, runs on a dedicated
    /// COM STA thread with `IFileOpenDialog` + automatic `GetOpenFileNameW`
    /// fallback so popups never silently fail or stall GPUI's DirectX thread.
    pub(super) fn prompt_open_single_file(
        request: crate::platform::OpenFileRequest,
        #[allow(unused_variables)] cx: &mut Context<Self>,
    ) -> tokio::sync::oneshot::Receiver<anyhow::Result<Option<PathBuf>>> {
        #[cfg(target_os = "windows")]
        {
            crate::platform::pick_open_file_windows(request)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(request.title.into()),
            });
            let (tx, out_rx) = tokio::sync::oneshot::channel();
            cx.spawn(async move |_, _| {
                let outcome = match rx.await {
                    Ok(Ok(Some(mut paths))) => Ok(paths.pop()),
                    Ok(Ok(None)) | Err(_) => Ok(None),
                    Ok(Err(err)) => Err(err),
                };
                let _ = tx.send(outcome);
            })
            .detach();
            out_rx
        }
    }

    /// Opens a native single-file save dialog. On Windows, runs on a dedicated
    /// COM STA thread with `IFileSaveDialog` + automatic `GetSaveFileNameW`
    /// fallback.
    pub(super) fn prompt_save_single_file(
        request: crate::platform::SaveFileRequest,
        #[allow(unused_variables)] cx: &mut Context<Self>,
    ) -> tokio::sync::oneshot::Receiver<anyhow::Result<Option<PathBuf>>> {
        #[cfg(target_os = "windows")]
        {
            crate::platform::pick_save_file_windows(request)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let initial_dir = request
                .initial_dir
                .unwrap_or_else(Self::initial_save_dir);
            let rx = cx.prompt_for_new_path(&initial_dir, request.default_name.as_deref());
            let (tx, out_rx) = tokio::sync::oneshot::channel();
            cx.spawn(async move |_, _| {
                let outcome = match rx.await {
                    Ok(Ok(path)) => Ok(path),
                    Ok(Err(err)) => Err(err),
                    Err(_) => Ok(None),
                };
                let _ = tx.send(outcome);
            })
            .detach();
            out_rx
        }
    }

    /// Opens an external `http://` or `https://` URL in the system browser
    /// using `ShellExecuteW` directly on Windows (zero `powershell.exe` spawns).
    pub(super) fn open_url_or_notify(&mut self, url: &str, context_label: &str) {
        if let Err(err) = crate::platform::open_external_url(url) {
            self.notify_error(format!("Could not open {context_label} ({url}): {err}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Free Helpers
// ---------------------------------------------------------------------------

fn load_all_documents(services: &Services) -> Vec<CvDocument> {
    let mut docs = Vec::new();
    if let Ok(roots) = services.repository.list_roots() {
        for root_summary in roots {
            if let Ok(root_doc) = services.repository.load(root_summary.id) {
                docs.push(root_doc);
            }
            if let Ok(children) = services.repository.list_children(root_summary.id) {
                for child_summary in children {
                    if let Ok(child_doc) = services.repository.load(child_summary.id) {
                        docs.push(child_doc);
                    }
                }
            }
        }
    }
    docs
}

fn apply_ai_proposal(doc: &mut CvDocument, proposal: &AiProposal) {
    match proposal {
        AiProposal::BulletRewrite {
            bullet_id,
            proposed,
            ..
        } => {
            if let Some(bullet) = doc.body.find_bullet_mut(*bullet_id) {
                bullet.text = proposed.clone();
            }
        }
        AiProposal::SummaryRewrite {
            section_id,
            proposed,
            ..
        } => {
            if let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == *section_id)
                && let SectionKind::Summary { text } = &mut sec.kind
            {
                *text = proposed.clone();
            }
        }
        AiProposal::ToggleVisibility {
            item_id, enabled, ..
        } => {
            for sec in &mut doc.body.sections {
                if sec.id == *item_id {
                    sec.enabled = *enabled;
                    return;
                }
                match &mut sec.kind {
                    SectionKind::TimelineEntries { entries } => {
                        for entry in entries {
                            if entry.id == *item_id {
                                entry.enabled = *enabled;
                                return;
                            }
                            for bullet in &mut entry.bullets {
                                if bullet.id == *item_id {
                                    bullet.enabled = *enabled;
                                    return;
                                }
                            }
                        }
                    }
                    SectionKind::SimpleBulletList { items } => {
                        for bullet in items {
                            if bullet.id == *item_id {
                                bullet.enabled = *enabled;
                                return;
                            }
                        }
                    }
                    SectionKind::CategorizedInlineList { categories } => {
                        for cat in categories {
                            if cat.id == *item_id {
                                cat.enabled = *enabled;
                                return;
                            }
                        }
                    }
                    SectionKind::Summary { .. } => {}
                }
            }
        }
        AiProposal::SkillReorder {
            category_id,
            items,
        } => {
            for sec in &mut doc.body.sections {
                if let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
                    && let Some(cat) = categories.iter_mut().find(|c| c.id == *category_id)
                {
                    cat.items = items.clone();
                    return;
                }
            }
        }
    }
}

fn slugify_skill_id(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn escape_yaml_str(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "")
        .replace('\n', "\\n")
}

fn rendered_page_to_image(page: &RenderedPage) -> Option<Arc<RenderImage>> {
    if page.rgba.len() != (page.width_px * page.height_px * 4) as usize {
        return None;
    }
    let mut bgra = page.rgba.clone();
    for chunk in bgra.as_chunks_mut::<4>().0 {
        chunk.swap(0, 2);
    }
    let frame = Frame::new(image::RgbaImage::from_raw(
        page.width_px,
        page.height_px,
        bgra,
    )?);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

impl Drop for WorkspaceView {
    fn drop(&mut self) {
        if let Some((_, abort_handle)) = self.active_job_search_task.take() {
            abort_handle.abort();
        }
        let _ = self.debounced_saver.flush();
    }
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}


mod ai_actions;
mod documents;
mod editor_actions;
mod import_actions;
mod jobs_actions;
mod render;
pub use render::{drag_item_index_pair_in_doc, reorder_item_in_body, reorder_item_in_doc};
mod settings_actions;

#[cfg(test)]
mod tests;
