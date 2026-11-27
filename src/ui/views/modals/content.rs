use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::ItemId;
use crate::ui::components::ui::{
    ButtonVariant, IconName, render_button, render_icon, render_input_box,
    render_multiline_input_box, render_selectable_pill, render_toggle,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::WorkspaceView;

use super::modal_frame;
pub(super) fn render_edit_summary_modal(
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
                    render_multiline_input_box(workspace.summary_text_input.clone(), 180.0, theme, cx),
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


pub(super) fn render_rename_section_modal(
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
                    render_input_box(workspace.rename_section_input.clone(), 34.0, theme, cx),
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


pub(super) fn render_edit_entry_modal(
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
                    render_input_box(workspace.entry_org_input.clone(), 34.0, theme, cx),
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
                    render_input_box(workspace.entry_role_input.clone(), 34.0, theme, cx),
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
                            render_input_box(workspace.entry_start_input.clone(), 34.0, theme, cx),
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
                            render_input_box(workspace.entry_end_input.clone(), 34.0, theme, cx),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .p(px(8.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .child(
                    div()
                        .text_size(px(13.0))
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


pub(super) fn render_edit_bullet_modal(
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
                    render_multiline_input_box(workspace.bullet_text_input.clone(), 140.0, theme, cx),
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


pub(super) fn render_edit_category_modal(
    workspace: &WorkspaceView,
    is_new: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let separators = [
        (", ", "Comma (, )"),
        (" Â· ", "Dot ( Â· )"),
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
                    render_input_box(workspace.cat_label_input.clone(), 34.0, theme, cx),
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
                    render_multiline_input_box(workspace.cat_items_input.clone(), 96.0, theme, cx),
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
                            render_selectable_pill(
                                format!("cat-sep-{label}"),
                                label,
                                None,
                                is_sel,
                                theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.cat_separator = sep.to_string();
                                    cx.notify();
                                }),
                            )
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


pub(super) fn render_add_section_modal(
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
                    render_input_box(workspace.sec_title_input.clone(), 34.0, theme, cx),
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
                            render_selectable_pill(
                                format!("sec-type-{t}"),
                                t,
                                None,
                                is_sel,
                                theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.sec_type_selection = t.to_string();
                                    cx.notify();
                                }),
                            )
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


pub(super) fn render_rewrite_bullet_modal(
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
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted)
                        .child("ACHIEVEMENT TO REWRITE"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
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
                                    "None selected Â· default prompt".to_string()
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
                            .rounded(px(6.0))
                            .bg(theme.input)
                            .text_size(px(12.0))
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
                                render_selectable_pill(
                                    format!("rewrite-skill-{}", skill.id),
                                    skill.name.clone(),
                                    Some(if is_selected {
                                        IconName::Check
                                    } else {
                                        IconName::Sparkles
                                    }),
                                    is_selected,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_rewrite_skill(&skill_id);
                                        cx.notify();
                                    }),
                                )
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
                    render_multiline_input_box(workspace.rewrite_instruction_input.clone(), 88.0, theme, cx),
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
                .rounded(px(6.0))
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
                        .text_size(px(12.0))
                        .text_color(theme.secondary)
                        .child("Zero-trust PII air-gap active Â· Suggested rewrite will appear inline for your approval."),
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
