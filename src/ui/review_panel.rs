//! AI Agent progress and proposal review panel.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Renders:
//! - Streaming AI progress: status line, streamed text, tool calls, live activity indicator
//! - ATS match score card with matched and missing keyword chips
//! - Reviewable list of AI proposals with word-level inline diffs (via `diff_engine`)
//! - Per-proposal Accept / Reject actions and bulk Accept All
//! - Zero-trust PII air-gap safeguard: blocks proposals containing `[REDACTED_PII_` tokens
//! - Graceful error handling and dismissal for unconfigured AI or failed runs

use gpui::{
    Context, IntoElement, MouseButton, MouseDownEvent, SharedString, div,
    prelude::*, px, rgb,
};

use crate::ai::diff_engine::{DiffSpan, diff_words};
use crate::contracts::{AiProposal, AtsScore};
use crate::theme;
use crate::ui::workspace::{AiAgentState, ProposalItem, ProposalStatus, WorkspaceView};

/// Checks if an AI proposal contains any unredacted PII placeholders.
pub fn proposal_has_pii_placeholder(proposal: &AiProposal) -> bool {
    const PII_TOKEN: &str = "[REDACTED_PII_";
    match proposal {
        AiProposal::BulletRewrite { proposed, .. } => proposed.contains(PII_TOKEN),
        AiProposal::SummaryRewrite { proposed, .. } => proposed.contains(PII_TOKEN),
        AiProposal::SkillReorder { items, .. } => {
            items.iter().any(|item| item.contains(PII_TOKEN))
        }
        AiProposal::ToggleVisibility { reason, .. } => reason.contains(PII_TOKEN),
    }
}

/// Renders the AI review and streaming progress panel as a dedicated section
/// above the editor.
pub fn render_ai_review_panel(
    state: &AiAgentState,
    cx: &mut Context<WorkspaceView>,
) -> impl IntoElement {
    let c = theme::colors();
    let t = theme::typography();

    let mut panel = div()
        .id("ai-review-panel")
        .flex()
        .flex_col()
        .w_full()
        .max_h(px(420.0))
        .bg(rgb(c.bg_surface))
        .border_b_1()
        .border_color(rgb(c.border_subtle))
        .p(px(14.0))
        .gap(px(10.0))
        .overflow_y_scroll();

    // 1. Top Header Bar
    let pending_count = state
        .proposals
        .iter()
        .filter(|p| p.status == ProposalStatus::Pending && p.warning.is_none())
        .count();
    let total_proposals = state.proposals.len();

    let mut header = div().flex().items_center().justify_between();

    let mut header_left = div().flex().items_center().gap(px(8.0));
    header_left = header_left.child(theme::pii_badge("✨ AI AGENT"));

    if state.is_running {
        header_left = header_left.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(4.0))
                .bg(rgb(c.bg_input))
                .child(theme::indicator_dot(c.tuta_crimson_bright))
                .child(
                    div()
                        .text_size(t.caption)
                        .text_color(rgb(c.tuta_crimson_bright))
                        .child("Running pass…"),
                ),
        );
    } else if let Some(summary) = &state.summary_message {
        header_left = header_left.child(
            div()
                .text_size(t.caption)
                .text_color(rgb(c.diff_add_text))
                .child(SharedString::from(format!("✓ {summary}"))),
        );
    }

    header_left = header_left.child(
        div()
            .text_size(t.caption)
            .text_color(rgb(c.text_secondary))
            .child(SharedString::from(state.status_message.clone())),
    );

    header = header.child(header_left);

    // Header Right Actions
    let mut header_right = div().flex().items_center().gap(px(8.0));

    if pending_count > 0 {
        header_right = header_right.child(
            div()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.accept_all_proposals(cx);
                    }),
                )
                .child(
                    theme::button_primary(format!("Accept All ({pending_count})"))
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(t.caption),
                ),
        );
    }

    header_right = header_right.child(
        div()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.dismiss_ai_panel(cx);
                }),
            )
            .child(
                theme::button_ghost("Dismiss")
                    .px(px(10.0))
                    .py(px(4.0))
                    .text_size(t.caption),
            ),
    );

    header = header.child(header_right);
    panel = panel.child(header);

    // 2. Error callout (if present)
    if let Some(error) = &state.error_message {
        let err_box = div()
            .flex()
            .items_center()
            .justify_between()
            .p(px(10.0))
            .rounded(px(6.0))
            .bg(rgb(c.diff_del_bg))
            .border_1()
            .border_color(rgb(c.tuta_crimson))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.diff_del_text))
                            .child(SharedString::from(format!("⚠️ AI Agent Error: {error}"))),
                    )
                    .child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.text_secondary))
                            .child(
                                "Open Settings to verify your Sign in with ChatGPT session or review usage at chatgpt.com/settings/usage.",
                            ),
                    ),
            )
            .child(
                div()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.dismiss_ai_panel(cx);
                        }),
                    )
                    .child(theme::button_ghost("Dismiss Error").text_size(t.caption)),
            );
        panel = panel.child(err_box);
    }

    // 3. Tool Calls Row (if any)
    if !state.tool_calls.is_empty() {
        let mut tool_row = div().flex().items_center().flex_wrap().gap(px(6.0));
        tool_row = tool_row.child(
            div()
                .text_size(t.caption)
                .text_color(rgb(c.text_secondary))
                .child("Executed Tools:"),
        );
        for tool in &state.tool_calls {
            tool_row = tool_row.child(
                div()
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .bg(rgb(c.bg_input))
                    .border_1()
                    .border_color(rgb(c.border_subtle))
                    .text_size(t.badge)
                    .text_color(rgb(c.text_primary))
                    .child(SharedString::from(format!("🔧 {tool}"))),
            );
        }
        panel = panel.child(tool_row);
    }

    // 4. ATS Match Score Card (if present)
    if let Some(ats) = &state.ats_score {
        panel = panel.child(render_ats_card(ats));
    }

    // 5. Streamed text preview (if generating and no proposals yet)
    if state.is_running && !state.streamed_text.trim().is_empty() && state.proposals.is_empty() {
        let stream_preview = div()
            .id("stream-preview-scroll")
            .flex()
            .flex_col()
            .gap(px(4.0))
            .p(px(8.0))
            .rounded(px(4.0))
            .bg(rgb(c.bg_input))
            .border_1()
            .border_color(rgb(c.border_subtle))
            .max_h(px(100.0))
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(t.badge)
                    .text_color(rgb(c.text_disabled))
                    .child("Streamed AI output:"),
            )
            .child(
                div()
                    .text_size(t.caption)
                    .text_color(rgb(c.text_secondary))
                    .child(SharedString::from(state.streamed_text.clone())),
            );
        panel = panel.child(stream_preview);
    }

    // 6. Proposals Review Section
    if total_proposals > 0 {
        panel = panel.child(theme::hairline());
        let mut proposals_section = div().flex().flex_col().gap(px(8.0));

        proposals_section = proposals_section.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(t.subheading)
                        .text_color(rgb(c.text_primary))
                        .child(SharedString::from(format!(
                            "Review Proposed Variations ({pending_count} pending / {total_proposals} total):"
                        ))),
                ),
        );

        for item in &state.proposals {
            proposals_section = proposals_section.child(render_proposal_card(item, cx));
        }

        panel = panel.child(proposals_section);
    }

    panel
}

fn render_ats_card(ats: &AtsScore) -> impl IntoElement {
    let c = theme::colors();
    let t = theme::typography();

    let score_color = if ats.score >= 70 {
        c.diff_add_text
    } else if ats.score >= 50 {
        0xE5A93C
    } else {
        c.diff_del_text
    };

    let mut card = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(10.0))
        .rounded(px(6.0))
        .bg(rgb(c.bg_input))
        .border_1()
        .border_color(rgb(c.border_subtle));

    // Top row: Score + title
    card = card.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(t.subheading)
                            .text_color(rgb(c.text_primary))
                            .child("ATS Alignment Score"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .bg(rgb(c.bg_surface))
                    .border_1()
                    .border_color(rgb(c.border_subtle))
                    .child(
                        div()
                            .text_size(t.heading)
                            .text_color(rgb(score_color))
                            .child(SharedString::from(format!("{}%", ats.score))),
                    ),
            ),
    );

    // Matched keywords
    if !ats.matched_keywords.is_empty() {
        let mut matched_row = div().flex().items_center().flex_wrap().gap(px(4.0));
        matched_row = matched_row.child(
            div()
                .text_size(t.caption)
                .text_color(rgb(c.diff_add_text))
                .child("Matched:"),
        );
        for kw in &ats.matched_keywords {
            matched_row = matched_row.child(
                div()
                    .px(px(6.0))
                    .py(px(1.0))
                    .rounded(px(3.0))
                    .bg(rgb(c.diff_add_bg))
                    .text_size(t.badge)
                    .text_color(rgb(c.diff_add_text))
                    .child(SharedString::from(format!("✓ {kw}"))),
            );
        }
        card = card.child(matched_row);
    }

    // Missing keywords
    if !ats.missing_keywords.is_empty() {
        let mut missing_row = div().flex().items_center().flex_wrap().gap(px(4.0));
        missing_row = missing_row.child(
            div()
                .text_size(t.caption)
                .text_color(rgb(c.diff_del_text))
                .child("Missing:"),
        );
        for kw in &ats.missing_keywords {
            missing_row = missing_row.child(
                div()
                    .px(px(6.0))
                    .py(px(1.0))
                    .rounded(px(3.0))
                    .bg(rgb(c.diff_del_bg))
                    .text_size(t.badge)
                    .text_color(rgb(c.diff_del_text))
                    .child(SharedString::from(format!("✗ {kw}"))),
            );
        }
        card = card.child(missing_row);
    }

    card
}

fn render_proposal_card(
    item: &ProposalItem,
    cx: &mut Context<WorkspaceView>,
) -> impl IntoElement {
    let c = theme::colors();
    let t = theme::typography();
    let proposal_id = item.id;

    let (badge_text, kind_title) = match &item.proposal {
        AiProposal::BulletRewrite { .. } => ("BULLET REWRITE", "Experience Bullet Optimization"),
        AiProposal::SummaryRewrite { .. } => ("SUMMARY REWRITE", "Executive Summary Alignment"),
        AiProposal::ToggleVisibility { .. } => ("VISIBILITY", "Section / Item Visibility Toggle"),
        AiProposal::SkillReorder { .. } => ("SKILL REORDER", "Skill Category Prioritization"),
    };

    let mut card = div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(6.0))
        .bg(rgb(c.bg_canvas))
        .border_1()
        .border_color(rgb(c.border_subtle));

    // Card Header: Badge + Type + Status + Accept/Reject Actions
    let mut header = div().flex().items_center().justify_between();

    let header_left = div()
        .flex()
        .items_center()
        .gap(px(6.0))
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
                .child(badge_text),
        )
        .child(
            div()
                .text_size(t.body)
                .text_color(rgb(c.text_primary))
                .child(kind_title),
        );

    header = header.child(header_left);

    // Header Right: Actions / Status Badge
    let header_right = match item.status {
        ProposalStatus::Accepted => div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .bg(rgb(c.diff_add_bg))
            .text_size(t.caption)
            .text_color(rgb(c.diff_add_text))
            .child("✓ Accepted")
            .into_any_element(),
        ProposalStatus::Rejected => div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .bg(rgb(c.diff_del_bg))
            .text_size(t.caption)
            .text_color(rgb(c.diff_del_text))
            .child("✗ Rejected")
            .into_any_element(),
        ProposalStatus::Pending => {
            let mut actions = div().flex().items_center().gap(px(6.0));

            if item.warning.is_none() {
                actions = actions.child(
                    div()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                this.accept_proposal(proposal_id, cx);
                            }),
                        )
                        .child(
                            theme::button_primary("Accept")
                                .px(px(8.0))
                                .py(px(2.0))
                                .text_size(t.badge),
                        ),
                );
            }

            actions = actions.child(
                div()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.reject_proposal(proposal_id, cx);
                        }),
                    )
                    .child(
                        theme::button_ghost("Reject")
                            .px(px(8.0))
                            .py(px(2.0))
                            .text_size(t.badge),
                    ),
            );

            actions.into_any_element()
        }
    };

    header = header.child(header_right);
    card = card.child(header);

    // Warning Banner (PII red flag)
    if let Some(warn) = &item.warning {
        card = card.child(
            div()
                .p(px(6.0))
                .rounded(px(4.0))
                .bg(rgb(c.diff_del_bg))
                .border_1()
                .border_color(rgb(c.tuta_crimson))
                .text_size(t.caption)
                .text_color(rgb(c.diff_del_text))
                .child(SharedString::from(format!("⚠️ {warn}"))),
        );
    }

    // Proposal Diff & Details
    match &item.proposal {
        AiProposal::BulletRewrite {
            original,
            proposed,
            rationale,
            ..
        } => {
            card = card.child(render_diff_view(original, proposed));
            if !rationale.trim().is_empty() {
                card = card.child(
                    div()
                        .text_size(t.caption)
                        .text_color(rgb(c.text_secondary))
                        .child(SharedString::from(format!("Rationale: {rationale}"))),
                );
            }
        }
        AiProposal::SummaryRewrite { original, proposed, .. } => {
            card = card.child(render_diff_view(original, proposed));
        }
        AiProposal::ToggleVisibility {
            enabled, reason, ..
        } => {
            let visibility_label = if *enabled { "Visible" } else { "Hidden" };
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.text_primary))
                            .child(SharedString::from(format!(
                                "Set target item to: {visibility_label}"
                            ))),
                    )
                    .child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.text_secondary))
                            .child(SharedString::from(format!("Reason: {reason}"))),
                    ),
            );
        }
        AiProposal::SkillReorder { items, .. } => {
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.text_secondary))
                            .child("Reordered Skill Sequence:"),
                    )
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.text_primary))
                            .child(SharedString::from(items.join("  •  "))),
                    ),
            );
        }
    }

    card
}

/// Renders a word-level inline diff using `crate::ai::diff_engine`.
fn render_diff_view(original: &str, proposed: &str) -> impl IntoElement {
    let c = theme::colors();
    let t = theme::typography();

    let spans = diff_words(original, proposed);

    let mut container = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(3.0))
        .p(px(8.0))
        .rounded(px(4.0))
        .bg(rgb(c.bg_input))
        .border_1()
        .border_color(rgb(c.border_subtle));

    for span in spans {
        match span {
            DiffSpan::Unchanged(text) => {
                container = container.child(
                    div()
                        .text_size(t.body)
                        .text_color(rgb(c.text_primary))
                        .child(text),
                );
            }
            DiffSpan::Added(text) => {
                container = container.child(
                    div()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded(px(3.0))
                        .bg(rgb(c.diff_add_bg))
                        .text_size(t.body)
                        .text_color(rgb(c.diff_add_text))
                        .child(text),
                );
            }
            DiffSpan::Removed(text) => {
                container = container.child(
                    div()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded(px(3.0))
                        .bg(rgb(c.diff_del_bg))
                        .text_size(t.body)
                        .text_color(rgb(c.diff_del_text))
                        .child(text),
                );
            }
        }
    }

    container
}
