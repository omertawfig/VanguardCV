use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::{AiProviderKind, CvDocument, DocumentId, ItemId};
use crate::db::lineage_diff::{ChangeKind, DiffSpanKind};
use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_button, render_empty_state, render_icon,
    render_icon_button, render_toggle,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::{Dialog, WorkspaceView};

pub fn render_modal_overlay(
    workspace: &WorkspaceView,
    cx: &mut Context<WorkspaceView>,
) -> Option<Stateful<Div>> {
    let dialog = workspace.active_dialog.as_ref()?;
    let theme = &workspace.theme;

    let content = match dialog {
        Dialog::NewMaster => render_new_master_modal(workspace, theme, cx),
        Dialog::NewVariation { root_id } => {
            render_new_variation_modal(workspace, *root_id, theme, cx)
        }
        Dialog::Compare => render_compare_modal(workspace, theme, cx),
        Dialog::PayloadPreview => render_payload_modal(workspace, theme, cx),
        Dialog::ExportPdf => render_export_modal(workspace, theme, cx),
        Dialog::PromoteItem => render_promote_modal(workspace, theme, cx),
        Dialog::EditSummary { .. } => render_edit_summary_modal(workspace, theme, cx),
        Dialog::RenameSection { section_id } => {
            render_rename_section_modal(workspace, *section_id, theme, cx)
        }
        Dialog::EditEntry { is_new, .. } => render_edit_entry_modal(workspace, *is_new, theme, cx),
        Dialog::EditBullet { is_new, .. } => {
            render_edit_bullet_modal(workspace, *is_new, theme, cx)
        }
        Dialog::EditCategory { is_new, .. } => {
            render_edit_category_modal(workspace, *is_new, theme, cx)
        }
        Dialog::AddSection => render_add_section_modal(workspace, theme, cx),
        Dialog::RenameCv => render_rename_modal(workspace, theme, cx),
        Dialog::EditSkill { is_new, read_only } => {
            render_edit_skill_modal(workspace, *is_new, *read_only, theme, cx)
        }
        Dialog::EditContact => render_edit_contact_modal(workspace, theme, cx),
        Dialog::EditSocialLink { link_id } => {
            render_edit_social_modal(workspace, link_id.is_some(), theme, cx)
        }
        Dialog::EditCustomField { field_id } => {
            render_edit_custom_field_modal(workspace, field_id.is_some(), theme, cx)
        }
        Dialog::DocumentMenu => render_document_menu_modal(workspace, theme, cx),
        Dialog::ConfirmDeleteCv { doc_id } => {
            render_confirm_delete_cv_modal(workspace, *doc_id, theme, cx)
        }
        Dialog::ConfirmResetApp => render_confirm_reset_app_modal(workspace, theme, cx),
        Dialog::RewriteBullet {
            section_id,
            bullet_id,
        } => render_rewrite_bullet_modal(workspace, *section_id, *bullet_id, theme, cx),
    };

    Some(
        div()
            .id("modal-overlay-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000b0))
            .on_scroll_wheel(|_, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_dialog();
                    cx.notify();
                }),
            )
            .child(content),
    )
}

fn modal_frame(
    workspace: &WorkspaceView,
    title: &str,
    description: Option<&str>,
    wide: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
    body: Div,
) -> Div {
    let target_w = if wide { 660.0 } else { 500.0 };
    let modal_w = (workspace.viewport_width - 32.0).clamp(320.0, target_w);
    let modal_max_h = (workspace.viewport_height - 48.0).clamp(300.0, 700.0);

    div()
        .w(px(modal_w))
        .max_h(px(modal_max_h))
        .flex()
        .flex_col()
        .rounded(px(8.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .shadow_lg()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        // Header
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(12.0))
                .p(px(18.0))
                .border_b_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(17.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child(title.to_string()),
                        )
                        .when_some(description, |el, desc| {
                            el.child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme.secondary)
                                    .child(desc.to_string()),
                            )
                        }),
                )
                .child(render_icon_button(
                    "close-modal-btn",
                    IconName::Close,
                    "Close dialog",
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                )),
        )
        // Body
        .child(
            div()
                .id("modal-body-scroll")
                .flex_1()
                .w_full()
                .min_h_0()
                .min_w_0()
                .p(px(18.0))
                .overflow_y_scroll()
                .child(body),
        )
}

fn render_new_master_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Master CV name"),
                )
                .child(
                    div()
                        .h(px(36.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.master_title_input.clone()),
                ),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .gap(px(10.0))
                .p(px(12.0))
                .rounded(px(5.0))
                .bg(theme.input)
                .child(div().flex_shrink_0().pt(px(1.5)).child(render_icon(IconName::Shield, 16.0, theme.accent)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.0))
                        .line_height(px(17.0))
                        .text_color(theme.secondary)
                        .child("AI can tailor copies, but it cannot overwrite your master."),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-master-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "submit-master-btn",
                    "Create master CV",
                    Some(IconName::Plus),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(move |this, _, _, cx| {
                        this.create_master_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Create a master CV",
        Some("Build a complete career record. Tailored variations will branch from this source."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_new_variation_modal(
    workspace: &WorkspaceView,
    root_id: DocumentId,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let roots: Vec<&CvDocument> = workspace
        .documents
        .iter()
        .filter(|d| d.parent_id.is_none())
        .collect();
    let skills = workspace.active_skills();
    let ai_configured = workspace.is_ai_configured();
    let active_provider = workspace.active_provider;
    let provider_name = workspace.active_provider_name();
    let selected_model = workspace.active_model_id().to_string();
    let active_model_label = workspace.active_model_display_name();
    let available_models = workspace.available_models.clone();
    let models_loading = workspace.models_loading;

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        // Source Master selector pills
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Source master CV"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.0))
                        .children(roots.iter().map(|root| {
                            let rid = root.id;
                            let is_sel = root.id == root_id;
                            div()
                                .id(el_id(format!("wizard-root-{}", root.id)))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded(px(5.0))
                                .bg(if is_sel {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .border_1()
                                .border_color(if is_sel {
                                    theme.accent
                                } else {
                                    theme.border
                                })
                                .text_size(px(12.5))
                                .font_weight(if is_sel {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_sel {
                                    theme.accent_text
                                } else {
                                    theme.text
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_variation_root(rid);
                                    cx.notify();
                                }))
                                .child(render_icon(
                                    IconName::Shield,
                                    12.0,
                                    if is_sel { theme.accent } else { theme.muted },
                                ))
                                .child(root.title.clone())
                        })),
                ),
        )
        // Company & Role pair
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Company"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.variation_company_input.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Target role"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.variation_role_input.clone()),
                        ),
                ),
        )
        // Job description
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Job description"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .h(px(120.0))
                        .p(px(8.0))
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.variation_jd_input.clone()),
                ),
        )
        // Skills checkboxes
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Skills to apply"),
                )
                .when(skills.is_empty(), |col| {
                    col.child(
                        div()
                            .p(px(8.0))
                            .rounded(px(4.0))
                            .bg(theme.input)
                            .text_size(px(11.5))
                            .text_color(theme.muted)
                            .child("No AI skills installed. Add or restore skills in the AI skills tab."),
                    )
                })
                .when(!skills.is_empty(), |col| {
                    col.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.0))
                            .children(skills.iter().map(|skill| {
                            let skill_id = skill.id.clone();
                            let is_selected = workspace.variation_skills.contains(&skill.id);
                            div()
                                .id(el_id(format!("wizard-skill-{}", skill.id)))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(8.0))
                                .py(px(4.0))
                                .rounded(px(4.0))
                                .bg(if is_selected {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .border_1()
                                .border_color(if is_selected {
                                    theme.accent
                                } else {
                                    theme.border_soft
                                })
                                .text_size(px(12.0))
                                .text_color(if is_selected {
                                    theme.accent_text
                                } else {
                                    theme.secondary
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_variation_skill(&skill_id);
                                    cx.notify();
                                }))
                                .child(render_icon(
                                    if is_selected {
                                        IconName::Check
                                    } else {
                                        IconName::Sparkles
                                    },
                                    11.0,
                                    if is_selected {
                                        theme.accent
                                    } else {
                                        theme.muted
                                    },
                                ))
                                .child(skill.name.clone())
                            })),
                    )
                }),
        )
        // ChatGPT Model Selector (when signed in with ChatGPT plan usage enabled)
        .when(ai_configured, |el| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.secondary)
                                            .child(match active_provider {
                                                AiProviderKind::ChatGpt => "ChatGPT model",
                                                AiProviderKind::GeminiApi => "Gemini model",
                                            }),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded(px(4.0))
                                            .bg(theme.input)
                                            .border_1()
                                            .border_color(theme.border_soft)
                                            .text_size(px(10.5))
                                            .text_color(theme.muted)
                                            .child(format!("Using {provider_name}")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.muted)
                                            .child(if models_loading && available_models.is_empty() {
                                                "Loading available models…".to_string()
                                            } else {
                                                selected_model.clone()
                                            }),
                                    )
                                    .when(active_provider == AiProviderKind::ChatGpt, |row| {
                                        row.child(render_button(
                                            "wizard-manage-usage-btn",
                                            "Manage usage",
                                            None,
                                            ButtonVariant::Ghost,
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.open_chatgpt_usage_settings();
                                                cx.notify();
                                            }),
                                        ))
                                    }),
                            ),
                    )
                    .when(!available_models.is_empty(), |col| {
                        col.child(
                            div()
                                .id("wizard-models-list")
                                .flex()
                                .flex_wrap()
                                .gap(px(6.0))
                                .max_h(px(88.0))
                                .overflow_y_scroll()
                                .children(available_models.iter().map(|model| {
                                    let model_id = model.id.clone();
                                    let is_selected = model.id == selected_model;
                                    div()
                                        .id(el_id(format!("wizard-model-{}", model.id)))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(5.0))
                                        .px(px(8.0))
                                        .py(px(4.0))
                                        .rounded(px(4.0))
                                        .bg(if is_selected {
                                            theme.accent_soft
                                        } else {
                                            theme.input
                                        })
                                        .border_1()
                                        .border_color(if is_selected {
                                            theme.accent
                                        } else {
                                            theme.border_soft
                                        })
                                        .text_size(px(11.5))
                                        .font_weight(if is_selected {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if is_selected {
                                            theme.accent_text
                                        } else {
                                            theme.secondary
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.select_model(&model_id, false, cx);
                                            cx.notify();
                                        }))
                                        .when(is_selected, |pill| {
                                            pill.child(render_icon(
                                                IconName::Check,
                                                11.0,
                                                theme.accent,
                                            ))
                                        })
                                        .child(model.display_name.clone())
                                })),
                        )
                    }),
            )
        })
        // Note
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .gap(px(8.0))
                .p(px(10.0))
                .rounded(px(5.0))
                .bg(theme.input)
                .child(
                    div()
                        .flex_shrink_0()
                        .pt(px(1.5))
                        .child(render_icon(
                            if ai_configured {
                                IconName::Sparkles
                            } else {
                                IconName::Lock
                            },
                            15.0,
                            theme.accent,
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .text_color(theme.secondary)
                        .child(if ai_configured {
                            format!(
                                "Branches an independent variation and streams zero-trust proposals from {provider_name} ({active_model_label}) using your selected skills."
                            )
                        } else {
                            "No AI provider is configured yet — this will branch a manual copy. Connect ChatGPT or Google Gemini API in Settings anytime to run AI tailoring.".to_string()
                        }),
                ),
        )
        // Buttons
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(6.0))
                .child(render_button(
                    "cancel-var-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "submit-var-btn",
                    if ai_configured {
                        "Create & tailor with AI"
                    } else {
                        "Create variation"
                    },
                    Some(if ai_configured {
                        IconName::Sparkles
                    } else {
                        IconName::Branch
                    }),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(move |this, _, _, cx| {
                        this.create_variation_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "A CV for your next opportunity",
        Some("Start from a master. Make it relevant. Keep the original intact."),
        true,
        theme,
        cx,
        body,
    )
}

fn render_compare_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let Some(doc) = workspace.active_document() else {
        return modal_frame(
            workspace,
            "Compare with master",
            None,
            false,
            theme,
            cx,
            render_empty_state(
                IconName::Branch,
                "No CV selected",
                "Select a variation from CV library to compare it with its master.",
                theme,
            ),
        );
    };
    let changes = workspace.calculate_lineage_changes();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .when(changes.is_empty(), |el| {
            el.child(render_empty_state(
                IconName::Branch,
                "No differences yet",
                "This variation currently matches its source. Edits will appear here as you make them.",
                theme,
            ))
        })
        .when(!changes.is_empty(), |el| {
            let mut list = div().flex().flex_col().gap(px(12.0));
            for change in &changes {
                let (badge_label, badge_bg, badge_fg) = match change.change_kind {
                    ChangeKind::Added => ("ADDED", theme.add_bg, theme.add_text),
                    ChangeKind::Removed => ("REMOVED", theme.del_bg, theme.del_text),
                    ChangeKind::Modified => ("MODIFIED", theme.accent_soft, theme.accent_text),
                    ChangeKind::Disabled => ("HIDDEN", theme.del_bg, theme.del_text),
                    ChangeKind::Enabled => ("SHOWN", theme.add_bg, theme.add_text),
                    ChangeKind::Reordered => ("REORDERED", theme.input, theme.secondary),
                };

                let mut card = div()
                    .flex()
                    .flex_col()
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(theme.input)
                    .border_1()
                    .border_color(theme.border_soft)
                    .gap(px(8.0))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .child(change.location.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(6.0))
                                    .py(px(1.5))
                                    .rounded(px(4.0))
                                    .bg(badge_bg)
                                    .text_color(badge_fg)
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child(badge_label),
                            ),
                    );

                if !change.text_diff.is_empty() {
                    let mut combined = String::new();
                    let mut highlights = Vec::new();
                    for span in &change.text_diff {
                        let start = combined.len();
                        combined.push_str(&span.text);
                        let end = combined.len();
                        if start < end {
                            match span.kind {
                                DiffSpanKind::Added => highlights.push((
                                    start..end,
                                    HighlightStyle {
                                        color: Some(theme.add_text.into()),
                                        background_color: Some(theme.add_bg.into()),
                                        font_weight: Some(FontWeight::MEDIUM),
                                        ..Default::default()
                                    },
                                )),
                                DiffSpanKind::Removed => highlights.push((
                                    start..end,
                                    HighlightStyle {
                                        color: Some(theme.del_text.into()),
                                        background_color: Some(theme.del_bg.into()),
                                        strikethrough: Some(StrikethroughStyle {
                                            thickness: px(1.0),
                                            color: Some(theme.del_text.into()),
                                        }),
                                        ..Default::default()
                                    },
                                )),
                                DiffSpanKind::Unchanged => {}
                            }
                        }
                    }
                    let styled = if highlights.is_empty() {
                        StyledText::new(combined)
                    } else {
                        StyledText::new(combined).with_highlights(highlights)
                    };
                    let spans_row = div()
                        .w_full()
                        .min_w_0()
                        .p(px(8.0))
                        .rounded(px(4.0))
                        .bg(theme.surface)
                        .text_size(px(12.0))
                        .line_height(px(18.0))
                        .text_color(theme.text)
                        .child(styled);
                    card = card.child(spans_row);
                } else if let Some(parent_t) = &change.parent_text {
                    card = card.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.secondary)
                            .child(parent_t.clone()),
                    );
                } else if let Some(child_t) = &change.child_text {
                    card = card.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.text)
                            .child(child_t.clone()),
                    );
                }

                list = list.child(card);
            }
            el.child(list)
        })
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .pt(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.5))
                        .text_color(theme.muted)
                        .child(format!(
                            "{} {} tracked via lineage IDs",
                            changes.len(),
                            if changes.len() == 1 {
                                "difference"
                            } else {
                                "differences"
                            }
                        )),
                )
                .child(render_button(
                    "done-compare-btn",
                    "Done",
                    None,
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Compare with master",
        Some(&format!(
            "Changes in {}. Your master record has not been overwritten.",
            doc.title
        )),
        true,
        theme,
        cx,
        body,
    )
}

fn render_payload_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let (payload_json, redaction_count) = workspace.build_payload_preview();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .p(px(8.0))
                .rounded(px(5.0))
                .bg(theme.input)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(div().flex_shrink_0().pt(px(1.5)).child(render_icon(IconName::Shield, 14.0, theme.accent)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.0))
                                .text_color(theme.secondary)
                                .child("Personal contact header excluded by Zero-Trust scrubber"),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .px(px(7.0))
                        .py(px(1.5))
                        .rounded(px(4.0))
                        .bg(theme.add_bg)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.add_text)
                        .child(format!(
                            "{redaction_count} PII {} redacted",
                            if redaction_count == 1 {
                                "token"
                            } else {
                                "tokens"
                            }
                        )),
                ),
        )
        .child(
            div()
                .id("payload-code-scroll")
                .w_full()
                .h(px((workspace.viewport_height - 270.0).clamp(160.0, 340.0)))
                .min_w_0()
                .p(px(12.0))
                .rounded(px(5.0))
                .bg(rgb(0x0a0a0a))
                .border_1()
                .border_color(theme.border)
                .overflow_y_scroll()
                .text_size(px(11.5))
                .font_family("Consolas, monospace")
                .text_color(rgb(0x7dcea1))
                .child(payload_json.clone()),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .pt(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.5))
                        .text_color(theme.muted)
                        .child("Exact outbound payload queued for AI processing."),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "copy-payload-btn",
                            "Copy JSON",
                            Some(IconName::Copy),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    payload_json.clone(),
                                ));
                                this.notify("Anonymized payload copied to clipboard.");
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "done-payload-btn",
                            "Done",
                            None,
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.close_dialog();
                                cx.notify();
                            }),
                        )),
                ),
        );

    modal_frame(
        workspace,
        "AI payload preview",
        Some("The anonymized content prepared from your current CV. Zero PII is transmitted."),
        true,
        theme,
        cx,
        body,
    )
}

fn render_export_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let Some(doc) = workspace.active_document() else {
        return modal_frame(
            workspace,
            "Export your CV",
            None,
            false,
            theme,
            cx,
            render_empty_state(
                IconName::File,
                "No CV selected",
                "Select a CV from CV library before exporting to PDF.",
                theme,
            ),
        );
    };

    let body = div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(14.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(40.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .child(render_icon(IconName::File, 22.0, theme.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child(doc.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.secondary)
                                .child("A4 · Classic academic serif · Typst engine"),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(12.5))
                .line_height(px(18.0))
                .text_color(theme.secondary)
                .child("Compiles your CV using the embedded Typst compiler. Select where to save your high-resolution PDF document."),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-export-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "confirm-export-btn",
                    if workspace.is_exporting_pdf {
                        "Exporting…"
                    } else {
                        "Choose destination & save PDF"
                    },
                    Some(IconName::Download),
                    ButtonVariant::Primary,
                    workspace.is_exporting_pdf,
                    theme,
                    cx.listener(|this, _, window, cx| {
                        this.execute_export_pdf(window, cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Export your CV",
        Some("A clean, classic serif document compiled directly to PDF."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_promote_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let Some(doc) = workspace.active_document() else {
        return modal_frame(
            workspace,
            "Promote to Master CV",
            None,
            false,
            theme,
            cx,
            render_empty_state(
                IconName::Shield,
                "No CV selected",
                "Select a variation from CV library first.",
                theme,
            ),
        );
    };
    let parent = doc.parent_id.and_then(|pid| workspace.find_document(pid));
    let promote_target = workspace.describe_promote_target();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .child(div().flex_shrink_0().child(render_icon(IconName::Shield, 20.0, theme.accent)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted)
                                .child("DESTINATION MASTER"),
                        )
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child(
                                    parent
                                        .map(|p| p.title.clone())
                                        .unwrap_or_else(|| "Master CV".to_string()),
                                ),
                        ),
                ),
        )
        .when_some(promote_target, |el, desc| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .p(px(12.0))
                    .rounded(px(5.0))
                    .bg(theme.surface_soft)
                    .border_1()
                    .border_color(theme.border_soft)
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted)
                            .child("ITEM TO PROMOTE"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(18.0))
                            .text_color(theme.text)
                            .child(desc),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-promote-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "confirm-promote-btn",
                    "Promote item",
                    Some(IconName::Up),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.execute_promote(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Promote to master CV?",
        Some(
            "Only the selected item will be copied or updated in your master career record. Other variation changes stay separate.",
        ),
        false,
        theme,
        cx,
        body,
    )
}

fn render_edit_summary_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Summary paragraph"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .h(px(180.0))
                        .p(px(10.0))
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.summary_text_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(8.0))
                .child(render_button(
                    "cancel-summary-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "save-summary-btn",
                    "Save summary",
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_summary_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Edit summary",
        Some("Keep your opening summary concise, factual, and tailored to your strengths."),
        true,
        theme,
        cx,
        body,
    )
}

fn render_rename_section_modal(
    workspace: &WorkspaceView,
    section_id: ItemId,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Section title"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.rename_section_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(10.0))
                .child(render_button(
                    "del-sec-modal-btn",
                    "Delete section",
                    Some(IconName::Trash),
                    ButtonVariant::Ghost,
                    false,
                    theme,
                    cx.listener(move |this, _, _, cx| {
                        this.delete_section(section_id, cx);
                        cx.notify();
                    }),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "cancel-rename-sec-btn",
                            "Cancel",
                            None,
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.close_dialog();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "save-rename-sec-btn",
                            "Save title",
                            Some(IconName::Check),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.save_rename_section(cx);
                                cx.notify();
                            }),
                        )),
                ),
        );

    modal_frame(
        workspace,
        "Rename section",
        Some("Rendered uppercase as the section heading in your PDF."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_edit_entry_modal(
    workspace: &WorkspaceView,
    is_new: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Organization"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.entry_org_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Role or qualification"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.entry_role_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Start date"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.entry_start_input.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .when(workspace.entry_is_current, |d| d.opacity(0.45))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child(if workspace.entry_is_current {
                                    "End date (ignored while current)"
                                } else {
                                    "End date"
                                }),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.entry_end_input.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .p(px(8.0))
                .rounded(px(5.0))
                .bg(theme.input)
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme.text)
                        .child("Current role (renders '(Currently)' in PDF)"),
                )
                .child(render_toggle(
                    "entry-is-current-toggle",
                    workspace.entry_is_current,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.entry_is_current = !this.entry_is_current;
                        cx.notify();
                    }),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(10.0))
                .child(div().when(!is_new, |el| {
                    el.child(render_button(
                        "del-entry-btn",
                        "Delete entry",
                        Some(IconName::Trash),
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.delete_current_entry(cx);
                            cx.notify();
                        }),
                    ))
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "cancel-entry-btn",
                            "Cancel",
                            None,
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.close_dialog();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "save-entry-btn",
                            "Save entry",
                            Some(IconName::Check),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.save_entry_from_dialog(cx);
                                cx.notify();
                            }),
                        )),
                ),
        );

    modal_frame(
        workspace,
        if is_new { "Add entry" } else { "Edit entry" },
        Some("Keep your career history factual and structured."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_edit_bullet_modal(
    workspace: &WorkspaceView,
    is_new: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Achievement"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .h(px(140.0))
                        .p(px(8.0))
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.bullet_text_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(10.0))
                .child(div().when(!is_new, |el| {
                    el.child(render_button(
                        "del-bullet-btn",
                        "Delete",
                        Some(IconName::Trash),
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.delete_current_bullet(cx);
                            cx.notify();
                        }),
                    ))
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "cancel-bullet-btn",
                            "Cancel",
                            None,
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.close_dialog();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "save-bullet-btn",
                            "Save achievement",
                            Some(IconName::Check),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.save_bullet_from_dialog(cx);
                                cx.notify();
                            }),
                        )),
                ),
        );

    modal_frame(
        workspace,
        if is_new {
            "Add achievement"
        } else {
            "Edit achievement"
        },
        Some("Describe what you did and the measurable impact it had."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_edit_category_modal(
    workspace: &WorkspaceView,
    is_new: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let separators = [
        (", ", "Comma (, )"),
        (" · ", "Dot ( · )"),
        (" - ", "Dash ( - )"),
    ];
    let current_sep = workspace.cat_separator.clone();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Category name"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.cat_label_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Skills (comma or newline separated)"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .h(px(96.0))
                        .p(px(8.0))
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.cat_items_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("PDF separator style"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.0))
                        .children(separators.iter().map(|&(sep, label)| {
                            let is_sel = current_sep == sep;
                            div()
                                .id(el_id(format!("cat-sep-{label}")))
                                .cursor_pointer()
                                .px(px(10.0))
                                .py(px(5.0))
                                .rounded(px(4.0))
                                .bg(if is_sel {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .border_1()
                                .border_color(if is_sel {
                                    theme.accent
                                } else {
                                    theme.border_soft
                                })
                                .text_size(px(12.0))
                                .text_color(if is_sel {
                                    theme.accent_text
                                } else {
                                    theme.secondary
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.cat_separator = sep.to_string();
                                    cx.notify();
                                }))
                                .child(label)
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(10.0))
                .child(div().when(!is_new, |el| {
                    el.child(render_button(
                        "del-cat-btn",
                        "Delete category",
                        Some(IconName::Trash),
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.delete_current_category(cx);
                            cx.notify();
                        }),
                    ))
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "cancel-cat-btn",
                            "Cancel",
                            None,
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.close_dialog();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "save-cat-btn",
                            "Save category",
                            Some(IconName::Check),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.save_category_from_dialog(cx);
                                cx.notify();
                            }),
                        )),
                ),
        );

    modal_frame(
        workspace,
        if is_new {
            "Add skill category"
        } else {
            "Edit skill category"
        },
        None,
        false,
        theme,
        cx,
        body,
    )
}

fn render_add_section_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let types = [
        "Timeline entries",
        "Paragraph summary",
        "Categorized skills",
        "Bullet list",
    ];
    let current_type = workspace.sec_type_selection.clone();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Section title"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.sec_title_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Content structure"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.0))
                        .children(types.iter().map(|&t| {
                            let is_sel = current_type == t;
                            div()
                                .id(el_id(format!("sec-type-{}", t)))
                                .cursor_pointer()
                                .px(px(10.0))
                                .py(px(5.0))
                                .rounded(px(4.0))
                                .bg(if is_sel {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .border_1()
                                .border_color(if is_sel {
                                    theme.accent
                                } else {
                                    theme.border_soft
                                })
                                .text_size(px(12.0))
                                .text_color(if is_sel {
                                    theme.accent_text
                                } else {
                                    theme.secondary
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.sec_type_selection = t.to_string();
                                    cx.notify();
                                }))
                                .child(t)
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-sec-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "confirm-sec-btn",
                    "Add section",
                    Some(IconName::Plus),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.execute_add_section(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Add a section",
        Some("Choose a title and structure for this part of your CV."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_rename_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("CV name"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.rename_title_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-rename-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "confirm-rename-btn",
                    "Save name",
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_rename(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(workspace, "Rename CV", None, false, theme, cx, body)
}

fn render_edit_skill_modal(
    workspace: &WorkspaceView,
    is_new: bool,
    read_only: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .when(read_only, |el| {
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .p(px(10.0))
                    .rounded(px(5.0))
                    .bg(theme.input)
                    .border_1()
                    .border_color(theme.border_soft)
                    .child(div().flex_shrink_0().pt(px(1.5)).child(render_icon(IconName::Shield, 14.0, theme.accent)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.0))
                            .line_height(px(17.0))
                            .text_color(theme.secondary)
                            .child("Bundled skill (read-only). Scroll or select text below to inspect directives, or create a custom user skill."),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Skill name"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.skill_name_input.clone()),
                        ),
                )
                .child(
                    div()
                        .w(px(160.0))
                        .flex_shrink_0()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Category"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.skill_category_input.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Description"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.skill_desc_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("System prompt instructions (SKILL.md body)"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .h(px(220.0))
                        .p(px(8.0))
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.skill_inst_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-skill-btn",
                    if read_only { "Close" } else { "Cancel" },
                    None,
                    if read_only {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    },
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .when(!read_only, |el| {
                    el.child(render_button(
                        "save-skill-btn",
                        if is_new {
                            "Save SKILL.md"
                        } else {
                            "Save instructions"
                        },
                        Some(IconName::Check),
                        ButtonVariant::Primary,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.save_skill_from_dialog(cx);
                            cx.notify();
                        }),
                    ))
                }),
        );

    modal_frame(
        workspace,
        if is_new {
            "Add an AI skill"
        } else if read_only {
            "Bundled skill instructions"
        } else {
            "Edit skill instructions"
        },
        Some(if read_only {
            "Bundled system directives injected when this skill is enabled."
        } else {
            "Instructions are saved as SKILL.md and injected during AI tailoring passes."
        }),
        true,
        theme,
        cx,
        body,
    )
}

fn render_edit_contact_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Full name"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.pii_name_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Professional headline"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.pii_headline_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Email address"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.pii_email_input.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Phone number"),
                        )
                        .child(
                            div()
                                .h(px(34.0))
                                .px(px(10.0))
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(workspace.pii_phone_input.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Location"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.pii_location_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-contact-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "save-contact-btn",
                    "Save contact details",
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_contact_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Contact details",
        Some("Combined only with your local document."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_edit_social_modal(
    workspace: &WorkspaceView,
    is_edit: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Platform"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.social_platform_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Display label (shown in PDF)"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.social_display_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Target URL"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.social_url_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-social-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "save-social-btn",
                    if is_edit { "Save link" } else { "Add link" },
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_social_link_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        if is_edit {
            "Edit social link"
        } else {
            "Add social or portfolio link"
        },
        Some("Rendered as a clickable hyperlink in your local PDF header. Never sent to AI."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_edit_custom_field_modal(
    workspace: &WorkspaceView,
    is_edit: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Field label"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.custom_field_label_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Value"),
                )
                .child(
                    div()
                        .h(px(34.0))
                        .px(px(10.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.custom_field_value_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-field-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "save-field-btn",
                    if is_edit { "Save field" } else { "Add field" },
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_custom_field_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        if is_edit {
            "Edit custom header field"
        } else {
            "Add custom header field"
        },
        Some("Displayed in your local PDF header and scrubbed from AI payloads."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_document_menu_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let masters: Vec<&CvDocument> = workspace
        .documents
        .iter()
        .filter(|d| d.parent_id.is_none())
        .collect();
    let variations: Vec<&CvDocument> = workspace
        .documents
        .iter()
        .filter(|d| d.parent_id.is_some())
        .collect();

    let mut list = div().flex().flex_col().gap(px(8.0));

    if masters.is_empty() && variations.is_empty() {
        list = list
            .child(render_empty_state(
                IconName::File,
                "No CVs in your workspace yet",
                "Create a master CV to get started.",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .justify_center()
                    .child(render_button(
                        "doc-menu-new-master-btn",
                        "New master CV",
                        Some(IconName::Plus),
                        ButtonVariant::Primary,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.open_new_master_dialog(cx);
                            cx.notify();
                        }),
                    )),
            );
    }

    for master in masters {
        let master_id = master.id;
        let is_current = Some(master.id) == workspace.active_doc_id;

        list = list.child(
            div()
                .id(el_id(format!("select-master-{}", master.id)))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .p(px(10.0))
                .rounded(px(5.0))
                .bg(if is_current { theme.hover } else { theme.input })
                .border_1()
                .border_color(if is_current {
                    theme.accent
                } else {
                    theme.border_soft
                })
                .hover(|s| s.bg(theme.hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_document(master_id, cx);
                    this.close_dialog();
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .overflow_hidden()
                        .child(div().flex_shrink_0().child(render_icon(IconName::Shield, 14.0, theme.accent)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(master.title.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.accent)
                        .child("MASTER"),
                ),
        );

        let master_variations: Vec<&CvDocument> = variations
            .iter()
            .filter(|v| v.parent_id == Some(master.id))
            .cloned()
            .collect();
        for var in master_variations {
            let var_id = var.id;
            let is_var_current = Some(var.id) == workspace.active_doc_id;

            list = list.child(
                div()
                    .id(el_id(format!("select-var-{}", var.id)))
                    .cursor_pointer()
                    .ml(px(16.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .p(px(8.0))
                    .rounded(px(5.0))
                    .bg(if is_var_current {
                        theme.hover
                    } else {
                        theme.input
                    })
                    .border_1()
                    .border_color(if is_var_current {
                        theme.accent
                    } else {
                        theme.border_soft
                    })
                    .hover(|s| s.bg(theme.hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_document(var_id, cx);
                        this.close_dialog();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .overflow_hidden()
                            .child(div().flex_shrink_0().child(render_icon(IconName::Branch, 12.0, theme.secondary)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.5))
                                    .text_color(theme.text)
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(var.title.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(10.0))
                            .text_color(theme.muted)
                            .child("VARIATION"),
                    ),
            );
        }
    }

    modal_frame(
        workspace,
        "Switch document",
        Some("Choose a document to edit or preview."),
        false,
        theme,
        cx,
        list,
    )
}
fn render_confirm_delete_cv_modal(
    workspace: &WorkspaceView,
    doc_id: DocumentId,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let Some(doc) = workspace.find_document(doc_id) else {
        return modal_frame(
            workspace,
            "Delete CV",
            None,
            false,
            theme,
            cx,
            render_empty_state(
                IconName::Trash,
                "CV not found",
                "This CV may have already been removed.",
                theme,
            ),
        );
    };

    let is_variation = doc.parent_id.is_some();
    let child_count = workspace
        .documents
        .iter()
        .filter(|d| d.parent_id == Some(doc_id))
        .count();

    let warning_text = if is_variation {
        "This tailored variation and its local snapshots will be permanently deleted. Your Master CV will remain untouched."
            .to_string()
    } else if child_count > 0 {
        format!(
            "This Master CV has {child_count} linked {}. Deleting it will permanently remove the master, all linked {}, and their recovery snapshots from local SQLite.",
            if child_count == 1 {
                "variation"
            } else {
                "variations"
            },
            if child_count == 1 {
                "variation"
            } else {
                "variations"
            }
        )
    } else {
        "This Master CV and its recovery snapshots will be permanently deleted from your local SQLite workspace."
            .to_string()
    };

    let confirm_label = if child_count > 0 {
        format!(
            "Delete master & {child_count} {}",
            if child_count == 1 {
                "variation"
            } else {
                "variations"
            }
        )
    } else if is_variation {
        "Delete variation".to_string()
    } else {
        "Delete master CV".to_string()
    };

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(36.0))
                        .rounded(px(6.0))
                        .bg(theme.del_bg)
                        .child(render_icon(IconName::Trash, 17.0, theme.del_text)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child(doc.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme.secondary)
                                .child(if is_variation {
                                    "Tailored variation".to_string()
                                } else {
                                    format!(
                                        "Master CV · {} {} · {} {}",
                                        doc.body.sections.len(),
                                        if doc.body.sections.len() == 1 {
                                            "section"
                                        } else {
                                            "sections"
                                        },
                                        child_count,
                                        if child_count == 1 {
                                            "variation"
                                        } else {
                                            "variations"
                                        }
                                    )
                                }),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(12.5))
                .line_height(px(18.5))
                .text_color(theme.secondary)
                .child(warning_text),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(8.0))
                .child(render_button(
                    "cancel-delete-cv-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "confirm-delete-cv-btn",
                    confirm_label,
                    Some(IconName::Trash),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(move |this, _, _, cx| {
                        this.delete_document(doc_id, cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        if is_variation {
            "Delete variation?"
        } else {
            "Delete master CV?"
        },
        Some("This action cannot be undone."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_confirm_reset_app_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let doc_count = workspace.documents.len();
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.del_bg)
                .border_1()
                .border_color(theme.del_text)
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(36.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .child(render_icon(IconName::Trash, 17.0, theme.del_text)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.del_text)
                                .child("Everything in VanguardCV will be permanently deleted"),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(17.5))
                                .text_color(theme.text)
                                .child(format!(
                                    "This will delete all {} saved {}, variations, recovery snapshots, saved job search keywords, workspace settings, custom AI skills, and your ChatGPT sign-in & Gemini API key.",
                                    doc_count,
                                    if doc_count == 1 { "CV" } else { "CVs" }
                                )),
                        ),
                ),
        )
        .child(
            div()
                .text_size(px(12.0))
                .line_height(px(18.0))
                .text_color(theme.secondary)
                .child("Only the bundled default AI skills will be restored and kept. Your CV library will start completely empty. Export a .vanguard archive first if you want a backup."),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(6.0))
                .child(render_button(
                    "cancel-reset-app-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "confirm-reset-app-btn",
                    "Yes, reset everything",
                    Some(IconName::Trash),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.reset_application_all(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Reset application?",
        Some("This factory reset cannot be undone."),
        false,
        theme,
        cx,
        body,
    )
}

fn render_rewrite_bullet_modal(
    workspace: &WorkspaceView,
    section_id: ItemId,
    bullet_id: ItemId,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let bullet_text = workspace
        .find_bullet_text(section_id, bullet_id)
        .unwrap_or_else(|| "(Achievement not found)".to_string());
    let enabled_skills: Vec<_> = workspace
        .active_skills()
        .into_iter()
        .filter(|s| s.enabled)
        .collect();
    let selected_count = workspace.rewrite_selected_skills.len();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        // Bullet preview
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.0))
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted)
                        .child("ACHIEVEMENT TO REWRITE"),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(18.0))
                        .text_color(theme.text)
                        .child(bullet_text),
                ),
        )
        // Per-bullet skill picker
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Skills to apply"),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.muted)
                                .child(if selected_count == 0 {
                                    "None selected · default prompt".to_string()
                                } else {
                                    format!(
                                        "{selected_count} {} active",
                                        if selected_count == 1 {
                                            "skill"
                                        } else {
                                            "skills"
                                        }
                                    )
                                }),
                        ),
                )
                .when(enabled_skills.is_empty(), |col| {
                    col.child(
                        div()
                            .p(px(10.0))
                            .rounded(px(5.0))
                            .bg(theme.input)
                            .text_size(px(11.5))
                            .text_color(theme.muted)
                            .child("No enabled AI skills found. Enable or create skills in the AI skills tab, or run with the default rewrite prompt."),
                    )
                })
                .when(!enabled_skills.is_empty(), |col| {
                    col.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.0))
                            .children(enabled_skills.iter().map(|skill| {
                                let skill_id = skill.id.clone();
                                let is_selected =
                                    workspace.rewrite_selected_skills.contains(&skill.id);
                                div()
                                    .id(el_id(format!("rewrite-skill-{}", skill.id)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .px(px(9.0))
                                    .py(px(5.0))
                                    .rounded(px(4.0))
                                    .bg(if is_selected {
                                        theme.accent_soft
                                    } else {
                                        theme.input
                                    })
                                    .border_1()
                                    .border_color(if is_selected {
                                        theme.accent
                                    } else {
                                        theme.border_soft
                                    })
                                    .text_size(px(12.0))
                                    .text_color(if is_selected {
                                        theme.accent_text
                                    } else {
                                        theme.secondary
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_rewrite_skill(&skill_id);
                                        cx.notify();
                                    }))
                                    .child(render_icon(
                                        if is_selected {
                                            IconName::Check
                                        } else {
                                            IconName::Sparkles
                                        },
                                        11.0,
                                        if is_selected {
                                            theme.accent
                                        } else {
                                            theme.muted
                                        },
                                    ))
                                    .child(skill.name.clone())
                            })),
                    )
                }),
        )
        // Optional custom instruction
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child("Custom instruction (optional)"),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(88.0))
                        .p(px(10.0))
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(workspace.rewrite_instruction_input.clone()),
                ),
        )
        // Privacy reminder
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(5.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(div().flex_shrink_0().child(render_icon(
                    IconName::Lock,
                    13.0,
                    theme.accent,
                )))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.5))
                        .text_color(theme.secondary)
                        .child("Zero-trust PII air-gap active · Suggested rewrite will appear inline for your approval."),
                ),
        )
        // Footer buttons
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(6.0))
                .child(render_button(
                    "cancel-rewrite-bullet-btn",
                    "Cancel",
                    None,
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                ))
                .child(render_button(
                    "submit-rewrite-bullet-btn",
                    "Rewrite with AI",
                    Some(IconName::Sparkles),
                    ButtonVariant::Primary,
                    workspace.ai_state.is_running,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.submit_bullet_rewrite_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Rewrite achievement with AI",
        Some("Choose which SKILL.md rules and optional guidance to apply to this bullet."),
        true,
        theme,
        cx,
        body,
    )
}
