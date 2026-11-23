//! LinkedIn-style hierarchical section cards and PII editor.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Provides interactive, real-time editing of the PII Header and all 4
//! section types with debounced persistence and live preview sync.

pub mod payload_modal;
pub mod pii_header_card;
pub mod sections;

use gpui::{
    Context, Div, Entity, EventEmitter, IntoElement, MouseButton, MouseDownEvent, SharedString,
    Window, div, prelude::*, px, rgb,
};
use std::time::Duration;
use uuid::Uuid;

use crate::contracts::{
    BulletItem, CustomHeaderField, CvDocument, ItemId, SectionKind, Services, SkillCategoryItem,
    SocialLink, TimelineEntry,
};
use crate::theme;
use crate::ui::text_input::TextInput;
use pii_header_card::PiiHeaderState;
use sections::{SectionField, SectionFieldKind};

/// Layout chosen when the user adds a custom section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewSectionKind {
    Summary,
    Timeline,
    Skills,
    Bullets,
}

impl NewSectionKind {
    fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Timeline => "Timeline",
            Self::Skills => "Skills",
            Self::Bullets => "Bullet list",
        }
    }

    fn default_title(self) -> &'static str {
        match self {
            Self::Summary => "NEW SECTION",
            Self::Timeline => "PROJECTS",
            Self::Skills => "CERTIFICATIONS",
            Self::Bullets => "PUBLICATIONS",
        }
    }

    fn into_section_kind(self) -> SectionKind {
        match self {
            Self::Summary => SectionKind::Summary {
                text: String::new(),
            },
            Self::Timeline => SectionKind::TimelineEntries {
                entries: Vec::new(),
            },
            Self::Skills => SectionKind::CategorizedInlineList {
                categories: Vec::new(),
            },
            Self::Bullets => SectionKind::SimpleBulletList { items: Vec::new() },
        }
    }
}

/// Events emitted by [`EditorView`] for the workspace shell and PDF preview.
#[derive(Clone, Debug)]
pub enum EditorEvent {
    /// In-memory document was mutated by user input.
    DocumentChanged(CvDocument),
    /// In-memory document was persisted to SQLite.
    DocumentSaved(CvDocument),
}

/// The active editor tab: the PII profile or one section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorTab {
    Profile,
    Section(ItemId),
}

pub struct EditorView {
    services: Services,
    pub(crate) document: Option<CvDocument>,
    pub(crate) pii_state: Option<PiiHeaderState>,
    pub(crate) section_states: Vec<SectionField>,
    pub(crate) active_tab: EditorTab,
    pub(crate) generation: u64,
    pub(crate) show_payload_modal: bool,
    pub(crate) payload_modal_json: Option<String>,
    pub(crate) is_payload_copied: bool,
    pub(crate) show_add_section: bool,
    pub(crate) add_section_title_input: Option<gpui::Entity<TextInput>>,
    pub(crate) add_section_kind: NewSectionKind,
}

impl EventEmitter<EditorEvent> for EditorView {}

impl EditorView {
    pub fn new(services: Services, _cx: &mut Context<Self>) -> Self {
        Self {
            services,
            document: None,
            pii_state: None,
            section_states: Vec::new(),
            active_tab: EditorTab::Profile,
            generation: 0,
            show_payload_modal: false,
            payload_modal_json: None,
            is_payload_copied: false,
            show_add_section: false,
            add_section_title_input: None,
            add_section_kind: NewSectionKind::Timeline,
        }
    }

    /// Sets or replaces the loaded document in the editor.
    pub fn set_document(&mut self, doc: Option<CvDocument>, cx: &mut Context<Self>) {
        self.document = doc.clone();
        self.generation = self.generation.wrapping_add(1);
        self.show_payload_modal = false;
        self.payload_modal_json = None;
        self.is_payload_copied = false;
        self.show_add_section = false;
        self.add_section_title_input = None;

        if let Some(doc) = doc {
            self.build_editor_fields(&doc, cx);
            if let EditorTab::Section(id) = self.active_tab
                && !self.section_states.iter().any(|section| section.id == id)
            {
                self.active_tab = EditorTab::Profile;
            }
        } else {
            self.pii_state = None;
            self.section_states.clear();
            self.active_tab = EditorTab::Profile;
        }

        cx.notify();
    }

    /// Builds child input entities for the document.
    fn build_editor_fields(&mut self, doc: &CvDocument, cx: &mut Context<Self>) {
        let pii_state = PiiHeaderState::new(&doc.pii_header, cx);
        self.pii_state = Some(pii_state);

        let mut section_states = Vec::new();
        for section in &doc.body.sections {
            section_states.push(SectionField::new(section, cx));
        }
        self.section_states = section_states;
        self.wire_inputs(cx);
    }

    /// Reads back current UI field values into `self.document`, emits a
    /// change event, and schedules debounced save.
    pub fn sync_and_mark_dirty(&mut self, cx: &mut Context<Self>) {
        let Some(mut doc) = self.document.clone() else {
            return;
        };

        if let Some(pii_state) = &self.pii_state {
            doc.pii_header = pii_state.read_pii(cx);
        }

        let mut new_sections = Vec::new();
        for s in &self.section_states {
            new_sections.push(s.read_section(cx));
        }
        doc.body.sections = new_sections;

        self.document = Some(doc.clone());
        let current_gen = self.generation.wrapping_add(1);
        self.generation = current_gen;

        // Emit change immediately so preview can update on typing
        cx.emit(EditorEvent::DocumentChanged(doc.clone()));
        cx.notify();

        // Background debounced save after 500ms
        let repo = self.services.repository.clone();
        let doc_to_save = doc.clone();

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |editor, cx| {
                if editor.generation == current_gen {
                    let res = repo.save(&doc_to_save);
                    match res {
                        Ok(()) => {
                            tracing::debug!("Debounced auto-save succeeded for {}", doc_to_save.id);
                            cx.emit(EditorEvent::DocumentSaved(doc_to_save));
                        }
                        Err(err) => {
                            tracing::warn!("Auto-save failed: {err}");
                        }
                    }
                }
            });
        })
        .detach();
    }

    /// Subscribes every current text input to the editor so typing marks the
    /// document dirty. `defer_in` runs the read-back after the input entity's
    /// own update has finished, avoiding a re-entrant borrow.
    fn wire_inputs(&self, cx: &mut Context<Self>) {
        let editor = cx.weak_entity();
        let mut inputs = Vec::new();

        if let Some(pii) = &self.pii_state {
            inputs.extend(pii.inputs());
        }
        for section in &self.section_states {
            inputs.extend(section.inputs());
        }

        for input in inputs {
            let editor = editor.clone();
            input.update(cx, |input, _cx| {
                input.set_on_change(move |_text, window, cx| {
                    let Some(editor) = editor.upgrade() else {
                        return;
                    };
                    cx.defer_in(window, move |_input, _window, cx| {
                        editor.update(cx, |this, cx| this.sync_and_mark_dirty(cx));
                    });
                });
            });
        }
    }

    /// Wires newly created fields and then commits the current UI state.
    fn commit(&mut self, cx: &mut Context<Self>) {
        self.wire_inputs(cx);
        self.sync_and_mark_dirty(cx);
    }

    // --- PII header mutations ---

    pub fn add_social_link(&mut self, cx: &mut Context<Self>) {
        let link = SocialLink {
            id: Uuid::new_v4(),
            platform: "LinkedIn".to_string(),
            display_text: "linkedin.com/in/handle".to_string(),
            url: "https://linkedin.com/in/handle".to_string(),
            enabled: true,
        };

        if let Some(pii) = &mut self.pii_state {
            pii.social_links
                .push(PiiHeaderState::create_social_link_field(&link, cx));
        }

        self.commit(cx);
    }

    pub fn remove_social_link(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(pii) = &mut self.pii_state {
            pii.social_links.retain(|l| l.id != id);
        }
        self.commit(cx);
    }

    pub fn toggle_social_link(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(pii) = &mut self.pii_state
            && let Some(link) = pii.social_links.iter_mut().find(|l| l.id == id)
        {
            link.enabled = !link.enabled;
        }
        self.commit(cx);
    }

    pub fn add_custom_field(&mut self, cx: &mut Context<Self>) {
        let field = CustomHeaderField {
            id: Uuid::new_v4(),
            label: "Clearance".to_string(),
            value: "Secret".to_string(),
            enabled: true,
        };

        if let Some(pii) = &mut self.pii_state {
            pii.custom_fields
                .push(PiiHeaderState::create_custom_field_entry(&field, cx));
        }

        self.commit(cx);
    }

    pub fn remove_custom_field(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(pii) = &mut self.pii_state {
            pii.custom_fields.retain(|f| f.id != id);
        }
        self.commit(cx);
    }

    pub fn toggle_custom_field(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if let Some(pii) = &mut self.pii_state
            && let Some(field) = pii.custom_fields.iter_mut().find(|f| f.id == id)
        {
            field.enabled = !field.enabled;
        }
        self.commit(cx);
    }

    // --- Section mutations ---

    pub fn toggle_section(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id) {
            section.enabled = !section.enabled;
        }
        self.commit(cx);
    }

    pub fn remove_section(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        self.section_states.retain(|s| s.id != section_id);
        if self.active_tab == EditorTab::Section(section_id) {
            self.active_tab = EditorTab::Profile;
        }
        self.commit(cx);
    }

    pub fn add_timeline_entry(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let entry = TimelineEntry::new("New Company / Institution", "2024");
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
        {
            entries.push(SectionField::create_timeline_entry(&entry, cx));
        }
        self.commit(cx);
    }

    pub fn remove_timeline_entry(
        &mut self,
        section_id: ItemId,
        entry_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
        {
            entries.retain(|e| e.id != entry_id);
        }
        self.commit(cx);
    }

    pub fn toggle_timeline_entry(
        &mut self,
        section_id: ItemId,
        entry_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
        {
            entry.enabled = !entry.enabled;
        }
        self.commit(cx);
    }

    pub fn toggle_currently(
        &mut self,
        section_id: ItemId,
        entry_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
        {
            entry.is_current = !entry.is_current;
            if entry.is_current {
                entry
                    .end_date_input
                    .update(cx, |input, cx| input.set_text("", cx));
            }
        }
        self.commit(cx);
    }

    pub fn add_timeline_bullet(
        &mut self,
        section_id: ItemId,
        entry_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        let bullet = BulletItem::new("Accomplished key objective with measurable impact.");
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
        {
            entry.bullets.push(SectionField::create_bullet(&bullet, cx));
        }
        self.commit(cx);
    }

    pub fn remove_timeline_bullet(
        &mut self,
        section_id: ItemId,
        entry_id: ItemId,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
        {
            entry.bullets.retain(|b| b.id != bullet_id);
        }
        self.commit(cx);
    }

    pub fn toggle_timeline_bullet(
        &mut self,
        section_id: ItemId,
        entry_id: ItemId,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Timeline { entries } = &mut section.kind
            && let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id)
            && let Some(b) = entry.bullets.iter_mut().find(|b| b.id == bullet_id)
        {
            b.enabled = !b.enabled;
        }
        self.commit(cx);
    }

    pub fn add_skill_category(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let cat = SkillCategoryItem {
            id: Uuid::new_v4(),
            origin_item_id: None,
            enabled: true,
            category_label: "Tools & Technologies".to_string(),
            separator: " - ".to_string(),
            items: vec!["Git".to_string(), "Linux".to_string()],
        };

        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Skills { categories } = &mut section.kind
        {
            categories.push(SectionField::create_skill_category(&cat, cx));
        }
        self.commit(cx);
    }

    pub fn remove_skill_category(
        &mut self,
        section_id: ItemId,
        cat_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Skills { categories } = &mut section.kind
        {
            categories.retain(|c| c.id != cat_id);
        }
        self.commit(cx);
    }

    pub fn toggle_skill_category(
        &mut self,
        section_id: ItemId,
        cat_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::Skills { categories } = &mut section.kind
            && let Some(c) = categories.iter_mut().find(|c| c.id == cat_id)
        {
            c.enabled = !c.enabled;
        }
        self.commit(cx);
    }

    pub fn add_simple_bullet(&mut self, section_id: ItemId, cx: &mut Context<Self>) {
        let bullet = BulletItem::new("Certified Information Systems Security Professional (CISSP)");
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::BulletList { items } = &mut section.kind
        {
            items.push(SectionField::create_bullet(&bullet, cx));
        }
        self.commit(cx);
    }

    pub fn remove_simple_bullet(
        &mut self,
        section_id: ItemId,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::BulletList { items } = &mut section.kind
        {
            items.retain(|b| b.id != bullet_id);
        }
        self.commit(cx);
    }

    pub fn toggle_simple_bullet(
        &mut self,
        section_id: ItemId,
        bullet_id: ItemId,
        cx: &mut Context<Self>,
    ) {
        if let Some(section) = self.section_states.iter_mut().find(|s| s.id == section_id)
            && let SectionFieldKind::BulletList { items } = &mut section.kind
            && let Some(b) = items.iter_mut().find(|b| b.id == bullet_id)
        {
            b.enabled = !b.enabled;
        }
        self.commit(cx);
    }

    pub fn add_new_section(&mut self, kind: SectionKind, title: &str, cx: &mut Context<Self>) {
        let section = crate::contracts::CvSection {
            id: Uuid::new_v4(),
            origin_item_id: None,
            title: title.to_string(),
            enabled: true,
            kind,
        };
        self.section_states.push(SectionField::new(&section, cx));
        self.active_tab = EditorTab::Section(section.id);
        self.commit(cx);
    }

    pub fn select_tab(&mut self, tab: EditorTab, cx: &mut Context<Self>) {
        self.active_tab = tab;
        cx.notify();
    }

    // --- Custom section composer ---

    pub fn open_add_section(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            TextInput::new(cx).with_placeholder("Section title (e.g. PROJECTS, PUBLICATIONS)")
        });
        self.add_section_title_input = Some(input);
        self.add_section_kind = NewSectionKind::Timeline;
        self.show_add_section = true;
        cx.notify();
    }

    pub fn cancel_add_section(&mut self, cx: &mut Context<Self>) {
        self.show_add_section = false;
        self.add_section_title_input = None;
        cx.notify();
    }

    pub fn set_add_section_kind(&mut self, kind: NewSectionKind, cx: &mut Context<Self>) {
        self.add_section_kind = kind;
        cx.notify();
    }

    pub fn confirm_add_section(&mut self, cx: &mut Context<Self>) {
        let title = self
            .add_section_title_input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_string())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.add_section_kind.default_title().to_string());

        let kind = self.add_section_kind.into_section_kind();
        self.show_add_section = false;
        self.add_section_title_input = None;
        self.add_new_section(kind, &title, cx);
    }

    // --- Payload Inspector ---

    pub fn open_payload_inspector(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = &self.document else { return };

        match self
            .services
            .privacy
            .inspect_outbound_payload(&doc.body, &doc.pii_header)
        {
            Ok(json) => {
                self.payload_modal_json = Some(json);
                self.is_payload_copied = false;
                self.show_payload_modal = true;
                cx.notify();
            }
            Err(err) => {
                tracing::warn!("Failed to inspect AI payload: {err}");
            }
        }
    }

    pub fn close_payload_inspector(&mut self, cx: &mut Context<Self>) {
        self.show_payload_modal = false;
        self.payload_modal_json = None;
        cx.notify();
    }

    pub fn copy_payload_json(&mut self, cx: &mut Context<Self>) {
        if let Some(json) = &self.payload_modal_json {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(json.clone()));
            self.is_payload_copied = true;
            cx.notify();
        }
    }

    // --- Render cards ---

    fn render_pii_header_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        let Some(state) = &self.pii_state else {
            return div().into_any_element();
        };

        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(14.0))
            .bg(rgb(c.bg_surface))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgb(c.border_subtle));

        // Top badge row + Inspect Payload button
        let top_row = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(theme::pii_badge("🔒 Local Only — Never Sent to AI")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(rgb(c.border_subtle))
                    .text_size(t.caption)
                    .text_color(rgb(c.text_secondary))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(c.bg_input)).text_color(rgb(c.text_primary)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.open_payload_inspector(cx);
                        }),
                    )
                    .child("🔍 Inspect AI Payload"),
            );
        card = card.child(top_row);

        // Core fields grid: Name + Headline
        card = card.child(
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .child(labeled_input("Full name", state.name_input.clone())),
                )
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .child(labeled_input(
                            "Headline / professional title",
                            state.headline_input.clone(),
                        )),
                ),
        );

        // Contact info: Email, Phone, Location
        card = card.child(
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .child(labeled_input("Email", state.email_input.clone())),
                )
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .child(labeled_input("Phone", state.phone_input.clone())),
                )
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .child(labeled_input("Location", state.location_input.clone())),
                ),
        );

        // Social links section
        let mut social_section = div().flex().flex_col().gap(px(6.0)).mt(px(4.0));
        let social_header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(t.caption)
                    .text_color(rgb(c.text_secondary))
                    .child("Social & Portfolio Links:"),
            )
            .child(
                div()
                    .text_size(t.badge)
                    .text_color(rgb(c.tuta_crimson))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(c.tuta_crimson_bright)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.add_social_link(cx);
                        }),
                    )
                    .child("+ Add Social Link"),
            );
        social_section = social_section.child(social_header);

        for link in &state.social_links {
            let link_id = link.id;
            let enabled = link.enabled;

            let row = div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(8.0))
                .rounded(px(6.0))
                .bg(rgb(c.bg_input))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(6.0))
                        .child(
                            div()
                                .pb(px(6.0))
                                .child(
                                    div()
                                        .size(px(18.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(3.0))
                                        .border_1()
                                        .border_color(if enabled {
                                            rgb(c.tuta_crimson)
                                        } else {
                                            rgb(c.border_subtle)
                                        })
                                        .bg(if enabled {
                                            rgb(c.tuta_crimson)
                                        } else {
                                            rgb(c.bg_input)
                                        })
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                this.toggle_social_link(link_id, cx);
                                            }),
                                        )
                                        .child(if enabled { "✓" } else { "" }),
                                ),
                        )
                        .child(
                            div().flex_grow().min_w_0().child(labeled_input(
                                "Platform",
                                link.platform_input.clone(),
                            )),
                        )
                        .child(
                            div()
                                .pb(px(6.0))
                                .px(px(6.0))
                                .text_color(rgb(c.text_disabled))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(c.diff_del_text)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        this.remove_social_link(link_id, cx);
                                    }),
                                )
                                .child("✕"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(6.0))
                        .child(
                            div().flex_grow().min_w_0().child(labeled_input(
                                "Display text",
                                link.display_input.clone(),
                            )),
                        )
                        .child(
                            div().flex_grow().min_w_0().child(labeled_input(
                                "URL",
                                link.url_input.clone(),
                            )),
                        ),
                );
            social_section = social_section.child(row);
        }
        card = card.child(social_section);

        // Custom fields section
        if !state.custom_fields.is_empty() {
            let mut custom_section = div().flex().flex_col().gap(px(6.0)).mt(px(4.0));
            let custom_header = div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(t.caption)
                        .text_color(rgb(c.text_secondary))
                        .child("Custom Header Fields:"),
                )
                .child(
                    div()
                        .text_size(t.badge)
                        .text_color(rgb(c.tuta_crimson))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(c.tuta_crimson_bright)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.add_custom_field(cx);
                            }),
                        )
                        .child("+ Add Custom Field"),
                );
            custom_section = custom_section.child(custom_header);

            for field in &state.custom_fields {
                let field_id = field.id;
                let enabled = field.enabled;

                let row = div()
                    .flex()
                    .items_end()
                    .gap(px(6.0))
                    .p(px(8.0))
                    .rounded(px(6.0))
                    .bg(rgb(c.bg_input))
                    .child(
                        div()
                            .pb(px(6.0))
                            .child(
                                div()
                                    .size(px(18.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(3.0))
                                    .border_1()
                                    .border_color(if enabled {
                                        rgb(c.tuta_crimson)
                                    } else {
                                        rgb(c.border_subtle)
                                    })
                                    .bg(if enabled {
                                        rgb(c.tuta_crimson)
                                    } else {
                                        rgb(c.bg_input)
                                    })
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.toggle_custom_field(field_id, cx);
                                        }),
                                    )
                                    .child(if enabled { "✓" } else { "" }),
                            ),
                    )
                    .child(
                        div().flex_grow().min_w_0().child(labeled_input(
                            "Label",
                            field.label_input.clone(),
                        )),
                    )
                    .child(
                        div().flex_grow().min_w_0().child(labeled_input(
                            "Value",
                            field.value_input.clone(),
                        )),
                    )
                    .child(
                        div()
                            .pb(px(6.0))
                            .px(px(6.0))
                            .text_color(rgb(c.text_disabled))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(c.diff_del_text)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.remove_custom_field(field_id, cx);
                                }),
                            )
                            .child("✕"),
                    );
                custom_section = custom_section.child(row);
            }
            card = card.child(custom_section);
        } else {
            card = card.child(
                div().flex().justify_end().child(
                    div()
                        .text_size(t.badge)
                        .text_color(rgb(c.text_disabled))
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(c.text_secondary)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.add_custom_field(cx);
                            }),
                        )
                        .child("+ Add Custom Field"),
                ),
            );
        }

        card.into_any_element()
    }

    fn render_section_card(
        &self,
        section: &SectionField,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();
        let section_id = section.id;
        let enabled = section.enabled;

        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .bg(rgb(c.bg_surface))
            .rounded(px(8.0))
            .border_1()
            .border_color(rgb(c.border_subtle));

        // Header bar: Enabled toggle, Section Title input, Delete section
        let header_bar = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .flex_grow()
                    .child(
                        div()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(if enabled {
                                rgb(c.tuta_crimson)
                            } else {
                                rgb(c.border_subtle)
                            })
                            .bg(if enabled {
                                rgb(c.tuta_crimson)
                            } else {
                                rgb(c.bg_input)
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.toggle_section(section_id, cx);
                                }),
                            )
                            .child(if enabled { "✓" } else { "" }),
                    )
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .child(section.title_input.clone()),
                    )
                    .child(if !enabled {
                        theme::pii_badge("Hidden").into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(rgb(c.text_disabled))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(c.diff_del_text)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.remove_section(section_id, cx);
                        }),
                    )
                    .child("🗑 Remove"),
            );
        card = card.child(header_bar);
        card = card.child(theme::hairline());

        // Body content based on kind
        match &section.kind {
            SectionFieldKind::Summary { text_input } => {
                card = card.child(div().w_full().child(text_input.clone()));
            }
            SectionFieldKind::Timeline { entries } => {
                let mut entries_container = div().flex().flex_col().gap(px(10.0));

                for entry in entries {
                    let entry_id = entry.id;
                    let entry_enabled = entry.enabled;
                    let is_current = entry.is_current;

                    let mut entry_box = div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .p(px(8.0))
                        .rounded(px(6.0))
                        .bg(rgb(c.bg_input))
                        .border_1()
                        .border_color(rgb(c.border_subtle));

                    // Row 1: Enabled toggle, organization and role fields, delete entry
                    let row1 = div()
                        .flex()
                        .items_end()
                        .gap(px(8.0))
                        .child(
                            div()
                                .pb(px(6.0))
                                .child(
                                    div()
                                        .size(px(18.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(3.0))
                                        .border_1()
                                        .border_color(if entry_enabled {
                                            rgb(c.tuta_crimson)
                                        } else {
                                            rgb(c.border_subtle)
                                        })
                                        .bg(if entry_enabled {
                                            rgb(c.tuta_crimson)
                                        } else {
                                            rgb(c.bg_surface)
                                        })
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                this.toggle_timeline_entry(section_id, entry_id, cx);
                                            }),
                                        )
                                        .child(if entry_enabled { "✓" } else { "" }),
                                ),
                        )
                        .child(
                            div()
                                .flex_grow()
                                .min_w_0()
                                .child(labeled_input(
                                    "Organization / Institution",
                                    entry.primary_heading_input.clone(),
                                )),
                        )
                        .child(
                            div()
                                .flex_grow()
                                .min_w_0()
                                .child(labeled_input(
                                    "Role / Degree",
                                    entry.secondary_heading_input.clone(),
                                )),
                        )
                        .child(
                            div()
                                .pb(px(6.0))
                                .px(px(6.0))
                                .text_color(rgb(c.text_disabled))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(c.diff_del_text)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        this.remove_timeline_entry(section_id, entry_id, cx);
                                    }),
                                )
                                .child("✕"),
                        );
                    entry_box = entry_box.child(row1);

                    // Row 2: Dates + Currently toggle
                    let row2 = div()
                        .flex()
                        .items_end()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(150.0))
                                .child(labeled_input("Start date", entry.start_date_input.clone())),
                        )
                        .child(if !is_current {
                            div()
                                .w(px(150.0))
                                .child(labeled_input("End date", entry.end_date_input.clone()))
                                .into_any_element()
                        } else {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .text_size(t.caption)
                                        .text_color(rgb(c.text_secondary))
                                        .child("End date"),
                                )
                                .child(
                                    div()
                                        .text_size(t.body)
                                        .text_color(rgb(c.text_disabled))
                                        .child("(Currently)"),
                                )
                                .into_any_element()
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .pb(px(6.0))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        this.toggle_currently(section_id, entry_id, cx);
                                    }),
                                )
                                .child(
                                    div()
                                        .size(px(16.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(3.0))
                                        .border_1()
                                        .border_color(if is_current {
                                            rgb(c.tuta_crimson)
                                        } else {
                                            rgb(c.border_subtle)
                                        })
                                        .bg(if is_current {
                                            rgb(c.tuta_crimson)
                                        } else {
                                            rgb(c.bg_surface)
                                        })
                                        .child(if is_current { "✓" } else { "" }),
                                )
                                .child(
                                    div()
                                        .text_size(t.caption)
                                        .text_color(rgb(c.text_secondary))
                                        .child("Currently active"),
                                ),
                        );
                    entry_box = entry_box.child(row2);

                    // Bullets list
                    let mut bullets_list = div().flex().flex_col().gap(px(4.0)).pl(px(8.0));
                    for b in &entry.bullets {
                        let bullet_id = b.id;
                        let b_enabled = b.enabled;

                        let b_row = div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .size(px(16.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(3.0))
                                    .border_1()
                                    .border_color(if b_enabled {
                                        rgb(c.tuta_crimson)
                                    } else {
                                        rgb(c.border_subtle)
                                    })
                                    .bg(if b_enabled {
                                        rgb(c.tuta_crimson)
                                    } else {
                                        rgb(c.bg_surface)
                                    })
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.toggle_timeline_bullet(
                                                section_id, entry_id, bullet_id, cx,
                                            );
                                        }),
                                    )
                                    .child(if b_enabled { "✓" } else { "" }),
                            )
                            .child(div().flex_grow().min_w_0().child(b.text_input.clone()))
                            .child(
                                div()
                                    .px(px(6.0))
                                    .py(px(2.0))
                                    .text_color(rgb(c.text_disabled))
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(rgb(c.diff_del_text)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.remove_timeline_bullet(
                                                section_id, entry_id, bullet_id, cx,
                                            );
                                        }),
                                    )
                                    .child("✕"),
                            );
                        bullets_list = bullets_list.child(b_row);
                    }

                    // Add bullet affordance
                    bullets_list = bullets_list.child(
                        div()
                            .text_size(t.badge)
                            .text_color(rgb(c.tuta_crimson))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(c.tuta_crimson_bright)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.add_timeline_bullet(section_id, entry_id, cx);
                                }),
                            )
                            .child("+ Add Bullet Point"),
                    );

                    entry_box = entry_box.child(bullets_list);
                    entries_container = entries_container.child(entry_box);
                }

                // Add entry affordance
                entries_container = entries_container.child(
                    div().flex().justify_start().child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .px(px(12.0))
                            .py(px(4.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(rgb(c.tuta_crimson))
                            .text_size(t.caption)
                            .text_color(rgb(c.tuta_crimson))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(c.bg_input)).text_color(rgb(c.tuta_crimson_bright)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.add_timeline_entry(section_id, cx);
                                }),
                            )
                            .child("+ Add Role / Position"),
                    ),
                );

                card = card.child(entries_container);
            }
            SectionFieldKind::Skills { categories } => {
                let mut cat_container = div().flex().flex_col().gap(px(8.0));

                for cat in categories {
                    let cat_id = cat.id;
                    let cat_enabled = cat.enabled;

                    let row = div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .p(px(8.0))
                        .rounded(px(6.0))
                        .bg(rgb(c.bg_input))
                        .child(
                            div()
                                .flex()
                                .items_end()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .pb(px(6.0))
                                        .child(
                                            div()
                                                .size(px(18.0))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px(3.0))
                                                .border_1()
                                                .border_color(if cat_enabled {
                                                    rgb(c.tuta_crimson)
                                                } else {
                                                    rgb(c.border_subtle)
                                                })
                                                .bg(if cat_enabled {
                                                    rgb(c.tuta_crimson)
                                                } else {
                                                    rgb(c.bg_surface)
                                                })
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                        this.toggle_skill_category(section_id, cat_id, cx);
                                                    }),
                                                )
                                                .child(if cat_enabled { "✓" } else { "" }),
                                        ),
                                )
                                .child(
                                    div().flex_grow().min_w_0().child(labeled_input(
                                        "Category label",
                                        cat.label_input.clone(),
                                    )),
                                )
                                .child(
                                    div().w(px(90.0)).child(labeled_input(
                                        "Separator",
                                        cat.separator_input.clone(),
                                    )),
                                )
                                .child(
                                    div()
                                        .pb(px(6.0))
                                        .px(px(6.0))
                                        .text_color(rgb(c.text_disabled))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(c.diff_del_text)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                                this.remove_skill_category(section_id, cat_id, cx);
                                            }),
                                        )
                                        .child("✕"),
                                ),
                        )
                        .child(labeled_input(
                            "Items (separated by the separator above, or commas)",
                            cat.items_input.clone(),
                        ));
                    cat_container = cat_container.child(row);
                }

                cat_container = cat_container.child(
                    div().flex().justify_start().child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.tuta_crimson))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(c.tuta_crimson_bright)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.add_skill_category(section_id, cx);
                                }),
                            )
                            .child("+ Add Skill Category"),
                    ),
                );

                card = card.child(cat_container);
            }
            SectionFieldKind::BulletList { items } => {
                let mut items_container = div().flex().flex_col().gap(px(6.0));

                for item in items {
                    let bullet_id = item.id;
                    let bullet_enabled = item.enabled;

                    let row = div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .size(px(18.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(3.0))
                                .border_1()
                                .border_color(if bullet_enabled {
                                    rgb(c.tuta_crimson)
                                } else {
                                    rgb(c.border_subtle)
                                })
                                .bg(if bullet_enabled {
                                    rgb(c.tuta_crimson)
                                } else {
                                    rgb(c.bg_input)
                                })
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        this.toggle_simple_bullet(section_id, bullet_id, cx);
                                    }),
                                )
                                .child(if bullet_enabled { "✓" } else { "" }),
                        )
                        .child(div().flex_grow().min_w_0().child(item.text_input.clone()))
                        .child(
                            div()
                                .px(px(6.0))
                                .py(px(2.0))
                                .text_color(rgb(c.text_disabled))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(c.diff_del_text)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        this.remove_simple_bullet(section_id, bullet_id, cx);
                                    }),
                                )
                                .child("✕"),
                        );
                    items_container = items_container.child(row);
                }

                items_container = items_container.child(
                    div().flex().justify_start().child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.tuta_crimson))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(c.tuta_crimson_bright)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.add_simple_bullet(section_id, cx);
                                }),
                            )
                            .child("+ Add Item"),
                    ),
                );

                card = card.child(items_container);
            }
        }

        card.into_any_element()
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        let mut bar = div()
            .id("editor-tabs")
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .px(px(12.0))
            .py(px(8.0))
            .bg(rgb(c.bg_surface))
            .border_b_1()
            .border_color(rgb(c.border_subtle))
            .child({
                let selected = self.active_tab == EditorTab::Profile;
                div()
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .bg(if selected {
                        rgb(c.tuta_crimson)
                    } else {
                        rgb(c.bg_input)
                    })
                    .text_size(t.caption)
                    .text_color(if selected {
                        rgb(c.text_on_accent)
                    } else {
                        rgb(c.text_secondary)
                    })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.select_tab(EditorTab::Profile, cx);
                        }),
                    )
                    .child("Profile")
            });

        for section in &self.section_states {
            let selected = self.active_tab == EditorTab::Section(section.id);
            let tab = EditorTab::Section(section.id);
            let title = section.title_input.read(cx).text().trim().to_string();
            let title = title.trim_end_matches(':');
            let label = if title.chars().count() > 18 {
                format!("{}…", title.chars().take(16).collect::<String>())
            } else {
                title.to_string()
            };
            bar = bar.child(
                div()
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .bg(if selected {
                        rgb(c.tuta_crimson)
                    } else {
                        rgb(c.bg_input)
                    })
                    .text_size(t.caption)
                    .text_color(if selected {
                        rgb(c.text_on_accent)
                    } else {
                        rgb(c.text_secondary)
                    })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.select_tab(tab, cx);
                        }),
                    )
                    .child(label),
            );
        }

        bar.child(
            div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(rgb(c.border_subtle))
                .text_size(t.caption)
                .text_color(rgb(c.tuta_crimson))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(c.bg_input)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.add_new_section(
                            SectionKind::TimelineEntries {
                                entries: Vec::new(),
                            },
                            "NEW SECTION",
                            cx,
                        );
                    }),
                )
                .child("＋"),
        )
    }
}

/// Caption + text input, used throughout the editor cards.
fn labeled_input(label: impl Into<SharedString>, input: Entity<TextInput>) -> Div {
    let c = theme::colors();
    let t = theme::typography();
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .min_w_0()
        .w_full()
        .child(
            div()
                .text_size(t.caption)
                .text_color(rgb(c.text_secondary))
                .child(label.into()),
        )
        .child(input)
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        let mut root = div().size_full().flex().flex_col().relative();

        if self.document.is_some() {
            root = root.child(self.render_tab_bar(cx));
        }

        let mut container = div()
            .id("editor-scroll")
            .flex()
            .flex_col()
            .flex_grow()
            .min_h_0()
            .bg(rgb(c.bg_canvas))
            .overflow_y_scroll()
            .p(px(16.0))
            .gap(px(14.0));

        if self.document.is_some() {
            match self.active_tab {
                EditorTab::Profile => {
                    container = container.child(self.render_pii_header_card(cx));
                }
                EditorTab::Section(section_id) => {
                    if let Some(section) = self
                        .section_states
                        .iter()
                        .find(|section| section.id == section_id)
                    {
                        container = container.child(self.render_section_card(section, cx));
                    } else {
                        self.active_tab = EditorTab::Profile;
                        container = container.child(self.render_pii_header_card(cx));
                    }
                }
            }

            // Section creation
            let mut add_sections_bar = div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .py(px(12.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child(
                            div()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        this.open_add_section(cx);
                                    }),
                                )
                                .child(theme::button_ghost("＋ Add Section")),
                        )
                        .child(
                            div()
                                .text_size(t.caption)
                                .text_color(rgb(c.text_disabled))
                                .child("Add any section: PROJECTS, PUBLICATIONS, VOLUNTEERING…"),
                        ),
                );

            if self.show_add_section {
                let mut form = div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .p(px(14.0))
                    .bg(rgb(c.bg_surface))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(rgb(c.tuta_crimson))
                    .child(
                        div()
                            .text_size(t.subheading)
                            .text_color(rgb(c.text_primary))
                            .child("New custom section"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(t.caption)
                                    .text_color(rgb(c.text_secondary))
                                    .child("Section title"),
                            )
                            .child(
                                if let Some(input) = &self.add_section_title_input {
                                    input.clone().into_any_element()
                                } else {
                                    div()
                                        .text_size(t.caption)
                                        .text_color(rgb(c.text_disabled))
                                        .child("Title input unavailable")
                                        .into_any_element()
                                },
                            ),
                    );

                let mut kind_row = div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.text_secondary))
                            .child("Layout"),
                    );
                for kind in [
                    NewSectionKind::Timeline,
                    NewSectionKind::Skills,
                    NewSectionKind::Bullets,
                    NewSectionKind::Summary,
                ] {
                    let selected = self.add_section_kind == kind;
                    kind_row = kind_row.child(
                        div()
                            .px(px(10.0))
                            .py(px(4.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(if selected {
                                rgb(c.tuta_crimson)
                            } else {
                                rgb(c.border_subtle)
                            })
                            .bg(if selected {
                                rgb(c.tuta_crimson)
                            } else {
                                rgb(c.bg_input)
                            })
                            .text_size(t.caption)
                            .text_color(if selected {
                                rgb(c.text_on_accent)
                            } else {
                                rgb(c.text_secondary)
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.set_add_section_kind(kind, cx);
                                }),
                            )
                            .child(kind.label()),
                    );
                }
                form = form.child(kind_row).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        this.confirm_add_section(cx);
                                    }),
                                )
                                .child(theme::button_primary("Add Section")),
                        )
                        .child(
                            div()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        this.cancel_add_section(cx);
                                    }),
                                )
                                .child(theme::button_ghost("Cancel")),
                        ),
                );

                add_sections_bar = add_sections_bar.child(form);
            }
            container = container.child(add_sections_bar);
        } else {
            // Empty state
            container = container.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_grow()
                    .gap(px(8.0))
                    .py(px(80.0))
                    .child(
                        div()
                            .text_size(t.heading)
                            .text_color(rgb(c.text_disabled))
                            .child("No CV Selected"),
                    )
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.text_disabled))
                            .child("Select a CV from the sidebar library to edit it here."),
                    ),
            );
        }

        root = root.child(container);

        // Modal overlay for Inspect AI Payload
        if self.show_payload_modal
            && let Some(json) = &self.payload_modal_json
        {
            let modal = payload_modal::render_payload_modal(
                json,
                self.is_payload_copied,
                |this: &mut EditorView, _, cx| {
                    this.close_payload_inspector(cx);
                },
                |this: &mut EditorView, _, cx| {
                    this.copy_payload_json(cx);
                },
                cx,
            );
            root = root.child(modal);
        }

        root
    }
}
