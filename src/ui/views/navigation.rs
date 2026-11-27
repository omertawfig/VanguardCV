use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::CvDocument;
use crate::ui::components::ui::{IconName, render_brand_logo_badge, render_icon};
use crate::ui::theme::{RADIUS_MD, TEXT_XS, Theme};
use crate::ui::workspace::{WorkspaceView, WorkspaceViewPage};
use crate::updater::UpdateStatus;

pub fn render_sidebar(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Stateful<Div> {
    let theme = &workspace.theme;
    let active_doc = workspace.active_document();
    let is_child = active_doc.is_some_and(|d| d.parent_id.is_some());
    let current_view = workspace.current_view;
    let total_docs = workspace.documents.len();
    let pending_proposals = workspace.active_proposals().len();
    let enabled_skills = workspace
        .active_skills()
        .iter()
        .filter(|s| s.enabled)
        .count();
    let ai_running = workspace.ai_state.is_running;

    let is_compact = workspace.viewport_width < 1020.0;
    let sidebar_w = if is_compact { px(204.0) } else { px(228.0) };
    let (brand_tile_size, brand_logo_size, brand_radius) = if is_compact {
        (48.0, 42.0, 11.0)
    } else {
        (54.0, 48.0, 13.0)
    };
    let brand_cv_accent = theme.brand_badge_accent;

    div()
        .id("sidebar")
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w(sidebar_w)
        .h_full()
        .overflow_y_scroll()
        .bg(theme.surface_soft)
        .border_r_1()
        .border_color(theme.border)
        .pt(px(16.0))
        .pb(px(20.0))
        .px(px(14.0))
        // Brand
        .child(
            div()
                .pb(px(16.0))
                .border_b_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .id("brand-btn")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(if is_compact { px(9.0) } else { px(11.0) })
                        .px(px(6.0))
                        .py(px(6.0))
                        .mx(px(-6.0))
                        .rounded(px(8.0))
                        .hover(|s| s.bg(theme.hover))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate_to(WorkspaceViewPage::Library, cx);
                            cx.notify();
                        }))
                        .child(render_brand_logo_badge(
                            brand_tile_size,
                            brand_logo_size,
                            brand_radius,
                            theme,
                        ))
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
                                        .font_weight(FontWeight::EXTRA_BOLD)
                                        .text_size(if is_compact { px(15.5) } else { px(17.0) })
                                        .line_height(px(20.0))
                                        .text_color(theme.text)
                                        .child("Vanguard")
                                        .child(div().text_color(brand_cv_accent).child("CV")),
                                )
                                .child(
                                    div()
                                        .text_size(px(TEXT_XS))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.secondary)
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child("Your career workspace"),
                                ),
                        ),
                ),
        )
        // Workspace section label
        .child(
            div()
                .pt(px(18.0))
                .pb(px(6.0))
                .px(px(6.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted)
                .child("WORKSPACE"),
        )
        // Nav items: CV library (CV editor opens by clicking a CV; full preview opens from the side preview)
        .child(render_nav_item(
            "nav-library",
            "CV library",
            IconName::Folder,
            matches!(
                current_view,
                WorkspaceViewPage::Library | WorkspaceViewPage::Editor | WorkspaceViewPage::Preview
            ),
            Some(total_docs),
            theme,
            cx.listener(|this, _, _, cx| {
                this.navigate_to(WorkspaceViewPage::Library, cx);
                cx.notify();
            }),
        ))
        .child(render_nav_item(
            "nav-jobs",
            "Job discovery",
            IconName::Briefcase,
            current_view == WorkspaceViewPage::Jobs,
            (!workspace.job_listings.is_empty()).then_some(workspace.job_listings.len()),
            theme,
            cx.listener(|this, _, _, cx| {
                this.navigate_to(WorkspaceViewPage::Jobs, cx);
                cx.notify();
            }),
        ))
        // Tools section label
        .child(
            div()
                .pt(px(20.0))
                .pb(px(6.0))
                .px(px(6.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted)
                .child("TOOLS"),
        )
        // Tools: Skills, Privacy
        .child(render_nav_item(
            "nav-skills",
            "AI skills",
            IconName::Sparkles,
            current_view == WorkspaceViewPage::Skills,
            Some(enabled_skills),
            theme,
            cx.listener(|this, _, _, cx| {
                this.navigate_to(WorkspaceViewPage::Skills, cx);
                cx.notify();
            }),
        ))
        .child(render_nav_item(
            "nav-privacy",
            "Personal info",
            IconName::Shield,
            current_view == WorkspaceViewPage::Privacy,
            None,
            theme,
            cx.listener(|this, _, _, cx| {
                this.navigate_to(WorkspaceViewPage::Privacy, cx);
                cx.notify();
            }),
        ))
        // Current document section
        .child(
            div()
                .mt(px(20.0))
                .pt(px(14.0))
                .border_t_1()
                .border_color(theme.border_soft)
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .px(px(6.0))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted)
                        .child("CURRENT DOCUMENT"),
                )
                .child(render_document_selector(
                    workspace, active_doc, is_child, theme, cx,
                ))
                .child(if active_doc.is_some() {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(6.0))
                        .text_size(px(11.0))
                        .text_color(theme.secondary)
                        .child(render_icon(
                            if is_child {
                                IconName::Branch
                            } else {
                                IconName::Shield
                            },
                            12.0,
                            theme.secondary,
                        ))
                        .child(if is_child {
                            "Independent variation"
                        } else {
                            "Protected master"
                        })
                } else {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(6.0))
                        .text_size(px(11.0))
                        .text_color(theme.muted)
                        .child(render_icon(IconName::Folder, 12.0, theme.muted))
                        .child("Select a CV from the library")
                })
                .when(pending_proposals > 0 && !ai_running, |el| {
                    el.child(
                        div()
                            .id("sidebar-pending-proposals")
                            .cursor_pointer()
                            .mt(px(4.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(RADIUS_MD))
                            .bg(theme.accent_soft)
                            .border_1()
                            .border_color(theme.accent)
                            .hover(|s| s.border_color(theme.accent_hover))
                            .text_size(px(TEXT_XS))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.accent_text)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigate_to(WorkspaceViewPage::Editor, cx);
                                cx.notify();
                            }))
                            .child(render_icon(IconName::Sparkles, 12.0, theme.accent))
                            .child(format!(
                                "{pending_proposals} AI {} to review",
                                if pending_proposals == 1 { "edit" } else { "edits" }
                            )),
                    )
                })
                .when(ai_running, |el| {
                    el.child(
                        div()
                            .mt(px(4.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .bg(theme.accent_soft)
                            .border_1()
                            .border_color(theme.accent)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.accent_text)
                            .child(render_icon(IconName::Sparkles, 12.0, theme.accent))
                            .child("AI tailoring runningâ€¦"),
                    )
                }),
        )
        // Spacer to push bottom items
        .child(div().flex_1())
        // Bottom: Update badge (when available), Settings & Local by design
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .when(
                    matches!(
                        workspace.update_status,
                        UpdateStatus::Available | UpdateStatus::ReadyToInstall
                    ),
                    |el| {
                        let is_ready =
                            matches!(workspace.update_status, UpdateStatus::ReadyToInstall);
                        let version_label = workspace
                            .update_release
                            .as_ref()
                            .map(|r| format!("v{}", r.version))
                            .unwrap_or_else(|| "new".to_string());
                        let text = if is_ready {
                            format!("Install update {version_label}")
                        } else {
                            format!("Update {version_label} available")
                        };
                        el.child(
                            div()
                                .id("sidebar-update-pill")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(6.0))
                                .px(px(8.0))
                                .py(px(6.0))
                                .rounded(px(6.0))
                                .bg(if is_ready {
                                    theme.add_bg
                                } else {
                                    theme.accent_soft
                                })
                                .border_1()
                                .border_color(if is_ready {
                                    theme.add_text
                                } else {
                                    theme.accent
                                })
                                .hover(|s| s.opacity(0.9))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.navigate_to(WorkspaceViewPage::Settings, cx);
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(render_icon(
                                            IconName::Download,
                                            13.0,
                                            if is_ready {
                                                theme.add_text
                                            } else {
                                                theme.accent
                                            },
                                        ))
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(if is_ready {
                                                    theme.add_text
                                                } else {
                                                    theme.accent_text
                                                })
                                                .child(text),
                                        ),
                                ),
                        )
                    },
                )
                .child(render_nav_item(
                    "nav-settings",
                    "Settings",
                    IconName::Settings,
                    current_view == WorkspaceViewPage::Settings,
                    None,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.navigate_to(WorkspaceViewPage::Settings, cx);
                        cx.notify();
                    }),
                ))
                .child(
                    div()
                        .id("inspect-payload-btn")
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .p(px(8.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.border)
                        .hover(|s| s.bg(theme.hover))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_payload_dialog();
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(div().flex_shrink_0().child(render_icon(IconName::Lock, 14.0, theme.accent)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.text)
                                                .child("Inspect payload"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(theme.muted)
                                                .child("PII air-gap active"),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .px(px(5.0))
                                .py(px(1.0))
                                .rounded(px(4.0))
                                .bg(theme.accent_soft)
                                .text_color(theme.accent_text)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::BOLD)
                                .child("SAFE"),
                        ),
                ),
        )
}

fn render_nav_item(
    id: &'static str,
    label: &'static str,
    icon: IconName,
    is_active: bool,
    count: Option<usize>,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .h(px(34.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .bg(if is_active {
            theme.hover
        } else {
            theme.transparent
        })
        .hover(|s| s.bg(theme.hover))
        .on_click(on_click)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(9.0))
                .child(render_icon(
                    icon,
                    15.0,
                    if is_active {
                        theme.accent
                    } else {
                        theme.secondary
                    },
                ))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(if is_active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if is_active {
                            theme.text
                        } else {
                            theme.secondary
                        })
                        .child(label),
                ),
        )
        .when_some(count, |el, c| {
            el.child(
                div()
                    .px(px(6.0))
                    .py(px(1.0))
                    .rounded_full()
                    .bg(if is_active {
                        theme.surface
                    } else {
                        theme.input
                    })
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if is_active {
                        theme.text
                    } else {
                        theme.muted
                    })
                    .child(c.to_string()),
            )
        })
}

fn render_document_selector(
    _workspace: &WorkspaceView,
    active_doc: Option<&CvDocument>,
    is_child: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Stateful<Div> {
    let has_doc = active_doc.is_some();
    let label = active_doc
        .map(|doc| doc.title.clone())
        .unwrap_or_else(|| "No CV selected".to_string());

    div()
        .id("document-selector-btn")
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .p(px(8.0))
        .rounded(px(6.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .hover(|s| s.bg(theme.hover))
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_document_menu();
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
                .child(div().flex_shrink_0().child(render_icon(
                    if is_child {
                        IconName::Branch
                    } else {
                        IconName::File
                    },
                    14.0,
                    if has_doc { theme.accent } else { theme.muted },
                )))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.0))
                        .font_weight(if has_doc {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if has_doc { theme.text } else { theme.muted })
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(label),
                ),
        )
        .child(div().flex_shrink_0().child(render_icon(IconName::Down, 12.0, theme.muted)))
}
