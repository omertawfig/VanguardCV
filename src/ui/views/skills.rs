use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::AiProviderKind;
use crate::ui::components::ui::{
    ButtonVariant, IconName, render_button, render_icon, render_icon_button, render_toggle,
};
use crate::ui::workspace::{WorkspaceView, WorkspaceViewPage};

pub fn render_skills_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
    let theme = &workspace.theme;
    let active_skills = workspace.active_skills();
    let enabled_count = active_skills.iter().filter(|s| s.enabled).count();
    let total_count = active_skills.len();
    let ai_configured = workspace.is_ai_configured();
    let active_provider = workspace.active_provider;
    let ai_model = workspace.active_model_id().to_string();
    let has_deleted_bundled = workspace.services.skills.has_deleted_bundled_skills();
    let pad_x = workspace.page_padding_x();

    let (banner_title, banner_desc, banner_badge, banner_cta) = match (
        active_provider,
        ai_configured,
    ) {
        (AiProviderKind::ChatGpt, true) => (
            format!("Using ChatGPT plan ({ai_model})"),
            "Enabled skills below are automatically injected during tailored ChatGPT runs.",
            "CONNECTED",
            "AI settings",
        ),
        (AiProviderKind::ChatGpt, false) => (
            "AI provider setup required".to_string(),
            "Sign in with ChatGPT or add a Google Gemini API key in Settings to run live tailoring.",
            "SETUP REQUIRED",
            "Configure AI",
        ),
        (AiProviderKind::GeminiApi, true) => (
            format!("Using Google Gemini API ({ai_model})"),
            "Enabled skills below are automatically injected during tailored Gemini runs.",
            "CONNECTED",
            "AI settings",
        ),
        (AiProviderKind::GeminiApi, false) => (
            "Google Gemini API key required".to_string(),
            "Add your Gemini API key or switch to Sign in with ChatGPT in Settings to run live tailoring.",
            "SETUP REQUIRED",
            "Configure AI",
        ),
    };

    div()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .max_w(px(1120.0))
        .mx_auto()
        .px(pad_x)
        .pt(px(28.0))
        .pb(px(72.0))
        .gap(px(20.0))
        // Page heading
        .child(
            div()
                .flex()
                .w_full()
                .min_w_0()
                .when(workspace.content_width() < 680.0, |el| el.flex_col())
                .when(workspace.content_width() >= 680.0, |el| el.flex_row().items_start().justify_between())
                .gap(px(12.0))
                .child(
                    div()
                        .when(workspace.content_width() >= 680.0, |el| el.flex_1())
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(24.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("AI skills"),
                        )
                        .child(
                            div()
                                .text_size(px(14.0))
                                .text_color(theme.secondary)
                                .child(
                                    "Choose the instructions injected into the AI tailoring agent.",
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .when(has_deleted_bundled, |row| {
                            row.child(render_button(
                                "restore-default-skills-btn",
                                "Restore defaults",
                                None,
                                ButtonVariant::Ghost,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.restore_default_skills();
                                    cx.notify();
                                }),
                            ))
                        })
                        .child(render_button(
                            "reload-skills-btn",
                            "Reload",
                            None,
                            ButtonVariant::Ghost,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.reload_skills();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "import-skill-btn",
                            "Import (.md/.toml)",
                            Some(IconName::Folder),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, window, cx| {
                                this.import_skill_from_file(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "add-skill-top-btn",
                            "Add skill",
                            Some(IconName::Plus),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_add_skill_dialog(cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // AI Provider Status Banner
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.surface)
                .border_1()
                .border_color(if ai_configured {
                    theme.border
                } else {
                    theme.accent
                })
                .overflow_hidden()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex_shrink_0()
                                .pt(px(2.0))
                                .child(render_icon(
                                    IconName::Sparkles,
                                    18.0,
                                    if ai_configured {
                                        theme.add_text
                                    } else {
                                        theme.accent
                                    },
                                )),
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
                                        .flex()
                                        .items_center()
                                        .flex_wrap()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text)
                                                .child(banner_title),
                                        )
                                        .child(
                                            div()
                                                .px(px(6.0))
                                                .py(px(1.0))
                                                .rounded(px(4.0))
                                                .bg(if ai_configured {
                                                    theme.add_bg
                                                } else {
                                                    theme.accent_soft
                                                })
                                                .text_color(if ai_configured {
                                                    theme.add_text
                                                } else {
                                                    theme.accent_text
                                                })
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::BOLD)
                                                .child(banner_badge),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(theme.secondary)
                                        .child(banner_desc),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .child(render_button(
                            "skills-open-settings-btn",
                            banner_cta,
                            Some(IconName::Settings),
                            if ai_configured {
                                ButtonVariant::Ghost
                            } else {
                                ButtonVariant::Secondary
                            },
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.navigate_to(WorkspaceViewPage::Settings, cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Summary line
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted)
                .child("INSTALLED SKILLS")
                .child(format!("{enabled_count} of {total_count} enabled")),
        )
        // Skills list
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(px(12.0))
                .when(active_skills.is_empty(), |col| {
                    col.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap(px(12.0))
                            .p(px(18.0))
                            .rounded(px(6.0))
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(220.0))
                                    .text_size(px(13.0))
                                    .text_color(theme.secondary)
                                    .child("No AI skills installed. Add a custom skill, import one, or restore the bundled defaults."),
                            )
                            .when(has_deleted_bundled, |box_el| {
                                box_el.child(render_button(
                                    "empty-restore-skills-btn",
                                    "Restore defaults",
                                    None,
                                    ButtonVariant::Secondary,
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.restore_default_skills();
                                        cx.notify();
                                    }),
                                ))
                            }),
                    )
                })
                .children(active_skills.iter().map(|skill| {
                    let skill_id_toggle = skill.id.clone();
                    let skill_id_delete = skill.id.clone();
                    let skill_clone = skill.clone();
                    let is_user = workspace.is_user_owned_skill(skill);

                    let icon = match skill.id.as_str() {
                        "page-compressor" | "page_compressor" => IconName::File,
                        "xyz-bullet-enhancer" | "xyz_bullet_enhancer" => IconName::Edit,
                        "ats-classic-optimizer" | "ats_classic_optimizer" => IconName::Shield,
                        "keyword-aligner" | "keyword_aligner" => IconName::Search,
                        _ => IconName::Sparkles,
                    };

                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(14.0))
                        .p(px(16.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .when(!skill.enabled, |s| s.opacity(0.65))
                        // Left: Symbol and Content
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(220.0))
                                .flex()
                                .items_start()
                                .gap(px(14.0))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(40.0))
                                        .rounded(px(6.0))
                                        .bg(theme.input)
                                        .child(render_icon(icon, 20.0, theme.accent)),
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
                                                .flex()
                                                .items_center()
                                                .flex_wrap()
                                                .gap(px(8.0))
                                                .child(
                                                    div()
                                                        .text_size(px(14.0))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(theme.text)
                                                        .child(skill.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .px(px(6.0))
                                                        .py(px(1.0))
                                                        .rounded(px(4.0))
                                                        .bg(theme.input)
                                                        .text_size(px(11.0))
                                                        .text_color(theme.secondary)
                                                        .child(skill.category.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .px(px(6.0))
                                                        .py(px(1.0))
                                                        .rounded(px(4.0))
                                                        .bg(if is_user {
                                                            theme.accent_soft
                                                        } else {
                                                            theme.input
                                                        })
                                                        .text_size(px(11.0))
                                                        .text_color(if is_user {
                                                            theme.accent_text
                                                        } else {
                                                            theme.muted
                                                        })
                                                        .child(if is_user {
                                                            "User"
                                                        } else {
                                                            "Bundled"
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(theme.muted)
                                                        .child(format!("v{}", skill.version)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .text_color(theme.secondary)
                                                .child(
                                                    skill
                                                        .description
                                                        .clone()
                                                        .unwrap_or_default(),
                                                ),
                                        ),
                                ),
                        )
                        // Right: Actions & Toggle
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(render_button(
                                    format!("edit-skill-{}", skill.id),
                                    if is_user {
                                        "Edit instructions"
                                    } else {
                                        "View instructions"
                                    },
                                    Some(if is_user {
                                        IconName::Edit
                                    } else {
                                        IconName::File
                                    }),
                                    ButtonVariant::Ghost,
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.open_edit_skill_dialog(&skill_clone, cx);
                                        cx.notify();
                                    }),
                                ))
                                .child(render_icon_button(
                                    format!("del-skill-{}", skill.id),
                                    IconName::Trash,
                                    "Delete skill",
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.delete_skill(&skill_id_delete);
                                        cx.notify();
                                    }),
                                ))
                                .child(render_toggle(
                                    format!("toggle-skill-{}", skill.id),
                                    skill.enabled,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_skill_enabled(&skill_id_toggle);
                                        cx.notify();
                                    }),
                                )),
                        )
                })),
        )
        // Principles / How skills work card
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(12.0))
                .p(px(16.0))
                .rounded(px(6.0))
                .bg(theme.surface_soft)
                .border_1()
                .border_color(theme.border_soft)
                .overflow_hidden()
                .child(
                    div()
                        .flex_shrink_0()
                        .pt(px(2.0))
                        .child(render_icon(IconName::Shield, 16.0, theme.accent)),
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
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("How AI skills stay safe and verifiable"),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(18.0))
                                .text_color(theme.secondary)
                                .child(format!(
                                    "Skills are hot-reloaded from {}. Every skill runs strictly against scrubbed career content (zero PII), can only propose edits on tailored variations, and never modifies your Master CV.",
                                    workspace.services.skills.skills_dir().display()
                                )),
                        ),
                ),
        )
}
