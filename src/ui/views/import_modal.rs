//! Interactive Master CV Import Verification modal (`.docx` & `.pdf`).
//!
//! Lets the user inspect the auto-parsed result, mark any line as a Section
//! start (with its layout kind), Entry, Role/Subheading, Point, or Skip, merge
//! wrapped lines, and preview the resulting `SanitizedCvBody` structure in real
//! time before completing the import. Personal info headers are strictly skipped
//! and never populated into `PiiContactHeader`.

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::SectionKind;
use crate::importer::{ImportDraft, ImportLine, ImportLineRole, ImportSectionKind};
use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_button, render_icon, render_icon_button,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::WorkspaceView;

pub fn render_import_cv_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let modal_w = (workspace.viewport_width - 32.0).clamp(340.0, 1120.0);
    let modal_h = (workspace.viewport_height - 36.0).clamp(380.0, 800.0);
    let side_by_side = modal_w >= 780.0;

    let Some(draft) = workspace.import_draft.as_ref() else {
        return div()
            .w(px(modal_w.min(500.0)))
            .p(px(20.0))
            .rounded(px(8.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .text_color(theme.secondary)
            .child("No import draft loaded.");
    };

    let stats = draft.stats();
    let parsed_body = draft.build_body();
    let can_import = stats.sections > 0;
    let file_name = draft.source_file_name.clone();
    let format_label = draft.source_format.label();
    let detected_vanguard = draft.detected_vanguard_format;
    let warnings = draft.warnings.clone();
    let line_count = draft.lines.len();

    div()
        .w(px(modal_w))
        .h(px(modal_h))
        .flex()
        .flex_col()
        .rounded(px(10.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .shadow_lg()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        // -------------------------------------------------------------------
        // Header
        // -------------------------------------------------------------------
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(12.0))
                .px(px(20.0))
                .py(px(14.0))
                .border_b_1()
                .border_color(theme.border_soft)
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
                                        .text_size(px(16.5))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text)
                                        .child("Verify imported master CV"),
                                )
                                .child(render_badge(
                                    format_label,
                                    theme.input,
                                    theme.text,
                                    theme.border,
                                ))
                                .child(if detected_vanguard {
                                    render_badge(
                                        "VanguardCV layout detected",
                                        theme.add_bg,
                                        theme.add_text,
                                        theme.add_text,
                                    )
                                } else {
                                    render_badge(
                                        "Auto-parsed · verify below",
                                        theme.accent_soft,
                                        theme.accent_text,
                                        theme.accent,
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(theme.secondary)
                                .child(format!(
                                    "Source: {file_name} — Mark any line as a Section start so lines underneath become its entries and points."
                                )),
                        ),
                )
                .child(render_icon_button(
                    "close-import-modal-btn",
                    IconName::Close,
                    "Close import review",
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                )),
        )
        // -------------------------------------------------------------------
        // Title & quick actions bar
        // -------------------------------------------------------------------
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .px(px(20.0))
                .py(px(10.0))
                .bg(theme.surface_soft)
                .border_b_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(220.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.secondary)
                                .child("Master CV title"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .flex()
                                .items_center()
                                .child(workspace.import_title_input.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.0))
                        .child(render_button(
                            "import-skip-header-btn",
                            "Skip header",
                            Some(IconName::Shield),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.skip_import_header_lines();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "import-auto-detect-btn",
                            "Auto-detect",
                            Some(IconName::Sparkles),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.reset_import_auto_detect();
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "import-pick-another-btn",
                            "Change file",
                            Some(IconName::Folder),
                            ButtonVariant::Ghost,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_import_cv_picker(cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
        // -------------------------------------------------------------------
        // Strict No-PII notice (+ optional parser warnings)
        // -------------------------------------------------------------------
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .px(px(20.0))
                .py(px(8.0))
                .border_b_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(10.0))
                        .py(px(7.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border_soft)
                        .child(render_icon(IconName::Lock, 13.0, theme.accent))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(11.5))
                                .line_height(px(16.0))
                                .text_color(theme.secondary)
                                .child(
                                    "Personal information header is intentionally excluded from import (not parsed). Enter your name, email, phone, and links manually in Personal info after importing.",
                                ),
                        ),
                )
                .children(warnings.into_iter().map(|warn| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .bg(theme.accent_soft)
                        .border_1()
                        .border_color(theme.accent)
                        .text_size(px(11.5))
                        .text_color(theme.accent_text)
                        .child(warn)
                })),
        )
        // -------------------------------------------------------------------
        // Two-pane verification workspace (Extracted Lines | Parsed Preview)
        // -------------------------------------------------------------------
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .when(side_by_side, |el| el.flex_row())
                .when(!side_by_side, |el| el.flex_col())
                .overflow_hidden()
                // Left pane: interactive line classifier
                .child(render_lines_pane(
                    workspace,
                    draft,
                    line_count,
                    side_by_side,
                    theme,
                    cx,
                ))
                // Right pane: live parsed CV preview
                .child(render_preview_pane(
                    workspace,
                    &parsed_body,
                    stats,
                    side_by_side,
                    theme,
                    cx,
                )),
        )
        // -------------------------------------------------------------------
        // Footer
        // -------------------------------------------------------------------
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .px(px(20.0))
                .py(px(12.0))
                .bg(theme.surface_soft)
                .border_t_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(180.0))
                        .text_size(px(12.0))
                        .text_color(if can_import {
                            theme.secondary
                        } else {
                            theme.del_text
                        })
                        .child(if can_import {
                            format!(
                                "Ready: {} {}, {} {}, {} {} ({} skipped). Personal info header stays blank.",
                                stats.sections,
                                if stats.sections == 1 { "section" } else { "sections" },
                                stats.entries,
                                if stats.entries == 1 { "entry" } else { "entries" },
                                stats.points,
                                if stats.points == 1 { "point" } else { "points" },
                                stats.skipped,
                            )
                        } else {
                            "Mark at least one line as a Section on the left to build your CV before importing."
                                .to_string()
                        }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "import-cancel-btn",
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
                            "import-confirm-btn",
                            "Import as master CV",
                            Some(IconName::Check),
                            ButtonVariant::Primary,
                            !can_import,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.confirm_import_master_cv(cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
}

fn render_lines_pane(
    workspace: &WorkspaceView,
    draft: &ImportDraft,
    line_count: usize,
    side_by_side: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let mut pane = div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .overflow_hidden();

    if side_by_side {
        pane = pane.border_r_1().border_color(theme.border_soft);
    } else {
        pane = pane.border_b_1().border_color(theme.border_soft);
    }

    pane.child(
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(9.0))
            .border_b_1()
            .border_color(theme.border_soft)
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .child(format!("1. Extracted lines ({line_count})")),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.muted)
                    .child("Assign Section / Entry / Role / Point / Skip"),
            ),
    )
    .child(
        div()
            .id("import-lines-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&workspace.import_lines_scroll)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let dy = event.delta.pixel_delta(px(28.0)).y;
                this.scroll_import_lines_by(dy, cx);
                cx.stop_propagation();
            }))
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(7.0))
            .children(
                draft
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(idx, line)| render_import_line_row(idx, line, theme, cx)),
            ),
    )
}

fn render_import_line_row(
    idx: usize,
    line: &ImportLine,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let is_skip = line.role == ImportLineRole::Skip;
    let is_section = matches!(line.role, ImportLineRole::Section { .. });
    let is_entry = line.role == ImportLineRole::Entry;
    let is_role = line.role == ImportLineRole::EntrySubheading;
    let is_point = line.role == ImportLineRole::Point;

    let (card_bg, card_border, indent_px) = match line.role {
        ImportLineRole::Skip => (theme.surface_soft, theme.border_soft, 0.0),
        ImportLineRole::Section { .. } => (theme.accent_soft, theme.accent, 0.0),
        ImportLineRole::Entry => (theme.input, theme.border, 6.0),
        ImportLineRole::EntrySubheading => (theme.input, theme.border_soft, 12.0),
        ImportLineRole::Point => (theme.surface, theme.border_soft, 16.0),
    };

    let mut row = div()
        .w_full()
        .pl(px(indent_px))
        .child(
            div()
                .w_full()
                .p(px(9.0))
                .rounded(px(6.0))
                .bg(card_bg)
                .border_1()
                .border_color(card_border)
                .when(is_skip, |el| el.opacity(0.68))
                .flex()
                .flex_col()
                .gap(px(6.0))
                // Top metadata + role switcher bar
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.0))
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.muted)
                                        .child(format!("#{}", idx + 1)),
                                )
                                .when(line.is_likely_pii_header && is_skip, |el| {
                                    el.child(render_badge(
                                        "PII header · skipped",
                                        theme.input,
                                        theme.muted,
                                        theme.border_soft,
                                    ))
                                })
                                .when(line.raw.is_heading, |el| {
                                    el.child(render_badge(
                                        "Heading",
                                        theme.input,
                                        theme.secondary,
                                        theme.border_soft,
                                    ))
                                })
                                .when(line.raw.is_list_item && !is_skip, |el| {
                                    el.child(render_badge(
                                        "Bullet",
                                        theme.input,
                                        theme.secondary,
                                        theme.border_soft,
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(4.0))
                                .child(render_role_pill(
                                    format!("imp-line-{idx}-skip"),
                                    "Skip",
                                    is_skip,
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.set_import_line_role(idx, ImportLineRole::Skip);
                                        cx.notify();
                                    }),
                                ))
                                .child(render_role_pill(
                                    format!("imp-line-{idx}-sec"),
                                    "Section",
                                    is_section,
                                    true,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.set_import_line_role(
                                            idx,
                                            ImportLineRole::Section {
                                                kind: ImportSectionKind::Timeline,
                                            },
                                        );
                                        cx.notify();
                                    }),
                                ))
                                .child(render_role_pill(
                                    format!("imp-line-{idx}-entry"),
                                    "Entry",
                                    is_entry,
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.set_import_line_role(idx, ImportLineRole::Entry);
                                        cx.notify();
                                    }),
                                ))
                                .child(render_role_pill(
                                    format!("imp-line-{idx}-role"),
                                    "Role",
                                    is_role,
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.set_import_line_role(
                                            idx,
                                            ImportLineRole::EntrySubheading,
                                        );
                                        cx.notify();
                                    }),
                                ))
                                .child(render_role_pill(
                                    format!("imp-line-{idx}-point"),
                                    "Point",
                                    is_point,
                                    false,
                                    theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.set_import_line_role(idx, ImportLineRole::Point);
                                        cx.notify();
                                    }),
                                ))
                                .when(idx > 0 && is_point, |el| {
                                    el.child(
                                        div()
                                            .id(el_id(format!("imp-line-{idx}-merge")))
                                            .cursor_pointer()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded(px(4.0))
                                            .bg(theme.input)
                                            .border_1()
                                            .border_color(theme.border)
                                            .hover(|s| s.bg(theme.hover))
                                            .text_size(px(10.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.secondary)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.merge_import_line_up(idx);
                                                cx.notify();
                                            }))
                                            .child("Merge ↑"),
                                    )
                                }),
                        ),
                )
                // Extracted line text
                .child(
                    div()
                        .w_full()
                        .text_size(if is_section { px(12.5) } else { px(12.0) })
                        .line_height(px(17.5))
                        .font_weight(if is_section || is_entry || is_role {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if is_skip {
                            theme.muted
                        } else if is_section {
                            theme.accent_text
                        } else {
                            theme.text
                        })
                        .child(line.text.clone()),
                )
                // Section layout type chooser (shown inline on Section lines)
                .when_some(
                    match line.role {
                        ImportLineRole::Section { kind } => Some(kind),
                        _ => None,
                    },
                    |el, active_kind| {
                        el.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(5.0))
                                .pt(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.secondary)
                                        .child("Section type:"),
                                )
                                .children(ImportSectionKind::ALL.into_iter().map(|kind| {
                                    let selected = kind == active_kind;
                                    div()
                                        .id(el_id(format!(
                                            "imp-sec-{idx}-{}",
                                            kind.label().to_ascii_lowercase()
                                        )))
                                        .cursor_pointer()
                                        .px(px(7.0))
                                        .py(px(2.0))
                                        .rounded(px(4.0))
                                        .bg(if selected {
                                            theme.accent
                                        } else {
                                            theme.surface
                                        })
                                        .border_1()
                                        .border_color(if selected {
                                            theme.accent
                                        } else {
                                            theme.border
                                        })
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if selected {
                                            rgb(0xffffff)
                                        } else {
                                            theme.secondary
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_import_section_kind(idx, kind);
                                            cx.notify();
                                        }))
                                        .child(kind.label())
                                })),
                        )
                    },
                ),
        );

    row = row.min_w_0();
    row
}

fn render_preview_pane(
    workspace: &WorkspaceView,
    body: &crate::contracts::SanitizedCvBody,
    stats: crate::importer::ImportStats,
    side_by_side: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let mut pane = div()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(theme.surface_soft);

    if side_by_side {
        pane = pane.w(px(410.0)).flex_shrink_0();
    } else {
        pane = pane.flex_1();
    }

    pane.child(
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(9.0))
            .border_b_1()
            .border_color(theme.border_soft)
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .child("2. Live CV preview"),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.muted)
                    .child(format!(
                        "{} sec · {} entries · {} pts",
                        stats.sections, stats.entries, stats.points
                    )),
            ),
    )
    .child(
        div()
            .id("import-preview-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&workspace.import_preview_scroll)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let dy = event.delta.pixel_delta(px(28.0)).y;
                this.scroll_import_preview_by(dy, cx);
                cx.stop_propagation();
            }))
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            // Contact header blank notice card
            .child(
                div()
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border_soft)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(render_icon(IconName::Shield, 13.0, theme.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.0))
                            .text_color(theme.muted)
                            .child("Contact header: Blank (enter manually after import)"),
                    ),
            )
            .when(body.sections.is_empty(), |el| {
                el.child(
                    div()
                        .p(px(16.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .text_size(px(12.0))
                        .line_height(px(18.0))
                        .text_color(theme.secondary)
                        .child(
                            "No sections selected yet. Click \"Section\" on any heading line on the left to start populating your Master CV.",
                        ),
                )
            })
            .children(body.sections.iter().map(|sec| {
                div()
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .pb(px(6.0))
                            .border_b_1()
                            .border_color(theme.border_soft)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .child(sec.title.to_uppercase()),
                            )
                            .child(render_badge(
                                sec.kind_name(),
                                theme.input,
                                theme.secondary,
                                theme.border_soft,
                            )),
                    )
                    .child(match &sec.kind {
                        SectionKind::Summary { text } => div()
                            .text_size(px(11.5))
                            .line_height(px(17.0))
                            .text_color(if text.trim().is_empty() {
                                theme.muted
                            } else {
                                theme.secondary
                            })
                            .child(if text.trim().is_empty() {
                                "(Empty summary — mark lines underneath as Point)".to_string()
                            } else {
                                text.clone()
                            }),
                        SectionKind::TimelineEntries { entries } => {
                            if entries.is_empty() {
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted)
                                    .child("(No entries yet — mark a line as Entry)")
                            } else {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .children(entries.iter().map(|entry| {
                                        let date_str = entry.date_range();
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.0))
                                            .pl(px(8.0))
                                            .border_l_2()
                                            .border_color(theme.accent)
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_wrap()
                                                    .items_baseline()
                                                    .justify_between()
                                                    .gap(px(6.0))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.0))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(theme.text)
                                                            .child(entry.primary_heading.clone()),
                                                    )
                                                    .when(!date_str.is_empty(), |el| {
                                                        el.child(
                                                            div()
                                                                .text_size(px(10.5))
                                                                .text_color(theme.muted)
                                                                .child(date_str),
                                                        )
                                                    }),
                                            )
                                            .when_some(
                                                entry.secondary_heading.clone(),
                                                |el, sub| {
                                                    el.child(
                                                        div()
                                                            .text_size(px(11.5))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(theme.secondary)
                                                            .child(sub),
                                                    )
                                                },
                                            )
                                            .children(entry.bullets.iter().map(|b| {
                                                div()
                                                    .flex()
                                                    .items_start()
                                                    .gap(px(5.0))
                                                    .text_size(px(11.0))
                                                    .line_height(px(16.0))
                                                    .text_color(theme.secondary)
                                                    .child(div().text_color(theme.accent).child("•"))
                                                    .child(div().flex_1().min_w_0().child(b.text.clone()))
                                            }))
                                    }))
                            }
                        }
                        SectionKind::CategorizedInlineList { categories } => {
                            if categories.is_empty() {
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted)
                                    .child("(No skill categories yet — mark lines as Point)")
                            } else {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(5.0))
                                    .children(categories.iter().map(|cat| {
                                        div()
                                            .text_size(px(11.5))
                                            .line_height(px(16.5))
                                            .text_color(theme.secondary)
                                            .child(format!(
                                                "• {}: {}",
                                                cat.category_label,
                                                cat.items.join(&cat.separator)
                                            ))
                                    }))
                            }
                        }
                        SectionKind::SimpleBulletList { items } => {
                            if items.is_empty() {
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted)
                                    .child("(No points yet — mark lines as Point)")
                            } else {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .children(items.iter().map(|item| {
                                        div()
                                            .flex()
                                            .items_start()
                                            .gap(px(5.0))
                                            .text_size(px(11.0))
                                            .line_height(px(16.0))
                                            .text_color(theme.secondary)
                                            .child(div().text_color(theme.accent).child("•"))
                                            .child(
                                                div().flex_1().min_w_0().child(item.text.clone()),
                                            )
                                    }))
                            }
                        }
                    })
            })),
    )
}

fn render_role_pill(
    id: String,
    label: &'static str,
    active: bool,
    accent: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, border, fg) = if active && accent {
        (theme.accent, theme.accent, rgb(0xffffff))
    } else if active {
        (theme.hover, theme.secondary, theme.text)
    } else {
        (theme.surface, theme.border_soft, theme.muted)
    };

    div()
        .id(el_id(id))
        .cursor_pointer()
        .px(px(6.5))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .hover(|s| s.border_color(theme.secondary))
        .text_size(px(10.0))
        .font_weight(if active {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(fg)
        .on_click(on_click)
        .child(label)
}

fn render_badge(label: &str, bg: Rgba, fg: Rgba, border: Rgba) -> Div {
    div()
        .flex_shrink_0()
        .px(px(6.0))
        .py(px(1.5))
        .rounded(px(4.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(label.to_string())
}
