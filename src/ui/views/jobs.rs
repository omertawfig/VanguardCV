use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::AiProviderKind;
use crate::jobs::{
    CvSearchSource, JOB_COUNTRIES, JobListing, MAX_PAGE_NUMBER, SearchProfile,
    build_external_search_links,
};
use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_button, render_decorated_input_box, render_icon,
    render_icon_button, tooltip_view,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::{WorkspaceView, WorkspaceViewPage};

pub fn render_jobs_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
    let theme = &workspace.theme;
    let content_w = workspace.content_width().max(320.0);
    let pad_x = workspace.page_padding_x();
    let pad_x_f = f32::from(pad_x);
    let page_w = content_w.min(1248.0);
    let card_w = (page_w - pad_x_f * 2.0).max(280.0);
    // Card horizontal inset = p(16.0) * 2 + border_1() * 2 = 34.0
    let inner_w = (card_w - 34.0).max(220.0);
    let stack_header = card_w < 560.0;

    let resolved_source = workspace.resolved_job_source();
    let ai_configured = workspace.is_ai_configured();
    let has_docs = !workspace.documents.is_empty();
    let has_active_query = !workspace.job_filters.manual_query.trim().is_empty()
        || workspace.job_profiles.iter().any(|p| p.enabled);
    let needs_search_update = workspace.job_search_has_unapplied_changes();

    let external_links =
        build_external_search_links(&workspace.job_profiles, &workspace.job_filters);
    let target_master_title = workspace.resolved_job_target_master_title();

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
                .pb(px(72.0))
                .gap(px(18.0))
                // Page Header
                .child(
                    div()
                        .w(px(card_w))
                        .flex_shrink_0()
                        .flex()
                        .when(stack_header, |el| el.flex_col().items_start())
                        .when(!stack_header, |el| {
                            el.flex_row().items_start().justify_between()
                        })
                        .gap(px(12.0))
                        .child(
                            div()
                                .when(!stack_header, |el| el.flex_1().min_w_0())
                                .when(stack_header, |el| el.w(px(card_w)))
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .text_size(px(24.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text)
                                        .child("Job discovery"),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .line_height(px(19.5))
                                        .text_color(theme.secondary)
                                        .child(
                                            "Turn your saved CVs into targeted search profiles privately, browse live roles, and tailor a variation in one click.",
                                        ),
                                ),
                        )

                )
                // Error / API Limit Reached Banner
                .when_some(workspace.job_error.clone(), |el, err| {
                    let is_limit = crate::ai::is_ai_limit_error(&err);
                    let is_chatgpt_limit =
                        workspace.active_provider == AiProviderKind::ChatGpt && is_limit;
                    let stack_err = card_w < 560.0;
                    el.child(
                        div()
                            .w(px(card_w))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .flex()
                            .when(stack_err, |d| d.flex_col().items_start())
                            .when(!stack_err, |d| {
                                d.flex_row().items_start().justify_between()
                            })
                            .gap(px(10.0))
                            .p(px(14.0))
                            .rounded(px(6.0))
                            .bg(theme.del_bg)
                            .border_1()
                            .border_color(theme.del_text)
                            .child(
                                div()
                                    .when(!stack_err, |d| d.flex_1().min_w_0())
                                    .when(stack_err, |d| d.w(px((card_w - 30.0).max(200.0))))
                                    .flex()
                                    .items_start()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .pt(px(2.0))
                                            .child(render_icon(
                                                IconName::Shield,
                                                16.0,
                                                theme.del_text,
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
                                                    .w_full()
                                                    .min_w_0()
                                                    .text_size(px(13.0))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(theme.del_text)
                                                    .child(if is_limit {
                                                        "API limit reached"
                                                    } else {
                                                        "Job discovery error"
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .min_w_0()
                                                    .text_size(px(12.0))
                                                    .line_height(px(17.5))
                                                    .text_color(theme.text)
                                                    .child(err),
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
                                    .when(is_limit, |actions| {
                                        actions.child(render_button(
                                            "jobs-limit-settings-btn",
                                            "Switch model",
                                            Some(IconName::Settings),
                                            ButtonVariant::Secondary,
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.navigate_to(
                                                    WorkspaceViewPage::Settings,
                                                    cx,
                                                );
                                                cx.notify();
                                            }),
                                        ))
                                    })
                                    .when(is_chatgpt_limit, |actions| {
                                        actions.child(render_button(
                                            "jobs-manage-usage-btn",
                                            "Manage usage",
                                            Some(IconName::Shield),
                                            ButtonVariant::Primary,
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.open_chatgpt_usage_settings();
                                                cx.notify();
                                            }),
                                        ))
                                    })
                                    .child(render_icon_button(
                                        "jobs-dismiss-error-btn",
                                        IconName::Close,
                                        "Dismiss",
                                        false,
                                        theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.dismiss_job_error();
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                })
                // Empty-library guidance
                .when(!has_docs, |el| {
                    let stack_empty = card_w < 580.0;
                    el.child(
                        div()
                            .w(px(card_w))
                            .flex_shrink_0()
                            .p(px(20.0))
                            .rounded(px(6.0))
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .when(stack_empty, |d| d.flex_col().items_start())
                            .when(!stack_empty, |d| {
                                d.flex_row().items_center().justify_between()
                            })
                            .gap(px(12.0))
                            .child(
                                div()
                                    .when(!stack_empty, |d| d.flex_1().min_w_0())
                                    .when(stack_empty, |d| d.w(px((card_w - 42.0).max(200.0))))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(
                                                "Create a CV first to generate targeted search profiles",
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.0))
                                            .text_color(theme.secondary)
                                            .child(
                                                "Add a Master CV in the library, or enter a custom search query below.",
                                            ),
                                    ),
                            )
                            .child(div().flex_shrink_0().child(render_button(
                                "jobs-go-library-btn",
                                "Open CV library",
                                Some(IconName::Folder),
                                ButtonVariant::Primary,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.navigate_to(WorkspaceViewPage::Library, cx);
                                    cx.notify();
                                }),
                            ))),
                    )
                })
                // CV Source, AI Profiles & Filter Controls Card
                .child(render_controls_card(
                    workspace,
                    resolved_source.as_ref(),
                    ai_configured,
                    has_docs,
                    card_w,
                    inner_w,
                    theme,
                    cx,
                ))
                // Prompt banner when search profiles, keywords, or query changed
                .when(needs_search_update, |el| {
                    let pulse_dot_color = theme.focus_ring;
                    el.child(
                        div()
                            .w(px(card_w))
                            .flex_shrink_0()
                            .px(px(14.0))
                            .py(px(10.0))
                            .rounded(px(6.0))
                            .bg(theme.accent_soft)
                            .border_1()
                            .border_color(theme.accent)
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .child(
                                div()
                                    .size(px(8.0))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(pulse_dot_color)
                                    .with_animation(
                                        "jobs-dirty-dot-pulse",
                                        Animation::new(Duration::from_millis(1200))
                                            .repeat()
                                            .with_easing(bounce(ease_in_out)),
                                        |dot, delta| dot.opacity(0.35 + delta * 0.65),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_x(px(5.0))
                                    .gap_y(px(3.0))
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child("Search profiles or criteria changed — click")
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .px(px(6.0))
                                            .py(px(1.5))
                                            .rounded(px(4.0))
                                            .bg(theme.accent)
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.on_accent)
                                            .child("Update search"),
                                    )
                                    .child("above to refresh the listings below."),
                            ),
                    )
                })
                // Subtle hint when user unchecked all profiles while previous listings remain visible
                .when(
                    !has_active_query
                        && !workspace.job_listings.is_empty()
                        && !workspace.job_listings_loading,
                    |el| {
                        el.child(
                            div()
                                .w(px(card_w))
                                .flex_shrink_0()
                                .px(px(14.0))
                                .py(px(9.0))
                                .rounded(px(6.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border_soft)
                                .text_size(px(12.0))
                                .text_color(theme.secondary)
                                .child(
                                    "No search profile selected — check a role above or enter a custom query, then click Update search.",
                                ),
                        )
                    },
                )
                // Warnings strip (if provider emitted non-fatal warnings)
                .when(!workspace.job_warnings.is_empty(), |el| {
                    el.child(
                        div()
                            .w(px(card_w))
                            .flex_shrink_0()
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(6.0))
                            .bg(theme.input)
                            .border_1()
                            .border_color(theme.border_soft)
                            .text_size(px(12.0))
                            .text_color(theme.secondary)
                            .child(workspace.job_warnings.join(" · ")),
                    )
                })
                // External Quick-Search Deep Links (LinkedIn & Indeed only)
                .when(!external_links.is_empty(), |el| {
                    el.child(
                        div()
                            .w(px(card_w))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(12.0))
                                    .text_color(theme.muted)
                                    .child("Quick search on external boards:"),
                            )
                            .children(external_links.into_iter().map(|link| {
                                let url = link.url.clone();
                                div()
                                    .id(el_id(format!("ext-link-{}", link.platform)))
                                    .flex_shrink_0()
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .px(px(10.0))
                                    .py(px(4.0))
                                    .rounded_full()
                                    .bg(theme.surface)
                                    .border_1()
                                    .border_color(theme.border)
                                    .hover(|s| s.bg(theme.hover).border_color(theme.secondary))
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_job_url(&url);
                                        cx.notify();
                                    }))
                                    .child(link.platform)
                                    .child(render_icon(IconName::Arrow, 11.0, theme.muted))
                            }))
                            .when_some(
                                {
                                    let mut parts: Vec<String> = Vec::new();
                                    if let Some(country) = workspace.job_filters.selected_country()
                                    {
                                        parts.push(country.name.to_string());
                                    }
                                    if let Some(days) = workspace.job_filters.posted_within_days {
                                        parts.push(if days == 1 {
                                            "24h".to_string()
                                        } else {
                                            format!("{days}d")
                                        });
                                    }
                                    if workspace.job_filters.remote_only {
                                        parts.push("Remote".to_string());
                                    }
                                    (!parts.is_empty()).then(|| parts.join(" · "))
                                },
                                |row, summary| {
                                    row.child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(px(11.0))
                                            .text_color(theme.muted)
                                            .child(format!("Includes: {summary}")),
                                    )
                                },
                            ),
                    )
                })
                // Live Opportunities Header + Cards + Pagination
                .child(render_results_section(
                    workspace,
                    target_master_title.as_deref(),
                    has_active_query,
                    card_w,
                    inner_w,
                    theme,
                    cx,
                )),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_controls_card(
    workspace: &WorkspaceView,
    resolved_source: Option<&CvSearchSource>,
    ai_configured: bool,
    has_docs: bool,
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    // Order masters first, then variations
    let mut ordered_docs: Vec<_> = workspace.documents.iter().collect();
    ordered_docs.sort_by_key(|d| (d.parent_id.is_some(), d.title.to_ascii_lowercase()));

    let has_manual = !workspace.job_filters.manual_query.trim().is_empty();
    let has_profiles = !workspace.job_profiles.is_empty();
    let freshness_options: [(Option<u32>, &'static str); 5] = [
        (None, "Any time"),
        (Some(1), "24h"),
        (Some(7), "7 days"),
        (Some(14), "14 days"),
        (Some(30), "30 days"),
    ];
    let page_size_options: [usize; 3] = [10, 15, 20];

    let profile_count = workspace.job_profiles.len();
    let profile_cols = if inner_w >= 800.0 && profile_count >= 3 {
        3usize
    } else if inner_w >= 520.0 && profile_count >= 2 {
        2usize
    } else {
        1usize
    };
    let profile_w = ((inner_w - 10.0 * (profile_cols.saturating_sub(1) as f32))
        / (profile_cols as f32))
        .floor()
        .max(200.0);

    let stack_input_row = inner_w < 520.0;

    div()
        .w(px(card_w))
        .flex_shrink_0()
        .p(px(16.0))
        .rounded(px(6.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap(px(14.0))
        // Row 1: CV Source Selector + Generate button
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .w(px(inner_w))
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(8.0))
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
                                        .child("CV source"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .px(px(7.0))
                                        .py(px(2.0))
                                        .rounded(px(4.0))
                                        .bg(theme.accent_soft)
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.accent_text)
                                        .child(render_icon(IconName::Lock, 10.5, theme.accent))
                                        .child("PII stripped before AI"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.muted)
                                .child("Keywords saved per CV · listings clear on exit"),
                        ),
                )
                .when(has_docs, |el| {
                    el.child(
                        div()
                            .w(px(inner_w))
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(6.0))
                            .children(ordered_docs.into_iter().map(|doc| {
                                let doc_id = doc.id;
                                let is_selected =
                                    resolved_source == Some(&CvSearchSource::Single(doc_id));
                                let is_default = workspace.default_cv_id == Some(doc_id);
                                let is_child = doc.parent_id.is_some();
                                let title = doc.title.clone();
                                div()
                                    .id(el_id(format!("job-src-cv-{doc_id}")))
                                    .flex_shrink_0()
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .px(px(10.0))
                                    .py(px(5.0))
                                    .rounded(px(6.0))
                                    .border_1()
                                    .border_color(if is_selected {
                                        theme.accent
                                    } else {
                                        theme.border
                                    })
                                    .bg(if is_selected {
                                        theme.accent_soft
                                    } else {
                                        theme.input
                                    })
                                    .hover(|s| s.border_color(theme.accent))
                                    .text_size(px(12.0))
                                    .font_weight(if is_selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(if is_selected {
                                        theme.accent_text
                                    } else {
                                        theme.text
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_job_cv_source(
                                            CvSearchSource::Single(doc_id),
                                            cx,
                                        );
                                        cx.notify();
                                    }))
                                    .child(render_icon(
                                        if is_child {
                                            IconName::Branch
                                        } else {
                                            IconName::Shield
                                        },
                                        12.0,
                                        if is_selected {
                                            theme.accent
                                        } else {
                                            theme.secondary
                                        },
                                    ))
                                    .child(title)
                                    .when(is_default, |pill| {
                                        pill.child(
                                            div()
                                                .px(px(5.0))
                                                .py(px(1.0))
                                                .rounded(px(4.0))
                                                .bg(theme.surface)
                                                .border_1()
                                                .border_color(theme.border_soft)
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.accent_text)
                                                .child("Default"),
                                        )
                                    })
                            }))
                            .when(workspace.documents.len() > 1, |row| {
                                let is_all = resolved_source == Some(&CvSearchSource::AllCvs);
                                row.child(
                                    div()
                                        .id("job-src-all-cvs")
                                        .flex_shrink_0()
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .px(px(10.0))
                                        .py(px(5.0))
                                        .rounded(px(6.0))
                                        .border_1()
                                        .border_color(if is_all {
                                            theme.accent
                                        } else {
                                            theme.border
                                        })
                                        .bg(if is_all {
                                            theme.accent_soft
                                        } else {
                                            theme.input
                                        })
                                        .hover(|s| s.border_color(theme.accent))
                                        .text_size(px(12.0))
                                        .font_weight(if is_all {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::MEDIUM
                                        })
                                        .text_color(if is_all {
                                            theme.accent_text
                                        } else {
                                            theme.text
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_job_cv_source(CvSearchSource::AllCvs, cx);
                                            cx.notify();
                                        }))
                                        .child(render_icon(
                                            IconName::Folder,
                                            12.0,
                                            if is_all {
                                                theme.accent
                                            } else {
                                                theme.secondary
                                            },
                                        ))
                                        .child(format!("All CVs ({})", workspace.documents.len())),
                                )
                            }),
                    )
                })
                // Action bar for generating search profiles
                .when(has_docs, |el| {
                    el.child(
                        div()
                            .w(px(inner_w))
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(8.0))
                            .child(render_button(
                                "jobs-generate-profiles-btn",
                                if workspace.job_profiles_loading {
                                    "Analyzing CV…"
                                } else if ai_configured {
                                    if has_profiles {
                                        "Regenerate AI profiles"
                                    } else {
                                        "Generate search profiles with AI"
                                    }
                                } else {
                                    "Extract local search profiles"
                                },
                                Some(IconName::Sparkles),
                                ButtonVariant::Primary,
                                workspace.job_profiles_loading,
                                theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.generate_job_profiles(has_profiles, cx);
                                    cx.notify();
                                }),
                            ))
                            .when(!ai_configured, |row| {
                                row.child(render_button(
                                    "jobs-connect-ai-btn",
                                    "Connect AI in Settings",
                                    Some(IconName::Settings),
                                    ButtonVariant::Secondary,
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.navigate_to(WorkspaceViewPage::Settings, cx);
                                        cx.notify();
                                    }),
                                ))
                            })
                            .when(workspace.job_profiles_stale && has_profiles, |row| {
                                row.child(
                                    div()
                                        .flex_shrink_0()
                                        .px(px(8.0))
                                        .py(px(3.0))
                                        .rounded(px(4.0))
                                        .bg(theme.accent_soft)
                                        .border_1()
                                        .border_color(theme.accent)
                                        .text_size(px(11.0))
                                        .text_color(theme.accent_text)
                                        .child("CV edited · refreshing saved keywords"),
                                )
                            })
                            .when(
                                !workspace.job_profiles_stale
                                    && workspace.job_profiles_from_cache
                                    && has_profiles,
                                |row| {
                                    row.child(
                                        div()
                                            .flex_shrink_0()
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(4.0))
                                            .bg(theme.input)
                                            .border_1()
                                            .border_color(theme.border_soft)
                                            .text_size(px(11.0))
                                            .text_color(theme.muted)
                                            .child("Saved for this CV (0 AI tokens used)"),
                                    )
                                },
                            )
                            .when(
                                workspace.job_profiles_used_fallback && has_profiles,
                                |row| {
                                    row.child(
                                        div()
                                            .flex_shrink_0()
                                            .px(px(8.0))
                                            .py(px(3.0))
                                            .rounded(px(4.0))
                                            .bg(theme.input)
                                            .border_1()
                                            .border_color(theme.border_soft)
                                            .text_size(px(11.0))
                                            .text_color(theme.secondary)
                                            .child("Local CV keyword extraction"),
                                    )
                                },
                            ),
                    )
                }),
        )
        // Row 2: Generated Search Profiles (toggleable profiles + toggleable keywords)
        .when(has_profiles, |el| {
            el.child(
                div()
                    .w(px(inner_w))
                    .pt(px(12.0))
                    .border_t_1()
                    .border_color(theme.border_soft)
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .w(px(inner_w))
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child(
                                        "Search profiles (toggle roles or keywords without re-running AI)",
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.muted)
                                    .child(format!(
                                        "{} of {} active",
                                        workspace
                                            .job_profiles
                                            .iter()
                                            .filter(|p| p.enabled)
                                            .count(),
                                        workspace.job_profiles.len()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .w(px(inner_w))
                            .flex()
                            .flex_wrap()
                            .gap(px(10.0))
                            .children(workspace.job_profiles.iter().map(|profile| {
                                render_profile_card(profile, profile_w, theme, cx)
                            })),
                    ),
            )
        })
        // Row 3: Manual query override + Remote / Freshness / Page size filters
        .child(
            div()
                .w(px(inner_w))
                .pt(px(12.0))
                .border_t_1()
                .border_color(theme.border_soft)
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(inner_w))
                        .flex()
                        .when(stack_input_row, |d| d.flex_col())
                        .when(!stack_input_row, |d| d.flex_row().items_center())
                        .gap(px(8.0))
                        .child(
                            render_decorated_input_box(
                                workspace.job_query_input.clone(),
                                34.0,
                                Some(IconName::Search),
                                None,
                                None,
                                theme,
                                cx,
                            )
                            .when(!stack_input_row, |d| d.flex_1())
                            .when(stack_input_row, |d| d.w(px(inner_w))),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .when(has_manual, |row| {
                                    row.child(render_button(
                                        "jobs-clear-query-btn",
                                        "Clear",
                                        Some(IconName::Close),
                                        ButtonVariant::Ghost,
                                        false,
                                        theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.clear_job_manual_query(cx);
                                            cx.notify();
                                        }),
                                    ))
                                })
                                .child({
                                    let needs_update = workspace.job_search_has_unapplied_changes();
                                    let disabled = workspace.job_listings_loading
                                        || (!has_manual
                                            && !workspace.job_profiles.iter().any(|p| p.enabled));
                                    let btn = render_button(
                                        "jobs-search-btn",
                                        if workspace.job_listings_loading {
                                            "Searching…"
                                        } else if needs_update {
                                            "Update search"
                                        } else {
                                            "Search jobs"
                                        },
                                        Some(IconName::Search),
                                        if needs_update {
                                            ButtonVariant::Primary
                                        } else {
                                            ButtonVariant::Secondary
                                        },
                                        disabled,
                                        theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.search_jobs(false, cx);
                                            cx.notify();
                                        }),
                                    )
                                    .tooltip(|_window, cx| {
                                        tooltip_view(
                                            "Search with selected profiles and filters (uses session cache when unchanged)",
                                            cx,
                                        )
                                    });

                                    if needs_update && !disabled {
                                        let glow = theme.focus_ring;
                                        div()
                                            .flex_shrink_0()
                                            .rounded(px(6.0))
                                            .child(btn)
                                            .with_animation(
                                                "jobs-search-btn-pulse",
                                                Animation::new(Duration::from_millis(1350))
                                                    .repeat()
                                                    .with_easing(bounce(ease_in_out)),
                                                move |wrapper, delta| {
                                                    let alpha = 0.14 + delta * 0.44;
                                                    let ring_color = Rgba {
                                                        r: glow.r,
                                                        g: glow.g,
                                                        b: glow.b,
                                                        a: alpha,
                                                    };
                                                    wrapper.shadow(vec![BoxShadow {
                                                        color: ring_color.into(),
                                                        offset: point(px(0.0), px(0.0)),
                                                        blur_radius: px(2.0 + delta * 6.0),
                                                        spread_radius: px(1.0 + delta * 2.5),
                                                    }])
                                                },
                                            )
                                            .into_any_element()
                                    } else {
                                        btn.into_any_element()
                                    }
                                }),
                        ),
                )
                // Filters row: Remote toggle + Freshness window + Per-page limit
                .child(
                    div()
                        .w(px(inner_w))
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            div()
                                .id("jobs-remote-toggle")
                                .flex_shrink_0()
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(10.0))
                                .py(px(4.0))
                                .rounded_full()
                                .border_1()
                                .border_color(if workspace.job_filters.remote_only {
                                    theme.accent
                                } else {
                                    theme.border
                                })
                                .bg(if workspace.job_filters.remote_only {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .hover(|s| s.border_color(theme.accent))
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if workspace.job_filters.remote_only {
                                    theme.accent_text
                                } else {
                                    theme.secondary
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_job_remote_filter(cx);
                                    cx.notify();
                                }))
                                .when(workspace.job_filters.remote_only, |el| {
                                    el.child(render_icon(IconName::Check, 11.5, theme.accent))
                                })
                                .child("Remote only"),
                        )
                        .child(div().flex_shrink_0().w(px(1.0)).h(px(16.0)).bg(theme.border))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child("Country:"),
                        )
                        .child({
                            let selected_country = workspace.job_filters.selected_country();
                            let active = selected_country.is_some();
                            let open = workspace.job_country_dropdown_open;
                            let trigger_label = match selected_country {
                                Some(c) => format!("{} ({})", c.name, c.code.to_ascii_uppercase()),
                                None => "All locations".to_string(),
                            };
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .id("jobs-country-dropdown-trigger")
                                        .flex_shrink_0()
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .px(px(10.0))
                                        .py(px(4.0))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(if active || open {
                                            theme.accent
                                        } else {
                                            theme.border
                                        })
                                        .bg(if active {
                                            theme.accent_soft
                                        } else {
                                            theme.input
                                        })
                                        .hover(|s| s.border_color(theme.accent))
                                        .text_size(px(12.0))
                                        .font_weight(if active {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::MEDIUM
                                        })
                                        .text_color(if active {
                                            theme.accent_text
                                        } else {
                                            theme.text
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_job_country_dropdown();
                                            cx.notify();
                                        }))
                                        .child(trigger_label)
                                        .child(render_icon(
                                            if open { IconName::Up } else { IconName::Down },
                                            11.0,
                                            if active { theme.accent } else { theme.muted },
                                        )),
                                )
                                .when(active, |wrap| {
                                    wrap.child(
                                        div()
                                            .id("jobs-country-clear-btn")
                                            .flex_shrink_0()
                                            .cursor_pointer()
                                            .size(px(22.0))
                                            .rounded_full()
                                            .border_1()
                                            .border_color(theme.border_soft)
                                            .bg(theme.input)
                                            .hover(|s| s.border_color(theme.accent).bg(theme.hover))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_job_country(None, cx);
                                                cx.notify();
                                            }))
                                            .child(render_icon(IconName::Close, 10.0, theme.muted)),
                                    )
                                })
                        })
                        .child(div().flex_shrink_0().w(px(1.0)).h(px(16.0)).bg(theme.border))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child("Posted:"),
                        )
                        .children(freshness_options.into_iter().map(|(days, label)| {
                            let active = workspace.job_filters.posted_within_days == days;
                            div()
                                .id(el_id(format!(
                                    "jobs-freshness-{}",
                                    days.map(|d| d.to_string())
                                        .unwrap_or_else(|| "any".to_string())
                                )))
                                .flex_shrink_0()
                                .cursor_pointer()
                                .px(px(9.0))
                                .py(px(4.0))
                                .rounded_full()
                                .border_1()
                                .border_color(if active {
                                    theme.accent
                                } else {
                                    theme.border_soft
                                })
                                .bg(if active {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .hover(|s| s.border_color(theme.accent))
                                .text_size(px(12.0))
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if active {
                                    theme.accent_text
                                } else {
                                    theme.secondary
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_job_freshness_days(days, cx);
                                    cx.notify();
                                }))
                                .child(label)
                        }))
                        .child(div().flex_shrink_0().w(px(1.0)).h(px(16.0)).bg(theme.border))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child("Per page:"),
                        )
                        .children(page_size_options.into_iter().map(|size| {
                            let active = workspace.job_filters.page_size == size;
                            div()
                                .id(el_id(format!("jobs-page-size-{size}")))
                                .flex_shrink_0()
                                .cursor_pointer()
                                .px(px(8.0))
                                .py(px(4.0))
                                .rounded_full()
                                .border_1()
                                .border_color(if active {
                                    theme.accent
                                } else {
                                    theme.border_soft
                                })
                                .bg(if active {
                                    theme.accent_soft
                                } else {
                                    theme.input
                                })
                                .hover(|s| s.border_color(theme.accent))
                                .text_size(px(12.0))
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if active {
                                    theme.accent_text
                                } else {
                                    theme.secondary
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_job_page_size(size, cx);
                                    cx.notify();
                                }))
                                .child(size.to_string())
                        })),
                )
                // Country / Location dropdown menu (rendered in-flow below filters)
                .when(workspace.job_country_dropdown_open, |el| {
                    el.child(render_country_dropdown_panel(workspace, inner_w, theme, cx))
                }),
        )
}

fn render_country_dropdown_panel(
    workspace: &WorkspaceView,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let panel_w = inner_w.clamp(240.0, 360.0);
    let list_h = 236.0_f32;
    let body_w = (panel_w - 18.0).max(200.0);
    let item_w = (body_w - 14.0).max(180.0);
    let base_scroll = workspace.job_country_base_scroll();
    let scroll_offset = base_scroll.offset();
    let scroll_max = base_scroll.max_offset();
    let scroll_bounds = base_scroll.bounds();
    let max_y = f32::from(scroll_max.height);
    let total_rows = JOB_COUNTRIES.len() + 1;

    div()
        .w(px(panel_w))
        .flex_shrink_0()
        .rounded(px(6.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .p(px(8.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .w(px(body_w))
                .pb(px(6.0))
                .border_b_1()
                .border_color(theme.border_soft)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.secondary)
                        .child("Select country (applies to API, LinkedIn & Indeed)"),
                )
                .child(render_icon_button(
                    "jobs-country-close-btn",
                    IconName::Close,
                    "Close",
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_job_country_dropdown();
                        cx.notify();
                    }),
                )),
        )
        .child(
            div()
                .relative()
                .w(px(body_w))
                .h(px(list_h))
                .overflow_hidden()
                .on_scroll_wheel(cx.listener(
                    |this, event: &ScrollWheelEvent, _window, cx| {
                        let delta = event.delta.pixel_delta(px(28.0));
                        if delta.y != px(0.0) && this.scroll_job_country_by(delta.y, cx) {
                            cx.stop_propagation();
                        }
                    },
                ))
                .child(
                    uniform_list(
                        "jobs-country-scroll-list",
                        total_rows,
                        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                            let t = &this.theme;
                            let sel_code = this.job_filters.country.as_deref();
                            let mut rows = Vec::with_capacity(range.len());
                            for ix in range {
                                let (id_suffix, label, code_badge, code_value, is_selected) =
                                    if ix == 0 {
                                        (
                                            "all".to_string(),
                                            "All locations (Worldwide)".to_string(),
                                            Some("ALL".to_string()),
                                            None,
                                            sel_code.is_none(),
                                        )
                                    } else {
                                        let country = &JOB_COUNTRIES[ix - 1];
                                        let is_sel = sel_code.is_some_and(|c| {
                                            c.eq_ignore_ascii_case(country.code)
                                        });
                                        (
                                            country.code.to_string(),
                                            country.name.to_string(),
                                            Some(country.code.to_ascii_uppercase()),
                                            Some(country.code.to_string()),
                                            is_sel,
                                        )
                                    };
                                let hover_bg = if is_selected {
                                    t.accent_soft
                                } else {
                                    t.hover
                                };
                                rows.push(
                                    div()
                                        .w(px(item_w))
                                        .h(px(32.0))
                                        .pb(px(2.0))
                                        .child(
                                            div()
                                                .id(el_id(format!(
                                                    "jobs-country-opt-{id_suffix}"
                                                )))
                                                .w_full()
                                                .h_full()
                                                .cursor_pointer()
                                                .px(px(10.0))
                                                .rounded(px(6.0))
                                                .bg(if is_selected {
                                                    t.accent_soft
                                                } else {
                                                    t.transparent
                                                })
                                                .border_1()
                                                .border_color(if is_selected {
                                                    t.accent
                                                } else {
                                                    t.transparent
                                                })
                                                .hover(move |s| s.bg(hover_bg))
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(8.0))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.set_job_country(code_value.clone(), cx);
                                                    cx.notify();
                                                }))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .text_size(px(12.0))
                                                        .font_weight(if is_selected {
                                                            FontWeight::SEMIBOLD
                                                        } else {
                                                            FontWeight::NORMAL
                                                        })
                                                        .text_color(if is_selected {
                                                            t.accent_text
                                                        } else {
                                                            t.text
                                                        })
                                                        .child(label),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.0))
                                                        .when_some(code_badge, |el, badge| {
                                                            el.child(
                                                                div()
                                                                    .px(px(5.0))
                                                                    .py(px(1.0))
                                                                    .rounded(px(4.0))
                                                                    .bg(t.input)
                                                                    .border_1()
                                                                    .border_color(t.border_soft)
                                                                    .text_size(px(11.0))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(t.muted)
                                                                    .child(badge),
                                                            )
                                                        })
                                                        .when(is_selected, |el| {
                                                            el.child(render_icon(
                                                                IconName::Check,
                                                                12.0,
                                                                t.accent,
                                                            ))
                                                        }),
                                                ),
                                        ),
                                );
                            }
                            rows
                        }),
                    )
                    .size_full()
                    .track_scroll(workspace.job_country_scroll.clone()),
                )
                .when(max_y > 1.0, |el| {
                    let view_h = f32::from(scroll_bounds.size.height).max(list_h);
                    let total_h = view_h + max_y;
                    let track_h = (view_h - 4.0).max(20.0);
                    let thumb_h = ((view_h / total_h) * track_h).clamp(24.0, track_h);
                    let progress = (-f32::from(scroll_offset.y) / max_y).clamp(0.0, 1.0);
                    let thumb_top = progress * (track_h - thumb_h);
                    el.child(
                        div()
                            .id("jobs-country-scrollbar-track")
                            .absolute()
                            .top(px(2.0))
                            .right(px(0.0))
                            .w(px(8.0))
                            .h(px(track_h))
                            .rounded(px(4.0))
                            .bg(theme.input)
                            .border_1()
                            .border_color(theme.border_soft)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    let handle = this.job_country_base_scroll();
                                    let bounds = handle.bounds();
                                    let rel = (f32::from(event.position.y - bounds.top()) - 2.0)
                                        / (f32::from(bounds.size.height) - 4.0).max(1.0);
                                    let max_h = f32::from(handle.max_offset().height);
                                    let cur = handle.offset();
                                    let next_y = px(-(rel.clamp(0.0, 1.0) * max_h));
                                    handle.set_offset(point(cur.x, next_y));
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
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
}

fn render_profile_card(
    profile: &SearchProfile,
    width: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let profile_id = profile.id.clone();
    let enabled = profile.enabled;

    div()
        .w(px(width))
        .flex_shrink_0()
        .p(px(12.0))
        .rounded(px(6.0))
        .bg(if enabled {
            theme.input
        } else {
            theme.surface_soft
        })
        .border_1()
        .border_color(if enabled {
            theme.accent
        } else {
            theme.border_soft
        })
        .opacity(if enabled { 1.0 } else { 0.62 })
        .flex()
        .flex_col()
        .gap(px(8.0))
        // Profile Header
        .child(
            div()
                .id(el_id(format!("job-profile-toggle-{}", profile.id)))
                .cursor_pointer()
                .rounded(px(4.0))
                .hover(|s| s.opacity(0.88))
                .flex()
                .items_start()
                .justify_between()
                .gap(px(8.0))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_job_profile(&profile_id, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_shrink_0()
                                .mt(px(2.0))
                                .size(px(16.0))
                                .rounded(px(4.0))
                                .border_1()
                                .border_color(if enabled { theme.accent } else { theme.border_control })
                                .bg(if enabled { theme.accent } else { theme.surface })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(enabled, |box_el| {
                                    box_el.child(render_icon(IconName::Check, 10.5, theme.on_accent))
                                }),
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
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.text)
                                        .child(profile.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.muted)
                                        .child(profile.label.clone()),
                                ),
                        ),
                ),
        )
        // Toggleable keyword chips
        .when(!profile.keywords.is_empty(), |card| {
            let pid = profile.id.clone();
            card.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap(px(5.0))
                    .children(profile.keywords.iter().enumerate().map(|(idx, kw)| {
                        let kw_enabled = !profile.disabled_keywords.contains(kw);
                        let pid_inner = pid.clone();
                        let kw_inner = kw.clone();
                        div()
                            .id(el_id(format!("job-kw-{}-{idx}", pid)))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded_full()
                            .border_1()
                            .border_color(if kw_enabled && enabled {
                                theme.border
                            } else {
                                theme.border_soft
                            })
                            .bg(if kw_enabled && enabled {
                                theme.surface
                            } else {
                                theme.transparent
                            })
                            .hover(|s| s.border_color(theme.accent))
                            .text_size(px(11.0))
                            .text_color(if kw_enabled && enabled {
                                theme.text
                            } else {
                                theme.muted
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_job_profile_keyword(&pid_inner, &kw_inner, cx);
                                cx.notify();
                            }))
                            .child(if kw_enabled {
                                kw.clone()
                            } else {
                                format!("× {kw}")
                            })
                    })),
            )
        })
}

fn render_results_section(
    workspace: &WorkspaceView,
    target_master_title: Option<&str>,
    has_active_query: bool,
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let shown = workspace.job_listings.len();
    let total = workspace.job_total_estimated;
    let page = workspace.job_filters.page.clamp(1, MAX_PAGE_NUMBER);
    let page_size = workspace.job_filters.page_size.max(1);
    let total_pages = if total > 0 {
        (total as usize)
            .div_ceil(page_size)
            .clamp(1, MAX_PAGE_NUMBER)
    } else if workspace.job_has_next_page {
        (page + 1).min(MAX_PAGE_NUMBER)
    } else {
        page
    };
    let range_start = (page - 1) * page_size + 1;
    let range_end = (page - 1) * page_size + shown;
    let show_pagination = shown > 0 && (page > 1 || workspace.job_has_next_page);
    let stack_results_header = card_w < 720.0;

    div()
        .w(px(card_w))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(12.0))
        // Results Header
        .child(
            div()
                .w(px(card_w))
                .flex()
                .when(stack_results_header, |d| d.flex_col().items_start())
                .when(!stack_results_header, |d| {
                    d.flex_row().items_center().justify_between()
                })
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(16.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Live opportunities"),
                        )
                        .when(shown > 0, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(8.0))
                                    .py(px(2.0))
                                    .rounded_full()
                                    .bg(theme.input)
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.secondary)
                                    .child(if total > shown as u64 {
                                        format!("Page {page} · {shown} shown · {total} matches")
                                    } else {
                                        format!(
                                            "{shown} {}",
                                            if shown == 1 { "role" } else { "roles" }
                                        )
                                    }),
                            )
                        })
                        .when(workspace.job_listings_from_cache && shown > 0, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(7.0))
                                    .py(px(2.0))
                                    .rounded(px(4.0))
                                    .bg(theme.input)
                                    .text_size(px(11.0))
                                    .text_color(theme.muted)
                                    .child("Cached · 10m"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .when_some(target_master_title, |el, master| {
                            el.child(format!("Tailor branches from: {master}"))
                        })
                        .child(
                            div()
                                .flex_shrink_0()
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded(px(4.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border_soft)
                                .text_size(px(11.0))
                                .text_color(theme.muted)
                                .child("Powered by Freehire.me"),
                        )
                        .child(
                            render_button(
                                "jobs-refresh-btn",
                                if workspace.job_listings_loading {
                                    "Refreshing…"
                                } else {
                                    "Refresh jobs"
                                },
                                Some(IconName::Search),
                                ButtonVariant::Secondary,
                                workspace.job_listings_loading || !has_active_query,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.search_jobs(true, cx);
                                    cx.notify();
                                }),
                            )
                            .tooltip(|_window, cx| {
                                tooltip_view(
                                    "Bypass cache and fetch fresh live jobs from Freehire.me",
                                    cx,
                                )
                            }),
                        ),
                ),
        )
        // Loading / Empty / Cards
        .child(if workspace.job_listings_loading || workspace.job_profiles_loading {
            div()
                .w(px(card_w))
                .flex_shrink_0()
                .p(px(28.0))
                .rounded(px(6.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .child(render_icon(IconName::Sparkles, 16.0, theme.accent))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child(if workspace.job_profiles_loading {
                            "Generating zero-PII search profiles from your CV…"
                        } else {
                            "Searching for live job opportunities…"
                        }),
                )
        } else if workspace.job_listings.is_empty() {
            let has_started = !workspace.job_profiles.is_empty()
                || !workspace.job_filters.manual_query.trim().is_empty();
            let is_24h_filter = workspace.job_filters.posted_within_days == Some(1);
            div()
                .w(px(card_w))
                .flex_shrink_0()
                .p(px(28.0))
                .rounded(px(6.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.0))
                .child(render_icon(IconName::Briefcase, 22.0, theme.muted))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(if has_started {
                            if is_24h_filter {
                                "No postings in the last 24 hours matched your profiles"
                            } else {
                                "No live postings matched your current filters"
                            }
                        } else {
                            "Ready to discover roles tailored to your CV"
                        }),
                )
                .child(
                    div()
                        .w(px((inner_w).min(540.0)))
                        .text_center()
                        .text_size(px(13.0))
                        .line_height(px(18.0))
                        .text_color(theme.secondary)
                        .child(if has_started {
                            "Try widening the posted window, turning off Remote only, toggling roles above, or clicking Refresh jobs."
                        } else {
                            "Set a Default CV in CV library for automatic 24h job discovery on launch, or pick a CV above to generate search profiles."
                        }),
                )
                .when(has_started && is_24h_filter, |el| {
                    el.child(render_button(
                        "jobs-widen-7d-btn",
                        "Try last 7 days",
                        Some(IconName::Search),
                        ButtonVariant::Secondary,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.set_job_freshness_days(Some(7), cx);
                            cx.notify();
                        }),
                    ))
                })
        } else {
            div()
                .w(px(card_w))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .children(workspace.job_listings.iter().enumerate().map(|(idx, job)| {
                    render_job_card(
                        idx,
                        job,
                        workspace.job_expanded_slugs.contains(&job.public_slug),
                        !workspace.documents.is_empty(),
                        card_w,
                        inner_w,
                        theme,
                        cx,
                    )
                }))
                .when(show_pagination, |list| {
                    list.child(
                        div()
                            .w(px(card_w))
                            .flex_shrink_0()
                            .mt(px(4.0))
                            .px(px(14.0))
                            .py(px(10.0))
                            .rounded(px(6.0))
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .justify_between()
                            .flex_wrap()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme.secondary)
                                    .child(format!(
                                        "Showing {range_start}–{range_end} · Page {page} of {total_pages}"
                                    )),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(render_button(
                                        "jobs-prev-page-btn",
                                        "Previous",
                                        None,
                                        ButtonVariant::Secondary,
                                        page <= 1 || workspace.job_listings_loading,
                                        theme,
                                        cx.listener(move |this, _, _, cx| {
                                            this.set_job_page(page.saturating_sub(1), cx);
                                            cx.notify();
                                        }),
                                    ))
                                    .child(render_button(
                                        "jobs-next-page-btn",
                                        "Next",
                                        Some(IconName::Arrow),
                                        ButtonVariant::Secondary,
                                        !workspace.job_has_next_page
                                            || page >= MAX_PAGE_NUMBER
                                            || workspace.job_listings_loading,
                                        theme,
                                        cx.listener(move |this, _, _, cx| {
                                            this.set_job_page(page + 1, cx);
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                })
        })
}

#[allow(clippy::too_many_arguments)]
fn render_job_card(
    idx: usize,
    job: &JobListing,
    is_expanded: bool,
    can_tailor: bool,
    card_w: f32,
    inner_w: f32,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let apply_url = job.apply_url.clone();
    let slug_for_toggle = job.public_slug.clone();
    let job_for_tailor = job.clone();
    let stack_card_header = inner_w < 580.0;

    let work_mode_badge = job.work_mode.as_deref().map(|mode| {
        let lower = mode.to_ascii_lowercase();
        if lower == "remote" {
            ("Remote", true)
        } else if lower == "hybrid" {
            ("Hybrid", false)
        } else if lower == "onsite" || lower == "on_site" {
            ("Onsite", false)
        } else {
            (mode, false)
        }
    });

    div()
        .w(px(card_w))
        .flex_shrink_0()
        .p(px(16.0))
        .rounded(px(6.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(10.0))
        // Header: Title + Company/Location + Action Buttons
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .when(stack_card_header, |d| d.flex_col().items_start())
                .when(!stack_card_header, |d| {
                    d.flex_row().items_start().justify_between()
                })
                .gap(px(10.0))
                .child(
                    div()
                        .when(!stack_card_header, |d| d.flex_1().min_w_0())
                        .when(stack_card_header, |d| d.w(px(inner_w)))
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_size(px(16.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(job.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_size(px(13.0))
                                .text_color(theme.secondary)
                                .child(format!("{} · {}", job.company, job.location)),
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
                            format!("job-open-btn-{idx}"),
                            "Open posting",
                            Some(IconName::Arrow),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_job_url(&apply_url);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            format!("job-tailor-btn-{idx}"),
                            "Tailor CV",
                            Some(IconName::Sparkles),
                            ButtonVariant::Primary,
                            !can_tailor,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.tailor_cv_for_job(job_for_tailor.clone(), cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Badges row: work mode, salary, seniority, age, matched profile
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(6.0))
                .when_some(work_mode_badge, |row, (label, is_remote)| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(if is_remote {
                                theme.accent_soft
                            } else {
                                theme.input
                            })
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if is_remote {
                                theme.accent_text
                            } else {
                                theme.secondary
                            })
                            .child(label.to_string()),
                    )
                })
                .when_some(job.salary_display.clone(), |row, salary| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(theme.add_bg)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.add_text)
                            .child(salary),
                    )
                })
                .when_some(job.seniority.clone(), |row, level| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(theme.input)
                            .text_size(px(11.0))
                            .text_color(theme.secondary)
                            .child(level),
                    )
                })
                .when_some(job.age_label.clone(), |row, age| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(theme.input)
                            .text_size(px(11.0))
                            .text_color(theme.muted)
                            .child(age),
                    )
                })
                .when_some(job.matched_profile.clone(), |row, profile_name| {
                    row.child(
                        div()
                            .flex_shrink_0()
                            .px(px(8.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(theme.border_soft)
                            .text_size(px(11.0))
                            .text_color(theme.muted)
                            .child(format!("Profile: {profile_name}")),
                    )
                }),
        )
        // Description preview
        .when(!job.preview.is_empty(), |card| {
            card.child(
                div()
                    .w(px(inner_w))
                    .text_size(px(13.0))
                    .line_height(px(18.5))
                    .text_color(theme.secondary)
                    .child(job.preview.clone()),
            )
        })
        // Skills tags + Expand full JD toggle
        .child(
            div()
                .w(px(inner_w))
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(160.0))
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(5.0))
                        .children(job.skills.iter().take(8).map(|skill| {
                            div()
                                .flex_shrink_0()
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded(px(4.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border_soft)
                                .text_size(px(11.0))
                                .text_color(theme.secondary)
                                .child(skill.clone())
                        })),
                )
                .when(!job.description_markdown.trim().is_empty(), |row| {
                    row.child(
                        div()
                            .id(el_id(format!("job-expand-jd-{idx}")))
                            .cursor_pointer()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.accent_text)
                            .hover(|s| s.text_color(theme.text))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_job_jd_expanded(&slug_for_toggle);
                                cx.notify();
                            }))
                            .child(if is_expanded {
                                "Hide full description"
                            } else {
                                "Show full description"
                            })
                            .child(render_icon(
                                if is_expanded {
                                    IconName::Up
                                } else {
                                    IconName::Down
                                },
                                11.0,
                                theme.accent,
                            )),
                    )
                }),
        )
        // Expanded Full Markdown JD
        .when(
            is_expanded && !job.description_markdown.trim().is_empty(),
            |card| {
                card.child(
                    div()
                        .w(px(inner_w))
                        .overflow_hidden()
                        .mt(px(4.0))
                        .p(px(12.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border_soft)
                        .text_size(px(12.0))
                        .line_height(px(18.0))
                        .text_color(theme.text)
                        .child(job.description_markdown.clone()),
                )
            },
        )
}
