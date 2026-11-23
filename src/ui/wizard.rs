//! "Generate Job Variation" wizard modal dialog.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Provides the interactive modal for launching an AI-tailored child CV
//! variation from a chosen Root CV, specifying company, target role, full job
//! description, and selectable `omp` skills.

use gpui::{
    Context, Entity, EventEmitter, IntoElement, MouseButton, MouseDownEvent, Render,
    SharedString, Window, div, prelude::*, px, rgb, rgba,
};

use crate::contracts::{CvSummary, DocumentId, JobContext, Services, SkillManifest};
use crate::theme;
use crate::ui::text_input::TextInput;

/// Events emitted by the wizard to the parent workspace shell.
#[derive(Clone, Debug, PartialEq)]
pub enum WizardEvent {
    /// User confirmed the inputs; start variation branching and AI tailoring.
    Generate {
        root_id: DocumentId,
        job_context: JobContext,
        skill_ids: Vec<String>,
    },
    /// User dismissed or cancelled the modal without generating.
    Cancel,
}

/// Local state for each selectable skill in the wizard.
#[derive(Clone, Debug)]
pub struct SkillOption {
    pub manifest: SkillManifest,
    pub enabled: bool,
}

/// The stateful modal view for configuring and launching a variation.
pub struct WizardView {
    pub(crate) roots: Vec<CvSummary>,
    pub(crate) selected_root_id: Option<DocumentId>,
    pub(crate) company_input: Entity<TextInput>,
    pub(crate) role_input: Entity<TextInput>,
    pub(crate) job_desc_input: Entity<TextInput>,
    pub(crate) skills: Vec<SkillOption>,
    pub(crate) error_message: Option<String>,
}

impl EventEmitter<WizardEvent> for WizardView {}

impl WizardView {
    /// Creates a new wizard instance. Preselects `preselected_root_id` if valid,
    /// or defaults to the first root in the repository.
    pub fn new(
        services: Services,
        preselected_root_id: Option<DocumentId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let roots = services.repository.list_roots().unwrap_or_default();
        let selected_root_id = if let Some(id) = preselected_root_id {
            if roots.iter().any(|r| r.id == id) {
                Some(id)
            } else {
                roots.first().map(|r| r.id)
            }
        } else {
            roots.first().map(|r| r.id)
        };

        let company_input = cx.new(|cx| {
            TextInput::new(cx).with_placeholder("Target Company (e.g. CrowdStrike, Cloudflare)")
        });

        let role_input = cx.new(|cx| {
            TextInput::new(cx).with_placeholder("Target Role (e.g. Senior Security Engineer)")
        });

        let job_desc_input = cx.new(|cx| {
            TextInput::new(cx)
                .with_placeholder("Paste target job description and requirements here...")
                .with_multiline(true)
        });

        let available_skills = services
            .skills
            .list()
            .into_iter()
            .filter(|skill| skill.enabled);
        let skills = available_skills
            .into_iter()
            .map(|manifest| {
                let enabled = manifest.default_enabled;
                SkillOption { manifest, enabled }
            })
            .collect();

        Self {
            roots,
            selected_root_id,
            company_input,
            role_input,
            job_desc_input,
            skills,
            error_message: None,
        }
    }

    /// Selects a source Root CV by id.
    pub fn select_root(&mut self, root_id: DocumentId, cx: &mut Context<Self>) {
        self.selected_root_id = Some(root_id);
        self.error_message = None;
        cx.notify();
    }

    /// Toggles the enabled state of a skill by its id.
    pub fn toggle_skill(&mut self, skill_id: &str, cx: &mut Context<Self>) {
        if let Some(skill) = self.skills.iter_mut().find(|s| s.manifest.id == skill_id) {
            skill.enabled = !skill.enabled;
            cx.notify();
        }
    }

    /// Cancels and closes the wizard.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(WizardEvent::Cancel);
    }

    /// Validates fields and emits `WizardEvent::Generate`.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(root_id) = self.selected_root_id else {
            self.error_message = Some("At least one source Root CV is required.".to_string());
            cx.notify();
            return;
        };

        let company = self.company_input.read(cx).text().trim().to_string();
        if company.is_empty() {
            self.error_message = Some("Target Company cannot be empty.".to_string());
            cx.notify();
            return;
        }

        let role = self.role_input.read(cx).text().trim().to_string();
        if role.is_empty() {
            self.error_message = Some("Target Role cannot be empty.".to_string());
            cx.notify();
            return;
        }

        let job_desc = self.job_desc_input.read(cx).text().trim().to_string();
        if job_desc.is_empty() {
            self.error_message = Some("Job Description cannot be empty.".to_string());
            cx.notify();
            return;
        }

        let selected_skill_ids: Vec<String> = self
            .skills
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.manifest.id.clone())
            .collect();

        let job_context = JobContext {
            company_name: company,
            target_role: role,
            job_description_raw: job_desc,
            application_url: None,
            applied_skill_ids: selected_skill_ids.clone(),
            ats_match_score: None,
        };

        self.error_message = None;
        cx.emit(WizardEvent::Generate {
            root_id,
            job_context,
            skill_ids: selected_skill_ids,
        });
    }

    fn render_root_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        let mut selector = div().flex().flex_col().gap(px(4.0));

        if self.roots.is_empty() {
            return div()
                .p(px(8.0))
                .rounded(px(6.0))
                .bg(rgb(c.diff_del_bg))
                .text_size(t.caption)
                .text_color(rgb(c.diff_del_text))
                .child("No Root Master CVs available. Please create a Root CV first.");
        }

        let mut list_box = div()
            .id("wizard-roots-list")
            .flex()
            .flex_col()
            .gap(px(4.0))
            .max_h(px(130.0))
            .overflow_y_scroll()
            .p(px(4.0))
            .bg(rgb(c.bg_input))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgb(c.border_subtle));

        for root in &self.roots {
            let root_id = root.id;
            let is_selected = self.selected_root_id == Some(root_id);
            let title: SharedString = root.display_title().into();

            let row = div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(6.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(if is_selected {
                    rgb(c.tuta_crimson)
                } else {
                    rgb(c.border_subtle)
                })
                .bg(if is_selected {
                    rgb(c.bg_surface)
                } else {
                    rgb(c.bg_input)
                })
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.select_root(root_id, cx);
                    }),
                )
                .child(
                    div()
                        .size(px(14.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(if is_selected {
                            rgb(c.tuta_crimson)
                        } else {
                            rgb(c.border_subtle)
                        })
                        .child(if is_selected {
                            div()
                                .size(px(6.0))
                                .rounded_full()
                                .bg(rgb(c.tuta_crimson))
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(
                    div()
                        .text_size(t.body)
                        .text_color(if is_selected {
                            rgb(c.text_primary)
                        } else {
                            rgb(c.text_secondary)
                        })
                        .child(SharedString::from(format!("🛡️ {title}"))),
                );

            list_box = list_box.child(row);
        }

        selector = selector.child(list_box);
        selector
    }

    fn render_skills_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        let mut container = div()
            .id("wizard-skills-list")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .max_h(px(160.0))
            .overflow_y_scroll()
            .p(px(4.0))
            .bg(rgb(c.bg_input))
            .rounded(px(6.0))
            .border_1()
            .border_color(rgb(c.border_subtle));

        if self.skills.is_empty() {
            container = container.child(
                div()
                    .p(px(8.0))
                    .text_size(t.caption)
                    .text_color(rgb(c.text_disabled))
                    .child("No skills found in ~/.config/vanguard-cv/skills/"),
            );
            return container;
        }

        for skill in &self.skills {
            let skill_id = skill.manifest.id.clone();
            let is_enabled = skill.enabled;
            let name: SharedString = skill.manifest.name.clone().into();
            let desc: SharedString = skill
                .manifest
                .description
                .clone()
                .unwrap_or_else(|| "Modular AI skill directive".to_string())
                .into();
            let category: SharedString = if skill.manifest.category.is_empty() {
                "General".into()
            } else {
                skill.manifest.category.clone().into()
            };

            let id_for_click = skill_id.clone();
            let row = div()
                .flex()
                .items_start()
                .gap(px(8.0))
                .p(px(8.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(if is_enabled {
                    rgb(c.tuta_crimson)
                } else {
                    rgb(c.border_subtle)
                })
                .bg(if is_enabled {
                    rgb(c.bg_surface)
                } else {
                    rgb(c.bg_input)
                })
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.toggle_skill(&id_for_click, cx);
                    }),
                )
                .child(
                    div()
                        .size(px(16.0))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(if is_enabled {
                            rgb(c.tuta_crimson)
                        } else {
                            rgb(c.border_subtle)
                        })
                        .bg(if is_enabled {
                            rgb(c.tuta_crimson)
                        } else {
                            rgb(c.bg_input)
                        })
                        .text_color(rgb(c.text_on_accent))
                        .text_size(t.caption)
                        .child(if is_enabled { "✓" } else { "" }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .flex_grow()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .text_size(t.body)
                                        .text_color(rgb(c.text_primary))
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .px(px(6.0))
                                        .py(px(1.0))
                                        .rounded(px(3.0))
                                        .bg(rgb(c.bg_input))
                                        .border_1()
                                        .border_color(rgb(c.border_subtle))
                                        .text_size(t.badge)
                                        .text_color(rgb(c.text_secondary))
                                        .child(category),
                                ),
                        )
                        .child(
                            div()
                                .text_size(t.caption)
                                .text_color(rgb(c.text_secondary))
                                .child(desc),
                        ),
                );

            container = container.child(row);
        }

        container
    }
}

impl Render for WizardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        div()
            .id("wizard-backdrop")
            .absolute()
            .inset_0()
            .bg(rgba(0x000000C0))
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.cancel(cx);
                }),
            )
            .child(
                div()
                    .id("wizard-card")
                    .flex()
                    .flex_col()
                    .w_full()
                    .max_w(px(740.0))
                    .max_h(px(840.0))
                    .bg(rgb(c.bg_surface))
                    .border_1()
                    .border_color(rgb(c.border_subtle))
                    .rounded(px(8.0))
                    .p(px(24.0))
                    .gap(px(14.0))
                    .overflow_y_scroll()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    // Header row
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(theme::pii_badge("✨ AI JOB VARIATION WIZARD"))
                                    .child(
                                        div()
                                            .text_size(t.heading)
                                            .text_color(rgb(c.text_primary))
                                            .child("Generate Tailored Sub-Variation"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(t.heading)
                                    .text_color(rgb(c.text_secondary))
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(rgb(c.text_primary)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.cancel(cx);
                                        }),
                                    )
                                    .child("✕"),
                            ),
                    )
                    // Subtitle
                    .child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.text_secondary))
                            .child(
                                "Branch a new Child Sub-Variation from your Master CV, scrub PII \
                                 locally, and run the Antigravity AI engine to tailor content for this role.",
                            ),
                    )
                    // Inline Error Banner (if any)
                    .child(if let Some(err) = &self.error_message {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .p(px(10.0))
                            .rounded(px(6.0))
                            .bg(rgb(c.diff_del_bg))
                            .border_1()
                            .border_color(rgb(c.tuta_crimson))
                            .text_size(t.caption)
                            .text_color(rgb(c.diff_del_text))
                            .child(SharedString::from(format!("⚠️ {err}")))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    // Field 1: Source Root CV
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(t.caption)
                                    .text_color(rgb(c.text_secondary))
                                    .child("Source Master Root CV:"),
                            )
                            .child(self.render_root_selector(cx)),
                    )
                    // Field 2: Target Company & Role (2-column layout)
                    .child(
                        div()
                            .flex()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .flex_grow()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .text_size(t.caption)
                                            .text_color(rgb(c.text_secondary))
                                            .child("Target Company:"),
                                    )
                                    .child(self.company_input.clone()),
                            )
                            .child(
                                div()
                                    .flex_grow()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .text_size(t.caption)
                                            .text_color(rgb(c.text_secondary))
                                            .child("Target Role:"),
                                    )
                                    .child(self.role_input.clone()),
                            ),
                    )
                    // Field 3: Job Description
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(t.caption)
                                    .text_color(rgb(c.text_secondary))
                                    .child("Target Job Description:"),
                            )
                            .child(self.job_desc_input.clone()),
                    )
                    // Field 4: Skills Checkboxes
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .text_size(t.caption)
                                    .text_color(rgb(c.text_secondary))
                                    .child("Active AI Skills (omp):"),
                            )
                            .child(self.render_skills_selector(cx)),
                    )
                    // Footer Action Buttons
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.0))
                            .pt(px(10.0))
                            .border_t_1()
                            .border_color(rgb(c.border_subtle))
                            .child(
                                div()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.cancel(cx);
                                        }),
                                    )
                                    .child(theme::button_ghost("Cancel")),
                            )
                            .child(
                                div()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.submit(cx);
                                        }),
                                    )
                                    .child(theme::button_primary(
                                        "Generate Tailored Sub-Variation",
                                    )),
                            ),
                    ),
            )
    }
}
