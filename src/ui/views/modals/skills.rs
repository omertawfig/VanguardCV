use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::ui::components::ui::{
    ButtonVariant, IconName, render_button, render_icon, render_input_box,
    render_multiline_input_box,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::WorkspaceView;

use super::modal_frame;
pub(super) fn render_edit_skill_modal(
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
                    .rounded(px(6.0))
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
                            render_input_box(workspace.skill_name_input.clone(), 34.0, theme, cx),
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
                            render_input_box(workspace.skill_category_input.clone(), 34.0, theme, cx),
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
                    render_input_box(workspace.skill_desc_input.clone(), 34.0, theme, cx),
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
                    render_multiline_input_box(workspace.skill_inst_input.clone(), 220.0, theme, cx),
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

