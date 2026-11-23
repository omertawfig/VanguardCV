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
use crate::pdf::exporter;
use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_button, render_icon, render_icon_button,
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
                            .text_size(px(12.5))
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
    pub job_country_scroll: ScrollHandle,
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
    pub auto_update_enabled: bool,
    pub update_status: UpdateStatus,
    pub update_release: Option<UpdateReleaseInfo>,
    pub update_downloaded: Option<DownloadedUpdate>,
    pub update_error: Option<String>,
    pub update_generation: u64,
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
            job_country_scroll: ScrollHandle::new(),
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
            auto_update_enabled,
            update_status: UpdateStatus::Idle,
            update_release: None,
            update_downloaded: None,
            update_error: None,
            update_generation: 0,
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
    // Persistence & Live Preview Helpers
    // -----------------------------------------------------------------------

    /// Marks the active document dirty, schedules a debounced SQLite flush, and
    /// schedules a debounced Typst PDF preview refresh.
    pub fn persist_active_doc(&mut self, cx: &mut Context<Self>) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        if let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) {
            doc.version += 1;
            doc.updated_at = Utc::now();
            self.debounced_saver.schedule(doc.clone());
        }

        self.save_generation = self.save_generation.wrapping_add(1);
        let generation = self.save_generation;
        let saver = self.debounced_saver.clone();

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;

            let still_latest = this
                .read_with(cx, |view, _| view.save_generation == generation)
                .unwrap_or(false);
            if !still_latest {
                return;
            }

            let flush_result = cx
                .background_executor()
                .spawn(async move { saver.flush() })
                .await;

            let _ = this.update(cx, |view, cx| {
                if let Err(err) = flush_result {
                    view.notify_error(format!("Failed to save changes: {err}"));
                }
                view.trigger_pdf_preview_render(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Synchronously flushes any pending debounced edits to SQLite right now.
    pub fn flush_pending_now(&mut self) {
        if let Err(err) = self.debounced_saver.flush() {
            tracing::error!("Failed to flush pending edits to SQLite: {err}");
            self.notify_error(format!("Could not save pending changes: {err}"));
        }
    }

    /// Reloads all documents from SQLite, preserving `active_doc_id` if still present.
    pub fn reload_documents_from_db(&mut self, cx: &mut Context<Self>) {
        self.documents = load_all_documents(&self.services);
        if let Some(active_id) = self.active_doc_id
            && !self.documents.iter().any(|d| d.id == active_id)
        {
            self.active_doc_id = None;
            self.rendered_images.clear();
            self.preview_error = None;
        }
        self.refresh_snapshots();
        if self.active_doc_id.is_some() {
            self.trigger_pdf_preview_render(cx);
        }
    }

    pub fn refresh_snapshots(&mut self) {
        let Some(active_id) = self.active_doc_id else {
            self.snapshots.clear();
            return;
        };
        let mut list = self
            .services
            .repository
            .list_snapshots(active_id)
            .unwrap_or_default();
        list.reverse(); // newest first for UI display
        self.snapshots = list;
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn active_document(&self) -> Option<&CvDocument> {
        let active_id = self.active_doc_id?;
        self.documents.iter().find(|d| d.id == active_id)
    }

    pub fn find_document(&self, id: DocumentId) -> Option<&CvDocument> {
        self.documents.iter().find(|d| d.id == id)
    }

    pub fn active_proposals(&self) -> Vec<UiProposal> {
        self.active_doc_id
            .and_then(|id| self.proposals.get(&id))
            .cloned()
            .unwrap_or_default()
    }

    pub fn active_ats_score(&self) -> Option<&AtsScore> {
        self.active_doc_id.and_then(|id| self.ats_scores.get(&id))
    }

    pub fn active_ai_summary(&self) -> Option<&str> {
        self.active_doc_id
            .and_then(|id| self.ai_summaries.get(&id))
            .map(String::as_str)
    }

    pub fn dismiss_active_ai_summary(&mut self) {
        if let Some(id) = self.active_doc_id {
            self.ai_summaries.remove(&id);
        }
    }

    /// Scrolls the Settings available-models list vertically, returning `true`
    /// when the scroll offset actually moved (so nested scroll handlers know
    /// whether to stop propagation to the outer Settings page).
    pub fn scroll_models_list_by(&mut self, dy: Pixels, cx: &mut Context<Self>) -> bool {
        let cur = self.models_list_scroll.offset();
        let max_y = self.models_list_scroll.max_offset().height;
        if max_y <= px(0.5) {
            return false;
        }
        let next_y = (cur.y + dy).clamp(-max_y, px(0.0));
        if (next_y - cur.y).abs() > px(0.1) {
            self.models_list_scroll.set_offset(point(cur.x, next_y));
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn reset_models_list_scroll(&self) {
        self.models_list_scroll.set_offset(point(px(0.0), px(0.0)));
    }

    pub fn find_proposal_for_item(&self, item_id: ItemId, _text: &str) -> Option<UiProposal> {
        let active_id = self.active_doc_id?;
        self.proposals
            .get(&active_id)?
            .iter()
            .find(|p| p.target_item_id == item_id)
            .cloned()
    }

    pub fn active_skills(&self) -> Vec<SkillManifest> {
        self.services.skills.list()
    }

    pub fn is_user_owned_skill(&self, skill: &SkillManifest) -> bool {
        let Some(src) = skill.source_path.as_ref() else {
            return false;
        };
        let user_dir = self.services.skills.skills_dir();
        let canon_user = user_dir.canonicalize().unwrap_or(user_dir);
        let canon_src = src.canonicalize().unwrap_or_else(|_| src.clone());
        canon_src.starts_with(&canon_user)
    }

    pub fn is_ai_configured(&self) -> bool {
        self.services.ai.is_configured()
    }

    pub fn active_model_id(&self) -> &str {
        match self.active_provider {
            AiProviderKind::ChatGpt => self.selected_model.as_str(),
            AiProviderKind::GeminiApi => self.selected_gemini_model.as_str(),
        }
    }

    pub fn active_provider_name(&self) -> &'static str {
        self.active_provider.display_name()
    }

    pub fn is_sidebar_visible(&self) -> bool {
        self.sidebar_user_toggle
            .unwrap_or(self.viewport_width >= 860.0)
    }

    pub fn toggle_sidebar(&mut self) {
        let currently_visible = self.is_sidebar_visible();
        self.sidebar_user_toggle = Some(!currently_visible);
    }

    pub fn is_compact(&self) -> bool {
        self.viewport_width < 920.0
    }

    pub fn is_narrow(&self) -> bool {
        self.viewport_width < 700.0
    }

    pub fn content_width(&self) -> f32 {
        let sidebar_w = if self.is_sidebar_visible() {
            if self.viewport_width < 1020.0 {
                204.0
            } else {
                228.0
            }
        } else {
            0.0
        };
        (self.viewport_width - sidebar_w).max(320.0)
    }

    pub fn page_padding_x(&self) -> Pixels {
        if self.viewport_width < 680.0 {
            px(14.0)
        } else if self.viewport_width < 960.0 {
            px(24.0)
        } else {
            px(40.0)
        }
    }

    pub fn can_show_split_preview(&self) -> bool {
        self.content_width() >= 900.0
    }

    pub fn is_split_preview_active(&self) -> bool {
        self.current_view == WorkspaceViewPage::Editor
            && self.active_document().is_some()
            && self.split_preview_open
            && self.can_show_split_preview()
    }

    pub fn split_preview_pane_width(&self) -> f32 {
        (self.content_width() * 0.42).clamp(360.0, 520.0)
    }

    pub fn editor_pane_width(&self) -> f32 {
        if self.is_split_preview_active() {
            (self.content_width() - self.split_preview_pane_width()).max(360.0)
        } else {
            self.content_width()
        }
    }

    pub fn editor_padding_x(&self) -> Pixels {
        if self.is_split_preview_active() {
            let w = self.editor_pane_width();
            if w < 560.0 {
                px(16.0)
            } else if w < 740.0 {
                px(22.0)
            } else {
                px(32.0)
            }
        } else {
            self.page_padding_x()
        }
    }

    pub fn toggle_split_preview(&mut self, cx: &mut Context<Self>) {
        if self.can_show_split_preview() {
            self.split_preview_open = !self.split_preview_open;
            if self.split_preview_open {
                self.flush_pending_now();
                self.trigger_pdf_preview_render(cx);
            }
        } else {
            self.navigate_to(WorkspaceViewPage::Preview, cx);
        }
    }

    pub fn preview_scroll_handle(&self, is_split: bool) -> &ScrollHandle {
        if is_split {
            &self.split_preview_scroll
        } else {
            &self.full_preview_scroll
        }
    }

    pub fn begin_preview_pan(&mut self, is_split: bool, mouse_pos: Point<Pixels>) {
        let start_offset = self.preview_scroll_handle(is_split).offset();
        self.preview_pan_state = Some(PreviewPanState {
            is_split,
            start_mouse: mouse_pos,
            start_offset,
        });
    }

    pub fn update_preview_pan(&mut self, mouse_pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(pan) = self.preview_pan_state else {
            return;
        };
        let handle = self.preview_scroll_handle(pan.is_split);
        let max = handle.max_offset();
        let dx = mouse_pos.x - pan.start_mouse.x;
        let dy = mouse_pos.y - pan.start_mouse.y;
        let next_x = (pan.start_offset.x + dx).clamp(-max.width, px(0.0));
        let next_y = (pan.start_offset.y + dy).clamp(-max.height, px(0.0));
        handle.set_offset(point(next_x, next_y));
        cx.notify();
    }

    pub fn end_preview_pan(&mut self) {
        self.preview_pan_state = None;
    }

    pub fn scroll_preview_by(
        &mut self,
        is_split: bool,
        dx: Pixels,
        dy: Pixels,
        cx: &mut Context<Self>,
    ) {
        let handle = self.preview_scroll_handle(is_split);
        let cur = handle.offset();
        let max = handle.max_offset();
        let next_x = (cur.x + dx).clamp(-max.width, px(0.0));
        let next_y = (cur.y + dy).clamp(-max.height, px(0.0));
        if next_x != cur.x || next_y != cur.y {
            handle.set_offset(point(next_x, next_y));
            cx.notify();
        }
    }

    pub fn clamp_preview_scroll(&self, is_split: bool) {
        let handle = self.preview_scroll_handle(is_split);
        let cur = handle.offset();
        let max = handle.max_offset();
        let clamped_x = cur.x.clamp(-max.width, px(0.0));
        let clamped_y = cur.y.clamp(-max.height, px(0.0));
        handle.set_offset(point(clamped_x, clamped_y));
    }

    // -----------------------------------------------------------------------
    // Notifications & Navigation
    // -----------------------------------------------------------------------

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.notice = Some(msg.into());
        self.notice_is_error = false;
        self.notice_generation = self.notice_generation.wrapping_add(1);
    }

    pub fn notify_error(&mut self, msg: impl Into<String>) {
        self.notice = Some(msg.into());
        self.notice_is_error = true;
        self.notice_generation = self.notice_generation.wrapping_add(1);
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
        self.notice_is_error = false;
        self.notice_generation = self.notice_generation.wrapping_add(1);
    }

    pub fn navigate_to(&mut self, page: WorkspaceViewPage, cx: &mut Context<Self>) {
        if page == WorkspaceViewPage::Preview {
            self.flush_pending_now();
            self.trigger_pdf_preview_render(cx);
        } else if page == WorkspaceViewPage::Editor
            && self.split_preview_open
            && self.rendered_images.is_empty()
        {
            self.trigger_pdf_preview_render(cx);
        } else if page == WorkspaceViewPage::Settings {
            self.refresh_snapshots();
            if self.available_models.is_empty() && !self.models_loading && self.is_ai_configured() {
                self.fetch_available_models(false, cx);
            }
        } else if page == WorkspaceViewPage::Jobs {
            self.sync_job_source_and_cached_profiles(cx);
        }
        self.job_country_dropdown_open = false;
        self.current_view = page;
    }

    pub fn open_document(
        &mut self,
        id: DocumentId,
        page: WorkspaceViewPage,
        cx: &mut Context<Self>,
    ) {
        self.flush_pending_now();
        self.active_doc_id = Some(id);
        self.current_view = page;
        self.refresh_snapshots();
        self.trigger_pdf_preview_render(cx);
    }

    pub fn select_document(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        self.flush_pending_now();
        self.active_doc_id = Some(id);
        self.editing_proposal_id = None;
        self.refresh_snapshots();
        self.trigger_pdf_preview_render(cx);
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme = match mode {
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::Light => Theme::light(),
        };
        theme::set_active_theme_mode(mode);
        let value = serde_json::json!({
            "theme": match mode {
                ThemeMode::Dark => "dark",
                ThemeMode::Light => "light",
            }
        });
        let _ = self
            .services
            .repository
            .save_setting(settings_keys::UI_PREFERENCES, &value.to_string());
    }

    pub fn toggle_theme(&mut self) {
        let next = match self.theme.mode {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        self.set_theme_mode(next);
    }

    pub fn close_dialog(&mut self) {
        self.active_dialog = None;
    }

    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.library_query.clear();
        self.search_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
    }

    // -----------------------------------------------------------------------
    // Library & Master / Variation Creation
    // -----------------------------------------------------------------------

    pub fn open_new_master_dialog(&mut self, cx: &mut Context<Self>) {
        self.master_title_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::NewMaster);
    }

    pub fn create_master_from_dialog(&mut self, cx: &mut Context<Self>) {
        let title = self.master_title_input.read(cx).text().trim().to_string();
        let final_title = if title.is_empty() {
            "Untitled Master CV".to_string()
        } else {
            title
        };

        let mut pii = self
            .active_document()
            .or_else(|| self.documents.first())
            .map(|d| d.pii_header.clone())
            .unwrap_or_default();
        if pii.full_name.trim().is_empty() {
            pii.full_name = "Your Name".to_string();
        }
        let body = crate::contracts::SanitizedCvBody {
            sections: vec![
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Professional summary".to_string(),
                    enabled: true,
                    kind: SectionKind::Summary {
                        text: String::new(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Professional experience".to_string(),
                    enabled: true,
                    kind: SectionKind::TimelineEntries {
                        entries: Vec::new(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Education".to_string(),
                    enabled: true,
                    kind: SectionKind::TimelineEntries {
                        entries: Vec::new(),
                    },
                },
                CvSection {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    title: "Skills & others".to_string(),
                    enabled: true,
                    kind: SectionKind::CategorizedInlineList {
                        categories: Vec::new(),
                    },
                },
            ],
        };

        match self
            .services
            .repository
            .create_root(&final_title, pii, body)
        {
            Ok(doc) => {
                let doc_id = doc.id;
                self.documents.push(doc);
                self.active_doc_id = Some(doc_id);
                self.current_view = WorkspaceViewPage::Editor;
                self.active_dialog = None;
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);
                self.notify(format!("Master CV \"{final_title}\" created and saved."));
            }
            Err(err) => {
                self.notify_error(format!("Failed to create Master CV: {err}"));
            }
        }
    }

    pub fn open_variation_dialog(&mut self, root_id: Option<DocumentId>, cx: &mut Context<Self>) {
        let Some(default_root) = root_id.or_else(|| {
            self.active_document()
                .map(|active| active.parent_id.unwrap_or(active.id))
                .or_else(|| {
                    self.documents
                        .iter()
                        .find(|d| d.parent_id.is_none())
                        .map(|d| d.id)
                })
        }) else {
            self.notify("Create a master CV first before creating a variation.");
            return;
        };

        self.variation_company_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.variation_role_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.variation_jd_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        self.variation_skills.clear();
        for skill in self.services.skills.list() {
            if skill.enabled && skill.default_enabled {
                self.variation_skills.insert(skill.id);
            }
        }
        // If no skill had default_enabled, pre-select enabled skills
        if self.variation_skills.is_empty() {
            for skill in self.services.skills.list() {
                if skill.enabled {
                    self.variation_skills.insert(skill.id);
                }
            }
        }

        if self.available_models.is_empty() && !self.models_loading && self.is_ai_configured() {
            self.fetch_available_models(false, cx);
        }

        self.active_dialog = Some(Dialog::NewVariation {
            root_id: default_root,
        });
    }

    pub fn select_variation_root(&mut self, root_id: DocumentId) {
        if let Some(Dialog::NewVariation {
            root_id: ref mut current,
        }) = self.active_dialog
        {
            *current = root_id;
        }
    }

    pub fn create_variation_from_dialog(&mut self, cx: &mut Context<Self>) {
        let root_id = match self.active_dialog {
            Some(Dialog::NewVariation { root_id }) => root_id,
            _ => return,
        };

        self.flush_pending_now();

        let company = self
            .variation_company_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let role = self.variation_role_input.read(cx).text().trim().to_string();
        let jd = self.variation_jd_input.read(cx).text().trim().to_string();

        let title = if !company.is_empty() && !role.is_empty() {
            format!("{company} — {role}")
        } else if !company.is_empty() {
            format!("{company} — Variation")
        } else if !role.is_empty() {
            role.clone()
        } else {
            "Tailored Variation".to_string()
        };

        let applied_skill_ids: Vec<String> = self.variation_skills.iter().cloned().collect();
        let job_ctx = JobContext {
            company_name: company.clone(),
            target_role: role.clone(),
            job_description_raw: jd.clone(),
            application_url: None,
            applied_skill_ids: applied_skill_ids.clone(),
            ats_match_score: None,
        };

        match self
            .services
            .repository
            .branch_sub_variation(root_id, &title, job_ctx)
        {
            Ok(child) => {
                let child_id = child.id;
                self.documents.push(child);
                self.active_doc_id = Some(child_id);
                self.current_view = WorkspaceViewPage::Editor;
                self.active_dialog = None;
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);

                if self.services.ai.is_configured() && (!jd.is_empty() || !role.is_empty()) {
                    self.start_ai_tailoring_for_active(cx);
                } else if !self.services.ai.is_configured() {
                    self.notify(format!(
                        "Branched \"{title}\". Configure ChatGPT or Google Gemini in Settings to run automated tailoring."
                    ));
                } else {
                    self.notify(format!("Branched \"{title}\" from Master CV."));
                }
            }
            Err(err) => {
                self.notify_error(format!("Failed to branch variation: {err}"));
            }
        }
    }

    pub fn open_delete_cv_dialog(&mut self, doc_id: DocumentId) {
        self.active_dialog = Some(Dialog::ConfirmDeleteCv { doc_id });
    }

    pub fn delete_document(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        let target_title = self
            .find_document(id)
            .map(|d| d.title.clone())
            .unwrap_or_else(|| "CV".to_string());
        let child_ids: Vec<DocumentId> = self
            .documents
            .iter()
            .filter(|d| d.parent_id == Some(id))
            .map(|d| d.id)
            .collect();

        self.flush_pending_now();

        // Repository guards against deleting a master with active children,
        // so delete linked variations first in deterministic order.
        for child_id in &child_ids {
            if let Err(err) = self.services.repository.delete(*child_id) {
                self.active_dialog = None;
                self.reload_documents_from_db(cx);
                self.notify_error(format!("Could not delete linked variation: {err}"));
                return;
            }
        }

        match self.services.repository.delete(id) {
            Ok(()) => {
                let mut removed_ids = child_ids.clone();
                removed_ids.push(id);
                self.documents
                    .retain(|d| !removed_ids.contains(&d.id));
                for rid in &removed_ids {
                    self.proposals.remove(rid);
                    self.ats_scores.remove(rid);
                    self.ai_summaries.remove(rid);
                }
                if self
                    .ai_state
                    .target_doc_id
                    .is_some_and(|tid| removed_ids.contains(&tid))
                {
                    self.ai_state = AiAgentState::default();
                }
                if self
                    .default_cv_id
                    .is_some_and(|did| removed_ids.contains(&did))
                {
                    self.default_cv_id = None;
                    let _ = self
                        .services
                        .repository
                        .save_setting(settings_keys::DEFAULT_CV_ID, "");
                }
                for rid in &removed_ids {
                    self.job_cache.remove_cv_profiles(*rid);
                }
                self.persist_job_profiles_cache();
                if let Some(CvSearchSource::Single(sid)) = self.job_source
                    && removed_ids.contains(&sid)
                {
                    self.job_source = None;
                    self.job_profiles.clear();
                    self.job_profiles_from_cache = false;
                    self.job_profiles_used_fallback = false;
                    self.job_profiles_stale = false;
                }
                self.active_dialog = None;
                self.editing_proposal_id = None;
                if self
                    .active_doc_id
                    .is_some_and(|active_id| removed_ids.contains(&active_id))
                {
                    self.active_doc_id = None;
                    self.rendered_images.clear();
                    self.preview_error = None;
                    self.snapshots.clear();
                    if matches!(
                        self.current_view,
                        WorkspaceViewPage::Editor | WorkspaceViewPage::Preview
                    ) {
                        self.current_view = WorkspaceViewPage::Library;
                    }
                } else {
                    self.refresh_snapshots();
                }

                if child_ids.is_empty() {
                    self.notify(format!("Deleted \"{target_title}\"."));
                } else {
                    self.notify(format!(
                        "Deleted \"{target_title}\" and {} linked {}.",
                        child_ids.len(),
                        if child_ids.len() == 1 {
                            "variation"
                        } else {
                            "variations"
                        }
                    ));
                }
            }
            Err(err) => {
                self.active_dialog = None;
                self.reload_documents_from_db(cx);
                self.notify_error(format!("Delete failed: {err}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Official Sign in with ChatGPT Engine (Tailoring + Inline Bullet Rewrite)
    // -----------------------------------------------------------------------

    pub fn start_ai_tailoring_for_active(&mut self, cx: &mut Context<Self>) {
        self.launch_ai_tailoring_pass(None, "Before AI tailoring pass", cx);
    }

    /// One-click Page Compressor launched directly from the PDF page-overflow warning banner.
    pub fn run_page_compressor(&mut self, cx: &mut Context<Self>) {
        if self.ai_state.is_running {
            self.notify("An AI tailoring run is already in progress.");
            return;
        }
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a CV from the library first.");
            return;
        };
        if active.parent_id.is_none() {
            self.open_variation_dialog(Some(active.id), cx);
            self.variation_skills.insert("page_compressor".to_string());
            self.notify(
                "Master CVs are AI-protected. Create a variation with Page Compressor selected to fit 1 page.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            self.navigate_to(WorkspaceViewPage::Settings, cx);
            self.notify(
                "Connect ChatGPT or Google Gemini API in Settings to run Page Compressor.",
            );
            return;
        }
        if self.current_view != WorkspaceViewPage::Editor {
            self.split_preview_open = true;
            self.navigate_to(WorkspaceViewPage::Editor, cx);
        }
        self.launch_ai_tailoring_pass(
            Some(vec!["page_compressor".to_string()]),
            "Before Page Compressor AI pass",
            cx,
        );
    }

    fn launch_ai_tailoring_pass(
        &mut self,
        skill_ids_override: Option<Vec<String>>,
        snapshot_label: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a variation CV first.");
            return;
        };
        if active.parent_id.is_none() {
            self.notify_error(
                "AI tailoring is blocked on Master CVs. Branch a tailored variation first.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            let msg = match self.active_provider {
                AiProviderKind::ChatGpt => {
                    if self.chatgpt_auth.signed_in && !self.chatgpt_auth.plan_usage_enabled {
                        "ChatGPT plan usage is disabled. Open Settings to enable ChatGPT plan usage."
                    } else {
                        "Sign in with ChatGPT (or switch to Google Gemini API) in Settings to run AI tailoring."
                    }
                }
                AiProviderKind::GeminiApi => {
                    "Enter and save your Google Gemini API key (or switch to ChatGPT) in Settings to run AI tailoring."
                }
            };
            self.notify_error(msg);
            return;
        }
        if self.ai_state.is_running {
            self.notify("An AI tailoring run is already in progress.");
            return;
        }

        self.flush_pending_now();

        // 1. Pre-AI recovery snapshot
        if let Err(err) = snapshots::snapshot_before_ai(
            self.services.repository.as_ref(),
            active.id,
            snapshot_label,
        ) {
            tracing::warn!("Pre-AI snapshot failed: {err}");
        } else {
            self.refresh_snapshots();
        }

        // 2. Zero-trust PII scrubbing
        let payload = match self
            .services
            .privacy
            .scrub(&active.body, &active.pii_header)
        {
            Ok(p) => p,
            Err(err) => {
                self.notify_error(format!("PII scrubber failed: {err}"));
                return;
            }
        };

        let mut job_context = active.job_context.clone().unwrap_or_default();
        let skill_ids = if let Some(override_ids) = skill_ids_override {
            if job_context.job_description_raw.trim().is_empty() {
                job_context.job_description_raw =
                    "Compress verbose bullets and hide lowest-priority entries so the CV fits cleanly on 1 A4 page while preserving all metrics and factual accuracy.".to_string();
            }
            override_ids
        } else if job_context.applied_skill_ids.is_empty() {
            self.services
                .skills
                .list()
                .into_iter()
                .filter(|s| s.enabled)
                .map(|s| s.id)
                .collect()
        } else {
            job_context.applied_skill_ids.clone()
        };

        let request = VariationRequest {
            child_cv_id: active.id,
            job_context,
            skill_ids,
        };

        let doc_id = active.id;
        let redactions = payload.redaction_count;
        let active_model = self.active_model_id().to_string();
        self.ai_state = AiAgentState {
            is_running: true,
            target_doc_id: Some(doc_id),
            step: format!(
                "Scrubbed ({redactions} PII tokens redacted). Streaming proposals from {active_model}…"
            ),
            streamed_text: String::new(),
            error: None,
        };

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<AiEvent>();

        crate::runtime::handle().spawn(async move {
            let _ = ai.generate_variation(request, payload, tx).await;
        });

        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let is_terminal = matches!(
                    event,
                    AiEvent::RunCompleted { .. } | AiEvent::RunFailed { .. }
                );
                let _ = this.update(cx, |view, cx| {
                    view.handle_ai_event(doc_id, event, cx);
                    cx.notify();
                });
                if is_terminal {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn find_bullet_text(&self, section_id: ItemId, bullet_id: ItemId) -> Option<String> {
        let doc = self.active_document()?;
        let sec = doc.body.sections.iter().find(|s| s.id == section_id)?;
        match &sec.kind {
            SectionKind::TimelineEntries { entries } => entries.iter().find_map(|entry| {
                entry
                    .bullets
                    .iter()
                    .find(|b| b.id == bullet_id)
                    .map(|b| b.text.clone())
            }),
            SectionKind::SimpleBulletList { items } => items
                .iter()
                .find(|b| b.id == bullet_id)
                .map(|b| b.text.clone()),
            _ => None,
        }
    }

    pub fn open_rewrite_bullet_dialog(
        &mut self,
        section_id: ItemId,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a variation CV first.");
            return;
        };
        if active.parent_id.is_none() {
            self.notify_error(
                "AI rewrites are only allowed on tailored variations. Master CVs stay protected.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            let msg = match self.active_provider {
                AiProviderKind::ChatGpt => {
                    if self.chatgpt_auth.signed_in && !self.chatgpt_auth.plan_usage_enabled {
                        "Enable ChatGPT plan usage in Settings to rewrite achievements with AI."
                    } else {
                        "Sign in with ChatGPT (or switch to Google Gemini API) in Settings to rewrite achievements with AI."
                    }
                }
                AiProviderKind::GeminiApi => {
                    "Enter and save your Google Gemini API key in Settings to rewrite achievements with AI."
                }
            };
            self.notify_error(msg);
            return;
        }
        if self.ai_state.is_running {
            self.notify("Wait for the current AI task to finish.");
            return;
        }

        let enabled_skills: Vec<SkillManifest> = self
            .services
            .skills
            .list()
            .into_iter()
            .filter(|s| s.enabled)
            .collect();
        let enabled_ids: HashSet<String> = enabled_skills.iter().map(|s| s.id.clone()).collect();

        self.rewrite_selected_skills.clear();
        if let Some(jc) = active.job_context.as_ref() {
            for id in &jc.applied_skill_ids {
                if enabled_ids.contains(id) {
                    self.rewrite_selected_skills.insert(id.clone());
                }
            }
        }
        if self.rewrite_selected_skills.is_empty() {
            for s in &enabled_skills {
                if s.default_enabled {
                    self.rewrite_selected_skills.insert(s.id.clone());
                }
            }
        }
        if self.rewrite_selected_skills.is_empty() {
            for s in &enabled_skills {
                self.rewrite_selected_skills.insert(s.id.clone());
            }
        }

        self.rewrite_instruction_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        self.active_dialog = Some(Dialog::RewriteBullet {
            section_id,
            bullet_id,
        });
    }

    pub fn toggle_rewrite_skill(&mut self, skill_id: &str) {
        if self.rewrite_selected_skills.contains(skill_id) {
            self.rewrite_selected_skills.remove(skill_id);
        } else {
            self.rewrite_selected_skills.insert(skill_id.to_string());
        }
    }

    pub fn submit_bullet_rewrite_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::RewriteBullet {
            section_id,
            bullet_id,
        }) = self.active_dialog
        else {
            return;
        };
        let custom_instruction = self
            .rewrite_instruction_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let mut skill_ids: Vec<String> = self.rewrite_selected_skills.iter().cloned().collect();
        skill_ids.sort();
        self.active_dialog = None;
        self.request_bullet_rewrite_with_options(
            section_id,
            bullet_id,
            (!custom_instruction.is_empty()).then_some(custom_instruction),
            skill_ids,
            cx,
        );
    }

    pub fn request_bullet_rewrite_with_options(
        &mut self,
        _sec_id: ItemId,
        bullet_id: ItemId,
        custom_instruction: Option<String>,
        skill_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self.active_document().cloned() else {
            self.notify("Select a variation CV first.");
            return;
        };
        if active.parent_id.is_none() {
            self.notify_error(
                "AI rewrites are only allowed on tailored variations. Master CVs stay protected.",
            );
            return;
        }
        self.chatgpt_auth = self.services.ai.auth_status();
        if !self.services.ai.is_configured() {
            self.notify_error("Configure ChatGPT or Google Gemini API in Settings first.");
            return;
        }
        if self.ai_state.is_running {
            self.notify("Wait for the current AI task to finish.");
            return;
        }

        self.flush_pending_now();

        let payload = match self
            .services
            .privacy
            .scrub(&active.body, &active.pii_header)
        {
            Ok(p) => p,
            Err(err) => {
                self.notify_error(format!("PII scrubber failed: {err}"));
                return;
            }
        };

        let target_role = active
            .job_context
            .as_ref()
            .map(|jc| jc.target_role.clone())
            .unwrap_or_default();
        let instruction = custom_instruction.unwrap_or_else(|| {
            if target_role.is_empty() {
                "Rewrite this bullet for stronger impact using the Google XYZ formula while preserving all factual details and metrics."
                    .to_string()
            } else {
                format!(
                    "Rewrite this bullet for a {target_role} role using the Google XYZ formula. Preserve all facts and metrics."
                )
            }
        });

        let request = InlineRewriteRequest {
            cv_id: active.id,
            bullet_id,
            instruction,
            skill_ids,
        };

        let doc_id = active.id;
        self.ai_state = AiAgentState {
            is_running: true,
            target_doc_id: Some(doc_id),
            step: "Rewriting achievement via zero-trust AI…".to_string(),
            streamed_text: String::new(),
            error: None,
        };

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<AiEvent>();

        crate::runtime::handle().spawn(async move {
            let _ = ai.rewrite_bullet(request, payload, tx).await;
        });

        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let is_terminal = matches!(
                    event,
                    AiEvent::RunCompleted { .. } | AiEvent::RunFailed { .. }
                );
                let _ = this.update(cx, |view, cx| {
                    view.handle_ai_event(doc_id, event, cx);
                    cx.notify();
                });
                if is_terminal {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_ai_event(&mut self, doc_id: DocumentId, event: AiEvent, cx: &mut Context<Self>) {
        match event {
            AiEvent::RunStarted { .. } => {
                self.ai_state.is_running = true;
                self.ai_state.error = None;
            }
            AiEvent::Status { message } => {
                self.ai_state.step = message;
            }
            AiEvent::TextDelta { delta } => {
                self.ai_state.streamed_text.push_str(&delta);
            }
            AiEvent::ToolCall { name, .. } => {
                self.ai_state.step = format!("Evaluating tool: {name}…");
            }
            AiEvent::Proposal { proposal } => {
                if let Some(doc) = self.find_document(doc_id).cloned() {
                    let ui_prop = UiProposal::from_ai_proposal(&proposal, &doc);
                    let target_id = ui_prop.target_item_id;
                    let list = self.proposals.entry(doc_id).or_default();
                    if let Some(editing_id) = self.editing_proposal_id.as_deref()
                        && list.iter().any(|e| e.target_item_id == target_id && e.id == editing_id)
                    {
                        self.editing_proposal_id = None;
                    }
                    list.retain(|existing| existing.target_item_id != target_id);
                    list.push(ui_prop);
                    self.expand_containers_for_item(&doc, target_id);
                }
            }
            AiEvent::AtsScore { score } => {
                let numeric = score.score;
                if let Some(doc) = self.documents.iter_mut().find(|d| d.id == doc_id) {
                    self.ats_scores.insert(doc_id, score);
                    if let Some(ref mut jc) = doc.job_context {
                        jc.ats_match_score = Some(numeric);
                    }
                    let _ = self.services.repository.save(doc);
                }
            }
            AiEvent::RunCompleted { summary, .. } => {
                self.ai_state.is_running = false;
                if !summary.trim().is_empty() {
                    self.ai_summaries.insert(doc_id, summary.clone());
                }
                self.ai_state.step = summary;
                let count = self.proposals.get(&doc_id).map(|v| v.len()).unwrap_or(0);
                if count > 0 {
                    self.notify(format!(
                        "AI review ready: {count} {} awaiting your approval.",
                        if count == 1 { "proposal" } else { "proposals" }
                    ));
                } else {
                    self.notify("AI tailoring run completed.");
                }
            }
            AiEvent::RunFailed { message, .. } => {
                self.ai_state.is_running = false;
                self.ai_state.error = Some(message.clone());
                self.chatgpt_auth = self.services.ai.auth_status();
                if crate::ai::is_ai_limit_error(&message) {
                    let lower = message.to_ascii_lowercase();
                    if lower.starts_with("api") || lower.starts_with("usage limit") {
                        self.notify_error(message);
                    } else {
                        self.notify_error(format!("API limit reached: {message}"));
                    }
                } else {
                    self.notify_error(format!("AI error: {message}"));
                }
            }
        }
        let _ = cx;
    }

    pub fn resolve_proposal(&mut self, prop_id: &str, accept: bool, cx: &mut Context<Self>) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let prop = self
            .proposals
            .get(&active_id)
            .and_then(|list| list.iter().find(|p| p.id == prop_id).cloned());

        let Some(proposal) = prop else {
            return;
        };

        if accept {
            if let Some(warn) = &proposal.pii_warning {
                self.notify_error(warn.clone());
                return;
            }
            self.flush_pending_now();
            if let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) {
                let mut candidate = doc.clone();
                apply_ai_proposal(&mut candidate, &proposal.proposal);
                match self.services.repository.save_ai_batch(&candidate) {
                    Ok(()) => {
                        if let Ok(reloaded) = self.services.repository.load(active_id) {
                            *doc = reloaded;
                        } else {
                            *doc = candidate;
                        }
                        if let Some(list) = self.proposals.get_mut(&active_id) {
                            list.retain(|p| p.id != prop_id);
                        }
                        if self.editing_proposal_id.as_deref() == Some(prop_id) {
                            self.editing_proposal_id = None;
                        }
                        self.trigger_pdf_preview_render(cx);
                        self.notify("Change accepted and saved. Master CV is unchanged.");
                    }
                    Err(err) => {
                        self.notify_error(format!("Could not save AI change: {err}"));
                    }
                }
            }
        } else {
            if let Some(list) = self.proposals.get_mut(&active_id) {
                list.retain(|p| p.id != prop_id);
            }
            if self.editing_proposal_id.as_deref() == Some(prop_id) {
                self.editing_proposal_id = None;
            }
            self.notify("Proposal reverted.");
        }
    }

    pub fn begin_editing_proposal(
        &mut self,
        prop_id: &str,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let Some(prop) = self
            .proposals
            .get(&active_id)
            .and_then(|list| list.iter().find(|p| p.id == prop_id))
            .cloned()
        else {
            return;
        };
        if !prop.is_text_editable() {
            return;
        }
        self.editing_proposal_id = Some(prop.id.clone());
        let initial_text = prop.proposed.clone();
        self.proposal_edit_input.update(cx, |input, cx| {
            input.set_value(initial_text);
            cx.notify();
        });
        if let Some(window) = window {
            let focus_handle = self.proposal_edit_input.read(cx).focus_handle(cx);
            window.focus(&focus_handle);
        }
    }

    pub fn cancel_editing_proposal(&mut self) {
        self.editing_proposal_id = None;
    }

    pub fn save_edited_proposal(
        &mut self,
        prop_id: &str,
        accept_immediately: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let edited_text = self.proposal_edit_input.read(cx).text().to_string();
        let doc_snapshot = self.find_document(active_id).cloned();

        let update_res = self
            .proposals
            .get_mut(&active_id)
            .and_then(|list| list.iter_mut().find(|p| p.id == prop_id))
            .map(|prop| update_proposal_text(prop, &edited_text, doc_snapshot.as_ref()));

        match update_res {
            Some(Ok(())) => {
                self.editing_proposal_id = None;
                if accept_immediately {
                    self.resolve_proposal(prop_id, true, cx);
                } else {
                    self.notify("Proposal draft updated. Review the diff or click Accept change.");
                }
            }
            Some(Err(msg)) => {
                self.notify_error(msg);
            }
            None => {
                self.editing_proposal_id = None;
            }
        }
    }

    pub fn accept_all_proposals(&mut self, cx: &mut Context<Self>) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let list = self.proposals.get(&active_id).cloned().unwrap_or_default();
        if list.is_empty() {
            return;
        }

        let mut safe = Vec::new();
        let mut blocked = Vec::new();
        for item in list {
            if item.pii_warning.is_some() {
                blocked.push(item);
            } else {
                safe.push(item);
            }
        }

        if safe.is_empty() {
            self.notify_error(
                "All pending proposals were blocked because they contain redaction placeholders.",
            );
            return;
        }

        self.flush_pending_now();

        if let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) {
            let mut candidate = doc.clone();
            for item in &safe {
                apply_ai_proposal(&mut candidate, &item.proposal);
            }
            match self.services.repository.save_ai_batch(&candidate) {
                Ok(()) => {
                    if let Ok(reloaded) = self.services.repository.load(active_id) {
                        *doc = reloaded;
                    } else {
                        *doc = candidate;
                    }
                    self.editing_proposal_id = None;
                    if blocked.is_empty() {
                        self.proposals.remove(&active_id);
                        self.notify(format!(
                            "Accepted {} {}. Master CV is unchanged.",
                            safe.len(),
                            if safe.len() == 1 { "change" } else { "changes" }
                        ));
                    } else {
                        self.proposals.insert(active_id, blocked.clone());
                        self.notify(format!(
                            "Accepted {} changes; {} skipped due to PII placeholder safety guard.",
                            safe.len(),
                            blocked.len()
                        ));
                    }
                    self.trigger_pdf_preview_render(cx);
                }
                Err(err) => {
                    self.notify_error(format!("Could not apply AI batch: {err}"));
                }
            }
        }
    }

    pub fn dismiss_all_proposals(&mut self) {
        if let Some(active_id) = self.active_doc_id {
            self.proposals.remove(&active_id);
            self.editing_proposal_id = None;
            self.notify("Dismissed pending AI proposals.");
        }
    }

    // -----------------------------------------------------------------------
    // Compare with Master & Promote to Master (Real Backend)
    // -----------------------------------------------------------------------

    pub fn open_compare_dialog(&mut self) {
        self.flush_pending_now();
        self.active_dialog = Some(Dialog::Compare);
    }

    pub fn calculate_lineage_changes(&self) -> Vec<ItemDiff> {
        let Some(doc) = self.active_document() else {
            return Vec::new();
        };
        let Some(parent) = doc.parent_id.and_then(|pid| self.find_document(pid)) else {
            return Vec::new();
        };
        lineage_diff::compute_diff(doc, parent)
            .map(|diff| diff.item_diffs)
            .unwrap_or_default()
    }

    pub fn open_promote_dialog(
        &mut self,
        sec_id: ItemId,
        item_id: ItemId,
        entry_id: Option<ItemId>,
    ) {
        self.promote_target = Some((sec_id, item_id, entry_id));
        self.active_dialog = Some(Dialog::PromoteItem);
    }

    pub fn describe_promote_target(&self) -> Option<String> {
        let (_, item_id, _) = self.promote_target?;
        Some(describe_item_in_doc(self.active_document()?, item_id))
    }

    fn expand_containers_for_item(&mut self, doc: &CvDocument, item_id: ItemId) {
        for sec in &doc.body.sections {
            if sec.id == item_id {
                self.expanded_sections.insert(sec.id, true);
                return;
            }
            match &sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    for entry in entries {
                        if entry.id == item_id || entry.bullets.iter().any(|b| b.id == item_id) {
                            self.expanded_sections.insert(sec.id, true);
                            self.expanded_entries.insert(entry.id, true);
                            return;
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if items.iter().any(|b| b.id == item_id) {
                        self.expanded_sections.insert(sec.id, true);
                        return;
                    }
                }
                SectionKind::CategorizedInlineList { categories } => {
                    if categories.iter().any(|c| c.id == item_id) {
                        self.expanded_sections.insert(sec.id, true);
                        return;
                    }
                }
                SectionKind::Summary { .. } => {}
            }
        }
    }

    fn prune_orphaned_proposals_for_active(&mut self) {
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let valid_ids: HashSet<ItemId> = {
            let Some(doc) = self.active_document() else {
                return;
            };
            let mut ids = HashSet::new();
            for sec in &doc.body.sections {
                ids.insert(sec.id);
                match &sec.kind {
                    SectionKind::TimelineEntries { entries } => {
                        for e in entries {
                            ids.insert(e.id);
                            for b in &e.bullets {
                                ids.insert(b.id);
                            }
                        }
                    }
                    SectionKind::SimpleBulletList { items } => {
                        for b in items {
                            ids.insert(b.id);
                        }
                    }
                    SectionKind::CategorizedInlineList { categories } => {
                        for cat in categories {
                            ids.insert(cat.id);
                        }
                    }
                    SectionKind::Summary { .. } => {}
                }
            }
            ids
        };
        if let Some(list) = self.proposals.get_mut(&active_id) {
            list.retain(|p| valid_ids.contains(&p.target_item_id));
            if list.is_empty() {
                self.proposals.remove(&active_id);
            }
        }
    }

    pub fn execute_promote(&mut self, cx: &mut Context<Self>) {
        let Some((_, item_id, _)) = self.promote_target.take() else {
            self.active_dialog = None;
            return;
        };

        let Some(child_id) = self.active_doc_id else {
            self.active_dialog = None;
            return;
        };
        self.flush_pending_now();

        match self.services.repository.promote_item(child_id, item_id) {
            Ok(outcome) => {
                // Reload both parent & child so origin_item_id links stay in sync
                if let Ok(updated_parent) =
                    self.services.repository.load(outcome.parent_document_id)
                    && let Some(slot) = self
                        .documents
                        .iter_mut()
                        .find(|d| d.id == outcome.parent_document_id)
                {
                    *slot = updated_parent;
                }
                if let Ok(updated_child) = self.services.repository.load(child_id)
                    && let Some(slot) = self.documents.iter_mut().find(|d| d.id == child_id)
                {
                    *slot = updated_child;
                }
                self.active_dialog = None;
                self.trigger_pdf_preview_render(cx);
                self.notify("Item promoted into your Master CV.");
            }
            Err(err) => {
                self.active_dialog = None;
                self.notify_error(format!("Promotion failed: {err}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Payload Preview & Non-Blocking PDF Export
    // -----------------------------------------------------------------------

    pub fn open_payload_dialog(&mut self) {
        if self.active_document().is_none() {
            self.notify("Select a CV from the library to inspect its scrubbed AI payload.");
            return;
        }
        self.active_dialog = Some(Dialog::PayloadPreview);
    }

    pub fn build_payload_preview(&self) -> (String, usize) {
        let Some(doc) = self.active_document() else {
            return ("{}".to_string(), 0);
        };
        match self.services.privacy.scrub(&doc.body, &doc.pii_header) {
            Ok(payload) => {
                let pretty = serde_json::from_str::<serde_json::Value>(&payload.json)
                    .and_then(|v| serde_json::to_string_pretty(&v))
                    .unwrap_or(payload.json);
                (pretty, payload.redaction_count)
            }
            Err(err) => (format!("{{\"error\": \"{err}\"}}"), 0),
        }
    }

    pub fn open_export_dialog(&mut self) {
        if self.active_document().is_none() {
            self.notify("Select a CV from the library before exporting to PDF.");
            return;
        }
        self.active_dialog = Some(Dialog::ExportPdf);
    }

    fn initial_save_dir() -> PathBuf {
        let default_dir = exporter::default_export_dir();
        if default_dir.is_dir() {
            return default_dir;
        }
        if let Some(docs_dir) = dirs::document_dir()
            && docs_dir.is_dir()
        {
            return docs_dir;
        }
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    pub fn execute_export_pdf(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_exporting_pdf {
            return;
        }
        self.flush_pending_now();
        let Some(doc) = self.active_document().cloned() else {
            self.active_dialog = None;
            self.notify("Select a CV from the library before exporting to PDF.");
            return;
        };
        let default_name = exporter::export_filename(&doc);
        let initial_dir = Self::initial_save_dir();
        let pdf_engine = self.services.pdf.clone();

        self.active_dialog = None;
        self.is_exporting_pdf = true;
        self.notify("Choose where to save your PDF…");

        let path_rx = cx.prompt_for_new_path(&initial_dir, Some(&default_name));

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => {
                    let _ = this.update(cx, |view, cx| {
                        view.is_exporting_pdf = false;
                        view.clear_notice();
                        cx.notify();
                    });
                    return;
                }
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.is_exporting_pdf = false;
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("pdf");
            }

            let write_result = cx
                .background_executor()
                .spawn(async move {
                    let bytes = pdf_engine.compile_to_pdf(&doc.pii_header, &doc.body)?;
                    std::fs::write(&path, bytes)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.is_exporting_pdf = false;
                match write_result {
                    Ok(path) => {
                        view.notify(format!("PDF exported to {}", path.display()));
                    }
                    Err(err) => {
                        view.notify_error(format!("PDF export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // Editor CRUD: Rename CV, Sections, Summary, Entries, Bullets, Categories
    // -----------------------------------------------------------------------

    pub fn open_rename_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(title) = self.active_document().map(|d| d.title.clone()) else {
            return;
        };
        self.rename_title_input.update(cx, |i, cx| {
            i.set_value(title);
            cx.notify();
        });
        self.active_dialog = Some(Dialog::RenameCv);
    }

    pub fn save_rename(&mut self, cx: &mut Context<Self>) {
        let new_title = self.rename_title_input.read(cx).text().trim().to_string();
        if !new_title.is_empty() {
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|d| Some(d.id) == self.active_doc_id)
            {
                doc.title = new_title;
            }
            self.persist_active_doc(cx);
            self.notify("CV renamed.");
        }
        self.active_dialog = None;
    }

    pub fn open_add_section_dialog(&mut self, cx: &mut Context<Self>) {
        self.sec_title_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.sec_type_selection = "Timeline entries".to_string();
        self.active_dialog = Some(Dialog::AddSection);
    }

    pub fn execute_add_section(&mut self, cx: &mut Context<Self>) {
        let title = self.sec_title_input.read(cx).text().trim().to_string();
        let final_title = if title.is_empty() {
            "New Section".to_string()
        } else {
            title
        };

        let kind = match self.sec_type_selection.as_str() {
            "Paragraph summary" => SectionKind::Summary {
                text: String::new(),
            },
            "Categorized skills" => SectionKind::CategorizedInlineList {
                categories: Vec::new(),
            },
            "Bullet list" => SectionKind::SimpleBulletList { items: Vec::new() },
            _ => SectionKind::TimelineEntries {
                entries: Vec::new(),
            },
        };

        let new_sec = CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: final_title.clone(),
            enabled: true,
            kind,
        };
        let sec_id = new_sec.id;

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.body.sections.push(new_sec);
        }
        self.expanded_sections.insert(sec_id, true);
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(format!("Added section \"{final_title}\"."));
    }

    pub fn open_rename_section_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let current = self
            .active_document()
            .and_then(|doc| doc.body.sections.iter().find(|s| s.id == section_id))
            .map(|s| s.title.clone())
            .unwrap_or_default();
        self.rename_section_input.update(cx, |i, cx| {
            i.set_value(current);
            cx.notify();
        });
        self.active_dialog = Some(Dialog::RenameSection { section_id });
    }

    pub fn save_rename_section(&mut self, cx: &mut Context<Self>) {
        let section_id = match self.active_dialog {
            Some(Dialog::RenameSection { section_id }) => section_id,
            _ => return,
        };
        let new_title = self.rename_section_input.read(cx).text().trim().to_string();
        if !new_title.is_empty() {
            if let Some(doc) = self
                .documents
                .iter_mut()
                .find(|d| Some(d.id) == self.active_doc_id)
                && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            {
                sec.title = new_title;
            }
            self.persist_active_doc(cx);
            self.notify("Section renamed.");
        }
        self.active_dialog = None;
    }

    pub fn delete_section(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.body.sections.retain(|s| s.id != section_id);
        }
        self.expanded_sections.remove(&section_id);
        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Section removed.");
    }

    pub fn open_edit_summary_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let current = self
            .active_document()
            .and_then(|doc| doc.body.sections.iter().find(|s| s.id == section_id))
            .and_then(|s| match &s.kind {
                SectionKind::Summary { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        self.summary_text_input.update(cx, |i, cx| {
            i.set_value(current);
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSummary { section_id });
    }

    pub fn save_summary_from_dialog(&mut self, cx: &mut Context<Self>) {
        let section_id = match self.active_dialog {
            Some(Dialog::EditSummary { section_id }) => section_id,
            _ => return,
        };
        let new_text = self.summary_text_input.read(cx).text().trim().to_string();
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::Summary { text } = &mut sec.kind
        {
            *text = new_text;
        }
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Summary updated.");
    }

    pub fn open_add_entry_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        self.editing_entry_id = None;
        self.entry_is_current = false;
        self.entry_org_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.entry_role_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.entry_start_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.entry_end_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditEntry {
            section_id,
            is_new: true,
        });
    }

    pub fn open_edit_entry_dialog(
        &mut self,
        section_id: ItemId,
        entry: &TimelineEntry,
        cx: &mut Context<Self>,
    ) {
        self.editing_entry_id = Some(entry.id);
        self.entry_is_current = entry.is_current;
        self.entry_org_input.update(cx, |i, cx| {
            i.set_value(entry.primary_heading.clone());
            cx.notify();
        });
        self.entry_role_input.update(cx, |i, cx| {
            i.set_value(entry.secondary_heading.clone().unwrap_or_default());
            cx.notify();
        });
        self.entry_start_input.update(cx, |i, cx| {
            i.set_value(entry.start_date.clone());
            cx.notify();
        });
        self.entry_end_input.update(cx, |i, cx| {
            i.set_value(entry.end_date.clone().unwrap_or_default());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditEntry {
            section_id,
            is_new: false,
        });
    }

    pub fn save_entry_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (section_id, is_new) = match self.active_dialog {
            Some(Dialog::EditEntry { section_id, is_new }) => (section_id, is_new),
            _ => return,
        };

        let org = self.entry_org_input.read(cx).text().trim().to_string();
        let role = self.entry_role_input.read(cx).text().trim().to_string();
        let start = self.entry_start_input.read(cx).text().trim().to_string();
        let end = self.entry_end_input.read(cx).text().trim().to_string();
        let is_current = self.entry_is_current;

        if org.is_empty() {
            self.notify_error("Organization or institution name is required.");
            return;
        }

        let secondary = if role.is_empty() { None } else { Some(role) };
        let end_date = if is_current || end.is_empty() {
            None
        } else {
            Some(end)
        };

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
        {
            if is_new {
                let mut entry = TimelineEntry::new(org, start);
                entry.secondary_heading = secondary;
                entry.end_date = end_date;
                entry.is_current = is_current;
                self.expanded_entries.insert(entry.id, true);
                entries.push(entry);
            } else if let Some(eid) = self.editing_entry_id
                && let Some(existing) = entries.iter_mut().find(|e| e.id == eid)
            {
                existing.primary_heading = org;
                existing.secondary_heading = secondary;
                existing.start_date = start;
                existing.end_date = end_date;
                existing.is_current = is_current;
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if is_new {
            "Entry added."
        } else {
            "Entry updated."
        });
    }

    pub fn delete_current_entry(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditEntry { section_id, .. }) = self.active_dialog else {
            return;
        };
        let Some(eid) = self.editing_entry_id.take() else {
            return;
        };
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
        {
            entries.retain(|e| e.id != eid);
        }
        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Entry deleted.");
    }

    pub fn open_add_bullet_dialog(
        &mut self,
        section_id: ItemId,
        entry_id: Option<ItemId>,
        cx: &mut Context<Self>,
    ) {
        self.editing_bullet_id = None;
        self.bullet_text_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditBullet {
            section_id,
            entry_id,
            is_new: true,
        });
    }

    pub fn open_edit_bullet_dialog(
        &mut self,
        section_id: ItemId,
        entry_id: Option<ItemId>,
        item: &BulletItem,
        cx: &mut Context<Self>,
    ) {
        self.editing_bullet_id = Some(item.id);
        self.bullet_text_input.update(cx, |i, cx| {
            i.set_value(item.text.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditBullet {
            section_id,
            entry_id,
            is_new: false,
        });
    }

    pub fn save_bullet_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (section_id, entry_id, is_new) = match self.active_dialog {
            Some(Dialog::EditBullet {
                section_id,
                entry_id,
                is_new,
            }) => (section_id, entry_id, is_new),
            _ => return,
        };

        let text = self.bullet_text_input.read(cx).text().trim().to_string();
        if text.is_empty() {
            self.notify_error("Achievement text cannot be empty.");
            return;
        }

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
        {
            match &mut sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    if let Some(eid) = entry_id
                        && let Some(entry) = entries.iter_mut().find(|e| e.id == eid)
                    {
                        if is_new {
                            entry.bullets.push(BulletItem::new(text));
                        } else if let Some(bid) = self.editing_bullet_id
                            && let Some(b) = entry.bullets.iter_mut().find(|b| b.id == bid)
                        {
                            b.text = text;
                        }
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if is_new {
                        items.push(BulletItem::new(text));
                    } else if let Some(bid) = self.editing_bullet_id
                        && let Some(b) = items.iter_mut().find(|b| b.id == bid)
                    {
                        b.text = text;
                    }
                }
                _ => {}
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if is_new {
            "Achievement added."
        } else {
            "Achievement updated."
        });
    }

    pub fn delete_current_bullet(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditBullet {
            section_id,
            entry_id,
            ..
        }) = self.active_dialog
        else {
            return;
        };
        let Some(bid) = self.editing_bullet_id.take() else {
            return;
        };

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
        {
            match &mut sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    if let Some(eid) = entry_id
                        && let Some(entry) = entries.iter_mut().find(|e| e.id == eid)
                    {
                        entry.bullets.retain(|b| b.id != bid);
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    items.retain(|b| b.id != bid);
                }
                _ => {}
            }
        }

        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Achievement deleted.");
    }

    pub fn open_add_category_dialog(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        self.editing_category_id = None;
        self.cat_separator = ", ".to_string();
        self.cat_label_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.cat_items_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCategory {
            section_id,
            is_new: true,
        });
    }

    pub fn open_edit_category_dialog(
        &mut self,
        section_id: ItemId,
        cat: &SkillCategoryItem,
        cx: &mut Context<Self>,
    ) {
        self.editing_category_id = Some(cat.id);
        self.cat_separator = if cat.separator.is_empty() {
            ", ".to_string()
        } else {
            cat.separator.clone()
        };
        self.cat_label_input.update(cx, |i, cx| {
            i.set_value(cat.category_label.clone());
            cx.notify();
        });
        self.cat_items_input.update(cx, |i, cx| {
            i.set_value(cat.items.join(", "));
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCategory {
            section_id,
            is_new: false,
        });
    }

    pub fn save_category_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (section_id, is_new) = match self.active_dialog {
            Some(Dialog::EditCategory { section_id, is_new }) => (section_id, is_new),
            _ => return,
        };

        let label = self.cat_label_input.read(cx).text().trim().to_string();
        let items_raw = self.cat_items_input.read(cx).text().to_string();
        if label.is_empty() {
            self.notify_error("Category name is required.");
            return;
        }
        let items: Vec<String> = items_raw
            .split([',', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let separator = self.cat_separator.clone();

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
        {
            if is_new {
                categories.push(SkillCategoryItem {
                    id: Uuid::new_v4(),
                    origin_item_id: None,
                    enabled: true,
                    category_label: label,
                    separator,
                    items,
                });
            } else if let Some(cid) = self.editing_category_id
                && let Some(existing) = categories.iter_mut().find(|c| c.id == cid)
            {
                existing.category_label = label;
                existing.separator = separator;
                existing.items = items;
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if is_new {
            "Skill category added."
        } else {
            "Skill category updated."
        });
    }

    pub fn delete_current_category(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditCategory { section_id, .. }) = self.active_dialog else {
            return;
        };
        let Some(cid) = self.editing_category_id.take() else {
            return;
        };
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
        {
            categories.retain(|c| c.id != cid);
        }
        self.prune_orphaned_proposals_for_active();
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Skill category deleted.");
    }

    pub fn toggle_category_enabled(
        &mut self,
        section_id: ItemId,
        category_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == section_id)
            && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
            && let Some(cat) = categories.iter_mut().find(|c| c.id == category_id)
        {
            cat.enabled = !cat.enabled;
        }
        self.persist_active_doc(cx);
    }

    // -----------------------------------------------------------------------
    // Personal Info (PII Header) CRUD
    // -----------------------------------------------------------------------

    pub fn open_edit_contact_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(h) = self.active_document().map(|d| d.pii_header.clone()) else {
            return;
        };
        self.pii_name_input.update(cx, |i, cx| {
            i.set_value(h.full_name);
            cx.notify();
        });
        self.pii_headline_input.update(cx, |i, cx| {
            i.set_value(h.headline_job_title.unwrap_or_default());
            cx.notify();
        });
        self.pii_email_input.update(cx, |i, cx| {
            i.set_value(h.email.unwrap_or_default());
            cx.notify();
        });
        self.pii_phone_input.update(cx, |i, cx| {
            i.set_value(h.phone.unwrap_or_default());
            cx.notify();
        });
        self.pii_location_input.update(cx, |i, cx| {
            i.set_value(h.location.unwrap_or_default());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditContact);
    }

    pub fn save_contact_from_dialog(&mut self, cx: &mut Context<Self>) {
        let full_name = self.pii_name_input.read(cx).text().trim().to_string();
        let headline = self.pii_headline_input.read(cx).text().trim().to_string();
        let email = self.pii_email_input.read(cx).text().trim().to_string();
        let phone = self.pii_phone_input.read(cx).text().trim().to_string();
        let location = self.pii_location_input.read(cx).text().trim().to_string();

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.pii_header.full_name = full_name;
            doc.pii_header.headline_job_title = (!headline.is_empty()).then_some(headline);
            doc.pii_header.email = (!email.is_empty()).then_some(email);
            doc.pii_header.phone = (!phone.is_empty()).then_some(phone);
            doc.pii_header.location = (!location.is_empty()).then_some(location);
        }
        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify("Contact header updated locally.");
    }

    pub fn open_add_social_dialog(&mut self, cx: &mut Context<Self>) {
        self.social_platform_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.social_display_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.social_url_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSocialLink { link_id: None });
    }

    pub fn open_edit_social_dialog(&mut self, link: &SocialLink, cx: &mut Context<Self>) {
        self.social_platform_input.update(cx, |i, cx| {
            i.set_value(link.platform.clone());
            cx.notify();
        });
        self.social_display_input.update(cx, |i, cx| {
            i.set_value(link.display_text.clone());
            cx.notify();
        });
        self.social_url_input.update(cx, |i, cx| {
            i.set_value(link.url.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSocialLink {
            link_id: Some(link.id),
        });
    }

    pub fn save_social_link_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditSocialLink { link_id }) = self.active_dialog else {
            return;
        };
        let platform = self
            .social_platform_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let display = self.social_display_input.read(cx).text().trim().to_string();
        let url = self.social_url_input.read(cx).text().trim().to_string();

        if platform.is_empty() && url.is_empty() {
            self.notify_error("Provide at least a platform name and URL.");
            return;
        }
        let final_platform = if platform.is_empty() {
            "Link".to_string()
        } else {
            platform
        };
        let final_display = if display.is_empty() {
            url.clone()
        } else {
            display
        };
        let final_url = if url.is_empty() {
            final_display.clone()
        } else {
            url
        };

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            if let Some(id) = link_id {
                if let Some(existing) = doc.pii_header.social_links.iter_mut().find(|l| l.id == id)
                {
                    existing.platform = final_platform;
                    existing.display_text = final_display;
                    existing.url = final_url;
                }
            } else {
                doc.pii_header.social_links.push(SocialLink {
                    id: Uuid::new_v4(),
                    platform: final_platform,
                    display_text: final_display,
                    url: final_url,
                    enabled: true,
                });
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if link_id.is_some() {
            "Social link updated."
        } else {
            "Social link added."
        });
    }

    pub fn toggle_social_link_enabled(&mut self, link_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(link) = doc
                .pii_header
                .social_links
                .iter_mut()
                .find(|l| l.id == link_id)
        {
            link.enabled = !link.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn delete_social_link(&mut self, link_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.pii_header.social_links.retain(|l| l.id != link_id);
        }
        self.persist_active_doc(cx);
        self.notify("Social link removed.");
    }

    pub fn open_add_custom_field_dialog(&mut self, cx: &mut Context<Self>) {
        self.custom_field_label_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.custom_field_value_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCustomField { field_id: None });
    }

    pub fn open_edit_custom_field_dialog(
        &mut self,
        field: &CustomHeaderField,
        cx: &mut Context<Self>,
    ) {
        self.custom_field_label_input.update(cx, |i, cx| {
            i.set_value(field.label.clone());
            cx.notify();
        });
        self.custom_field_value_input.update(cx, |i, cx| {
            i.set_value(field.value.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditCustomField {
            field_id: Some(field.id),
        });
    }

    pub fn save_custom_field_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::EditCustomField { field_id }) = self.active_dialog else {
            return;
        };
        let label = self
            .custom_field_label_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let value = self
            .custom_field_value_input
            .read(cx)
            .text()
            .trim()
            .to_string();

        if label.is_empty() || value.is_empty() {
            self.notify_error("Both label and value are required.");
            return;
        }

        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            if let Some(id) = field_id {
                if let Some(existing) = doc.pii_header.custom_fields.iter_mut().find(|f| f.id == id)
                {
                    existing.label = label;
                    existing.value = value;
                }
            } else {
                doc.pii_header.custom_fields.push(CustomHeaderField {
                    id: Uuid::new_v4(),
                    label,
                    value,
                    enabled: true,
                });
            }
        }

        self.persist_active_doc(cx);
        self.active_dialog = None;
        self.notify(if field_id.is_some() {
            "Custom field updated."
        } else {
            "Custom field added."
        });
    }

    pub fn toggle_custom_field_enabled(&mut self, field_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(field) = doc
                .pii_header
                .custom_fields
                .iter_mut()
                .find(|f| f.id == field_id)
        {
            field.enabled = !field.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn delete_custom_field(&mut self, field_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
        {
            doc.pii_header.custom_fields.retain(|f| f.id != field_id);
        }
        self.persist_active_doc(cx);
        self.notify("Custom field removed.");
    }

    // -----------------------------------------------------------------------
    // AI Skills Management (Real FileSkillRegistry CRUD)
    // -----------------------------------------------------------------------

    pub fn open_add_skill_dialog(&mut self, cx: &mut Context<Self>) {
        self.editing_skill_id = None;
        self.skill_name_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("");
            cx.notify();
        });
        self.skill_desc_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("");
            cx.notify();
        });
        self.skill_category_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("Custom");
            cx.notify();
        });
        self.skill_inst_input.update(cx, |i, cx| {
            i.set_read_only(false, cx);
            i.set_value("");
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSkill {
            is_new: true,
            read_only: false,
        });
    }

    pub fn open_edit_skill_dialog(&mut self, skill: &SkillManifest, cx: &mut Context<Self>) {
        let is_user = self.is_user_owned_skill(skill);
        let read_only = !is_user;
        self.editing_skill_id = Some(skill.id.clone());
        self.skill_name_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.name.clone());
            cx.notify();
        });
        self.skill_desc_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.description.clone().unwrap_or_default());
            cx.notify();
        });
        self.skill_category_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.category.clone());
            cx.notify();
        });
        self.skill_inst_input.update(cx, |i, cx| {
            i.set_read_only(read_only, cx);
            i.set_value(skill.prompt_injection.system_directives.clone());
            cx.notify();
        });
        self.active_dialog = Some(Dialog::EditSkill {
            is_new: false,
            read_only,
        });
    }

    pub fn save_skill_from_dialog(&mut self, cx: &mut Context<Self>) {
        let (is_new, read_only) = match self.active_dialog {
            Some(Dialog::EditSkill { is_new, read_only }) => (is_new, read_only),
            _ => return,
        };
        if read_only {
            self.active_dialog = None;
            return;
        }

        let name = self.skill_name_input.read(cx).text().trim().to_string();
        let desc = self.skill_desc_input.read(cx).text().trim().to_string();
        let category = self.skill_category_input.read(cx).text().trim().to_string();
        let instructions = self.skill_inst_input.read(cx).text().trim().to_string();

        if name.is_empty() || instructions.is_empty() {
            self.notify_error("Skill name and instructions are required.");
            return;
        }

        let slug = if is_new {
            slugify_skill_id(&name)
        } else {
            self.editing_skill_id
                .clone()
                .unwrap_or_else(|| slugify_skill_id(&name))
        };
        if slug.is_empty() {
            self.notify_error("Skill name must contain letters or digits.");
            return;
        }

        let final_cat = if category.is_empty() {
            "Custom".to_string()
        } else {
            category
        };
        let final_desc = if desc.is_empty() { name.clone() } else { desc };

        let content = format!(
            "---\nname: \"{}\"\ndescription: \"{}\"\ncategory: \"{}\"\nversion: \"1.0.0\"\n---\n\n{}\n",
            escape_yaml_str(&name),
            escape_yaml_str(&final_desc),
            escape_yaml_str(&final_cat),
            instructions
        );

        let dir = self.services.skills.skills_dir().join(&slug);
        if let Err(err) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join("SKILL.md"), content))
        {
            self.notify_error(format!("Failed to write SKILL.md: {err}"));
            return;
        }
        // Remove legacy flat .toml / .md duplicate in user skills dir if converting from imported file
        for ext in ["toml", "md"] {
            let flat = self.services.skills.skills_dir().join(format!("{slug}.{ext}"));
            if flat.is_file() {
                let _ = std::fs::remove_file(flat);
            }
        }

        match self.services.skills.reload() {
            Ok(_) => {
                self.active_dialog = None;
                self.notify(if is_new {
                    format!("Skill \"{name}\" saved to user skills directory.")
                } else {
                    format!("Skill \"{name}\" updated.")
                });
            }
            Err(err) => {
                self.notify_error(format!("Skill saved, but reload failed: {err}"));
            }
        }
    }

    pub fn toggle_skill_enabled(&mut self, skill_id: &str) {
        let (current, display_name) = self
            .services
            .skills
            .get(skill_id)
            .map(|s| (s.enabled, s.name))
            .unwrap_or_else(|| (false, skill_id.to_string()));
        match self.services.skills.set_skill_enabled(skill_id, !current) {
            Ok(()) => {
                self.notify(if !current {
                    format!("Enabled \"{display_name}\".")
                } else {
                    format!("Disabled \"{display_name}\".")
                });
            }
            Err(err) => {
                self.notify_error(format!("Could not toggle skill: {err}"));
            }
        }
    }

    pub fn delete_skill(&mut self, skill_id: &str) {
        let display_name = self
            .services
            .skills
            .get(skill_id)
            .map(|s| s.name)
            .unwrap_or_else(|| skill_id.to_string());
        match self.services.skills.delete_skill(skill_id) {
            Ok(()) => {
                self.variation_skills.remove(skill_id);
                self.notify(format!("Deleted skill \"{display_name}\"."));
            }
            Err(err) => {
                self.notify_error(format!("Could not delete skill: {err}"));
            }
        }
    }

    pub fn restore_default_skills(&mut self) {
        match self.services.skills.restore_bundled_skills() {
            Ok(_) => {
                self.notify("Restored default bundled AI skills.");
            }
            Err(err) => {
                self.notify_error(format!("Could not restore default skills: {err}"));
            }
        }
    }

    pub fn reload_skills(&mut self) {
        match self.services.skills.reload() {
            Ok(list) => {
                self.notify(format!("Reloaded {} AI skills.", list.len()));
            }
            Err(err) => {
                self.notify_error(format!("Skill reload failed: {err}"));
            }
        }
    }

    pub fn import_skill_from_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let skills = self.services.skills.clone();
        let paths_rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import AI skill (.md or .toml)".into()),
        });

        cx.spawn(async move |this, cx| {
            let path = match paths_rx.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => path,
                    None => return,
                },
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open file picker: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let result = cx
                .background_executor()
                .spawn(async move { skills.import_skill(&path) })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(manifest) => {
                        view.notify(format!(
                            "Imported skill \"{}\" ({}).",
                            manifest.name, manifest.id
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Skill import failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // Settings, Sign in with ChatGPT, Backups, JSON Import/Export & Snapshots
    // -----------------------------------------------------------------------

    pub fn open_chatgpt_usage_settings(&mut self) {
        if let Err(err) = open::that(CHATGPT_USAGE_SETTINGS_URL) {
            self.notify_error(format!(
                "Could not open ChatGPT usage settings ({CHATGPT_USAGE_SETTINGS_URL}): {err}"
            ));
        }
    }

    pub fn dismiss_plan_welcome_banner(&mut self) {
        self.show_plan_welcome_banner = false;
        let _ = self
            .services
            .repository
            .save_setting("ui.chatgpt_welcome_seen", "true");
    }

    pub fn start_chatgpt_sign_in(
        &mut self,
        force_consent: bool,
        fresh_registration: bool,
        cx: &mut Context<Self>,
    ) {
        if self.auth_in_progress {
            self.notify("A Sign in with ChatGPT flow is already waiting in your browser.");
            return;
        }

        self.auth_in_progress = true;
        self.auth_error = None;
        self.notify("Opening ChatGPT sign-in in your browser…");

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<ChatGptAuthStatus>>();

        crate::runtime::handle().spawn(async move {
            let result = ai
                .sign_in_with_chatgpt(force_consent, fresh_registration)
                .await;
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            if let Some(result) = rx.recv().await {
                let _ = this.update(cx, |view, cx| {
                    view.auth_in_progress = false;
                    match result {
                        Ok(status) => {
                            view.chatgpt_auth = status.clone();
                            view.auth_error = None;
                            if status.is_ready_for_inference() {
                                if view.active_provider != AiProviderKind::ChatGpt {
                                    view.active_provider = AiProviderKind::ChatGpt;
                                    let mut cfg = view.services.ai.config();
                                    cfg.provider = AiProviderKind::ChatGpt;
                                    let _ = view.services.ai.configure(cfg);
                                    let _ = view
                                        .services
                                        .repository
                                        .save_setting("ai.provider", AiProviderKind::ChatGpt.as_str());
                                }
                                let welcome_seen = view
                                    .services
                                    .repository
                                    .load_setting("ui.chatgpt_welcome_seen")
                                    .ok()
                                    .flatten()
                                    .is_some_and(|v| v == "true");
                                if !welcome_seen {
                                    view.show_plan_welcome_banner = true;
                                }
                                view.notify(format!(
                                    "Connected to ChatGPT as {}. Loading available models…",
                                    status.account_label()
                                ));
                                view.fetch_available_models(false, cx);
                            } else {
                                view.available_models.clear();
                                view.notify(
                                    "Signed in to ChatGPT, but ChatGPT plan usage was not enabled. Click \"Enable plan usage\" to grant access.",
                                );
                            }
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            view.chatgpt_auth = view.services.ai.auth_status();
                            view.auth_error = Some(msg.clone());
                            view.notify_error(msg);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn sign_out_chatgpt(&mut self, cx: &mut Context<Self>) {
        if self.auth_in_progress {
            return;
        }
        self.auth_in_progress = true;
        self.auth_error = None;

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<Option<String>>>();

        crate::runtime::handle().spawn(async move {
            let result = ai.sign_out().await;
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            if let Some(result) = rx.recv().await {
                let _ = this.update(cx, |view, cx| {
                    view.auth_in_progress = false;
                    view.chatgpt_auth = view.services.ai.auth_status();
                    view.available_models.clear();
                    view.models_error = None;
                    view.models_loading = false;
                    view.show_plan_welcome_banner = false;
                    match result {
                        Ok(Some(warning)) => view.notify(warning),
                        Ok(None) => view.notify("Signed out of ChatGPT and revoked local session."),
                        Err(err) => view.notify_error(format!("Sign-out error: {err}")),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn set_ai_provider(&mut self, provider: AiProviderKind, cx: &mut Context<Self>) {
        if self.active_provider == provider {
            return;
        }
        self.active_provider = provider;
        self.available_models.clear();
        self.models_error = None;
        self.models_loading = false;
        self.model_filter_query.clear();
        self.reset_models_list_scroll();
        self.model_filter_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        let mut cfg = self.services.ai.config();
        cfg.provider = provider;
        if cfg.base_url.trim().is_empty() {
            cfg.base_url = CHATGPT_API_BASE_URL.to_string();
        }
        if cfg.gemini_base_url.trim().is_empty() {
            cfg.gemini_base_url = GEMINI_API_BASE_URL.to_string();
        }
        if cfg.gemini_model.trim().is_empty() {
            cfg.gemini_model = DEFAULT_GEMINI_MODEL.to_string();
        }
        if let Err(err) = self.services.ai.configure(cfg) {
            self.notify_error(format!("Failed to switch AI provider: {err}"));
            return;
        }
        let _ = self
            .services
            .repository
            .save_setting("ai.provider", provider.as_str());

        self.notify(format!("Switched AI engine to {}.", provider.display_name()));
        if self.is_ai_configured() {
            self.fetch_available_models(false, cx);
        }
    }

    pub fn open_gemini_api_key_page(&mut self) {
        if let Err(err) = open::that(GEMINI_API_KEY_URL) {
            self.notify_error(format!(
                "Could not open Google AI Studio ({GEMINI_API_KEY_URL}): {err}"
            ));
        }
    }

    pub fn toggle_gemini_key_visibility(&mut self, cx: &mut Context<Self>) {
        self.gemini_key_visible = !self.gemini_key_visible;
        let masked = !self.gemini_key_visible;
        self.gemini_api_key_input.update(cx, |i, cx| {
            i.set_masked(masked, cx);
        });
    }

    pub fn save_gemini_api_key(&mut self, cx: &mut Context<Self>) {
        let key = self
            .gemini_api_key_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        if key.is_empty() {
            self.notify_error("Paste a Google Gemini API key first.");
            return;
        }

        if let Err(err) = credentials::save_gemini_api_key(&key) {
            self.notify_error(format!("Could not save Gemini API key: {err}"));
            return;
        }

        self.gemini_key_configured = true;
        let mut cfg = self.services.ai.config();
        cfg.provider = AiProviderKind::GeminiApi;
        cfg.gemini_api_key = Some(key);
        if cfg.gemini_base_url.trim().is_empty() {
            cfg.gemini_base_url = GEMINI_API_BASE_URL.to_string();
        }
        if cfg.gemini_model.trim().is_empty() {
            cfg.gemini_model = self.selected_gemini_model.clone();
        }
        if let Err(err) = self.services.ai.configure(cfg) {
            self.notify_error(format!("Failed to configure Gemini API: {err}"));
            return;
        }
        self.active_provider = AiProviderKind::GeminiApi;
        let _ = self
            .services
            .repository
            .save_setting("ai.provider", AiProviderKind::GeminiApi.as_str());

        self.notify("Gemini API key saved locally. Loading available Gemini models…");
        self.fetch_available_models(true, cx);
    }

    pub fn clear_gemini_api_key(&mut self, cx: &mut Context<Self>) {
        let _ = credentials::clear_gemini_api_key();
        self.gemini_key_configured = false;
        self.gemini_api_key_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        let mut cfg = self.services.ai.config();
        cfg.gemini_api_key = None;
        let _ = self.services.ai.configure(cfg);
        if self.active_provider == AiProviderKind::GeminiApi {
            self.available_models.clear();
            self.models_error = None;
            self.models_loading = false;
        }
        self.notify("Removed Google Gemini API key.");
    }

    pub fn active_model_display_name(&self) -> String {
        let current = self.active_model_id();
        self.available_models
            .iter()
            .find(|m| m.id == current)
            .map(|m| m.display_name.clone())
            .unwrap_or_else(|| current.to_string())
    }

    pub fn clear_model_filter(&mut self, cx: &mut Context<Self>) {
        self.model_filter_query.clear();
        self.reset_models_list_scroll();
        self.model_filter_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
    }

    pub fn filtered_available_models(&self) -> Vec<&ModelInfo> {
        let query = self.model_filter_query.trim().to_lowercase();
        if query.is_empty() {
            return self.available_models.iter().collect();
        }
        self.available_models
            .iter()
            .filter(|m| {
                m.id.to_lowercase().contains(&query)
                    || m.display_name.to_lowercase().contains(&query)
                    || m.description.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn select_model(&mut self, model_id: &str, notify_user: bool, _cx: &mut Context<Self>) {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return;
        }
        let mut cfg = self.services.ai.config();
        let setting_key = match self.active_provider {
            AiProviderKind::ChatGpt => {
                self.selected_model = trimmed.to_string();
                if cfg.base_url.trim().is_empty() {
                    cfg.base_url = CHATGPT_API_BASE_URL.to_string();
                }
                cfg.model = self.selected_model.clone();
                "ai.model"
            }
            AiProviderKind::GeminiApi => {
                self.selected_gemini_model = trimmed.to_string();
                if cfg.gemini_base_url.trim().is_empty() {
                    cfg.gemini_base_url = GEMINI_API_BASE_URL.to_string();
                }
                cfg.gemini_model = self.selected_gemini_model.clone();
                "ai.gemini_model"
            }
        };
        if let Err(err) = self.services.ai.configure(cfg) {
            self.notify_error(format!("Failed to select model: {err}"));
            return;
        }
        let _ = self
            .services
            .repository
            .save_setting(setting_key, trimmed);
        if notify_user {
            let label = self.active_model_display_name();
            self.notify(format!(
                "Selected {} model: {label}",
                self.active_provider_name()
            ));
        }
    }

    pub fn fetch_available_models(&mut self, notify_on_success: bool, cx: &mut Context<Self>) {
        self.chatgpt_auth = self.services.ai.auth_status();
        let provider = self.active_provider;

        match provider {
            AiProviderKind::ChatGpt => {
                if !self.chatgpt_auth.is_ready_for_inference() {
                    self.models_loading = false;
                    self.available_models.clear();
                    self.models_error = if self.chatgpt_auth.signed_in
                        && !self.chatgpt_auth.plan_usage_enabled
                    {
                        Some("Enable ChatGPT plan usage above to load available models.".to_string())
                    } else {
                        Some("Sign in with ChatGPT above to browse models included in your plan.".to_string())
                    };
                    if notify_on_success {
                        self.notify_error("Sign in with ChatGPT first to load models.");
                    }
                    return;
                }
            }
            AiProviderKind::GeminiApi => {
                if !self.gemini_key_configured {
                    self.models_loading = false;
                    self.available_models.clear();
                    self.models_error = Some(
                        "Save your Google Gemini API key above to load available Gemini models."
                            .to_string(),
                    );
                    if notify_on_success {
                        self.notify_error("Save a Google Gemini API key first to load models.");
                    }
                    return;
                }
            }
        }

        self.models_fetch_generation = self.models_fetch_generation.wrapping_add(1);
        let generation = self.models_fetch_generation;
        self.models_loading = true;
        self.models_error = None;

        let ai = self.services.ai.clone();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<Vec<ModelInfo>>>();

        crate::runtime::handle().spawn(async move {
            let result = ai.list_models().await;
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            if let Some(result) = rx.recv().await {
                let _ = this.update(cx, |view, cx| {
                    if view.models_fetch_generation != generation
                        || view.active_provider != provider
                    {
                        return;
                    }
                    view.models_loading = false;
                    view.chatgpt_auth = view.services.ai.auth_status();
                    let provider_label = provider.display_name();
                    match result {
                        Ok(models) => {
                            view.models_error = None;
                            let count = models.len();
                            let active_id = view.active_model_id().to_string();
                            let current_valid = models.iter().any(|m| m.id == active_id);
                            let auto_pick = if !current_valid && !models.is_empty() {
                                models.first().map(|m| m.id.clone())
                            } else {
                                None
                            };
                            view.available_models = models;
                            view.reset_models_list_scroll();
                            if let Some(chosen) = auto_pick {
                                view.select_model(&chosen, false, cx);
                            }
                            if notify_on_success {
                                view.notify(format!(
                                    "Loaded {count} available models from {provider_label}."
                                ));
                            }
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            view.models_error = Some(msg.clone());
                            if notify_on_success {
                                view.notify_error(format!(
                                    "Could not fetch models from {provider_label}: {msg}"
                                ));
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn backup_database(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let default_name = format!("vanguardcv-backup-{}.db", Utc::now().format("%Y%m%d-%H%M"));
        let initial_dir = Self::initial_save_dir();
        let path_rx = cx.prompt_for_new_path(&initial_dir, Some(&default_name));

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("db");
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    repo.backup_to(&path)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!("Database backed up to {}", path.display()));
                    }
                    Err(err) => {
                        view.notify_error(format!("Database backup failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_active_json(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.active_document() else {
            self.notify("Select a CV from the library before exporting JSON.");
            return;
        };
        let active_id = active.id;
        let slug = slugify_skill_id(&active.title);
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let default_name = format!("{}.json", if slug.is_empty() { "cv" } else { &slug });
        let initial_dir = Self::initial_save_dir();
        let path_rx = cx.prompt_for_new_path(&initial_dir, Some(&default_name));

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("json");
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let json = repo.export_json(active_id)?;
                    std::fs::write(&path, json)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!("Exported CV JSON to {}", path.display()));
                    }
                    Err(err) => {
                        view.notify_error(format!("JSON export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn import_cv_json(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let paths_rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import CV from JSON".into()),
        });

        cx.spawn(async move |this, cx| {
            let path = match paths_rx.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => path,
                    None => return,
                },
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open file picker: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let raw = std::fs::read_to_string(&path)?;
                    repo.import_json(&raw)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(doc) => {
                        let title = doc.title.clone();
                        let id = doc.id;
                        view.documents.push(doc);
                        view.active_doc_id = Some(id);
                        view.refresh_snapshots();
                        view.trigger_pdf_preview_render(cx);
                        view.notify(format!("Imported \"{title}\" as a new Master CV."));
                    }
                    Err(err) => {
                        view.notify_error(format!("JSON import failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_vanguard_archive(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let skills = self.services.skills.clone();
        let default_name = format!(
            "vanguardcv-workspace-{}.vanguard",
            Utc::now().format("%Y%m%d-%H%M")
        );
        let initial_dir = Self::initial_save_dir();
        let path_rx = cx.prompt_for_new_path(&initial_dir, Some(&default_name));

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("vanguard");
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let payload = archive::export_vanguard_archive(repo.as_ref(), skills.as_ref())?;
                    std::fs::write(&path, payload)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!(
                            "Exported portable workspace archive to {}",
                            path.display()
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Archive export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn import_vanguard_archive(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.flush_pending_now();
        let repo = self.services.repository.clone();
        let skills = self.services.skills.clone();
        let paths_rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import .vanguard workspace archive".into()),
        });

        cx.spawn(async move |this, cx| {
            let path = match paths_rx.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => path,
                    None => return,
                },
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open file picker: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };

            let result = cx
                .background_executor()
                .spawn(async move {
                    let raw = std::fs::read_to_string(&path)?;
                    archive::import_vanguard_archive(&raw, repo.as_ref(), skills.as_ref())
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(outcome) => {
                        let reloaded = load_all_documents(&view.services);
                        if !reloaded.is_empty() {
                            view.documents = reloaded;
                        }
                        if let Some(first_id) = outcome.first_doc_id {
                            view.active_doc_id = Some(first_id);
                        }
                        view.refresh_snapshots();
                        view.trigger_pdf_preview_render(cx);
                        view.notify(format!(
                            "Imported {} master(s), {} variation(s), and {} custom skill(s).",
                            outcome.imported_masters,
                            outcome.imported_variations,
                            outcome.imported_skills
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Archive import failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_anonymized_template(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_document().cloned() else {
            self.notify("Select a CV from the library before exporting an anonymized template.");
            return;
        };
        self.flush_pending_now();
        let privacy = self.services.privacy.clone();
        let slug = slugify_skill_id(&doc.title);
        let default_name = format!(
            "{}-template.json",
            if slug.is_empty() { "cv" } else { &slug }
        );
        let initial_dir = Self::initial_save_dir();
        let path_rx = cx.prompt_for_new_path(&initial_dir, Some(&default_name));

        cx.spawn(async move |this, cx| {
            let mut path = match path_rx.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let _ = this.update(cx, |view, cx| {
                        view.notify_error(format!("Could not open save dialog: {err}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("json");
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let json = archive::export_anonymized_template_json(&doc, privacy.as_ref())?;
                    std::fs::write(&path, json)?;
                    Ok::<PathBuf, anyhow::Error>(path)
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(path) => {
                        view.notify(format!(
                            "Exported PII-free anonymized template to {}",
                            path.display()
                        ));
                    }
                    Err(err) => {
                        view.notify_error(format!("Template export failed: {err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn create_manual_snapshot(&mut self) {
        let Some(active_id) = self.active_doc_id else {
            self.notify("Select a CV from the library before creating a checkpoint.");
            return;
        };
        self.flush_pending_now();
        match self
            .services
            .repository
            .create_snapshot(active_id, "Manual checkpoint")
        {
            Ok(_) => {
                self.refresh_snapshots();
                self.notify("Snapshot checkpoint saved.");
            }
            Err(err) => {
                self.notify_error(format!("Snapshot failed: {err}"));
            }
        }
    }

    pub fn restore_snapshot_by_id(&mut self, snapshot_id: Uuid, cx: &mut Context<Self>) {
        self.flush_pending_now();
        match self.services.repository.restore_snapshot(snapshot_id) {
            Ok(restored) => {
                let restored_id = restored.id;
                if let Some(slot) = self.documents.iter_mut().find(|d| d.id == restored_id) {
                    *slot = restored;
                }
                self.proposals.remove(&restored_id);
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);
                self.notify("Restored CV to selected snapshot.");
            }
            Err(err) => {
                self.notify_error(format!("Restore failed: {err}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Editor Section / Entry / Bullet Toggles & Reordering
    // -----------------------------------------------------------------------

    pub fn toggle_section_expand(&mut self, sec_id: ItemId) {
        let current = self
            .expanded_sections
            .get(&sec_id)
            .copied()
            .unwrap_or_else(|| {
                self.active_document()
                    .and_then(|doc| doc.body.sections.iter().find(|s| s.id == sec_id))
                    .map(|s| match &s.kind {
                        SectionKind::Summary { .. } => true,
                        SectionKind::TimelineEntries { .. } => {
                            s.title.to_lowercase().contains("experience")
                        }
                        _ => false,
                    })
                    .unwrap_or(false)
            });
        self.expanded_sections.insert(sec_id, !current);
    }

    pub fn toggle_entry_expand(&mut self, entry_id: ItemId) {
        let current = self
            .expanded_entries
            .get(&entry_id)
            .copied()
            .unwrap_or(true);
        self.expanded_entries.insert(entry_id, !current);
    }

    pub fn toggle_section_enabled(&mut self, sec_id: ItemId, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == sec_id)
        {
            sec.enabled = !sec.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn begin_editor_drag(&mut self, kind: EditorDragKind) {
        self.active_drag_item = Some(kind);
        self.drag_did_reorder = false;
    }

    pub fn handle_editor_drag_move(
        &mut self,
        dragged: &EditorDragKind,
        target: &EditorDragKind,
        mouse_pos: Point<Pixels>,
        target_bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if dragged == target || !target_bounds.contains(&mouse_pos) {
            return;
        }
        let Some(active_id) = self.active_doc_id else {
            return;
        };
        let Some(doc) = self.documents.iter_mut().find(|d| d.id == active_id) else {
            return;
        };
        let Some((from_idx, to_idx)) = drag_item_index_pair_in_doc(doc, dragged, target) else {
            return;
        };
        // Directional midpoint hysteresis prevents rapid oscillation across unequal card heights.
        let threshold = target_bounds.size.height * 0.35;
        let crossed = if from_idx < to_idx {
            mouse_pos.y >= target_bounds.top() + threshold
        } else {
            mouse_pos.y <= target_bounds.bottom() - threshold
        };
        if !crossed {
            return;
        }
        if reorder_item_in_doc(doc, dragged, target) {
            self.active_drag_item = Some(dragged.clone());
            self.drag_did_reorder = true;
            cx.notify();
        }
    }

    pub fn finish_editor_drag(&mut self, cx: &mut Context<Self>) {
        let did_reorder = self.drag_did_reorder;
        self.active_drag_item = None;
        self.drag_did_reorder = false;
        if did_reorder {
            self.persist_active_doc(cx);
            cx.notify();
        }
    }

    pub fn toggle_entry_enabled(
        &mut self,
        sec_id: ItemId,
        entry_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == sec_id)
            && let SectionKind::TimelineEntries { entries } = &mut sec.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
        {
            entry.enabled = !entry.enabled;
        }
        self.persist_active_doc(cx);
    }

    pub fn toggle_bullet_enabled(
        &mut self,
        sec_id: ItemId,
        entry_id: Option<ItemId>,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .documents
            .iter_mut()
            .find(|d| Some(d.id) == self.active_doc_id)
            && let Some(sec) = doc.body.sections.iter_mut().find(|s| s.id == sec_id)
        {
            match &mut sec.kind {
                SectionKind::TimelineEntries { entries } => {
                    if let Some(eid) = entry_id
                        && let Some(entry) = entries.iter_mut().find(|e| e.id == eid)
                        && let Some(b) = entry.bullets.iter_mut().find(|b| b.id == bullet_id)
                    {
                        b.enabled = !b.enabled;
                    }
                }
                SectionKind::SimpleBulletList { items } => {
                    if let Some(b) = items.iter_mut().find(|b| b.id == bullet_id) {
                        b.enabled = !b.enabled;
                    }
                }
                _ => {}
            }
        }
        self.persist_active_doc(cx);
    }

    pub fn toggle_variation_skill(&mut self, skill_id: &str) {
        if self.variation_skills.contains(skill_id) {
            self.variation_skills.remove(skill_id);
        } else {
            self.variation_skills.insert(skill_id.to_string());
        }
    }

    pub fn toggle_master_collapse(&mut self, root_id: DocumentId) {
        if self.collapsed_masters.contains(&root_id) {
            self.collapsed_masters.remove(&root_id);
        } else {
            self.collapsed_masters.insert(root_id);
        }
    }

    pub fn toggle_document_menu(&mut self) {
        if self.active_dialog == Some(Dialog::DocumentMenu) {
            self.active_dialog = None;
        } else {
            self.active_dialog = Some(Dialog::DocumentMenu);
        }
    }

    // -----------------------------------------------------------------------
    // Typst Live Preview Rasterization
    // -----------------------------------------------------------------------

    pub fn trigger_pdf_preview_render(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.active_document().cloned() else {
            self.is_rendering_pdf = false;
            self.rendered_images.clear();
            self.preview_error = None;
            return;
        };
        let pdf_engine = self.services.pdf.clone();
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        let generation = self.pdf_render_generation;

        self.is_rendering_pdf = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { pdf_engine.render_preview_pages(&doc.pii_header, &doc.body, 10) },
                )
                .await;

            let _ = this.update(cx, |view, cx| {
                if view.pdf_render_generation != generation {
                    return;
                }
                view.is_rendering_pdf = false;
                match result {
                    Ok(pages) => {
                        view.rendered_images = pages
                            .into_iter()
                            .filter_map(|p| rendered_page_to_image(&p))
                            .collect();
                        view.preview_error = None;
                    }
                    Err(err) => {
                        view.rendered_images.clear();
                        view.preview_error = Some(err.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // Default CV, Job Discovery & Factory Reset
    // -----------------------------------------------------------------------

    /// Marks or unmarks a CV as the workspace Default CV (persisted in SQLite).
    pub fn set_default_cv(&mut self, doc_id: DocumentId, cx: &mut Context<Self>) {
        if self.default_cv_id == Some(doc_id) {
            self.default_cv_id = None;
            let _ = self
                .services
                .repository
                .save_setting(settings_keys::DEFAULT_CV_ID, "");
            self.notify("Cleared default CV.");
            return;
        }
        let title = self
            .find_document(doc_id)
            .map(|d| d.title.clone())
            .unwrap_or_else(|| "CV".to_string());
        self.default_cv_id = Some(doc_id);
        let _ = self
            .services
            .repository
            .save_setting(settings_keys::DEFAULT_CV_ID, &doc_id.to_string());
        self.select_job_cv_source(CvSearchSource::Single(doc_id), cx);
        self.notify(format!(
            "Set \"{title}\" as default CV (used automatically on launch for Job discovery)."
        ));
    }

    fn persist_job_profiles_cache(&self) {
        if let Some(json) = self.job_cache.serialize_persisted_profiles() {
            let _ = self
                .services
                .repository
                .save_setting(settings_keys::JOB_PROFILES_CACHE, &json);
        }
    }

    /// Runs silently at application startup: if a Default CV is set, prepares its
    /// saved keywords (auto-refreshing once via AI if configured and missing/stale,
    /// otherwise extracting locally) and fetches page 1 of last-24h opportunities.
    fn bootstrap_default_cv_job_search(&mut self, cx: &mut Context<Self>) {
        let Some(def_id) = self.default_cv_id else {
            return;
        };
        if !self.documents.iter().any(|d| d.id == def_id) {
            return;
        }
        let source = CvSearchSource::Single(def_id);
        self.job_source = Some(source.clone());
        self.job_filters.posted_within_days = Some(1);
        self.job_filters.page = 1;

        let docs = self.resolved_job_source_docs(&source);
        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let hash = compute_cv_content_hash(&doc_refs);

        if let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &hash) {
            self.job_profiles = cached.to_vec();
            self.job_profiles_from_cache = true;
            self.job_profiles_used_fallback = used_fb;
            self.job_profiles_stale = false;
            self.search_jobs_internal(false, true, cx);
        } else {
            self.generate_job_profiles_internal(false, true, cx);
        }
    }

    /// Resolves the active CV source for Job Discovery, prioritizing explicit
    /// selection, then Default CV, then active document, then first Master CV.
    pub fn resolved_job_source(&self) -> Option<CvSearchSource> {
        if let Some(ref src) = self.job_source {
            match src {
                CvSearchSource::Single(id) if self.documents.iter().any(|d| d.id == *id) => {
                    return Some(src.clone());
                }
                CvSearchSource::AllCvs if !self.documents.is_empty() => {
                    return Some(CvSearchSource::AllCvs);
                }
                _ => {}
            }
        }
        if let Some(def_id) = self.default_cv_id
            && self.documents.iter().any(|d| d.id == def_id)
        {
            return Some(CvSearchSource::Single(def_id));
        }
        if let Some(active_id) = self.active_doc_id
            && self.documents.iter().any(|d| d.id == active_id)
        {
            return Some(CvSearchSource::Single(active_id));
        }
        self.documents
            .iter()
            .find(|d| d.parent_id.is_none())
            .or_else(|| self.documents.first())
            .map(|d| CvSearchSource::Single(d.id))
    }

    fn resolved_job_source_docs(&self, source: &CvSearchSource) -> Vec<CvDocument> {
        match source {
            CvSearchSource::Single(id) => self
                .documents
                .iter()
                .find(|d| d.id == *id)
                .cloned()
                .into_iter()
                .collect(),
            CvSearchSource::AllCvs => self.documents.clone(),
        }
    }

    /// Resolves which Master CV will serve as the parent when clicking "Tailor CV".
    pub fn resolved_job_target_master_id(&self) -> Option<DocumentId> {
        if let Some(CvSearchSource::Single(id)) = self.resolved_job_source()
            && let Some(doc) = self.find_document(id)
        {
            return Some(doc.parent_id.unwrap_or(doc.id));
        }
        if let Some(def_id) = self.default_cv_id
            && let Some(doc) = self.find_document(def_id)
        {
            return Some(doc.parent_id.unwrap_or(doc.id));
        }
        self.active_document()
            .map(|active| active.parent_id.unwrap_or(active.id))
            .or_else(|| {
                self.documents
                    .iter()
                    .find(|d| d.parent_id.is_none())
                    .map(|d| d.id)
            })
            .or_else(|| self.documents.first().map(|d| d.id))
    }

    pub fn resolved_job_target_master_title(&self) -> Option<String> {
        let id = self.resolved_job_target_master_id()?;
        self.find_document(id).map(|d| d.title.clone())
    }

    fn sync_job_source_and_cached_profiles(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.resolved_job_source() else {
            return;
        };
        if self.job_source.as_ref() != Some(&source) {
            self.job_source = Some(source.clone());
        }
        let docs = self.resolved_job_source_docs(&source);
        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let hash = compute_cv_content_hash(&doc_refs);

        if let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &hash) {
            self.job_profiles_stale = false;
            if self.job_profiles.is_empty() {
                self.job_profiles = cached.to_vec();
                self.job_profiles_from_cache = true;
                self.job_profiles_used_fallback = used_fb;
                if self.job_listings.is_empty() && !self.job_listings_loading {
                    self.search_jobs_internal(false, true, cx);
                }
            }
        } else if self.job_cache.has_stale_profiles(&source, &hash) {
            // CV was edited since its keywords were last extracted — refresh lazily once!
            self.job_profiles_stale = true;
            if !self.job_profiles_loading {
                self.generate_job_profiles_internal(true, true, cx);
            }
        }
    }

    pub fn select_job_cv_source(&mut self, source: CvSearchSource, cx: &mut Context<Self>) {
        self.job_source = Some(source.clone());
        self.job_error = None;
        self.job_filters.page = 1;
        let docs = self.resolved_job_source_docs(&source);
        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let hash = compute_cv_content_hash(&doc_refs);
        if let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &hash) {
            self.job_profiles = cached.to_vec();
            self.job_profiles_from_cache = true;
            self.job_profiles_used_fallback = used_fb;
            self.job_profiles_stale = false;
            self.search_jobs(false, cx);
        } else if self.job_cache.has_stale_profiles(&source, &hash) {
            self.job_profiles.clear();
            self.job_profiles_from_cache = false;
            self.job_profiles_used_fallback = false;
            self.job_profiles_stale = true;
            self.generate_job_profiles_internal(true, false, cx);
        } else {
            self.job_profiles.clear();
            self.job_profiles_from_cache = false;
            self.job_profiles_used_fallback = false;
            self.job_profiles_stale = false;
        }
    }

    pub fn generate_job_profiles(&mut self, force_refresh: bool, cx: &mut Context<Self>) {
        self.generate_job_profiles_internal(force_refresh, false, cx);
    }

    fn generate_job_profiles_internal(
        &mut self,
        force_refresh: bool,
        silent: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.resolved_job_source() else {
            if !silent {
                self.notify("Create or select a CV first to generate search profiles.");
            }
            return;
        };
        self.job_source = Some(source.clone());
        self.job_filters.page = 1;
        let docs = self.resolved_job_source_docs(&source);
        if docs.is_empty() {
            if !silent {
                self.notify("No saved CVs found to analyze.");
            }
            return;
        }

        let doc_refs: Vec<&CvDocument> = docs.iter().collect();
        let content_hash = compute_cv_content_hash(&doc_refs);

        if !force_refresh
            && let Some((cached, used_fb)) = self.job_cache.get_profiles(&source, &content_hash)
        {
            self.job_profiles = cached.to_vec();
            self.job_profiles_from_cache = true;
            self.job_profiles_used_fallback = used_fb;
            self.job_profiles_stale = false;
            self.job_error = None;
            self.search_jobs_internal(false, silent, cx);
            return;
        }

        if !self.is_ai_configured() {
            let local_profiles =
                extract_fallback_profiles(&doc_refs, self.services.privacy.as_ref());
            self.job_cache.put_profiles(
                &source,
                &content_hash,
                local_profiles.clone(),
                true,
            );
            self.persist_job_profiles_cache();
            self.job_profiles = local_profiles;
            self.job_profiles_from_cache = false;
            self.job_profiles_used_fallback = true;
            self.job_profiles_stale = false;
            self.job_error = None;
            if !silent {
                self.notify(
                    "Extracted and saved local search profiles for this CV. Connect AI in Settings for richer role suggestions.",
                );
            }
            self.search_jobs_internal(false, silent, cx);
            return;
        }

        self.job_profile_generation = self.job_profile_generation.wrapping_add(1);
        let generation = self.job_profile_generation;
        self.job_profiles_loading = true;
        self.job_error = None;

        let ai = self.services.ai.clone();
        let scrubber = self.services.privacy.clone();
        let source_for_task = source.clone();
        let hash_for_task = content_hash.clone();

        cx.spawn(async move |this, cx| {
            let handle = crate::runtime::handle().spawn(async move {
                let refs: Vec<&CvDocument> = docs.iter().collect();
                generate_search_profiles_with_ai(ai.as_ref(), &refs, scrubber.as_ref()).await
            });
            let result = match handle.await {
                Ok(res) => res,
                Err(join_err) => Err(anyhow::anyhow!("Profile generation task failed: {join_err}")),
            };

            let _ = this.update(cx, |view, cx| {
                if view.job_profile_generation != generation {
                    return;
                }
                view.job_profiles_loading = false;
                match result {
                    Ok((profiles, used_fallback)) => {
                        view.job_cache.put_profiles(
                            &source_for_task,
                            &hash_for_task,
                            profiles.clone(),
                            used_fallback,
                        );
                        view.persist_job_profiles_cache();
                        view.job_profiles = profiles;
                        view.job_profiles_from_cache = false;
                        view.job_profiles_used_fallback = used_fallback;
                        view.job_profiles_stale = false;
                        view.job_error = None;
                        if used_fallback && !silent {
                            view.notify(
                                "AI returned unstructured output; using clean local search profiles instead.",
                            );
                        }
                        view.search_jobs_internal(false, silent, cx);
                    }
                    Err(err) => {
                        let msg = format!("{err:#}");
                        view.job_error = Some(msg.clone());
                        if !silent || view.current_view == WorkspaceViewPage::Jobs {
                            if crate::ai::is_ai_limit_error(&msg) {
                                let lower = msg.to_ascii_lowercase();
                                if lower.starts_with("api") || lower.starts_with("usage limit") {
                                    view.notify_error(msg);
                                } else {
                                    view.notify_error(format!("API limit reached: {msg}"));
                                }
                            } else {
                                view.notify_error(format!(
                                    "Could not generate search profiles: {msg}"
                                ));
                            }
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn toggle_job_profile(&mut self, profile_id: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.job_profiles.iter_mut().find(|p| p.id == profile_id) {
            p.enabled = !p.enabled;
        }
        self.job_filters.page = 1;
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        } else {
            self.job_listings.clear();
            self.job_total_estimated = 0;
            self.job_has_next_page = false;
            self.job_listings_from_cache = false;
        }
    }

    pub fn toggle_job_profile_keyword(
        &mut self,
        profile_id: &str,
        keyword: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(p) = self.job_profiles.iter_mut().find(|p| p.id == profile_id)
            && !p.disabled_keywords.remove(keyword)
        {
            p.disabled_keywords.insert(keyword.to_string());
        }
        // Persist toggled keyword preference for this CV without burning AI tokens.
        if let Some(source) = self.resolved_job_source() {
            let docs = self.resolved_job_source_docs(&source);
            let doc_refs: Vec<&CvDocument> = docs.iter().collect();
            let hash = compute_cv_content_hash(&doc_refs);
            self.job_cache.put_profiles(
                &source,
                &hash,
                self.job_profiles.clone(),
                self.job_profiles_used_fallback,
            );
            self.persist_job_profiles_cache();
        }
        self.job_filters.page = 1;
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn toggle_job_remote_filter(&mut self, cx: &mut Context<Self>) {
        self.job_filters.remote_only = !self.job_filters.remote_only;
        self.job_filters.page = 1;
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn toggle_job_country_dropdown(&mut self) {
        self.job_country_dropdown_open = !self.job_country_dropdown_open;
    }

    pub fn close_job_country_dropdown(&mut self) {
        self.job_country_dropdown_open = false;
    }

    pub fn set_job_country(&mut self, country_code: Option<String>, cx: &mut Context<Self>) {
        let normalized = country_code.and_then(|raw| normalize_country_code(&raw));
        self.job_country_dropdown_open = false;
        if self.job_filters.country == normalized {
            return;
        }
        self.job_filters.country = normalized.clone();
        self.job_filters.page = 1;
        let _ = self.services.repository.save_setting(
            settings_keys::JOB_COUNTRY,
            normalized.as_deref().unwrap_or(""),
        );
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn scroll_job_country_by(&mut self, dy: Pixels, cx: &mut Context<Self>) -> bool {
        let cur = self.job_country_scroll.offset();
        let max_y = self.job_country_scroll.max_offset().height;
        if max_y <= px(0.5) {
            return false;
        }
        let next_y = (cur.y + dy).clamp(-max_y, px(0.0));
        if (next_y - cur.y).abs() > px(0.1) {
            self.job_country_scroll.set_offset(point(cur.x, next_y));
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn set_job_freshness_days(&mut self, days: Option<u32>, cx: &mut Context<Self>) {
        if self.job_filters.posted_within_days == days {
            return;
        }
        self.job_filters.posted_within_days = days;
        self.job_filters.page = 1;
        if self.job_profiles.iter().any(|p| p.enabled)
            || !self.job_filters.manual_query.trim().is_empty()
        {
            self.search_jobs(false, cx);
        }
    }

    pub fn set_job_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let clamped = page.clamp(1, MAX_PAGE_NUMBER);
        if self.job_filters.page == clamped {
            return;
        }
        self.job_filters.page = clamped;
        self.search_jobs(false, cx);
    }

    pub fn set_job_page_size(&mut self, size: usize, cx: &mut Context<Self>) {
        let clamped = size.clamp(5, MAX_PAGE_SIZE);
        if self.job_filters.page_size == clamped {
            return;
        }
        self.job_filters.page_size = clamped;
        self.job_filters.page = 1;
        self.search_jobs(false, cx);
    }

    pub fn clear_job_manual_query(&mut self, cx: &mut Context<Self>) {
        self.job_filters.manual_query.clear();
        self.job_filters.page = 1;
        self.job_query_input.update(cx, |input, cx| {
            input.set_value("");
            cx.notify();
        });
        if self.job_profiles.iter().any(|p| p.enabled) {
            self.search_jobs(false, cx);
        } else {
            self.job_listings.clear();
            self.job_total_estimated = 0;
            self.job_has_next_page = false;
        }
    }

    pub fn search_jobs(&mut self, force_refresh: bool, cx: &mut Context<Self>) {
        self.search_jobs_internal(force_refresh, false, cx);
    }

    fn search_jobs_internal(&mut self, force_refresh: bool, silent: bool, cx: &mut Context<Self>) {
        self.job_filters.manual_query = self.job_query_input.read(cx).text().to_string();
        let has_query = !self.job_filters.manual_query.trim().is_empty()
            || self.job_profiles.iter().any(|p| p.enabled);
        if !has_query {
            self.job_listings.clear();
            self.job_total_estimated = 0;
            self.job_has_next_page = false;
            self.job_listings_from_cache = false;
            return;
        }

        let cache_sig = self.job_filters.cache_signature(&self.job_profiles);
        if force_refresh {
            if self.job_cache.cooldown_remaining().is_some() {
                if !silent {
                    self.notify("Please wait a moment before refreshing again.");
                }
                return;
            }
            self.job_cache.invalidate_listings();
        } else if let Some(cached) = self.job_cache.get_listings(&cache_sig) {
            self.job_listings = cached.jobs;
            self.job_total_estimated = cached.total_estimated;
            self.job_has_next_page = cached.has_next_page;
            self.job_warnings = cached.warnings;
            self.job_listings_from_cache = true;
            self.job_error = None;
            return;
        }

        self.job_cache.record_live_fetch();
        self.job_search_generation = self.job_search_generation.wrapping_add(1);
        let generation = self.job_search_generation;
        self.job_listings_loading = true;
        self.job_error = None;

        let provider = self.job_provider.clone();
        let profiles = self.job_profiles.clone();
        let filters = self.job_filters.clone();

        cx.spawn(async move |this, cx| {
            let handle = crate::runtime::handle()
                .spawn(async move { provider.search(&profiles, &filters).await });
            let result = match handle.await {
                Ok(res) => res,
                Err(join_err) => Err(anyhow::anyhow!("Job search task failed: {join_err}")),
            };

            let _ = this.update(cx, |view, cx| {
                if view.job_search_generation != generation {
                    return;
                }
                view.job_listings_loading = false;
                match result {
                    Ok(search_res) => {
                        view.job_cache.put_listings(&cache_sig, search_res.clone());
                        view.job_listings = search_res.jobs;
                        view.job_total_estimated = search_res.total_estimated;
                        view.job_has_next_page = search_res.has_next_page;
                        view.job_warnings = search_res.warnings;
                        view.job_listings_from_cache = false;
                        view.job_expanded_slugs.clear();
                        view.job_error = None;
                    }
                    Err(err) => {
                        let msg = format!("{err:#}");
                        view.job_error = Some(msg.clone());
                        if !silent || view.current_view == WorkspaceViewPage::Jobs {
                            view.notify_error(msg);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn toggle_job_jd_expanded(&mut self, slug: &str) {
        if !self.job_expanded_slugs.remove(slug) {
            self.job_expanded_slugs.insert(slug.to_string());
        }
    }

    pub fn dismiss_job_error(&mut self) {
        self.job_error = None;
    }

    pub fn open_job_url(&mut self, url: &str) {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Err(err) = open::that(trimmed) {
            self.notify_error(format!("Could not open URL in browser: {err}"));
        }
    }

    /// One-click closed-loop handoff: branches a new variation from the chosen
    /// Master CV pre-populated with the job's company, role, full Markdown JD,
    /// and employer URL, opens the Editor, and starts AI tailoring automatically.
    pub fn tailor_cv_for_job(&mut self, job: JobListing, cx: &mut Context<Self>) {
        if self.ai_state.is_running {
            self.notify("Wait for the current AI tailoring run to finish before starting another.");
            return;
        }
        let Some(root_id) = self.resolved_job_target_master_id() else {
            self.notify("Create a Master CV first before tailoring for a job.");
            return;
        };

        self.flush_pending_now();

        let company = job.company.trim().to_string();
        let role = job.title.trim().to_string();
        let title = if !company.is_empty() && !role.is_empty() {
            format!("{company} — {role}")
        } else if !role.is_empty() {
            role.clone()
        } else if !company.is_empty() {
            format!("{company} — Variation")
        } else {
            "Tailored Variation".to_string()
        };

        let mut applied_skill_ids: Vec<String> = self
            .services
            .skills
            .list()
            .into_iter()
            .filter(|s| s.enabled && s.default_enabled)
            .map(|s| s.id)
            .collect();
        if applied_skill_ids.is_empty() {
            applied_skill_ids = self
                .services
                .skills
                .list()
                .into_iter()
                .filter(|s| s.enabled)
                .map(|s| s.id)
                .collect();
        }

        let jd = if !job.description_markdown.trim().is_empty() {
            job.description_markdown.trim().to_string()
        } else {
            job.preview.trim().to_string()
        };

        let job_ctx = JobContext {
            company_name: company,
            target_role: role,
            job_description_raw: jd,
            application_url: (!job.apply_url.trim().is_empty()).then(|| job.apply_url.clone()),
            applied_skill_ids,
            ats_match_score: None,
        };

        match self
            .services
            .repository
            .branch_sub_variation(root_id, &title, job_ctx)
        {
            Ok(child) => {
                let child_id = child.id;
                self.documents.push(child);
                self.active_doc_id = Some(child_id);
                self.current_view = WorkspaceViewPage::Editor;
                self.active_dialog = None;
                self.refresh_snapshots();
                self.trigger_pdf_preview_render(cx);

                if self.services.ai.is_configured() {
                    self.start_ai_tailoring_for_active(cx);
                } else {
                    self.notify(format!(
                        "Branched \"{title}\" with full job description. Connect ChatGPT or Google Gemini in Settings to run automated tailoring."
                    ));
                }
            }
            Err(err) => {
                self.notify_error(format!("Failed to branch CV variation for job: {err}"));
            }
        }
    }

    pub fn open_reset_app_dialog(&mut self) {
        self.active_dialog = Some(Dialog::ConfirmResetApp);
    }

    /// Factory-resets the entire application: wipes SQLite (all CVs, variations,
    /// snapshots, saved job profiles, settings — keeping `workspace.seeded=true`
    /// so the library stays empty), deletes custom user skills & restores bundled
    /// default skills, and clears saved ChatGPT & Google Gemini credentials.
    pub fn reset_application_all(&mut self, cx: &mut Context<Self>) {
        // Cancel any pending debounced saves so they cannot write back after reset.
        self.debounced_saver.discard();
        self.save_generation = self.save_generation.wrapping_add(1);
        self.pdf_render_generation = self.pdf_render_generation.wrapping_add(1);
        self.job_profile_generation = self.job_profile_generation.wrapping_add(1);
        self.job_search_generation = self.job_search_generation.wrapping_add(1);

        if let Err(err) = self.services.repository.reset_all() {
            self.active_dialog = None;
            self.notify_error(format!("Database reset failed: {err}"));
            return;
        }

        if let Err(err) = self.services.skills.reset_to_defaults() {
            tracing::warn!("Skill registry reset reported warning: {err}");
        }

        // Clear OS Credential Manager entries & reset AI provider state.
        let _ = credentials::clear_gemini_api_key();
        let ai_for_signout = self.services.ai.clone();
        crate::runtime::handle().spawn(async move {
            let _ = ai_for_signout.sign_out().await;
        });
        let _ = self.services.ai.configure(AiConfig::default());

        // Clear all in-memory workspace state.
        self.documents.clear();
        self.active_doc_id = None;
        self.default_cv_id = None;
        self.proposals.clear();
        self.ats_scores.clear();
        self.ai_summaries.clear();
        self.snapshots.clear();
        self.ai_state = AiAgentState::default();
        self.rendered_images.clear();
        self.preview_error = None;
        self.is_rendering_pdf = false;
        self.is_exporting_pdf = false;
        self.editing_proposal_id = None;
        self.active_dialog = None;

        // Reset AI UI fields.
        self.active_provider = AiProviderKind::ChatGpt;
        self.chatgpt_auth = ChatGptAuthStatus::default();
        self.auth_in_progress = false;
        self.auth_error = None;
        self.show_plan_welcome_banner = false;
        self.gemini_key_configured = false;
        self.gemini_key_visible = false;
        self.selected_model = DEFAULT_CHATGPT_MODEL.to_string();
        self.selected_gemini_model = DEFAULT_GEMINI_MODEL.to_string();
        self.available_models.clear();
        self.models_loading = false;
        self.models_error = None;
        self.model_filter_query.clear();

        // Reset Job Discovery state & caches.
        self.job_source = None;
        self.job_profiles.clear();
        self.job_profiles_from_cache = false;
        self.job_profiles_used_fallback = false;
        self.job_profiles_stale = false;
        self.job_filters = JobFilters::default();
        self.job_country_dropdown_open = false;
        self.job_country_scroll.set_offset(point(px(0.0), px(0.0)));
        self.job_listings.clear();
        self.job_total_estimated = 0;
        self.job_has_next_page = false;
        self.job_listings_from_cache = false;
        self.job_warnings.clear();
        self.job_expanded_slugs.clear();
        self.job_profiles_loading = false;
        self.job_listings_loading = false;
        self.job_error = None;
        self.job_cache.clear_all();

        // Reset auto-update state to default enabled.
        self.update_generation = self.update_generation.wrapping_add(1);
        self.auto_update_enabled = true;
        self.update_status = UpdateStatus::Idle;
        self.update_release = None;
        self.update_downloaded = None;
        self.update_error = None;

        // Reset theme & search inputs.
        self.library_query.clear();
        self.set_theme_mode(ThemeMode::Dark);
        self.gemini_api_key_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.search_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.model_filter_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });
        self.job_query_input.update(cx, |i, cx| {
            i.set_value("");
            cx.notify();
        });

        self.current_view = WorkspaceViewPage::Library;
        self.notify(
            "Workspace reset complete. All CVs, settings, and AI credentials were cleared; default skills preserved.",
        );
    }

    // -----------------------------------------------------------------------
    // Software Auto-Updater (Windows GitHub Releases)
    // -----------------------------------------------------------------------

    fn schedule_startup_update_check(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(4))
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.auto_update_enabled && matches!(view.update_status, UpdateStatus::Idle) {
                    view.check_for_updates(false, cx);
                }
            });
        })
        .detach();
    }

    pub fn toggle_auto_update(&mut self, cx: &mut Context<Self>) {
        self.auto_update_enabled = !self.auto_update_enabled;
        let value = if self.auto_update_enabled {
            "true"
        } else {
            "false"
        };
        if let Err(err) = self
            .services
            .repository
            .save_setting(settings_keys::AUTO_UPDATE_ENABLED, value)
        {
            tracing::warn!("Failed to persist auto-update setting: {err}");
        }

        if self.auto_update_enabled {
            self.notify("Auto-update enabled. VanguardCV will check for new Windows releases automatically.");
            if matches!(
                self.update_status,
                UpdateStatus::Idle | UpdateStatus::Error(_)
            ) {
                self.check_for_updates(false, cx);
            }
        } else {
            self.notify("Auto-update disabled. You can still check for updates manually anytime.");
        }
    }

    pub fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(
            self.update_status,
            UpdateStatus::Checking | UpdateStatus::Downloading | UpdateStatus::Installing
        ) {
            return;
        }

        self.update_generation = self.update_generation.wrapping_add(1);
        let generation = self.update_generation;
        self.update_status = UpdateStatus::Checking;
        self.update_error = None;
        cx.notify();

        let (tx, mut rx) = unbounded_channel::<anyhow::Result<UpdateCheckOutcome>>();
        crate::runtime::handle().spawn(async move {
            let outcome = updater::check_for_updates(LATEST_RELEASE_API_URL, CURRENT_VERSION).await;
            let _ = tx.send(outcome);
        });

        cx.spawn(async move |this, cx| {
            let Some(result) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if view.update_generation != generation {
                    return;
                }
                match result {
                    Ok(UpdateCheckOutcome::UpToDate { current_version }) => {
                        view.update_status = UpdateStatus::UpToDate;
                        view.update_release = None;
                        view.update_downloaded = None;
                        view.update_error = None;
                        if manual {
                            view.notify(format!("VanguardCV v{current_version} is up to date."));
                        }
                    }
                    Ok(UpdateCheckOutcome::UpdateAvailable(info)) => {
                        let already_ready = view.update_downloaded.as_ref().is_some_and(|d| {
                            d.version == info.version && d.installer_path.is_file()
                        });
                        view.update_release = Some(info.clone());
                        view.update_error = None;
                        if already_ready {
                            view.update_status = UpdateStatus::ReadyToInstall;
                        } else {
                            view.update_downloaded = None;
                            view.update_status = UpdateStatus::Available;
                            if view.auto_update_enabled {
                                view.download_available_update(false, cx);
                            } else if manual {
                                view.notify(format!(
                                    "VanguardCV v{} is available to download.",
                                    info.version
                                ));
                            }
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if manual {
                            view.update_status = UpdateStatus::Error(msg.clone());
                            view.update_error = Some(msg.clone());
                            view.notify_error(msg);
                        } else {
                            tracing::warn!("Background update check skipped: {msg}");
                            view.update_status = UpdateStatus::Idle;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn download_available_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(
            self.update_status,
            UpdateStatus::Downloading | UpdateStatus::Installing
        ) {
            return;
        }
        let Some(release) = self.update_release.clone() else {
            if manual {
                self.check_for_updates(true, cx);
            }
            return;
        };

        self.update_generation = self.update_generation.wrapping_add(1);
        let generation = self.update_generation;
        self.update_status = UpdateStatus::Downloading;
        self.update_error = None;
        cx.notify();

        let staging_dir = updater::default_update_staging_dir();
        let (tx, mut rx) = unbounded_channel::<anyhow::Result<DownloadedUpdate>>();
        crate::runtime::handle().spawn(async move {
            let outcome = updater::download_and_verify_installer(&release, &staging_dir).await;
            let _ = tx.send(outcome);
        });

        cx.spawn(async move |this, cx| {
            let Some(result) = rx.recv().await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if view.update_generation != generation {
                    return;
                }
                match result {
                    Ok(downloaded) => {
                        let ver = downloaded.version.clone();
                        view.update_downloaded = Some(downloaded);
                        view.update_status = UpdateStatus::ReadyToInstall;
                        view.update_error = None;
                        view.notify(format!(
                            "VanguardCV v{ver} downloaded and verified. Click Install & restart in Settings when ready."
                        ));
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if manual {
                            view.update_status = UpdateStatus::Error(msg.clone());
                            view.update_error = Some(msg.clone());
                            view.notify_error(msg);
                        } else {
                            tracing::warn!("Background update download failed: {msg}");
                            view.update_status = UpdateStatus::Available;
                            view.update_error = Some(msg);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn apply_downloaded_update(&mut self, cx: &mut Context<Self>) {
        let Some(downloaded) = self.update_downloaded.clone() else {
            self.notify_error("No verified update installer is ready yet.");
            return;
        };
        if matches!(self.update_status, UpdateStatus::Installing) {
            return;
        }

        // Flush any pending SQLite edits safely before handing off to the installer.
        self.flush_pending_now();
        self.update_status = UpdateStatus::Installing;
        self.update_error = None;
        cx.notify();

        match updater::launch_windows_installer(&downloaded.installer_path) {
            Ok(()) => {
                cx.quit();
            }
            Err(err) => {
                let msg = format!("Could not start update installer: {err}");
                self.update_status = UpdateStatus::Error(msg.clone());
                self.update_error = Some(msg.clone());
                self.notify_error(msg);
                cx.notify();
            }
        }
    }

    pub fn open_releases_page(&mut self) {
        let url = self
            .update_release
            .as_ref()
            .map(|r| r.html_url.as_str())
            .unwrap_or(RELEASES_PAGE_URL);
        if let Err(err) = open::that(url) {
            self.notify_error(format!("Could not open releases page ({url}): {err}"));
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
        let _ = self.debounced_saver.flush();
    }
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        self.viewport_width = f32::from(vp.width);
        self.viewport_height = f32::from(vp.height);

        // If a drag-reorder ended outside a drop target, finalize and persist cleanly.
        if self.active_drag_item.is_some() && !cx.has_active_drag() {
            self.finish_editor_drag(cx);
        }

        // Schedule automatic toast dismissal for any new notification
        if self.notice.is_some() && self.notice_generation != self.scheduled_notice_generation {
            self.scheduled_notice_generation = self.notice_generation;
            let notice_gen = self.notice_generation;
            let timeout = if self.notice_is_error {
                Duration::from_millis(6000)
            } else {
                Duration::from_millis(3500)
            };
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(timeout).await;
                let _ = this.update(cx, |view, cx| {
                    if view.notice_generation == notice_gen && view.notice.is_some() {
                        view.notice = None;
                        view.notice_is_error = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }

        let theme = self.theme.clone();
        let view = self.current_view;
        let active_title = self.active_document().map(|d| d.title.clone());
        let is_dirty = self.debounced_saver.is_dirty();
        let show_sidebar = self.is_sidebar_visible();
        let is_compact = self.is_compact();
        let is_narrow = self.is_narrow();
        let toast_max_w = (self.viewport_width - 32.0).clamp(240.0, 440.0);

        div()
            .relative()
            .flex()
            .size_full()
            .font_family("Inter")
            .track_focus(&self.focus_handle)
            .bg(theme.canvas)
            .text_color(theme.text)
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.preview_pan_state.is_some() {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.update_preview_pan(event.position, cx);
                    } else {
                        this.end_preview_pan();
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    if this.preview_pan_state.is_some() {
                        this.end_preview_pan();
                        cx.notify();
                    }
                    if this.active_drag_item.is_some() {
                        this.finish_editor_drag(cx);
                    }
                }),
            )
            .on_drop::<EditorDragPayload>(cx.listener(|this, _, _window, cx| {
                this.finish_editor_drag(cx);
            }))
            .on_action(cx.listener(|this, _: &NewVariation, _window, cx| {
                if this.active_dialog.is_none() {
                    this.open_variation_dialog(None, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ExportPdfShortcut, _window, cx| {
                if this.active_dialog.is_none() {
                    this.open_export_dialog();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleAiPanel, _window, cx| {
                if this.active_dialog.is_none() {
                    match this.current_view {
                        WorkspaceViewPage::Editor => {
                            this.toggle_split_preview(cx);
                        }
                        WorkspaceViewPage::Preview => {
                            this.navigate_to(WorkspaceViewPage::Editor, cx);
                        }
                        _ => {
                            if this.active_document().is_some() {
                                this.navigate_to(WorkspaceViewPage::Preview, cx);
                            } else {
                                this.notify("Select a CV from the library to open PDF preview.");
                            }
                        }
                    }
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &AcceptAllProposals, _window, cx| {
                if this.active_dialog.is_none() {
                    this.accept_all_proposals(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CloseModal, _window, cx| {
                if this.active_dialog.is_some() {
                    this.close_dialog();
                    cx.notify();
                }
            }))
            // Sidebar (responsive: auto-hides below 860px, toggleable anytime)
            .when(show_sidebar, |el| el.child(render_sidebar(self, cx)))
            // Main shell
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .h_full()
                    .min_w_0()
                    .overflow_hidden()
                    // Topbar
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.0))
                            .h(px(54.0))
                            .px(if is_narrow { px(12.0) } else { px(20.0) })
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(theme.surface_soft)
                            // Sidebar toggle + Breadcrumbs
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .text_size(px(13.0))
                                    .overflow_hidden()
                                    .child(render_icon_button(
                                        "toggle-sidebar-btn",
                                        IconName::List,
                                        if show_sidebar {
                                            "Hide sidebar"
                                        } else {
                                            "Show sidebar"
                                        },
                                        false,
                                        &theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_sidebar();
                                            cx.notify();
                                        }),
                                    ))
                                    .when(!is_narrow, |el| {
                                        el.child(
                                            div()
                                                .id("breadcrumb-workspace")
                                                .cursor_pointer()
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .text_color(theme.secondary)
                                                .hover(|s| s.text_color(theme.text))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.navigate_to(
                                                        WorkspaceViewPage::Library,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }))
                                                .child(render_icon(
                                                    IconName::Folder,
                                                    14.0,
                                                    theme.secondary,
                                                ))
                                                .child("Workspace"),
                                        )
                                        .child(render_icon(IconName::Chevron, 11.0, theme.muted))
                                    })
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(view.display_name()),
                                    )
                                    .when_some(
                                        if !is_narrow
                                            && matches!(
                                                view,
                                                WorkspaceViewPage::Editor
                                                    | WorkspaceViewPage::Preview
                                                    | WorkspaceViewPage::Privacy
                                            )
                                        {
                                            active_title
                                        } else {
                                            None
                                        },
                                        |el, title| {
                                            el.child(render_icon(
                                                IconName::Chevron,
                                                11.0,
                                                theme.muted,
                                            ))
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_color(theme.secondary)
                                                    .child(title),
                                            )
                                        },
                                    ),
                            )
                            // Topbar right
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .when(!is_compact, |el| {
                                        el.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .text_size(px(12.0))
                                                .text_color(theme.muted)
                                                .child(render_icon(
                                                    IconName::Lock,
                                                    12.0,
                                                    theme.accent,
                                                ))
                                                .child(if is_dirty {
                                                    "Saving…"
                                                } else {
                                                    "Saved locally"
                                                }),
                                        )
                                        .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
                                    })
                                    .when(
                                        matches!(
                                            view,
                                            WorkspaceViewPage::Editor | WorkspaceViewPage::Preview
                                        ),
                                        |el| {
                                            el.child(render_button(
                                                "topbar-export-btn",
                                                if self.is_exporting_pdf {
                                                    "Exporting…"
                                                } else if is_narrow {
                                                    "Export"
                                                } else {
                                                    "Export PDF"
                                                },
                                                Some(IconName::Download),
                                                ButtonVariant::Secondary,
                                                self.is_exporting_pdf,
                                                &theme,
                                                cx.listener(|this, _, _, cx| {
                                                    this.open_export_dialog();
                                                    cx.notify();
                                                }),
                                            ))
                                        },
                                    )
                                    .child(render_icon_button(
                                        "theme-toggle-btn",
                                        if theme.mode == ThemeMode::Dark {
                                            IconName::Sun
                                        } else {
                                            IconName::Moon
                                        },
                                        "Toggle dark/light theme",
                                        false,
                                        &theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_theme();
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    // View Content (split live PDF preview in Editor on wide viewports)
                    .child(if self.is_split_preview_active() {
                        let preview_w = self.split_preview_pane_width();
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id(el_id(format!("main-content-scroll-{view:?}")))
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_y_scroll()
                                    .child(render_editor_view(self, cx)),
                            )
                            .child(render_live_preview_pane(self, preview_w, cx))
                            .into_any_element()
                    } else {
                        div()
                            .id(el_id(format!("main-content-scroll-{view:?}")))
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scroll()
                            .child(match view {
                                WorkspaceViewPage::Library => render_library_view(self, cx),
                                WorkspaceViewPage::Jobs => render_jobs_view(self, cx),
                                WorkspaceViewPage::Editor => render_editor_view(self, cx),
                                WorkspaceViewPage::Preview => render_preview_view(self, cx),
                                WorkspaceViewPage::Skills => render_skills_view(self, cx),
                                WorkspaceViewPage::Privacy => render_privacy_view(self, cx),
                                WorkspaceViewPage::Settings => render_settings_view(self, cx),
                            })
                            .into_any_element()
                    })
                    // Status Footer
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .h(px(29.0))
                            .px(if is_narrow { px(12.0) } else { px(20.0) })
                            .border_t_1()
                            .border_color(theme.border)
                            .bg(theme.surface_soft)
                            .text_size(px(11.0))
                            .text_color(theme.muted)
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child("VanguardCV")
                                    .child("·")
                                    .child(if is_dirty {
                                        "Syncing to local SQLite…"
                                    } else {
                                        "Saved to local SQLite"
                                    }),
                            )
                            .when(!is_narrow, |el| {
                                el.child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(render_icon(IconName::Lock, 11.0, theme.muted))
                                        .child("Zero-Trust PII Air Gap Active"),
                                )
                            }),
                    ),
            )
            // Modal dialog overlay
            .when_some(render_modal_overlay(self, cx), |el, modal| el.child(modal))
            // Toast notification banner (rendered above modals so validation errors stay visible)
            .when_some(self.notice.clone(), |el, notice_text| {
                let is_err = self.notice_is_error;
                el.child(
                    div()
                        .occlude()
                        .absolute()
                        .top(px(16.0))
                        .right(px(16.0))
                        .max_w(px(toast_max_w))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .py(px(10.0))
                        .px(px(14.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(if is_err { theme.accent } else { theme.border })
                        .shadow_lg()
                        .child(render_icon(
                            if is_err {
                                IconName::Shield
                            } else {
                                IconName::Check
                            },
                            16.0,
                            if is_err {
                                theme.del_text
                            } else {
                                theme.add_text
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.5))
                                .text_color(theme.text)
                                .child(notice_text),
                        )
                        .child(render_icon_button(
                            "dismiss-notice-btn",
                            IconName::Close,
                            "Dismiss notice",
                            false,
                            &theme,
                            cx.listener(|this, _, _, cx| {
                                this.clear_notice();
                                cx.notify();
                            }),
                        )),
                )
            })
    }
}

fn move_by_id<T>(
    list: &mut Vec<T>,
    is_from: impl Fn(&T) -> bool,
    is_to: impl Fn(&T) -> bool,
) -> bool {
    let Some(from_idx) = list.iter().position(&is_from) else {
        return false;
    };
    let Some(to_idx) = list.iter().position(&is_to) else {
        return false;
    };
    if from_idx == to_idx {
        return false;
    }
    let item = list.remove(from_idx);
    list.insert(to_idx, item);
    true
}

fn index_pair_by_id<T>(
    list: &[T],
    is_from: impl Fn(&T) -> bool,
    is_to: impl Fn(&T) -> bool,
) -> Option<(usize, usize)> {
    let from_idx = list.iter().position(is_from)?;
    let to_idx = list.iter().position(is_to)?;
    if from_idx == to_idx {
        None
    } else {
        Some((from_idx, to_idx))
    }
}

pub fn drag_item_index_pair(
    body: &SanitizedCvBody,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> Option<(usize, usize)> {
    match (dragged, target) {
        (
            EditorDragKind::Section { section_id: from_id },
            EditorDragKind::Section { section_id: to_id },
        ) => index_pair_by_id(&body.sections, |s| s.id == *from_id, |s| s.id == *to_id),
        (
            EditorDragKind::TimelineEntry {
                section_id: fs,
                entry_id: from_id,
            },
            EditorDragKind::TimelineEntry {
                section_id: ts,
                entry_id: to_id,
            },
        ) if fs == ts => {
            let sec = body.sections.iter().find(|s| s.id == *fs)?;
            let SectionKind::TimelineEntries { entries } = &sec.kind else {
                return None;
            };
            index_pair_by_id(entries, |e| e.id == *from_id, |e| e.id == *to_id)
        }
        (
            EditorDragKind::Bullet {
                section_id: fs,
                entry_id: fe,
                bullet_id: from_id,
            },
            EditorDragKind::Bullet {
                section_id: ts,
                entry_id: te,
                bullet_id: to_id,
            },
        ) if fs == ts && fe == te => {
            let sec = body.sections.iter().find(|s| s.id == *fs)?;
            match (&sec.kind, fe) {
                (SectionKind::TimelineEntries { entries }, Some(eid)) => {
                    let entry = entries.iter().find(|e| e.id == *eid)?;
                    index_pair_by_id(&entry.bullets, |b| b.id == *from_id, |b| b.id == *to_id)
                }
                (SectionKind::SimpleBulletList { items }, None) => {
                    index_pair_by_id(items, |b| b.id == *from_id, |b| b.id == *to_id)
                }
                _ => None,
            }
        }
        (
            EditorDragKind::SkillCategory {
                section_id: fs,
                category_id: from_id,
            },
            EditorDragKind::SkillCategory {
                section_id: ts,
                category_id: to_id,
            },
        ) if fs == ts => {
            let sec = body.sections.iter().find(|s| s.id == *fs)?;
            let SectionKind::CategorizedInlineList { categories } = &sec.kind else {
                return None;
            };
            index_pair_by_id(categories, |c| c.id == *from_id, |c| c.id == *to_id)
        }
        _ => None,
    }
}

pub fn reorder_item_in_body(
    body: &mut SanitizedCvBody,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> bool {
    if dragged == target {
        return false;
    }
    match (dragged, target) {
        (
            EditorDragKind::Section { section_id: from_id },
            EditorDragKind::Section { section_id: to_id },
        ) => move_by_id(&mut body.sections, |s| s.id == *from_id, |s| s.id == *to_id),
        (
            EditorDragKind::TimelineEntry {
                section_id: fs,
                entry_id: from_id,
            },
            EditorDragKind::TimelineEntry {
                section_id: ts,
                entry_id: to_id,
            },
        ) if fs == ts => {
            if let Some(sec) = body.sections.iter_mut().find(|s| s.id == *fs)
                && let SectionKind::TimelineEntries { entries } = &mut sec.kind
            {
                move_by_id(entries, |e| e.id == *from_id, |e| e.id == *to_id)
            } else {
                false
            }
        }
        (
            EditorDragKind::Bullet {
                section_id: fs,
                entry_id: fe,
                bullet_id: from_id,
            },
            EditorDragKind::Bullet {
                section_id: ts,
                entry_id: te,
                bullet_id: to_id,
            },
        ) if fs == ts && fe == te => {
            if let Some(sec) = body.sections.iter_mut().find(|s| s.id == *fs) {
                match (&mut sec.kind, fe) {
                    (SectionKind::TimelineEntries { entries }, Some(eid)) => {
                        if let Some(entry) = entries.iter_mut().find(|e| e.id == *eid) {
                            move_by_id(&mut entry.bullets, |b| b.id == *from_id, |b| b.id == *to_id)
                        } else {
                            false
                        }
                    }
                    (SectionKind::SimpleBulletList { items }, None) => {
                        move_by_id(items, |b| b.id == *from_id, |b| b.id == *to_id)
                    }
                    _ => false,
                }
            } else {
                false
            }
        }
        (
            EditorDragKind::SkillCategory {
                section_id: fs,
                category_id: from_id,
            },
            EditorDragKind::SkillCategory {
                section_id: ts,
                category_id: to_id,
            },
        ) if fs == ts => {
            if let Some(sec) = body.sections.iter_mut().find(|s| s.id == *fs)
                && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
            {
                move_by_id(categories, |c| c.id == *from_id, |c| c.id == *to_id)
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn drag_item_index_pair_in_doc(
    doc: &CvDocument,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> Option<(usize, usize)> {
    match (dragged, target) {
        (
            EditorDragKind::SocialLink { link_id: from_id },
            EditorDragKind::SocialLink { link_id: to_id },
        ) => index_pair_by_id(
            &doc.pii_header.social_links,
            |l| l.id == *from_id,
            |l| l.id == *to_id,
        ),
        (
            EditorDragKind::CustomHeaderField { field_id: from_id },
            EditorDragKind::CustomHeaderField { field_id: to_id },
        ) => index_pair_by_id(
            &doc.pii_header.custom_fields,
            |f| f.id == *from_id,
            |f| f.id == *to_id,
        ),
        _ => drag_item_index_pair(&doc.body, dragged, target),
    }
}

pub fn reorder_item_in_doc(
    doc: &mut CvDocument,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> bool {
    if dragged == target {
        return false;
    }
    match (dragged, target) {
        (
            EditorDragKind::SocialLink { link_id: from_id },
            EditorDragKind::SocialLink { link_id: to_id },
        ) => move_by_id(
            &mut doc.pii_header.social_links,
            |l| l.id == *from_id,
            |l| l.id == *to_id,
        ),
        (
            EditorDragKind::CustomHeaderField { field_id: from_id },
            EditorDragKind::CustomHeaderField { field_id: to_id },
        ) => move_by_id(
            &mut doc.pii_header.custom_fields,
            |f| f.id == *from_id,
            |f| f.id == *to_id,
        ),
        _ => reorder_item_in_body(&mut doc.body, dragged, target),
    }
}

#[cfg(test)]
mod drag_reorder_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn reorders_sections_entries_bullets_and_skill_categories() {
        let mut doc = crate::mocks::sample_root_document();
        let orig_sec0 = doc.body.sections[0].id;
        let orig_sec1 = doc.body.sections[1].id;
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::Section { section_id: orig_sec0 },
            &EditorDragKind::Section { section_id: orig_sec1 },
        ));
        assert_eq!(doc.body.sections[0].id, orig_sec1);
        assert_eq!(doc.body.sections[1].id, orig_sec0);

        // Reorder timeline entries & nested bullets
        let exp_sec = doc
            .body
            .sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::TimelineEntries { ref entries } if entries.len() >= 2))
            .expect("sample has experience timeline entries");
        let exp_sec_id = exp_sec.id;
        let (e0, e1, b0, b1) = match &exp_sec.kind {
            SectionKind::TimelineEntries { entries } => {
                (entries[0].id, entries[1].id, entries[0].bullets[0].id, entries[0].bullets[1].id)
            }
            _ => unreachable!(),
        };
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::TimelineEntry {
                section_id: exp_sec_id,
                entry_id: e0,
            },
            &EditorDragKind::TimelineEntry {
                section_id: exp_sec_id,
                entry_id: e1,
            },
        ));
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::Bullet {
                section_id: exp_sec_id,
                entry_id: Some(e0),
                bullet_id: b0,
            },
            &EditorDragKind::Bullet {
                section_id: exp_sec_id,
                entry_id: Some(e0),
                bullet_id: b1,
            },
        ));
        let exp_after = doc.body.sections.iter().find(|s| s.id == exp_sec_id).unwrap();
        if let SectionKind::TimelineEntries { entries } = &exp_after.kind {
            assert_eq!(entries[0].id, e1);
            assert_eq!(entries[1].id, e0);
            assert_eq!(entries[1].bullets[0].id, b1);
            assert_eq!(entries[1].bullets[1].id, b0);
        }

        // Reorder skill categories
        let skill_sec = doc
            .body
            .sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::CategorizedInlineList { ref categories } if categories.len() >= 2))
            .expect("sample has skill categories");
        let skill_sec_id = skill_sec.id;
        let (c0, c1) = match &skill_sec.kind {
            SectionKind::CategorizedInlineList { categories } => (categories[0].id, categories[1].id),
            _ => unreachable!(),
        };
        assert!(reorder_item_in_body(
            &mut doc.body,
            &EditorDragKind::SkillCategory {
                section_id: skill_sec_id,
                category_id: c0,
            },
            &EditorDragKind::SkillCategory {
                section_id: skill_sec_id,
                category_id: c1,
            },
        ));
        let skill_after = doc.body.sections.iter().find(|s| s.id == skill_sec_id).unwrap();
        if let SectionKind::CategorizedInlineList { categories } = &skill_after.kind {
            assert_eq!(categories[0].id, c1);
            assert_eq!(categories[1].id, c0);
        }

        // Reorder social links and custom header fields on Personal Info
        while doc.pii_header.social_links.len() < 2 {
            doc.pii_header.social_links.push(SocialLink {
                id: Uuid::new_v4(),
                platform: "Portfolio".into(),
                display_text: "example.dev".into(),
                url: "https://example.dev".into(),
                enabled: true,
            });
        }
        assert!(doc.pii_header.social_links.len() >= 2);
        let l0 = doc.pii_header.social_links[0].id;
        let l1 = doc.pii_header.social_links[1].id;
        assert!(reorder_item_in_doc(
            &mut doc,
            &EditorDragKind::SocialLink { link_id: l0 },
            &EditorDragKind::SocialLink { link_id: l1 },
        ));
        assert_eq!(doc.pii_header.social_links[0].id, l1);
        assert_eq!(doc.pii_header.social_links[1].id, l0);

        while doc.pii_header.custom_fields.len() < 2 {
            doc.pii_header.custom_fields.push(CustomHeaderField {
                id: Uuid::new_v4(),
                label: "Clearance".into(),
                value: "EU Secret".into(),
                enabled: true,
            });
        }
        assert!(doc.pii_header.custom_fields.len() >= 2);
        let f0 = doc.pii_header.custom_fields[0].id;
        let f1 = doc.pii_header.custom_fields[1].id;
        assert!(reorder_item_in_doc(
            &mut doc,
            &EditorDragKind::CustomHeaderField { field_id: f0 },
            &EditorDragKind::CustomHeaderField { field_id: f1 },
        ));
        assert_eq!(doc.pii_header.custom_fields[0].id, f1);
        assert_eq!(doc.pii_header.custom_fields[1].id, f0);
    }

    #[::core::prelude::v1::test]
    fn toggle_visibility_proposal_hides_sections_entries_bullets_and_skill_categories() {
        let mut doc = crate::mocks::sample_root_document();
        let last_sec_id = doc.body.sections.last().expect("section").id;
        let (entry_id, bullet_id) = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::TimelineEntries { entries } => entries.first().and_then(|e| {
                    e.bullets.first().map(|b| (e.id, b.id))
                }),
                _ => None,
            })
            .expect("timeline entry and bullet");
        let cat_id = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::CategorizedInlineList { categories } => {
                    categories.first().map(|c| c.id)
                }
                _ => None,
            })
            .expect("skill category");

        for target in [last_sec_id, entry_id, bullet_id, cat_id] {
            apply_ai_proposal(
                &mut doc,
                &AiProposal::ToggleVisibility {
                    item_id: target,
                    enabled: false,
                    reason: "Irrelevant to target JD".to_string(),
                },
            );
        }

        assert!(
            !doc.body
                .sections
                .iter()
                .find(|s| s.id == last_sec_id)
                .unwrap()
                .enabled
        );
        assert!(!doc.body.find_bullet(bullet_id).unwrap().enabled);
        let entry_enabled = doc.body.sections.iter().find_map(|s| match &s.kind {
            SectionKind::TimelineEntries { entries } => {
                entries.iter().find(|e| e.id == entry_id).map(|e| e.enabled)
            }
            _ => None,
        });
        assert_eq!(entry_enabled, Some(false));
        let cat_enabled = doc.body.sections.iter().find_map(|s| match &s.kind {
            SectionKind::CategorizedInlineList { categories } => {
                categories.iter().find(|c| c.id == cat_id).map(|c| c.enabled)
            }
            _ => None,
        });
        assert_eq!(cat_enabled, Some(false));
    }

    #[::core::prelude::v1::test]
    fn update_proposal_text_edits_bullet_summary_skills_and_clears_pii_warning() {
        let mut doc = crate::mocks::sample_root_document();
        let bullet_id = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::TimelineEntries { entries } => entries
                    .first()
                    .and_then(|e| e.bullets.first().map(|b| b.id)),
                _ => None,
            })
            .expect("bullet");
        let summary_id = if let Some(sec) = doc
            .body
            .sections
            .iter()
            .find(|s| matches!(s.kind, SectionKind::Summary { .. }))
        {
            sec.id
        } else {
            let id = Uuid::new_v4();
            doc.body.sections.insert(
                0,
                CvSection {
                    id,
                    origin_item_id: None,
                    title: "SUMMARY".to_string(),
                    enabled: true,
                    kind: SectionKind::Summary {
                        text: "Original summary".to_string(),
                    },
                },
            );
            id
        };
        let cat_id = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::CategorizedInlineList { categories } => {
                    categories.first().map(|c| c.id)
                }
                _ => None,
            })
            .expect("skill category");

        // 1. Bullet with a PII redaction placeholder gets unblocked once edited
        let mut bullet_prop = UiProposal::from_ai_proposal(
            &AiProposal::BulletRewrite {
                bullet_id,
                original: "Old bullet text".to_string(),
                proposed: "Contact [REDACTED_PII_EMAIL] for details".to_string(),
                rationale: "XYZ polish".to_string(),
            },
            &doc,
        );
        assert!(bullet_prop.pii_warning.is_some());
        assert!(bullet_prop.is_text_editable());
        assert!(update_proposal_text(&mut bullet_prop, "   ", Some(&doc)).is_err());

        update_proposal_text(
            &mut bullet_prop,
            "Identified 60+ critical vulnerabilities across HackerOne programs.",
            Some(&doc),
        )
        .expect("bullet edit");
        assert!(bullet_prop.is_edited);
        assert!(bullet_prop.pii_warning.is_none());
        assert_eq!(
            bullet_prop.proposed,
            "Identified 60+ critical vulnerabilities across HackerOne programs."
        );
        apply_ai_proposal(&mut doc, &bullet_prop.proposal);
        assert_eq!(
            doc.body.find_bullet(bullet_id).unwrap().text,
            "Identified 60+ critical vulnerabilities across HackerOne programs."
        );

        // 2. Summary rewrite edit
        let mut summary_prop = UiProposal::from_ai_proposal(
            &AiProposal::SummaryRewrite {
                section_id: summary_id,
                original: "Original summary".to_string(),
                proposed: "AI summary".to_string(),
            },
            &doc,
        );
        update_proposal_text(&mut summary_prop, "Custom edited summary.", Some(&doc))
            .expect("summary edit");
        apply_ai_proposal(&mut doc, &summary_prop.proposal);
        let updated_summary = doc
            .body
            .sections
            .iter()
            .find_map(|s| match &s.kind {
                SectionKind::Summary { text } if s.id == summary_id => Some(text.as_str()),
                _ => None,
            })
            .unwrap();
        assert_eq!(updated_summary, "Custom edited summary.");

        // 3. Skill reorder edit
        let mut skill_prop = UiProposal::from_ai_proposal(
            &AiProposal::SkillReorder {
                category_id: cat_id,
                items: vec!["Rust".to_string(), "Go".to_string()],
            },
            &doc,
        );
        update_proposal_text(&mut skill_prop, "Burp Suite, Rust, Python", Some(&doc))
            .expect("skill edit");
        apply_ai_proposal(&mut doc, &skill_prop.proposal);
        let updated_items = find_skill_category(&doc, cat_id).unwrap().items.clone();
        assert_eq!(updated_items, vec!["Burp Suite", "Rust", "Python"]);

        // 4. ToggleVisibility is not text-editable
        let mut vis_prop = UiProposal::from_ai_proposal(
            &AiProposal::ToggleVisibility {
                item_id: bullet_id,
                enabled: false,
                reason: "Off-topic".to_string(),
            },
            &doc,
        );
        assert!(!vis_prop.is_text_editable());
        assert!(update_proposal_text(&mut vis_prop, "anything", Some(&doc)).is_err());
    }
}