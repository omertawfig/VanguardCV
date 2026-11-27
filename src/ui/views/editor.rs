use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::{
    AiProposal, AiProviderKind, BulletItem, CvDocument, CvSection, ItemId, SectionKind,
    TimelineEntry,
};
use crate::ui::components::ui::{render_checkbox,
    ButtonVariant, IconName, el_id, render_button, render_diff_text, render_empty_state,
    render_icon, render_icon_button, render_toggle,
};
use crate::ui::theme::{RADIUS_MD, Theme};
use crate::ui::workspace::{
    DragGhostView, EditorDragKind, EditorDragPayload, UiProposal, WorkspaceView, WorkspaceViewPage,
};

pub fn render_editor_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
    let theme = &workspace.theme;
    let Some(doc) = workspace.active_document() else {
        return div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(720.0))
            .mx_auto()
            .px(workspace.page_padding_x())
            .pt(px(48.0))
            .gap(px(16.0))
            .items_center()
            .child(render_empty_state(
                IconName::File,
                "No CV selected",
                "Open a CV from CV library to edit its sections and live preview.",
                theme,
            ))
            .child(render_button(
                "editor-empty-go-library",
                "Go to CV library",
                Some(IconName::Folder),
                ButtonVariant::Primary,
                false,
                theme,
                cx.listener(|this, _, _, cx| {
                    this.navigate_to(WorkspaceViewPage::Library, cx);
                    cx.notify();
                }),
            ));
    };
    let parent = doc.parent_id.and_then(|pid| workspace.find_document(pid));
    let is_child = doc.parent_id.is_some();
    let proposals = workspace.active_proposals();
    let ats_detail = workspace.active_ats_score().cloned();
    let stored_ats = doc.job_context.as_ref().and_then(|j| j.ats_match_score);
    let ai_running = workspace.ai_state.is_running;
    let ai_for_this_doc = workspace.ai_state.target_doc_id == Some(doc.id);
    let ai_error = if ai_for_this_doc {
        workspace.ai_state.error.clone()
    } else {
        None
    };
    let ai_step = workspace.ai_state.step.clone();
    let ai_stream = workspace.ai_state.streamed_text.clone();
    let ai_summary = workspace.active_ai_summary().map(str::to_string);

    // Proposals that target visibility toggles (rendered directly inside the review section)
    let visibility_proposals: Vec<UiProposal> = proposals
        .iter()
        .filter(|p| matches!(p.proposal, AiProposal::ToggleVisibility { .. }))
        .cloned()
        .collect();

    let pad_x = workspace.editor_padding_x();
    let can_split = workspace.can_show_split_preview();
    let split_active = workspace.is_split_preview_active();

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
        // Page heading (constant stacked layout so action buttons never jump between views)
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap(px(12.0))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        // Document kind line
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted)
                                .child(render_icon(
                                    if is_child {
                                        IconName::Branch
                                    } else {
                                        IconName::Shield
                                    },
                                    13.0,
                                    if is_child {
                                        theme.accent
                                    } else {
                                        theme.secondary
                                    },
                                ))
                                .child(if is_child {
                                    "Tailored variation"
                                } else {
                                    "Master CV"
                                })
                                .child("·")
                                .child(format!("{} sections", doc.body.sections.len()))
                                .when(!is_child, |el| {
                                    el.child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded(px(4.0))
                                            .bg(theme.accent_soft)
                                            .text_color(theme.accent_text)
                                            .text_size(px(11.0))
                                            .child("AI-protected"),
                                    )
                                }),
                        )
                        // Title + rename button
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .w_full()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(24.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text)
                                        .child(doc.title.clone()),
                                )
                                .child(div().flex_shrink_0().child(render_icon_button(
                                    "rename-cv-btn",
                                    IconName::Edit,
                                    "Rename CV",
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.open_rename_dialog(cx);
                                        cx.notify();
                                    }),
                                ))),
                        )
                        // Subtitle
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(theme.secondary)
                                .child(if is_child {
                                    format!(
                                        "Based on {}. Your master stays unchanged.",
                                        parent.map(|p| p.title.as_str()).unwrap_or("Master CV")
                                    )
                                } else {
                                    "Your complete career record. A protected starting point for every opportunity."
                                        .to_string()
                                }),
                        ),
                )
                // Top-right action buttons
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .when(is_child, |el| {
                            el.child(render_button(
                                "run-ai-tailor-btn",
                                if ai_running {
                                    "Tailoring…"
                                } else {
                                    "Run AI tailoring"
                                },
                                Some(IconName::Sparkles),
                                ButtonVariant::Secondary,
                                ai_running,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.start_ai_tailoring_for_active(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(render_button(
                                "compare-master-btn",
                                "Compare with master",
                                Some(IconName::Branch),
                                ButtonVariant::Secondary,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.open_compare_dialog();
                                    cx.notify();
                                }),
                            ))
                        })
                        .child(render_button(
                            "preview-pdf-btn",
                            if can_split {
                                if split_active {
                                    "Hide preview"
                                } else {
                                    "Live preview"
                                }
                            } else {
                                "Preview PDF"
                            },
                            Some(IconName::File),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.toggle_split_preview(cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "create-variation-btn",
                            "Create variation",
                            Some(IconName::Plus),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_variation_dialog(None, cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Personal Contact Header Card
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(12.0))
                .p(px(16.0))
                .rounded(px(6.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(200.0))
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(38.0))
                                .rounded(px(6.0))
                                .bg(theme.input)
                                .child(render_icon(IconName::Lock, 18.0, theme.accent)),
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
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text)
                                                .child("Contact header"),
                                        )
                                        .child(
                                            div()
                                                .px(px(6.0))
                                                .py(px(1.0))
                                                .rounded(px(4.0))
                                                .bg(theme.accent_soft)
                                                .text_color(theme.accent_text)
                                                .text_size(px(11.0))
                                                .child("Local only"),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(theme.secondary)
                                        .child(if doc.pii_header.full_name.is_empty() {
                                            "Add your name and contact information".to_string()
                                        } else {
                                            let title_str = doc
                                                .pii_header
                                                .headline_job_title
                                                .as_deref()
                                                .unwrap_or("");
                                            if title_str.is_empty() {
                                                doc.pii_header.full_name.clone()
                                            } else {
                                                format!("{} · {}", doc.pii_header.full_name, title_str)
                                            }
                                        }),
                                ),
                        ),
                )
                .child(render_button(
                    "edit-pii-btn",
                    "Edit details",
                    Some(IconName::Edit),
                    ButtonVariant::Ghost,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.navigate_to(WorkspaceViewPage::Privacy, cx);
                        cx.notify();
                    }),
                )),
        )
        // Live AI Progress / Error Banner
        .when((ai_for_this_doc && ai_running) || ai_error.is_some(), |el| {
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .p(px(14.0))
                    .rounded(px(6.0))
                    .bg(if ai_error.is_some() {
                        theme.del_bg
                    } else {
                        theme.surface
                    })
                    .border_1()
                    .border_color(if ai_error.is_some() {
                        theme.del_text
                    } else {
                        theme.accent
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_start()
                                    .gap(px(8.0))
                                    .child(
                                        div().flex_shrink_0().pt(px(2.0)).child(render_icon(
                                            if ai_error.is_some() {
                                                IconName::Shield
                                            } else {
                                                IconName::Sparkles
                                            },
                                            16.0,
                                            if ai_error.is_some() {
                                                theme.del_text
                                            } else {
                                                theme.accent
                                            },
                                        )),
                                    )
                                    .child({
                                        let is_limit = ai_error
                                            .as_deref()
                                            .is_some_and(crate::ai::is_ai_limit_error);
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
                                                    .text_color(if ai_error.is_some() {
                                                        theme.del_text
                                                    } else {
                                                        theme.text
                                                    })
                                                    .child(if is_limit {
                                                        "API limit reached".to_string()
                                                    } else if let Some(err) = ai_error.as_ref() {
                                                        format!("AI run failed: {err}")
                                                    } else {
                                                        ai_step.clone()
                                                    }),
                                            )
                                            .when(is_limit, |el| {
                                                el.child(
                                                    div()
                                                        .w_full()
                                                        .min_w_0()
                                                        .text_size(px(12.0))
                                                        .line_height(px(17.5))
                                                        .text_color(theme.text)
                                                        .child(
                                                            ai_error
                                                                .clone()
                                                                .unwrap_or_default(),
                                                        ),
                                                )
                                            })
                                    }),
                            )
                            .when(!ai_running, |hdr| {
                                let is_limit = ai_error
                                    .as_deref()
                                    .is_some_and(crate::ai::is_ai_limit_error);
                                let is_usage_limit = workspace.active_provider
                                    == AiProviderKind::ChatGpt
                                    && is_limit;
                                hdr.child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .when(is_limit, |actions| {
                                            actions.child(render_button(
                                                "editor-limit-settings-btn",
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
                                        .when(is_usage_limit, |actions| {
                                            actions.child(render_button(
                                                "editor-manage-usage-btn",
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
                                            "dismiss-ai-err-btn",
                                            IconName::Close,
                                            "Dismiss",
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.ai_state.error = None;
                                                cx.notify();
                                            }),
                                        )),
                                )
                            }),
                    )
                    .when(ai_running && !ai_stream.trim().is_empty(), |card| {
                        let snippet: String = ai_stream
                            .chars()
                            .rev()
                            .take(220)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        card.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .p(px(8.0))
                                .rounded(px(4.0))
                                .bg(theme.input)
                                .text_size(px(12.0))
                                .line_height(px(17.0))
                                .text_color(theme.secondary)
                                .child(snippet),
                        )
                    }),
            )
        })
        // ATS Alignment Score Card (variations only)
        .when(is_child && (ats_detail.is_some() || stored_ats.is_some()), |el| {
            let numeric = ats_detail
                .as_ref()
                .map(|s| s.score)
                .or(stored_ats)
                .unwrap_or(0);
            let is_high = numeric >= 75;
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .p(px(16.0))
                    .rounded(px(6.0))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .child(div().flex_shrink_0().child(render_icon(IconName::Search, 16.0, theme.accent)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.text)
                                            .child("ATS keyword alignment"),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(10.0))
                                    .py(px(3.0))
                                    .rounded(px(4.0))
                                    .bg(if is_high {
                                        theme.add_bg
                                    } else {
                                        theme.accent_soft
                                    })
                                    .text_color(if is_high {
                                        theme.add_text
                                    } else {
                                        theme.accent_text
                                    })
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child(format!("{numeric}% match")),
                            ),
                    )
                    .when_some(ats_detail, |card, score| {
                        card.when(!score.matched_keywords.is_empty(), |c| {
                            c.child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.muted)
                                            .child("Matched:"),
                                    )
                                    .children(score.matched_keywords.iter().map(|kw| {
                                        div()
                                            .px(px(7.0))
                                            .py(px(2.0))
                                            .rounded(px(4.0))
                                            .bg(theme.add_bg)
                                            .text_color(theme.add_text)
                                            .text_size(px(11.0))
                                            .child(kw.clone())
                                    })),
                            )
                        })
                        .when(!score.missing_keywords.is_empty(), |c| {
                            c.child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.muted)
                                            .child("Missing:"),
                                    )
                                    .children(score.missing_keywords.iter().map(|kw| {
                                        div()
                                            .px(px(7.0))
                                            .py(px(2.0))
                                            .rounded(px(4.0))
                                            .bg(theme.del_bg)
                                            .text_color(theme.del_text)
                                            .text_size(px(11.0))
                                            .child(kw.clone())
                                    })),
                            )
                        })
                    }),
            )
        })
        // AI Tailoring Reasoning Card (end-of-run explanation for the user)
        .when_some(
            if is_child && !ai_running {
                ai_summary
            } else {
                None
            },
            |el, summary_text| {
                let lines: Vec<String> = summary_text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect();
                el.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .p(px(16.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
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
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div().flex_shrink_0().child(render_icon(
                                                IconName::Sparkles,
                                                16.0,
                                                theme.accent,
                                            )),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.text)
                                                .child("AI tailoring reasoning"),
                                        ),
                                )
                                .child(div().flex_shrink_0().child(render_icon_button(
                                    "dismiss-ai-summary-btn",
                                    IconName::Close,
                                    "Dismiss reasoning",
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.dismiss_active_ai_summary();
                                        cx.notify();
                                    }),
                                ))),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .children(lines.into_iter().map(|line| {
                                    let is_heading = line.ends_with(':')
                                        && !line.starts_with('•')
                                        && !line.starts_with('-');
                                    let is_bullet = line.starts_with('•')
                                        || line.starts_with("- ")
                                        || line.starts_with("* ");
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .when(is_bullet, |d| d.pl(px(8.0)))
                                        .when(is_heading, |d| d.pt(px(4.0)))
                                        .text_size(px(13.0))
                                        .line_height(px(18.5))
                                        .font_weight(if is_heading {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if is_heading {
                                            theme.text
                                        } else {
                                            theme.secondary
                                        })
                                        .child(line)
                                })),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .pt(px(6.0))
                                .border_t_1()
                                .border_color(theme.border_soft)
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(theme.muted)
                                .child("Removed items are hidden from this variation's PDF and stay safely preserved in your Master CV."),
                        ),
                )
            },
        )
        // Review banner (when proposals exist)
        .when(!proposals.is_empty(), |el| {
            let mut review_stack = div().w_full().min_w_0().flex().flex_col().gap(px(10.0));
            review_stack = review_stack.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(12.0))
                    .p(px(14.0))
                    .rounded(px(6.0))
                    .bg(theme.accent_soft)
                    .border_1()
                    .border_color(theme.accent)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(div().flex_shrink_0().child(render_icon(IconName::Sparkles, 20.0, theme.accent)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.accent_text)
                                            .child(format!(
                                                "{} suggested {} to review",
                                                proposals.len(),
                                                if proposals.len() == 1 {
                                                    "change"
                                                } else {
                                                    "changes"
                                                }
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(theme.secondary)
                                            .child("Suggestions · Nothing changes until you accept."),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(render_button(
                                "dismiss-all-proposals-btn",
                                "Dismiss all",
                                None,
                                ButtonVariant::Ghost,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.dismiss_all_proposals();
                                    cx.notify();
                                }),
                            ))
                            .child(render_button(
                                "accept-all-proposals-btn",
                                "Accept all",
                                Some(IconName::Check),
                                ButtonVariant::Primary,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.accept_all_proposals(cx);
                                    cx.notify();
                                }),
                            )),
                    ),
            );

            // Render any ToggleVisibility proposals directly beneath the review banner
            for prop in &visibility_proposals {
                let label = match prop.proposal {
                    AiProposal::ToggleVisibility { enabled: false, .. } => {
                        "RECOMMENDED REMOVAL · HIDE FROM PDF"
                    }
                    AiProposal::ToggleVisibility { enabled: true, .. } => {
                        "RECOMMENDED RESTORE · INCLUDE IN PDF"
                    }
                    _ => "VISIBILITY PROPOSAL",
                };
                review_stack =
                    review_stack.child(render_proposal_card(workspace, prop, label, theme, cx));
            }

            el.child(review_stack)
        })
        // Section list header
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(10.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_size(px(16.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("CV sections"),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child(format!(
                                    "{} included in PDF",
                                    doc.body.sections.iter().filter(|s| s.enabled).count()
                                )),
                        ),
                )
                .child(render_button(
                    "add-section-top-btn",
                    "Add section",
                    Some(IconName::Plus),
                    ButtonVariant::Ghost,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.open_add_section_dialog(cx);
                        cx.notify();
                    }),
                )),
        )
        // Sections list
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .children(doc.body.sections.iter().map(|sec| {
                    render_section_card(workspace, doc, sec, is_child, theme, cx)
                }))
                .when(doc.body.sections.is_empty(), |el| {
                    el.child(render_empty_state(
                        IconName::File,
                        "Start with a section",
                        "Add your summary, experience, education, or skills.",
                        theme,
                    ))
                }),
        )
        // Footnote
        .child(
            div()
                .w_full()
                .min_w_0()
                .max_w(px(680.0))
                .mx_auto()
                .flex()
                .items_start()
                .gap(px(6.0))
                .pt(px(16.0))
                .px(px(8.0))
                .child(
                    div()
                        .flex_shrink_0()
                        .pt(px(2.5))
                        .child(render_icon(IconName::Grip, 12.0, theme.muted)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.0))
                        .line_height(px(16.5))
                        .text_color(theme.muted)
                        .child("Drag the six-dot handle on the left to reorder sections, entries, bullets, or skills · Only enabled items appear in your PDF"),
                ),
        )
}

fn render_proposal_card(
    workspace: &WorkspaceView,
    prop: &UiProposal,
    badge_label: &str,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let prop_id = prop.id.clone();
    let prop_id_revert = prop.id.clone();
    let prop_id_edit = prop.id.clone();
    let prop_id_save_draft = prop.id.clone();
    let prop_id_keep_edited = prop.id.clone();
    let has_warning = prop.pii_warning.is_some();
    let is_editable = prop.is_text_editable();
    let is_editing =
        is_editable && workspace.editing_proposal_id.as_deref() == Some(prop.id.as_str());
    let is_removal = matches!(
        prop.proposal,
        AiProposal::ToggleVisibility { enabled: false, .. }
    );
    let is_restore = matches!(
        prop.proposal,
        AiProposal::ToggleVisibility { enabled: true, .. }
    );
    let accent_color = if has_warning || is_removal {
        theme.del_text
    } else {
        theme.accent
    };
    let revert_label = if is_removal {
        "Keep in CV"
    } else if is_restore {
        "Keep hidden"
    } else {
        "Revert"
    };
    let accept_label = if is_removal {
        "Hide from PDF"
    } else if is_restore {
        "Include in PDF"
    } else {
        "Accept change"
    };
    let editor_height = match prop.proposal {
        AiProposal::SummaryRewrite { .. } => 150.0,
        AiProposal::SkillReorder { .. } => 96.0,
        _ => 112.0,
    };
    let edit_field_label = match prop.proposal {
        AiProposal::SkillReorder { .. } => "Edit proposed skills",
        AiProposal::SummaryRewrite { .. } => "Edit proposed summary",
        _ => "Edit proposed text",
    };

    div()
        .w_full()
        .min_w_0()
        .mt(px(8.0))
        .p(px(12.0))
        .rounded(px(6.0))
        .bg(theme.surface)
        .border_1()
        .border_color(accent_color)
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap(px(10.0))
        // Header badge row
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(6.0))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(accent_color)
                        .child(div().flex_shrink_0().child(render_icon(
                            if has_warning {
                                IconName::Shield
                            } else if is_removal {
                                IconName::Trash
                            } else {
                                IconName::Sparkles
                            },
                            12.0,
                            accent_color,
                        )))
                        .child(badge_label.to_string())
                        .when(prop.is_edited && !is_editing, |el| {
                            el.child(
                                div()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(theme.input)
                                    .border_1()
                                    .border_color(theme.border_soft)
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.secondary)
                                    .child("Edited"),
                            )
                        })
                        .when(is_editing, |el| {
                            el.child(
                                div()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(theme.accent_soft)
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.accent_text)
                                    .child("Editing"),
                            )
                        }),
                ),
        )
        // Full wrapped diff container
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(6.0))
                // Original ("-")
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px(px(10.0))
                        .py(px(8.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border_soft)
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_shrink_0()
                                .w(px(14.0))
                                .pt(px(1.0))
                                .text_center()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.del_text)
                                .child("-"),
                        )
                        .child(render_diff_text(
                            &prop.original,
                            &prop.proposed,
                            "before",
                            theme,
                        )),
                )
                // Proposed ("+") or inline full-text editor
                .when(!is_editing, |el| {
                    el.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(px(6.0))
                            .bg(theme.input)
                            .border_1()
                            .border_color(theme.border_soft)
                            .flex()
                            .items_start()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .w(px(14.0))
                                    .pt(px(1.0))
                                    .text_center()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.add_text)
                                    .child("+"),
                            )
                            .child(render_diff_text(
                                &prop.original,
                                &prop.proposed,
                                "after",
                                theme,
                            )),
                    )
                })
                .when(is_editing, |el| {
                    el.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(5.0))
                            .pt(px(2.0))
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .flex_wrap()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(edit_field_label),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.muted)
                                            .child("Adjust wording before keeping it"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .min_w_0()
                                    .h(px(editor_height))
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .rounded(px(6.0))
                                    .bg(theme.input)
                                    .border_1()
                                    .border_color(theme.accent)
                                    .overflow_hidden()
                                    .child(workspace.proposal_edit_input.clone()),
                            ),
                    )
                }),
        )
        // PII placeholder safety warning (if present)
        .when_some(prop.pii_warning.clone(), |el, warn| {
            el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .p(px(8.0))
                    .rounded(px(4.0))
                    .bg(theme.del_bg)
                    .text_size(px(12.0))
                    .line_height(px(17.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.del_text)
                    .child(warn),
            )
        })
        // Rationale
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_size(px(12.0))
                .line_height(px(17.0))
                .text_color(theme.secondary)
                .child(if prop.rationale.is_empty() {
                    "Facts and metrics preserved".to_string()
                } else {
                    prop.rationale.clone()
                }),
        )
        // Footer action bar
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_end()
                .flex_wrap()
                .gap(px(8.0))
                .pt(px(8.0))
                .border_t_1()
                .border_color(theme.border_soft)
                .when(!is_editing, |bar| {
                    bar.when(is_editable, |b| {
                        b.child(render_button(
                            format!("edit-prop-{}", prop.id),
                            "Edit",
                            Some(IconName::Edit),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(move |this, _, window, cx| {
                                this.begin_editing_proposal(&prop_id_edit, Some(window), cx);
                                cx.notify();
                            }),
                        ))
                    })
                    .child(render_button(
                        format!("revert-prop-{}", prop.id),
                        revert_label,
                        None,
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.resolve_proposal(&prop_id_revert, false, cx);
                            cx.notify();
                        }),
                    ))
                    .child(render_button(
                        format!("accept-prop-{}", prop.id),
                        accept_label,
                        Some(IconName::Check),
                        ButtonVariant::Primary,
                        has_warning,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.resolve_proposal(&prop_id, true, cx);
                            cx.notify();
                        }),
                    ))
                })
                .when(is_editing, |bar| {
                    bar.child(render_button(
                        format!("cancel-edit-prop-{}", prop.id),
                        "Cancel",
                        None,
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.cancel_editing_proposal();
                            cx.notify();
                        }),
                    ))
                    .child(render_button(
                        format!("save-draft-prop-{}", prop.id),
                        "Save draft",
                        None,
                        ButtonVariant::Secondary,
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.save_edited_proposal(&prop_id_save_draft, false, cx);
                            cx.notify();
                        }),
                    ))
                    .child(render_button(
                        format!("keep-edited-prop-{}", prop.id),
                        "Keep & accept",
                        Some(IconName::Check),
                        ButtonVariant::Primary,
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.save_edited_proposal(&prop_id_keep_edited, true, cx);
                            cx.notify();
                        }),
                    ))
                }),
        )
}

fn section_pending_proposals(sec: &CvSection, proposals: &[UiProposal]) -> usize {
    if proposals.is_empty() {
        return 0;
    }
    proposals
        .iter()
        .filter(|p| match &p.proposal {
            AiProposal::SummaryRewrite { section_id, .. } => *section_id == sec.id,
            AiProposal::SkillReorder { category_id, .. } => match &sec.kind {
                SectionKind::CategorizedInlineList { categories } => {
                    categories.iter().any(|c| c.id == *category_id)
                }
                _ => false,
            },
            AiProposal::BulletRewrite { bullet_id, .. } => match &sec.kind {
                SectionKind::TimelineEntries { entries } => entries
                    .iter()
                    .any(|e| e.bullets.iter().any(|b| b.id == *bullet_id)),
                SectionKind::SimpleBulletList { items } => {
                    items.iter().any(|b| b.id == *bullet_id)
                }
                _ => false,
            },
            AiProposal::ToggleVisibility { item_id, .. } => {
                *item_id == sec.id
                    || match &sec.kind {
                        SectionKind::TimelineEntries { entries } => entries.iter().any(|e| {
                            e.id == *item_id || e.bullets.iter().any(|b| b.id == *item_id)
                        }),
                        SectionKind::CategorizedInlineList { categories } => {
                            categories.iter().any(|c| c.id == *item_id)
                        }
                        SectionKind::SimpleBulletList { items } => {
                            items.iter().any(|b| b.id == *item_id)
                        }
                        SectionKind::Summary { .. } => false,
                    }
            }
        })
        .count()
}

fn section_count(sec: &CvSection) -> String {
    match &sec.kind {
        SectionKind::Summary { .. } => "Paragraph".to_string(),
        SectionKind::TimelineEntries { entries } => {
            let active = entries.iter().filter(|e| e.enabled).count();
            format!("{} of {} active", active, entries.len())
        }
        SectionKind::CategorizedInlineList { categories } => {
            let active = categories.iter().filter(|c| c.enabled).count();
            format!("{} of {} active", active, categories.len())
        }
        SectionKind::SimpleBulletList { items } => {
            let active = items.iter().filter(|i| i.enabled).count();
            format!("{} of {} active", active, items.len())
        }
    }
}

pub fn render_drag_handle(
    id: impl Into<SharedString>,
    payload: EditorDragPayload,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Stateful<Div> {
    let ghost_theme = theme.clone();
    let entity = cx.entity().downgrade();
    let drag_kind = payload.kind.clone();
    div()
        .id(el_id(id))
        .flex_shrink_0()
        .cursor_grab()
        .px(px(3.0))
        .py(px(4.0))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(theme.hover))
        .active(|s| s.cursor_grabbing())
        .on_drag(payload, move |drag, _offset, window, cx| {
            cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
            let kind = drag_kind.clone();
            let _ = entity.update(cx, |this, cx| {
                this.begin_editor_drag(kind);
                cx.notify();
            });
            let t = ghost_theme.clone();
            cx.new(move |_| DragGhostView::new(drag, t))
        })
        .child(render_icon(IconName::Grip, 14.0, theme.muted))
}

fn render_section_card(
    workspace: &WorkspaceView,
    doc: &CvDocument,
    sec: &CvSection,
    is_child: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let sec_id = sec.id;
    let sec_id_toggle = sec.id;
    let sec_id_rename = sec.id;
    let sec_id_delete = sec.id;
    let sec_id_body = sec.id;
    let sec_pending = section_pending_proposals(sec, &workspace.active_proposals());
    let drag_kind = EditorDragKind::Section { section_id: sec.id };
    let is_dragging = workspace.active_drag_item.as_ref() == Some(&drag_kind);
    let target_kind_move = drag_kind.clone();

    let is_open = workspace
        .expanded_sections
        .get(&sec.id)
        .copied()
        .unwrap_or_else(|| match &sec.kind {
            SectionKind::Summary { .. } => true,
            SectionKind::TimelineEntries { .. } => sec.title.to_lowercase().contains("experience"),
            _ => false,
        });

    let kind_icon = match &sec.kind {
        SectionKind::Summary { .. } => IconName::File,
        SectionKind::TimelineEntries { .. } => {
            if sec.title.to_lowercase().contains("education") {
                IconName::Graduation
            } else {
                IconName::Briefcase
            }
        }
        SectionKind::CategorizedInlineList { .. } => IconName::Code,
        SectionKind::SimpleBulletList { .. } => IconName::List,
    };

    let mut card = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .rounded(px(8.0))
        .overflow_hidden()
        .border_1()
        .border_color(if is_dragging {
            theme.accent
        } else {
            theme.border
        })
        .bg(if is_dragging {
            theme.accent_soft
        } else {
            theme.surface
        })
        .when(is_dragging, |s| s.opacity(0.72).shadow_md())
        .when(!sec.enabled && !is_dragging, |s| s.opacity(0.65))
        .on_drag_move::<EditorDragPayload>(cx.listener(
            move |this, event: &DragMoveEvent<EditorDragPayload>, _window, cx| {
                let dragged = event.drag(cx).kind.clone();
                this.handle_editor_drag_move(
                    &dragged,
                    &target_kind_move,
                    event.event.position,
                    event.bounds,
                    cx,
                );
            },
        ))
        .on_drop::<EditorDragPayload>(cx.listener(|this, _, _window, cx| {
            this.finish_editor_drag(cx);
        }));

    // Section Header row
    card = card.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(10.0))
            .min_h(px(52.0))
            .py(px(10.0))
            .px(px(14.0))
            .bg(if is_dragging {
                theme.accent_soft
            } else {
                theme.surface
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(160.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .overflow_hidden()
                    // Six-dot drag handle
                    .child(render_drag_handle(
                        format!("drag-sec-{}", sec.id),
                        EditorDragPayload {
                            kind: drag_kind,
                            label: sec.title.clone(),
                            sublabel: Some(section_count(sec)),
                        },
                        theme,
                        cx,
                    ))
                    // Section icon badge
                    .child(
                        div()
                            .flex_shrink_0()
                            .size(px(28.0))
                            .rounded(px(6.0))
                            .bg(theme.input)
                            .border_1()
                            .border_color(theme.border_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_icon(kind_icon, 14.0, theme.secondary)),
                    )
                    // Title & count button
                    .child(
                        div()
                            .id(el_id(format!("sec-toggle-{}", sec.id)))
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_x(px(8.0))
                            .gap_y(px(4.0))
                            .rounded(px(4.0))
                            .hover(|s| s.opacity(0.85))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_section_expand(sec_id);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .child(sec.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(8.0))
                                    .py(px(2.0))
                                    .rounded_full()
                                    .bg(theme.input)
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.secondary)
                                    .child(section_count(sec)),
                            )
                            .when(sec_pending > 0, |el| {
                                el.child(
                                    div()
                                        .flex_shrink_0()
                                        .px(px(6.0))
                                        .py(px(1.0))
                                        .rounded(px(4.0))
                                        .bg(theme.accent_soft)
                                        .text_color(theme.accent_text)
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::BOLD)
                                        .child(format!("{sec_pending} pending")),
                                )
                            }),
                    ),
            )
            // Controls: Rename, Delete, Up/Down reorder, Include toggle, Chevron
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(render_icon_button(
                        format!("sec-rename-{}", sec.id),
                        IconName::Edit,
                        "Rename section",
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.open_rename_section_dialog(sec_id_rename, cx);
                            cx.notify();
                        }),
                    ))
                    .child(render_icon_button(
                        format!("sec-delete-{}", sec.id),
                        IconName::Trash,
                        "Delete section",
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.delete_section(sec_id_delete, cx);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .w(px(1.0))
                            .h(px(18.0))
                            .mx(px(2.0))
                            .bg(theme.border_soft),
                    )
                    .child(render_toggle(
                        format!("sec-toggle-enabled-{}", sec.id),
                        sec.enabled,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_section_enabled(sec_id_toggle, cx);
                            cx.notify();
                        }),
                    ))
                    .child(render_icon_button(
                        format!("sec-chevron-{}", sec.id),
                        if is_open {
                            IconName::Down
                        } else {
                            IconName::Chevron
                        },
                        if is_open {
                            "Collapse section"
                        } else {
                            "Expand section"
                        },
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_section_expand(sec_id_body);
                            cx.notify();
                        }),
                    )),
            ),
    );

    // Section body (when expanded)
    if is_open {
        card = card.child(
            div()
                .w_full()
                .min_w_0()
                .p(px(16.0))
                .border_t_1()
                .border_color(theme.border_soft)
                .bg(theme.surface_soft)
                .child(render_section_content(
                    workspace, doc, sec, is_child, theme, cx,
                )),
        );
    }

    card
}

fn render_section_content(
    workspace: &WorkspaceView,
    _doc: &CvDocument,
    sec: &CvSection,
    is_child: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let sec_id_for_add = sec.id;

    match &sec.kind {
        SectionKind::Summary { text } => {
            let summary_sec_id = sec.id;
            let proposal = workspace
                .find_proposal_for_item(sec.id, text)
                .filter(|p| matches!(p.proposal, AiProposal::SummaryRewrite { .. }));

            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .p(px(14.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(13.0))
                        .line_height(px(21.0))
                        .text_color(if text.is_empty() {
                            theme.muted
                        } else {
                            theme.text
                        })
                        .child(if text.is_empty() {
                            "Write a concise introduction to your experience…".to_string()
                        } else {
                            text.clone()
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(160.0))
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child("A short, factual introduction to your professional experience."),
                        )
                        .child(render_button(
                            format!("edit-summary-{}", sec.id),
                            "Edit text",
                            Some(IconName::Edit),
                            ButtonVariant::Ghost,
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_edit_summary_dialog(summary_sec_id, cx);
                                cx.notify();
                            }),
                        )),
                )
                .when_some(proposal, |el, prop| {
                    el.child(render_proposal_card(
                        workspace,
                        &prop,
                        "SUMMARY PROPOSAL",
                        theme,
                        cx,
                    ))
                })
        }
        SectionKind::TimelineEntries { entries } => {
            let mut list = div().w_full().min_w_0().flex().flex_col().gap(px(12.0));
            if entries.is_empty() {
                list = list.child(
                    div()
                        .p(px(10.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .child("No entries yet. Click below to add your first entry."),
                );
            }
            for entry in entries {
                list = list.child(render_timeline_entry(
                    workspace, sec, entry, is_child, theme, cx,
                ));
            }

            let lower_title = sec.title.to_lowercase();
            let add_label = if lower_title.contains("education") {
                "Add education"
            } else if lower_title.contains("experience") {
                "Add experience"
            } else {
                "Add entry"
            };
            list.child(div().pt(px(6.0)).flex().items_center().child(render_button(
                format!("add-entry-{}", sec.id),
                add_label,
                Some(IconName::Plus),
                ButtonVariant::Ghost,
                false,
                theme,
                cx.listener(move |this, _, _, cx| {
                    this.open_add_entry_dialog(sec_id_for_add, cx);
                    cx.notify();
                }),
            )))
        }
        SectionKind::CategorizedInlineList { categories } => {
            let mut list = div().w_full().min_w_0().flex().flex_col().gap(px(8.0));
            if categories.is_empty() {
                list = list.child(
                    div()
                        .p(px(10.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .child("No skill categories yet. Add a category to group related skills."),
                );
            }
            for cat in categories {
                let cat_sec_id = sec.id;
                let cat_sec_id_edit = sec.id;
                let cat_sec_id_toggle = sec.id;
                let cat_id_promote = cat.id;
                let cat_id_toggle = cat.id;
                let cat_clone = cat.clone();
                let proposal = workspace
                    .find_proposal_for_item(cat.id, "")
                    .filter(|p| matches!(p.proposal, AiProposal::SkillReorder { .. }));

                let cat_drag_kind = EditorDragKind::SkillCategory {
                    section_id: sec.id,
                    category_id: cat.id,
                };
                let cat_is_dragging =
                    workspace.active_drag_item.as_ref() == Some(&cat_drag_kind);
                let cat_target_move = cat_drag_kind.clone();

                let mut cat_block = div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(12.0))
                    .rounded(px(6.0))
                    .bg(if cat_is_dragging {
                        theme.accent_soft
                    } else {
                        theme.surface
                    })
                    .border_1()
                    .border_color(if cat_is_dragging {
                        theme.accent
                    } else {
                        theme.border
                    })
                    .when(cat_is_dragging, |s| s.opacity(0.72).shadow_md())
                    .when(!cat.enabled && !cat_is_dragging, |s| s.opacity(0.6))
                    .on_drag_move::<EditorDragPayload>(cx.listener(
                        move |this, event: &DragMoveEvent<EditorDragPayload>, _window, cx| {
                            let dragged = event.drag(cx).kind.clone();
                            this.handle_editor_drag_move(
                                &dragged,
                                &cat_target_move,
                                event.event.position,
                                event.bounds,
                                cx,
                            );
                        },
                    ))
                    .on_drop::<EditorDragPayload>(cx.listener(|this, _, _window, cx| {
                        this.finish_editor_drag(cx);
                    }))
                    .child(
                        div()
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
                                    .gap(px(8.0))
                                    .child(render_drag_handle(
                                        format!("drag-cat-{}", cat.id),
                                        EditorDragPayload {
                                            kind: cat_drag_kind,
                                            label: cat.category_label.clone(),
                                            sublabel: Some(cat.joined_items()),
                                        },
                                        theme,
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .flex_wrap()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .text_size(px(14.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.text)
                                                    .child(cat.category_label.clone()),
                                            )
                                            .child(
                                                div()
                                                    .flex_shrink_0()
                                                    .px(px(7.0))
                                                    .py(px(1.5))
                                                    .rounded_full()
                                                    .bg(theme.input)
                                                    .text_size(px(11.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(theme.muted)
                                                    .child(format!(
                                                        "{} {}",
                                                        cat.items.len(),
                                                        if cat.items.len() == 1 {
                                                            "skill"
                                                        } else {
                                                            "skills"
                                                        }
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .when(is_child, |el| {
                                        el.child(render_icon_button(
                                            format!("promote-cat-{}", cat.id),
                                            IconName::Up,
                                            "Promote category to master",
                                            false,
                                            theme,
                                            cx.listener(move |this, _, _, cx| {
                                                this.open_promote_dialog(
                                                    cat_sec_id,
                                                    cat_id_promote,
                                                    None,
                                                );
                                                cx.notify();
                                            }),
                                        ))
                                    })
                                    .child(render_toggle(
                                        format!("toggle-cat-{}", cat.id),
                                        cat.enabled,
                                        theme,
                                        cx.listener(move |this, _, _, cx| {
                                            this.toggle_category_enabled(
                                                cat_sec_id_toggle,
                                                cat_id_toggle,
                                                cx,
                                            );
                                            cx.notify();
                                        }),
                                    ))
                                    .child(render_icon_button(
                                        format!("edit-cat-{}", cat.id),
                                        IconName::Edit,
                                        "Edit skill category",
                                        false,
                                        theme,
                                        cx.listener(move |this, _, _, cx| {
                                            this.open_edit_category_dialog(
                                                cat_sec_id_edit,
                                                &cat_clone,
                                                cx,
                                            );
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(28.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.0))
                            .children(cat.items.iter().map(|skill| {
                                div()
                                    .px(px(8.0))
                                    .py(px(3.0))
                                    .rounded(px(4.0))
                                    .bg(theme.input)
                                    .border_1()
                                    .border_color(theme.border_soft)
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(skill.clone())
                            }))
                            .when(cat.items.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(theme.muted)
                                        .child("No skills added yet."),
                                )
                            }),
                    );

                if let Some(prop) = proposal {
                    cat_block = cat_block.child(render_proposal_card(
                        workspace,
                        &prop,
                        "SKILL REORDER PROPOSAL",
                        theme,
                        cx,
                    ));
                }

                list = list.child(cat_block);
            }

            list.child(div().pt(px(6.0)).flex().items_center().child(render_button(
                format!("add-cat-{}", sec.id),
                "Add category",
                Some(IconName::Plus),
                ButtonVariant::Ghost,
                false,
                theme,
                cx.listener(move |this, _, _, cx| {
                    this.open_add_category_dialog(sec_id_for_add, cx);
                    cx.notify();
                }),
            )))
        }
        SectionKind::SimpleBulletList { items } => {
            let mut list = div().w_full().min_w_0().flex().flex_col().gap(px(6.0));
            if items.is_empty() {
                list = list.child(
                    div()
                        .p(px(10.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .child("No items yet. Click \"Add item\" below to get started."),
                );
            }
            for item in items {
                list = list.child(render_bullet_row(
                    workspace, sec, None, item, is_child, theme, cx,
                ));
            }

            list.child(div().pt(px(6.0)).flex().items_center().child(render_button(
                format!("add-bullet-{}", sec.id),
                "Add item",
                Some(IconName::Plus),
                ButtonVariant::Ghost,
                false,
                theme,
                cx.listener(move |this, _, _, cx| {
                    this.open_add_bullet_dialog(sec_id_for_add, None, cx);
                    cx.notify();
                }),
            )))
        }
    }
}

fn render_timeline_entry(
    workspace: &WorkspaceView,
    sec: &CvSection,
    entry: &TimelineEntry,
    is_child: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let entry_open = workspace
        .expanded_entries
        .get(&entry.id)
        .copied()
        .unwrap_or(true);
    let entry_id = entry.id;
    let entry_id_toggle = entry.id;
    let entry_id_open = entry.id;
    let sec_id = sec.id;
    let entry_clone = entry.clone();

    let entry_drag_kind = EditorDragKind::TimelineEntry {
        section_id: sec.id,
        entry_id: entry.id,
    };
    let entry_is_dragging = workspace.active_drag_item.as_ref() == Some(&entry_drag_kind);
    let entry_target_move = entry_drag_kind.clone();

    let date_text = entry.date_range();
    let active_bullets = entry.bullets.iter().filter(|b| b.enabled).count();
    let total_bullets = entry.bullets.len();
    let secondary_text = entry.secondary_heading.as_deref().unwrap_or("").trim().to_string();

    let mut container = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .rounded(px(RADIUS_MD))
        .overflow_hidden()
        .border_1()
        .border_color(if entry_is_dragging {
            theme.accent
        } else {
            theme.border
        })
        .bg(if entry_is_dragging {
            theme.accent_soft
        } else {
            theme.surface
        })
        .when(entry_is_dragging, |s| s.opacity(0.72).shadow_md())
        .when(!entry.enabled && !entry_is_dragging, |s| s.opacity(0.6))
        .on_drag_move::<EditorDragPayload>(cx.listener(
            move |this, event: &DragMoveEvent<EditorDragPayload>, _window, cx| {
                let dragged = event.drag(cx).kind.clone();
                this.handle_editor_drag_move(
                    &dragged,
                    &entry_target_move,
                    event.event.position,
                    event.bounds,
                    cx,
                );
            },
        ))
        .on_drop::<EditorDragPayload>(cx.listener(|this, _, _window, cx| {
            this.finish_editor_drag(cx);
        }));

    // Entry header row
    container = container.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(10.0))
            .px(px(14.0))
            .py(px(12.0))
            .child(
                div()
                    .flex_1()
                    .min_w(px(180.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    // Paired drag grip + checkbox cluster
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(render_drag_handle(
                                format!("drag-entry-{}", entry.id),
                                EditorDragPayload {
                                    kind: entry_drag_kind,
                                    label: if entry.primary_heading.is_empty() {
                                        "Untitled entry".to_string()
                                    } else {
                                        entry.primary_heading.clone()
                                    },
                                    sublabel: entry.secondary_heading.clone(),
                                },
                                theme,
                                cx,
                            ))
                            .child(render_checkbox(
                                format!("entry-chk-{}", entry.id),
                                entry.enabled,
                                16.0,
                                theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_entry_enabled(sec_id, entry_id_toggle, cx);
                                    cx.notify();
                                }),
                            )),
                    )
                    // Headings
                    .child(
                        div()
                            .id(el_id(format!("entry-title-{}", entry.id)))
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .rounded(px(4.0))
                            .hover(|s| s.opacity(0.85))
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_entry_expand(entry_id_open);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .child(if entry.primary_heading.is_empty() {
                                        "Untitled entry".to_string()
                                    } else {
                                        entry.primary_heading.clone()
                                    }),
                            )
                            .when(!secondary_text.is_empty() || (!entry_open && total_bullets > 0), |el| {
                                let subtitle = if !entry_open && total_bullets > 0 {
                                    let bullet_label = if total_bullets == 1 {
                                        format!("{active_bullets} of 1 achievement")
                                    } else {
                                        format!("{active_bullets} of {total_bullets} achievements")
                                    };
                                    if secondary_text.is_empty() {
                                        bullet_label
                                    } else {
                                        format!("{secondary_text} · {bullet_label}")
                                    }
                                } else {
                                    secondary_text.clone()
                                };
                                el.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(theme.secondary)
                                        .child(subtitle),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(!date_text.trim().is_empty(), |el| {
                        el.child(
                            div()
                                .px(px(8.0))
                                .py(px(3.0))
                                .rounded(px(4.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border_soft)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child(date_text),
                        )
                    })
                    .when(is_child, |el| {
                        let sec_id_p = sec.id;
                        let eid_p = entry.id;
                        el.child(render_icon_button(
                            format!("promote-entry-{}", entry.id),
                            IconName::Up,
                            "Promote entry to master",
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_promote_dialog(sec_id_p, eid_p, None);
                                cx.notify();
                            }),
                        ))
                    })
                    .child({
                        let sec_id_edit = sec.id;
                        render_icon_button(
                            format!("edit-entry-{}", entry.id),
                            IconName::Edit,
                            "Edit entry",
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_edit_entry_dialog(sec_id_edit, &entry_clone, cx);
                                cx.notify();
                            }),
                        )
                    })
                    .child(render_icon_button(
                        format!("chevron-entry-{}", entry.id),
                        if entry_open {
                            IconName::Down
                        } else {
                            IconName::Chevron
                        },
                        "Toggle entry achievements",
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_entry_expand(entry_id);
                            cx.notify();
                        }),
                    )),
            ),
    );

    // Bullets list when open
    if entry_open {
        let sec_id_b = sec.id;
        let entry_id_b = entry.id;

        let mut bullets_div = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .px(px(14.0))
            .py(px(12.0))
            .bg(theme.surface_soft)
            .border_t_1()
            .border_color(theme.border_soft);

        if entry.bullets.is_empty() {
            bullets_div = bullets_div.child(
                div()
                    .p(px(8.0))
                    .rounded(px(4.0))
                    .bg(theme.input)
                    .text_size(px(12.0))
                    .text_color(theme.muted)
                    .child("No achievements yet. Add measurable accomplishments for this role."),
            );
        }

        for bullet in &entry.bullets {
            bullets_div = bullets_div.child(render_bullet_row(
                workspace,
                sec,
                Some(entry.id),
                bullet,
                is_child,
                theme,
                cx,
            ));
        }

        bullets_div = bullets_div.child(
            div().pt(px(4.0)).flex().items_center().child(render_button(
                format!("add-bullet-to-{}", entry.id),
                "Add achievement",
            Some(IconName::Plus),
            ButtonVariant::Ghost,
            false,
            theme,
                cx.listener(move |this, _, _, cx| {
                    this.open_add_bullet_dialog(sec_id_b, Some(entry_id_b), cx);
                    cx.notify();
                }),
            )),
        );

        container = container.child(bullets_div);
    }

    container
}

fn render_bullet_row(
    workspace: &WorkspaceView,
    sec: &CvSection,
    entry_id: Option<ItemId>,
    item: &BulletItem,
    is_child: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let proposal = workspace
        .find_proposal_for_item(item.id, &item.text)
        .filter(|p| matches!(p.proposal, AiProposal::BulletRewrite { .. }));
    let sec_id = sec.id;
    let item_id_chk = item.id;
    let item_clone = item.clone();
    let ai_running = workspace.ai_state.is_running;

    let bullet_drag_kind = EditorDragKind::Bullet {
        section_id: sec.id,
        entry_id,
        bullet_id: item.id,
    };
    let bullet_is_dragging = workspace.active_drag_item.as_ref() == Some(&bullet_drag_kind);
    let bullet_target_move = bullet_drag_kind.clone();
    let bullet_preview: String = {
        let trimmed = item.text.trim();
        if trimmed.chars().count() <= 56 {
            trimmed.to_string()
        } else {
            let mut s: String = trimmed.chars().take(56).collect();
            s.push('…');
            s
        }
    };

    let mut block = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(6.0))
        .bg(if bullet_is_dragging {
            theme.accent_soft
        } else {
            theme.surface
        })
        .border_1()
        .border_color(if bullet_is_dragging {
            theme.accent
        } else {
            theme.border_soft
        })
        .when(bullet_is_dragging, |s| s.opacity(0.72).shadow_md())
        .when(!bullet_is_dragging, |s| {
            let hover_border = theme.border_control;
            s.hover(move |h| h.border_color(hover_border))
        })
        .when(!item.enabled && !bullet_is_dragging, |s| s.opacity(0.55))
        .on_drag_move::<EditorDragPayload>(cx.listener(
            move |this, event: &DragMoveEvent<EditorDragPayload>, _window, cx| {
                let dragged = event.drag(cx).kind.clone();
                this.handle_editor_drag_move(
                    &dragged,
                    &bullet_target_move,
                    event.event.position,
                    event.bounds,
                    cx,
                );
            },
        ))
        .on_drop::<EditorDragPayload>(cx.listener(|this, _, _window, cx| {
            this.finish_editor_drag(cx);
        }));

    // Bullet text row
    block = block.child(
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(10.0))
            .min_w_0()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    // Paired drag grip + checkbox cluster aligned with first line of text
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(render_drag_handle(
                                format!("drag-bullet-{}", item.id),
                                EditorDragPayload {
                                    kind: bullet_drag_kind,
                                    label: if bullet_preview.is_empty() {
                                        "Achievement".to_string()
                                    } else {
                                        bullet_preview
                                    },
                                    sublabel: None,
                                },
                                theme,
                                cx,
                            ))
                            .child(render_checkbox(
                                format!("bullet-chk-{}", item.id),
                                item.enabled,
                                16.0,
                                theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_bullet_enabled(sec_id, entry_id, item_id_chk, cx);
                                    cx.notify();
                                }),
                            )),
                    )
                    // Bullet text (top-padded 2.5px so 19px first-line center matches 24px controls)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pt(px(2.5))
                            .text_size(px(13.0))
                            .line_height(px(19.0))
                            .text_color(theme.text)
                            .child(item.text.clone()),
                    ),
            )
            // Bullet actions (aligned with first-line horizontal axis)
            .child(
                div()
                    .flex_shrink_0()
                    .mt(px(-3.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .when(is_child, |el| {
                        let sec_id_p = sec.id;
                        let iid_p = item.id;
                        el.child(render_icon_button(
                            format!("promote-bullet-{}", item.id),
                            IconName::Up,
                            "Promote achievement to master",
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_promote_dialog(sec_id_p, iid_p, entry_id);
                                cx.notify();
                            }),
                        ))
                    })
                    .when(is_child && proposal.is_none(), |el| {
                        let sec_id_s = sec.id;
                        let bullet_id_s = item.id;
                        el.child(render_icon_button(
                            format!("suggest-bullet-{}", item.id),
                            IconName::Sparkles,
                            "Rewrite achievement with AI (choose skills)",
                            ai_running,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_rewrite_bullet_dialog(sec_id_s, bullet_id_s, cx);
                                cx.notify();
                            }),
                        ))
                    })
                    .child({
                        let sec_id_edit = sec.id;
                        render_icon_button(
                            format!("edit-bullet-{}", item.id),
                            IconName::Edit,
                            "Edit achievement",
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.open_edit_bullet_dialog(
                                    sec_id_edit,
                                    entry_id,
                                    &item_clone,
                                    cx,
                                );
                                cx.notify();
                            }),
                        )
                    }),
            ),
    );

    // Proposal Diff Card if present
    if let Some(prop) = proposal {
        block = block.child(render_proposal_card(
            workspace,
            &prop,
            "AI PROPOSAL",
            theme,
            cx,
        ));
    }

    block
}
