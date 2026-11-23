use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::{AiProviderKind, ChatGptAuthStatus, ModelInfo};
use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_button, render_icon, render_icon_button, render_toggle,
};
use crate::ui::theme::{Theme, ThemeMode};
use crate::ui::workspace::WorkspaceView;
use crate::updater::{CURRENT_VERSION, UpdateStatus, format_download_size};

fn format_context_limit(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let millions = tokens as f64 / 1_000_000.0;
        if (millions - millions.round()).abs() < 0.08 {
            format!("{}M ctx", millions.round() as u64)
        } else {
            format!("{millions:.1}M ctx")
        }
    } else if tokens >= 1_000 {
        format!("{}k ctx", tokens / 1_000)
    } else {
        format!("{tokens} ctx")
    }
}

fn truncate_description(desc: &str, max_chars: usize) -> String {
    let trimmed = desc.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let prefix: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", prefix.trim_end())
}

pub fn render_settings_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
    let theme = &workspace.theme;
    let content_w = workspace.content_width().max(320.0);
    let pad_x = workspace.page_padding_x();
    let pad_x_f = f32::from(pad_x);
    let page_w = content_w.min(880.0);
    let card_w = (page_w - pad_x_f * 2.0).max(280.0);
    let inner_w = (card_w - 42.0).max(220.0);

    div()
        .w(px(content_w))
        .flex()
        .justify_center()
        .child(
            div()
                .w(px(page_w))
                .flex()
                .flex_col()
                .px(pad_x)
                .pt(px(28.0))
                .pb(px(84.0))
                .gap(px(20.0))
                // Page header
                .child(
                    div()
                        .w(px(card_w))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(26.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("Settings"),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .line_height(px(20.0))
                                .text_color(theme.secondary)
                                .child(
                                    "Appearance, AI providers (ChatGPT & Google Gemini), automatic updates, and local backups.",
                                ),
                        ),
                )
                .child(render_appearance_card(card_w, inner_w, theme, cx))
                .child(render_ai_engine_card(workspace, card_w, inner_w, theme, cx))
                .child(render_updates_card(workspace, card_w, inner_w, theme, cx))
                .child(render_data_card(workspace, card_w, inner_w, theme, cx)),
        )
}

fn render_section_header(
    inner_w: f32,
    icon: IconName,
    title: &str,
    subtitle: &str,
    theme: &Theme,
    trailing: Option<AnyElement>,
) -> Div {
    let stack = trailing.is_some() && inner_w < 540.0;
    let mut row = div()
        .w(px(inner_w))
        .flex()
        .gap(px(12.0))
        .when(stack, |el| el.flex_col().items_start())
        .when(!stack, |el| el.flex_row().items_start().justify_between())
        .child(
            div()
                .when(!stack, |el| el.flex_1().min_w_0())
                .when(stack, |el| el.w(px(inner_w)))
                .flex()
                .items_start()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_shrink_0()
                        .mt(px(1.0))
                        .size(px(32.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(render_icon(icon, 16.0, theme.accent)),
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
                                .text_size(px(15.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child(title.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(17.5))
                                .text_color(theme.muted)
                                .child(subtitle.to_string()),
                        ),
                ),
        );

    if let Some(right) = trailing {
        row = row.child(div().flex_shrink_0().child(right));
    }

    row
}

fn render_status_pill(label: &'static str, bg: Rgba, fg: Rgba, border: Rgba) -> Div {
    div()
        .flex_shrink_0()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(10.0))
        .font_weight(FontWeight::BOLD)
        .text_color(fg)
        .child(label)
}

fn render_privacy_callout(inner_w: f32, text: &'static str, theme: &Theme) -> Div {
    div()
        .w(px(inner_w))
        .flex()
        .items_start()
        .gap(px(10.0))
        .p(px(12.0))
        .rounded(px(6.0))
        .bg(theme.input)
        .border_1()
        .border_color(theme.border_soft)
        .child(div().flex_shrink_0().pt(px(1.5)).child(render_icon(
            IconName::Shield,
            15.0,
            theme.accent,
        )))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(11.5))
                .line_height(px(17.0))
                .text_color(theme.secondary)
                .child(text),
        )
}

fn render_appearance_card(
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let mode = theme.mode;
    let stack_cards = inner_w < 500.0;
    let option_w = if stack_cards {
        inner_w
    } else {
        ((inner_w - 12.0) / 2.0).floor().max(200.0)
    };

    div()
        .w(px(card_w))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .p(px(20.0))
        .rounded(px(8.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .gap(px(16.0))
        .child(render_section_header(
            inner_w,
            if mode == ThemeMode::Dark {
                IconName::Moon
            } else {
                IconName::Sun
            },
            "Appearance",
            "Choose the environment that feels comfortable to work in.",
            theme,
            None,
        ))
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_cards, |el| el.flex_col())
                .when(!stack_cards, |el| el.flex_row())
                .gap(px(12.0))
                .child(render_theme_option_card(
                    "theme-dark-btn",
                    "Dark",
                    "Carbon surfaces, focused contrast",
                    IconName::Moon,
                    mode == ThemeMode::Dark,
                    option_w,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.set_theme_mode(ThemeMode::Dark);
                        cx.notify();
                    }),
                ))
                .child(render_theme_option_card(
                    "theme-light-btn",
                    "Light",
                    "Clean surfaces, balanced contrast",
                    IconName::Sun,
                    mode == ThemeMode::Light,
                    option_w,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.set_theme_mode(ThemeMode::Light);
                        cx.notify();
                    }),
                )),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_theme_option_card(
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    icon: IconName,
    selected: bool,
    width: f32,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(width))
        .flex_shrink_0()
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .p(px(14.0))
        .rounded(px(6.0))
        .bg(if selected { theme.hover } else { theme.input })
        .border_1()
        .border_color(if selected {
            theme.accent
        } else {
            theme.border_soft
        })
        .hover(|s| s.bg(theme.hover))
        .on_click(on_click)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_shrink_0()
                        .size(px(34.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(render_icon(
                            icon,
                            18.0,
                            if selected { theme.accent } else { theme.muted },
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(16.0))
                                .text_color(theme.muted)
                                .child(subtitle),
                        ),
                ),
        )
        .when(selected, |el| {
            el.child(
                div()
                    .flex_shrink_0()
                    .child(render_icon(IconName::Check, 16.0, theme.accent)),
            )
        })
}

fn render_ai_engine_card(
    workspace: &WorkspaceView,
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let active_provider = workspace.active_provider;
    let auth = workspace.chatgpt_auth.clone();
    let chatgpt_ready = auth.is_ready_for_inference();
    let gemini_ready = workspace.gemini_key_configured;
    let active_ready = workspace.is_ai_configured();

    let (chatgpt_badge_label, chatgpt_badge_bg, chatgpt_badge_fg, chatgpt_badge_border) =
        if chatgpt_ready {
            ("Connected", theme.add_bg, theme.add_text, theme.add_bg)
        } else if auth.needs_reauth {
            (
                "Session expired",
                theme.del_bg,
                theme.del_text,
                theme.del_bg,
            )
        } else if auth.signed_in && !auth.plan_usage_enabled {
            (
                "Plan disabled",
                theme.accent_soft,
                theme.accent_text,
                theme.accent_soft,
            )
        } else {
            (
                "Not signed in",
                theme.surface,
                theme.muted,
                theme.border_soft,
            )
        };

    let (gemini_badge_label, gemini_badge_bg, gemini_badge_fg, gemini_badge_border) =
        if gemini_ready {
            ("API key saved", theme.add_bg, theme.add_text, theme.add_bg)
        } else {
            (
                "Not configured",
                theme.surface,
                theme.muted,
                theme.border_soft,
            )
        };

    let stack_providers = inner_w < 540.0;
    let provider_w = if stack_providers {
        inner_w
    } else {
        ((inner_w - 12.0) / 2.0).floor().max(220.0)
    };

    div()
        .w(px(card_w))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .p(px(20.0))
        .rounded(px(8.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .gap(px(18.0))
        .child(render_section_header(
            inner_w,
            IconName::Sparkles,
            "AI engine",
            "Choose between your ChatGPT plan (official OpenAI OAuth) or your Google Gemini API key. Both stay strictly local and receive only zero-PII scrubbed CVs.",
            theme,
            None,
        ))
        // Provider switcher cards
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_providers, |el| el.flex_col())
                .when(!stack_providers, |el| el.flex_row())
                .gap(px(12.0))
                .child(render_provider_card(
                    "provider-chatgpt-card",
                    "Sign in with ChatGPT",
                    "Use your ChatGPT plan via official OpenAI OAuth.",
                    chatgpt_badge_label,
                    chatgpt_badge_bg,
                    chatgpt_badge_fg,
                    chatgpt_badge_border,
                    active_provider == AiProviderKind::ChatGpt,
                    provider_w,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.set_ai_provider(AiProviderKind::ChatGpt, cx);
                        cx.notify();
                    }),
                ))
                .child(render_provider_card(
                    "provider-gemini-card",
                    "Google Gemini API",
                    "Use your own Google AI Studio Gemini API key.",
                    gemini_badge_label,
                    gemini_badge_bg,
                    gemini_badge_fg,
                    gemini_badge_border,
                    active_provider == AiProviderKind::GeminiApi,
                    provider_w,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.set_ai_provider(AiProviderKind::GeminiApi, cx);
                        cx.notify();
                    }),
                )),
        )
        // Active provider configuration
        .child(match active_provider {
            AiProviderKind::ChatGpt => {
                render_chatgpt_panel(workspace, &auth, chatgpt_ready, inner_w, theme, cx)
            }
            AiProviderKind::GeminiApi => {
                render_gemini_panel(workspace, gemini_ready, inner_w, theme, cx)
            }
        })
        // Shared models selector
        .child(render_models_panel(
            workspace,
            active_provider,
            active_ready,
            inner_w,
            theme,
            cx,
        ))
}

#[allow(clippy::too_many_arguments)]
fn render_provider_card(
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    badge_label: &'static str,
    badge_bg: Rgba,
    badge_fg: Rgba,
    badge_border: Rgba,
    selected: bool,
    width: f32,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(width))
        .flex_shrink_0()
        .cursor_pointer()
        .flex()
        .items_start()
        .justify_between()
        .gap(px(10.0))
        .p(px(14.0))
        .rounded(px(6.0))
        .bg(if selected { theme.hover } else { theme.input })
        .border_1()
        .border_color(if selected {
            theme.accent
        } else {
            theme.border_soft
        })
        .hover(|s| s.bg(theme.hover))
        .on_click(on_click)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(title),
                        )
                        .child(render_status_pill(
                            badge_label,
                            badge_bg,
                            badge_fg,
                            badge_border,
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(16.5))
                        .text_color(theme.muted)
                        .child(subtitle),
                ),
        )
        .when(selected, |el| {
            el.child(div().flex_shrink_0().pt(px(2.0)).child(render_icon(
                IconName::Check,
                16.0,
                theme.accent,
            )))
        })
}

fn render_chatgpt_panel(
    workspace: &WorkspaceView,
    auth: &ChatGptAuthStatus,
    chatgpt_ready: bool,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let auth_busy = workspace.auth_in_progress;
    let auth_error = workspace.auth_error.clone();
    let show_welcome = workspace.show_plan_welcome_banner && chatgpt_ready;
    let stack_header = inner_w < 560.0;

    div()
        .w(px(inner_w))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .pt(px(16.0))
        .border_t_1()
        .border_color(theme.border_soft)
        // Header + CTA
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_header, |el| el.flex_col().items_start())
                .when(!stack_header, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(12.0))
                .child(
                    div()
                        .when(!stack_header, |el| el.flex_1().min_w_0())
                        .when(stack_header, |el| el.w(px(inner_w)))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Use your ChatGPT plan"),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(17.5))
                                .text_color(theme.muted)
                                .child(
                                    "Complete eligible AI requests in VanguardCV with usage included in your ChatGPT plan or credits balance.",
                                ),
                        ),
                )
                .when(!auth.signed_in || auth.needs_reauth, |row| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(8.0))
                            .child(render_button(
                                "continue-with-chatgpt-btn",
                                if auth_busy {
                                    "Waiting for browser…"
                                } else {
                                    "Continue with ChatGPT"
                                },
                                Some(IconName::Sparkles),
                                ButtonVariant::Primary,
                                auth_busy,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.start_chatgpt_sign_in(false, false, cx);
                                    cx.notify();
                                }),
                            ))
                            .when(auth.signed_in && auth.needs_reauth, |r| {
                                r.child(render_button(
                                    "sign-out-expired-chatgpt-btn",
                                    "Sign out",
                                    Some(IconName::Trash),
                                    ButtonVariant::Secondary,
                                    auth_busy,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.sign_out_chatgpt(cx);
                                        cx.notify();
                                    }),
                                ))
                            }),
                    )
                }),
        )
        // One-time welcome banner
        .when(show_welcome, |card| {
            let stack_banner = inner_w < 520.0;
            card.child(
                div()
                    .w(px(inner_w))
                    .flex()
                    .when(stack_banner, |el| el.flex_col().items_start())
                    .when(!stack_banner, |el| {
                        el.flex_row().items_center().justify_between()
                    })
                    .gap(px(12.0))
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(theme.add_bg)
                    .border_1()
                    .border_color(theme.add_text)
                    .child(
                        div()
                            .when(!stack_banner, |el| el.flex_1().min_w_0())
                            .when(stack_banner, |el| el.w(px((inner_w - 26.0).max(160.0))))
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.add_text)
                                    .child("You’re using your ChatGPT plan"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .line_height(px(17.0))
                                    .text_color(theme.text)
                                    .child(
                                        "Eligible AI requests in VanguardCV will use your ChatGPT plan. You can review or adjust limits anytime in ChatGPT usage settings.",
                                    ),
                            ),
                    )
                    .child(div().flex_shrink_0().child(render_button(
                        "dismiss-plan-welcome-btn",
                        "Got it",
                        None,
                        ButtonVariant::Secondary,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.dismiss_plan_welcome_banner();
                            cx.notify();
                        }),
                    ))),
            )
        })
        // Auth error banner
        .when_some(auth_error, |card, err| {
            card.child(
                div()
                    .w(px(inner_w))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(theme.del_bg)
                    .border_1()
                    .border_color(theme.del_text)
                    .text_size(px(12.0))
                    .line_height(px(17.0))
                    .text_color(theme.del_text)
                    .child(div().flex_1().min_w_0().child(err)),
            )
        })
        // Connected account summary
        .when(auth.signed_in && !auth.needs_reauth, |card| {
            let account_label = auth.account_label();
            let stack_acct = inner_w < 640.0;
            card.child(
                div()
                    .w(px(inner_w))
                    .flex()
                    .when(stack_acct, |el| el.flex_col().items_start())
                    .when(!stack_acct, |el| {
                        el.flex_row().items_center().justify_between()
                    })
                    .gap(px(12.0))
                    .p(px(14.0))
                    .rounded(px(6.0))
                    .bg(theme.input)
                    .border_1()
                    .border_color(theme.border_soft)
                    .child(
                        div()
                            .when(!stack_acct, |el| el.flex_1().min_w_0())
                            .when(stack_acct, |el| el.w(px((inner_w - 30.0).max(160.0))))
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
                                            .text_size(px(13.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(account_label),
                                    )
                                    .when(chatgpt_ready, |r| {
                                        r.child(render_status_pill(
                                            "Using ChatGPT plan",
                                            theme.surface,
                                            theme.secondary,
                                            theme.border_soft,
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .line_height(px(16.5))
                                    .text_color(theme.muted)
                                    .child(if chatgpt_ready {
                                        "View and manage your ChatGPT usage or per-app limits directly in ChatGPT settings."
                                    } else {
                                        "Signed in, but ChatGPT plan usage was declined. Grant access to run AI tailoring with your ChatGPT plan."
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(8.0))
                            .when(!auth.plan_usage_enabled, |actions| {
                                actions.child(render_button(
                                    "enable-plan-usage-btn",
                                    if auth_busy {
                                        "Waiting for browser…"
                                    } else {
                                        "Enable plan usage"
                                    },
                                    Some(IconName::Sparkles),
                                    ButtonVariant::Primary,
                                    auth_busy,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.start_chatgpt_sign_in(true, false, cx);
                                        cx.notify();
                                    }),
                                ))
                            })
                            .when(chatgpt_ready, |actions| {
                                actions.child(render_button(
                                    "manage-chatgpt-usage-btn",
                                    "Manage usage",
                                    Some(IconName::Shield),
                                    ButtonVariant::Secondary,
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.open_chatgpt_usage_settings();
                                        cx.notify();
                                    }),
                                ))
                            })
                            .child(render_button(
                                "switch-chatgpt-account-btn",
                                "Switch account",
                                None,
                                ButtonVariant::Secondary,
                                auth_busy,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.start_chatgpt_sign_in(false, true, cx);
                                    cx.notify();
                                }),
                            ))
                            .child(render_button(
                                "sign-out-chatgpt-btn",
                                "Sign out",
                                Some(IconName::Trash),
                                ButtonVariant::Ghost,
                                auth_busy,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.sign_out_chatgpt(cx);
                                    cx.notify();
                                }),
                            )),
                    ),
            )
        })
        // Privacy note when signed out
        .when(!auth.signed_in || auth.needs_reauth, |card| {
            card.child(render_privacy_callout(
                inner_w,
                "Sign in opens auth.openai.com in your system browser using OpenAI's official OAuth 2.0 + PKCE flow. Tokens stay locally on your device, and VanguardCV strips all contact PII before every AI request.",
                theme,
            ))
        })
}

fn render_gemini_panel(
    workspace: &WorkspaceView,
    gemini_ready: bool,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let stack_header = inner_w < 540.0;
    let stack_input_row = inner_w < 580.0;

    div()
        .w(px(inner_w))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .pt(px(16.0))
        .border_t_1()
        .border_color(theme.border_soft)
        // Header + Get API key button
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_header, |el| el.flex_col().items_start())
                .when(!stack_header, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(10.0))
                .child(
                    div()
                        .when(!stack_header, |el| el.flex_1().min_w_0())
                        .when(stack_header, |el| el.w(px(inner_w)))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Google Gemini API key"),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(17.5))
                                .text_color(theme.muted)
                                .child(
                                    "Connect directly to Google's official Generative Language API using your own API key from Google AI Studio.",
                                ),
                        ),
                )
                .child(div().flex_shrink_0().child(render_button(
                    "open-gemini-key-page-btn",
                    "Get API key",
                    Some(IconName::Arrow),
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.open_gemini_api_key_page();
                        cx.notify();
                    }),
                ))),
        )
        // API key input + actions
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_input_row, |el| el.flex_col())
                .when(!stack_input_row, |el| el.flex_row().items_center())
                .gap(px(8.0))
                .child(
                    div()
                        .when(!stack_input_row, |el| el.flex_1().min_w_0())
                        .when(stack_input_row, |el| el.w(px(inner_w)))
                        .h(px(34.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border)
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(workspace.gemini_api_key_input.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(render_button(
                            "toggle-gemini-key-vis-btn",
                            if workspace.gemini_key_visible {
                                "Hide"
                            } else {
                                "Show"
                            },
                            None,
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.toggle_gemini_key_visibility(cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "save-gemini-key-btn",
                            "Save & load models",
                            Some(IconName::Check),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.save_gemini_api_key(cx);
                                cx.notify();
                            }),
                        ))
                        .when(gemini_ready, |row| {
                            row.child(render_button(
                                "clear-gemini-key-btn",
                                "Clear",
                                Some(IconName::Trash),
                                ButtonVariant::Ghost,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.clear_gemini_api_key(cx);
                                    cx.notify();
                                }),
                            ))
                        }),
                ),
        )
        // Privacy callout
        .child(render_privacy_callout(
            inner_w,
            "Your API key is stored securely in your OS credential manager (never in SQLite). Requests go directly to generativelanguage.googleapis.com after VanguardCV strips all contact PII.",
            theme,
        ))
}

fn render_models_panel(
    workspace: &WorkspaceView,
    active_provider: AiProviderKind,
    active_ready: bool,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let selected_model = workspace.active_model_id().to_string();
    let models_loading = workspace.models_loading;
    let models_error = workspace.models_error.clone();
    let total_models = workspace.available_models.len();
    let filtered_models: Vec<ModelInfo> = workspace
        .filtered_available_models()
        .into_iter()
        .cloned()
        .collect();
    let show_model_filter = total_models > 6;
    let stack_header = inner_w < 500.0;

    let models_heading = match active_provider {
        AiProviderKind::ChatGpt => "Available ChatGPT models",
        AiProviderKind::GeminiApi => "Available Gemini models",
    };

    div()
        .w(px(inner_w))
        .flex()
        .flex_col()
        .gap(px(12.0))
        .pt(px(16.0))
        .border_t_1()
        .border_color(theme.border_soft)
        // Header + Refresh models button
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_header, |el| el.flex_col().items_start())
                .when(!stack_header, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(10.0))
                .child(
                    div()
                        .when(!stack_header, |el| el.flex_1().min_w_0())
                        .when(stack_header, |el| el.w(px(inner_w)))
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(models_heading),
                        )
                        .child(
                            div()
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded(px(4.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border_soft)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child(format!("Active: {selected_model}")),
                        )
                        .when(total_models > 0, |el| {
                            el.child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted)
                                    .child(format!("{total_models} available")),
                            )
                        }),
                )
                .child(div().flex_shrink_0().child(render_button(
                    "refresh-models-btn",
                    if models_loading {
                        "Loading…"
                    } else {
                        "Refresh models"
                    },
                    Some(IconName::Sparkles),
                    ButtonVariant::Secondary,
                    models_loading || !active_ready,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.fetch_available_models(true, cx);
                        cx.notify();
                    }),
                ))),
        )
        // Error banner if model fetch failed
        .when_some(models_error, |el, err| {
            el.child(
                div()
                    .w(px(inner_w))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(theme.del_bg)
                    .border_1()
                    .border_color(theme.del_text)
                    .text_size(px(12.0))
                    .line_height(px(17.0))
                    .text_color(theme.del_text)
                    .child(div().flex_1().min_w_0().child(err)),
            )
        })
        // Filter input when many models are returned
        .when(show_model_filter, |el| {
            el.child(
                div()
                    .w(px(inner_w))
                    .h(px(34.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(5.0))
                    .bg(theme.input)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(render_icon(IconName::Search, 14.0, theme.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(workspace.model_filter_input.clone()),
                    )
                    .when(!workspace.model_filter_query.is_empty(), |row| {
                        row.child(render_icon_button(
                            "clear-model-filter-btn",
                            IconName::Close,
                            "Clear model filter",
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.clear_model_filter(cx);
                                cx.notify();
                            }),
                        ))
                    }),
            )
        })
        // Model list or placeholder state
        .child(if models_loading && total_models == 0 {
            div()
                .w(px(inner_w))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .text_size(px(12.0))
                .line_height(px(17.0))
                .text_color(theme.secondary)
                .child(match active_provider {
                    AiProviderKind::ChatGpt => "Fetching available ChatGPT models for your plan…",
                    AiProviderKind::GeminiApi => "Fetching available Google Gemini models…",
                })
        } else if total_models == 0 {
            div()
                .w(px(inner_w))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(format!("Selected model: {selected_model}")),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(16.5))
                        .text_color(theme.muted)
                        .child(match (active_provider, active_ready) {
                            (AiProviderKind::ChatGpt, true) => {
                                "Click \"Refresh models\" to fetch all ChatGPT models enabled for your plan."
                            }
                            (AiProviderKind::ChatGpt, false) => {
                                "Click \"Continue with ChatGPT\" above to sign in and browse models available on your ChatGPT plan."
                            }
                            (AiProviderKind::GeminiApi, true) => {
                                "Click \"Refresh models\" to fetch all Gemini models available for your API key."
                            }
                            (AiProviderKind::GeminiApi, false) => {
                                "Paste and save your Google Gemini API key above to browse and select available Gemini models."
                            }
                        }),
                )
        } else if filtered_models.is_empty() {
            div()
                .w(px(inner_w))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .text_size(px(12.0))
                .text_color(theme.muted)
                .child("No models match your filter.")
        } else {
            let shown_count = filtered_models.len();
            let needs_scroll = shown_count > 4;
            let list_h = 320.0_f32;
            let item_w = if needs_scroll {
                (inner_w - 14.0).max(200.0)
            } else {
                inner_w
            };
            let scroll_offset = workspace.models_list_scroll.offset();
            let scroll_max = workspace.models_list_scroll.max_offset();
            let scroll_bounds = workspace.models_list_scroll.bounds();
            let max_y = f32::from(scroll_max.height);

            div()
                .w(px(inner_w))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .relative()
                        .w(px(inner_w))
                        .when(needs_scroll, |el| el.h(px(list_h)).overflow_hidden())
                        .child(
                            div()
                                .id("ai-models-scroll-list")
                                .when(needs_scroll, |el| {
                                    el.size_full()
                                        .overflow_y_scroll()
                                        .track_scroll(&workspace.models_list_scroll)
                                })
                                .when(!needs_scroll, |el| el.w(px(inner_w)))
                                .child(
                                    div()
                                        .w(px(item_w))
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.0))
                                        .when(needs_scroll, |el| {
                                            el.pb(px(4.0)).on_scroll_wheel(cx.listener(
                                                |this, event: &ScrollWheelEvent, _window, cx| {
                                                    let delta = event.delta.pixel_delta(px(28.0));
                                                    if delta.y != px(0.0)
                                                        && this.scroll_models_list_by(delta.y, cx)
                                                    {
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ))
                                        })
                                        .children(filtered_models.into_iter().map(|model| {
                                            let model_id = model.id.clone();
                                            let is_selected = model.id == selected_model;
                                            let ctx_badge =
                                                model.input_token_limit.map(format_context_limit);
                                            let desc =
                                                truncate_description(&model.description, 118);

                                            div()
                                                .id(el_id(format!("select-model-{}", model.id)))
                                                .w(px(item_w))
                                                .flex_shrink_0()
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(12.0))
                                                .p(px(10.0))
                                                .rounded(px(6.0))
                                                .bg(if is_selected {
                                                    theme.hover
                                                } else {
                                                    theme.input
                                                })
                                                .border_1()
                                                .border_color(if is_selected {
                                                    theme.accent
                                                } else {
                                                    theme.border_soft
                                                })
                                                .hover(|s| s.bg(theme.hover))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.select_model(&model_id, true, cx);
                                                    cx.notify();
                                                }))
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
                                                                .gap(px(6.0))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(13.0))
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(theme.text)
                                                                        .child(
                                                                            model
                                                                                .display_name
                                                                                .clone(),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(11.0))
                                                                        .text_color(theme.muted)
                                                                        .child(model.id.clone()),
                                                                )
                                                                .when(
                                                                    model.supports_thinking,
                                                                    |row| {
                                                                        row.child(
                                                                            render_status_pill(
                                                                                "Reasoning",
                                                                                theme.accent_soft,
                                                                                theme.accent_text,
                                                                                theme.accent_soft,
                                                                            ),
                                                                        )
                                                                    },
                                                                )
                                                                .when_some(
                                                                    ctx_badge,
                                                                    |row, badge| {
                                                                        row.child(
                                                                            div()
                                                                                .px(px(6.0))
                                                                                .py(px(1.5))
                                                                                .rounded(px(4.0))
                                                                                .bg(theme.surface)
                                                                                .border_1()
                                                                                .border_color(
                                                                                    theme
                                                                                        .border_soft,
                                                                                )
                                                                                .text_size(px(10.0))
                                                                                .text_color(
                                                                                    theme.secondary,
                                                                                )
                                                                                .child(badge),
                                                                        )
                                                                    },
                                                                ),
                                                        )
                                                        .when(!desc.is_empty(), |col| {
                                                            col.child(
                                                                div()
                                                                    .text_size(px(11.5))
                                                                    .line_height(px(16.0))
                                                                    .text_color(theme.muted)
                                                                    .child(desc),
                                                            )
                                                        }),
                                                )
                                                .when(is_selected, |card| {
                                                    card.child(div().flex_shrink_0().child(
                                                        render_icon(
                                                            IconName::Check,
                                                            16.0,
                                                            theme.accent,
                                                        ),
                                                    ))
                                                })
                                        })),
                                ),
                        )
                        .when(needs_scroll && max_y > 1.0, |el| {
                            let vp_h = f32::from(scroll_bounds.size.height).max(list_h);
                            let track_h = (vp_h - 4.0).max(60.0);
                            let ratio = (vp_h / (vp_h + max_y)).clamp(0.12, 0.92);
                            let thumb_h = (track_h * ratio).max(28.0);
                            let progress =
                                ((-f32::from(scroll_offset.y)) / max_y).clamp(0.0, 1.0);
                            let thumb_top = (track_h - thumb_h) * progress;
                            el.child(
                                div()
                                    .id("models-list-vbar")
                                    .cursor_pointer()
                                    .absolute()
                                    .top(px(2.0))
                                    .bottom(px(2.0))
                                    .right(px(0.0))
                                    .w(px(8.0))
                                    .rounded(px(4.0))
                                    .bg(theme.surface_soft)
                                    .border_1()
                                    .border_color(theme.border_soft)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            |this, event: &MouseDownEvent, _window, cx| {
                                                let bounds = this.models_list_scroll.bounds();
                                                let rel = (f32::from(
                                                    event.position.y - bounds.top(),
                                                ) - 2.0)
                                                    / (f32::from(bounds.size.height) - 4.0)
                                                        .max(1.0);
                                                let max_h = f32::from(
                                                    this.models_list_scroll.max_offset().height,
                                                );
                                                let cur = this.models_list_scroll.offset();
                                                let next_y = px(-(rel.clamp(0.0, 1.0) * max_h));
                                                this.models_list_scroll
                                                    .set_offset(point(cur.x, next_y));
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(thumb_top))
                                            .w_full()
                                            .h(px(thumb_h))
                                            .rounded(px(4.0))
                                            .bg(theme.muted),
                                    ),
                            )
                        }),
                )
                .when(needs_scroll, |el| {
                    el.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.muted)
                            .child(format!(
                                "Showing {shown_count} of {total_models} models · Scroll inside the list or use the filter above"
                            )),
                    )
                })
        })
}

fn render_updates_card(
    workspace: &WorkspaceView,
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let stack_rows = inner_w < 540.0;
    let is_busy = matches!(
        workspace.update_status,
        UpdateStatus::Checking | UpdateStatus::Downloading | UpdateStatus::Installing
    );

    let (pill_label, pill_bg, pill_fg, pill_border) = match &workspace.update_status {
        UpdateStatus::Idle => (
            if workspace.auto_update_enabled {
                "Auto-check on"
            } else {
                "Manual only"
            },
            theme.input,
            theme.secondary,
            theme.border_soft,
        ),
        UpdateStatus::Checking => (
            "Checking…",
            theme.accent_soft,
            theme.accent_text,
            theme.accent,
        ),
        UpdateStatus::UpToDate => ("Up to date", theme.add_bg, theme.add_text, theme.add_text),
        UpdateStatus::Available => (
            "Update available",
            theme.accent_soft,
            theme.accent_text,
            theme.accent,
        ),
        UpdateStatus::Downloading => (
            "Downloading…",
            theme.accent_soft,
            theme.accent_text,
            theme.accent,
        ),
        UpdateStatus::ReadyToInstall => (
            "Ready to install",
            theme.add_bg,
            theme.add_text,
            theme.add_text,
        ),
        UpdateStatus::Installing => (
            "Installing…",
            theme.accent_soft,
            theme.accent_text,
            theme.accent,
        ),
        UpdateStatus::Error(_) => ("Error", theme.del_bg, theme.del_text, theme.del_text),
    };

    let status_summary = match &workspace.update_status {
        UpdateStatus::Idle => format!("Installed version: v{CURRENT_VERSION} · Windows x64 installer channel"),
        UpdateStatus::Checking => "Contacting GitHub Releases to check for newer Windows builds…".to_string(),
        UpdateStatus::UpToDate => format!("You are running the latest release (v{CURRENT_VERSION})."),
        UpdateStatus::Available => {
            if let Some(rel) = workspace.update_release.as_ref() {
                let size_str = if rel.installer_size_bytes > 0 {
                    format!(" ({})", format_download_size(rel.installer_size_bytes))
                } else {
                    String::new()
                };
                format!("Version v{} is available{size_str}.", rel.version)
            } else {
                "A newer Windows release is available.".to_string()
            }
        }
        UpdateStatus::Downloading => {
            if let Some(rel) = workspace.update_release.as_ref() {
                format!(
                    "Downloading and verifying VanguardCV v{} installer…",
                    rel.version
                )
            } else {
                "Downloading and verifying update installer…".to_string()
            }
        }
        UpdateStatus::ReadyToInstall => {
            if let Some(dl) = workspace.update_downloaded.as_ref() {
                let verify = if dl.checksum_verified {
                    "SHA-256 verified"
                } else {
                    "Installer verified"
                };
                format!(
                    "VanguardCV v{} ({}, {verify}) is ready. Click Install & restart to apply.",
                    dl.version,
                    format_download_size(dl.size_bytes)
                )
            } else {
                "Update downloaded and verified. Click Install & restart to apply.".to_string()
            }
        }
        UpdateStatus::Installing => {
            "Launching Windows installer and closing VanguardCV to apply the update…".to_string()
        }
        UpdateStatus::Error(msg) => format!("Update check error: {msg}"),
    };

    let release_notes = workspace
        .update_release
        .as_ref()
        .map(|r| r.release_notes.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut card = div()
        .w(px(card_w))
        .p(px(20.0))
        .rounded(px(10.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(render_section_header(
            inner_w,
            IconName::Download,
            "Software updates",
            "Keep VanguardCV up to date with verified Windows installers from official GitHub Releases.",
            theme,
            Some(render_status_pill(pill_label, pill_bg, pill_fg, pill_border).into_any_element()),
        ))
        // Auto-update toggle row
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_rows, |el| el.flex_col().items_start())
                .when(!stack_rows, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(8.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .when(!stack_rows, |el| el.flex_1().min_w_0())
                        .when(stack_rows, |el| el.w(px((inner_w - 30.0).max(160.0))))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Automatic updates"),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(16.5))
                                .text_color(theme.muted)
                                .child(
                                    "Automatically check GitHub Releases on launch and download verified Windows installers in the background. Updates are never applied until you click Install & restart.",
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if workspace.auto_update_enabled {
                                    theme.text
                                } else {
                                    theme.muted
                                })
                                .child(if workspace.auto_update_enabled {
                                    "Enabled"
                                } else {
                                    "Disabled"
                                }),
                        )
                        .child(render_toggle(
                            "settings-auto-update-toggle",
                            workspace.auto_update_enabled,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.toggle_auto_update(cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Current version + actions
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_rows, |el| el.flex_col().items_start())
                .when(!stack_rows, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(12.0))
                .child(
                    div()
                        .when(!stack_rows, |el| el.flex_1().min_w_0())
                        .when(stack_rows, |el| el.w(px(inner_w)))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(format!("VanguardCV v{CURRENT_VERSION}")),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(16.5))
                                .text_color(if matches!(workspace.update_status, UpdateStatus::Error(_)) {
                                    theme.del_text
                                } else {
                                    theme.secondary
                                })
                                .child(status_summary),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .when(
                            matches!(workspace.update_status, UpdateStatus::ReadyToInstall),
                            |el| {
                                el.child(render_button(
                                    "settings-install-update-btn",
                                    "Install & restart",
                                    Some(IconName::Download),
                                    ButtonVariant::Primary,
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.apply_downloaded_update(cx);
                                        cx.notify();
                                    }),
                                ))
                            },
                        )
                        .when(
                            matches!(workspace.update_status, UpdateStatus::Available),
                            |el| {
                                el.child(render_button(
                                    "settings-download-update-btn",
                                    "Download update",
                                    Some(IconName::Download),
                                    ButtonVariant::Primary,
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.download_available_update(true, cx);
                                        cx.notify();
                                    }),
                                ))
                            },
                        )
                        .child(render_button(
                            "settings-check-updates-btn",
                            if matches!(workspace.update_status, UpdateStatus::Checking) {
                                "Checking…"
                            } else {
                                "Check for updates"
                            },
                            Some(IconName::Search),
                            ButtonVariant::Secondary,
                            is_busy,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.check_for_updates(true, cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "settings-open-releases-btn",
                            "Releases",
                            Some(IconName::Arrow),
                            ButtonVariant::Ghost,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_releases_page();
                                cx.notify();
                            }),
                        )),
                ),
        );

    if let Some(notes) = release_notes {
        let preview = truncate_description(&notes, 420);
        card = card.child(
            div()
                .w(px(inner_w))
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(
                            workspace
                                .update_release
                                .as_ref()
                                .map(|r| format!("What's new in {}", r.tag_name))
                                .unwrap_or_else(|| "Release notes".to_string()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .text_color(theme.secondary)
                        .child(preview),
                ),
        );
    }

    card
}

fn render_data_card(
    workspace: &WorkspaceView,
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let active_title = workspace.active_document().map(|d| d.title.clone());
    let has_active_doc = active_title.is_some();
    let doc_count = workspace.documents.len();
    let stack_rows = inner_w < 600.0;
    let stack_snap_header = inner_w < 500.0;

    let subtitle = format!(
        "Stored locally in SQLite · {} {} in workspace",
        doc_count,
        if doc_count == 1 {
            "document"
        } else {
            "documents"
        }
    );

    div()
        .w(px(card_w))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .p(px(20.0))
        .rounded(px(8.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .gap(px(16.0))
        // Card header + SQLite Backup CTA
        .child(render_section_header(
            inner_w,
            IconName::Shield,
            "Data, backups & snapshots",
            &subtitle,
            theme,
            Some(
                render_button(
                    "backup-db-btn",
                    "Backup SQLite DB",
                    Some(IconName::Download),
                    ButtonVariant::Secondary,
                    false,
                    theme,
                    cx.listener(|this, _, window, cx| {
                        this.backup_database(window, cx);
                        cx.notify();
                    }),
                )
                .into_any_element(),
            ),
        ))
        // Portable workspace archive (.vanguard)
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_rows, |el| el.flex_col().items_start())
                .when(!stack_rows, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .when(!stack_rows, |el| el.flex_1().min_w_0())
                        .when(stack_rows, |el| el.w(px((inner_w - 30.0).max(160.0))))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Portable workspace archive (.vanguard)"),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(16.5))
                                .text_color(theme.muted)
                                .child(
                                    "Bundle all Master CVs, variations with parent-child lineage, and custom skills into a single portable file.",
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(render_button(
                            "export-vanguard-btn",
                            "Export .vanguard",
                            Some(IconName::Download),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, window, cx| {
                                this.export_vanguard_archive(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "import-vanguard-btn",
                            "Import .vanguard",
                            Some(IconName::Folder),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, window, cx| {
                                this.import_vanguard_archive(window, cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Single-CV JSON & Zero-Trust Anonymized Template
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_rows, |el| el.flex_col().items_start())
                .when(!stack_rows, |el| {
                    el.flex_row().items_center().justify_between()
                })
                .gap(px(12.0))
                .p(px(14.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .border_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .when(!stack_rows, |el| el.flex_1().min_w_0())
                        .when(stack_rows, |el| el.w(px((inner_w - 30.0).max(160.0))))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Single CV JSON & anonymized templates"),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(16.5))
                                .text_color(theme.muted)
                                .child(
                                    "Export the active CV with PII stripped for safe public sharing, or import/export full CV JSON.",
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(render_button(
                            "export-template-btn",
                            "Export anonymized template",
                            Some(IconName::Lock),
                            ButtonVariant::Secondary,
                            !has_active_doc,
                            theme,
                            cx.listener(|this, _, window, cx| {
                                this.export_anonymized_template(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "export-json-btn",
                            "Export JSON",
                            Some(IconName::File),
                            ButtonVariant::Secondary,
                            !has_active_doc,
                            theme,
                            cx.listener(|this, _, window, cx| {
                                this.export_active_json(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "import-json-btn",
                            "Import JSON",
                            Some(IconName::Folder),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, window, cx| {
                                this.import_cv_json(window, cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Recovery snapshots section
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .pt(px(16.0))
                .border_t_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .w(px(inner_w))
                        .flex()
                        .when(stack_snap_header, |el| el.flex_col().items_start())
                        .when(!stack_snap_header, |el| {
                            el.flex_row().items_center().justify_between()
                        })
                        .gap(px(10.0))
                        .child(
                            div()
                                .when(!stack_snap_header, |el| el.flex_1().min_w_0())
                                .when(stack_snap_header, |el| el.w(px(inner_w)))
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text)
                                        .child(match &active_title {
                                            Some(title) => {
                                                format!("Recovery snapshots for \"{title}\"")
                                            }
                                            None => "Recovery snapshots".to_string(),
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .line_height(px(16.5))
                                        .text_color(theme.muted)
                                        .child(if has_active_doc {
                                            "Automatic checkpoints are taken before every AI run. Restore any version safely."
                                        } else {
                                            "Select a CV from CV library to view or create recovery checkpoints."
                                        }),
                                ),
                        )
                        .child(div().flex_shrink_0().child(render_button(
                            "create-snapshot-btn",
                            "Create checkpoint",
                            Some(IconName::Plus),
                            ButtonVariant::Secondary,
                            !has_active_doc,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.create_manual_snapshot();
                                cx.notify();
                            }),
                        ))),
                )
                .child(
                    div()
                        .w(px(inner_w))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .children(workspace.snapshots.iter().map(|snap| {
                            let snap_id = snap.id;
                            let stack_snap = inner_w < 460.0;
                            div()
                                .w(px(inner_w))
                                .flex()
                                .when(stack_snap, |el| el.flex_col().items_start())
                                .when(!stack_snap, |el| {
                                    el.flex_row().items_center().justify_between()
                                })
                                .gap(px(10.0))
                                .p(px(10.0))
                                .rounded(px(6.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border_soft)
                                .child(
                                    div()
                                        .when(!stack_snap, |el| el.flex_1().min_w_0())
                                        .when(stack_snap, |el| el.w(px((inner_w - 22.0).max(160.0))))
                                        .flex()
                                        .items_center()
                                        .gap(px(10.0))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .px(px(7.0))
                                                .py(px(2.0))
                                                .rounded(px(4.0))
                                                .bg(theme.surface)
                                                .border_1()
                                                .border_color(theme.border_soft)
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.accent)
                                                .child(format!("v{}", snap.version)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .gap(px(1.0))
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(theme.text)
                                                        .child(snap.reason.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(theme.muted)
                                                        .child(
                                                            snap.created_at
                                                                .format("%d %b %Y · %H:%M UTC")
                                                                .to_string(),
                                                        ),
                                                ),
                                        ),
                                )
                                .child(div().flex_shrink_0().child(render_button(
                                    format!("restore-snap-{}", snap.id),
                                    "Restore",
                                    None,
                                    ButtonVariant::Secondary,
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.restore_snapshot_by_id(snap_id, cx);
                                        cx.notify();
                                    }),
                                )))
                        }))
                        .when(workspace.snapshots.is_empty(), |el| {
                            el.child(
                                div()
                                    .w(px(inner_w))
                                    .p(px(12.0))
                                    .rounded(px(6.0))
                                    .bg(theme.input)
                                    .border_1()
                                    .border_color(theme.border_soft)
                                    .text_size(px(12.0))
                                    .line_height(px(17.0))
                                    .text_color(theme.muted)
                                    .child(if has_active_doc {
                                        "No snapshots recorded for this CV yet. Click \"Create checkpoint\" to save a restore point."
                                    } else {
                                        "No CV is currently loaded. Select a CV in CV library to manage its checkpoints."
                                    }),
                            )
                        }),
                ),
        )
        // Danger zone: full factory reset
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .pt(px(16.0))
                .border_t_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .w(px(inner_w))
                        .flex()
                        .when(stack_rows, |el| el.flex_col().items_start())
                        .when(!stack_rows, |el| {
                            el.flex_row().items_center().justify_between()
                        })
                        .gap(px(12.0))
                        .p(px(14.0))
                        .rounded(px(6.0))
                        .bg(theme.del_bg)
                        .border_1()
                        .border_color(theme.del_text)
                        .child(
                            div()
                                .when(!stack_rows, |el| el.flex_1().min_w_0())
                                .when(stack_rows, |el| el.w(px((inner_w - 30.0).max(160.0))))
                                .flex()
                                .flex_col()
                                .gap(px(3.0))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.del_text)
                                        .child("Reset application"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .line_height(px(16.5))
                                        .text_color(theme.text)
                                        .child(
                                            "Delete all CVs, variations, snapshots, saved job search profiles, settings, custom skills, and saved AI credentials. Bundled default skills are preserved.",
                                        ),
                                ),
                        )
                        .child(div().flex_shrink_0().child(render_button(
                            "reset-app-open-modal-btn",
                            "Reset everything",
                            Some(IconName::Trash),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_reset_app_dialog();
                                cx.notify();
                            }),
                        ))),
                ),
        )
}
