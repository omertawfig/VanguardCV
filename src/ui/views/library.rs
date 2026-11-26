use chrono::{DateTime, Utc};
use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::CvDocument;
use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_brand_logo_badge, render_button, render_empty_state, render_icon,
    render_icon_button,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::{LibraryFilter, LibrarySort, WorkspaceView, WorkspaceViewPage};

pub fn render_library_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
    let theme = &workspace.theme;

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

    let query = workspace.library_query.trim().to_lowercase();
    let matches = |doc: &CvDocument| -> bool {
        if query.is_empty() {
            return true;
        }
        let job_company = doc
            .job_context
            .as_ref()
            .map(|j| j.company_name.as_str())
            .unwrap_or("");
        let job_role = doc
            .job_context
            .as_ref()
            .map(|j| j.target_role.as_str())
            .unwrap_or("");
        doc.title.to_lowercase().contains(&query)
            || job_company.to_lowercase().contains(&query)
            || job_role.to_lowercase().contains(&query)
    };

    let sort_docs = |items: &mut Vec<&CvDocument>| match workspace.library_sort {
        LibrarySort::Name => items.sort_by(|a, b| a.title.cmp(&b.title)),
        LibrarySort::LastEdited => items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
    };

    let mut visible_masters: Vec<&CvDocument> = masters
        .iter()
        .cloned()
        .filter(|root| {
            matches(root)
                || (workspace.library_filter == LibraryFilter::All
                    && variations
                        .iter()
                        .any(|v| v.parent_id == Some(root.id) && matches(v)))
        })
        .collect();
    sort_docs(&mut visible_masters);

    let mut visible_variations: Vec<&CvDocument> = variations
        .iter()
        .cloned()
        .filter(|doc| {
            let root = masters.iter().find(|m| Some(m.id) == doc.parent_id);
            matches(doc) || root.map(|r| matches(r)).unwrap_or(false)
        })
        .collect();
    sort_docs(&mut visible_variations);

    let has_results = if workspace.library_filter == LibraryFilter::Variations {
        !visible_variations.is_empty()
    } else {
        !visible_masters.is_empty()
    };

    let pad_x = workspace.page_padding_x();
    let is_compact = workspace.is_compact();
    let is_narrow = workspace.is_narrow();
    let show_date_col = !is_compact;
    let show_type_col = !is_narrow;
    let actions_w = if is_narrow { px(236.0) } else { px(300.0) };

    div()
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .max_w(px(1248.0))
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
                .when(is_narrow, |el| el.flex_col())
                .when(!is_narrow, |el| el.flex_row().items_start().justify_between())
                .gap(px(12.0))
                .child(
                    div()
                        .when(!is_narrow, |el| el.flex_1())
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted)
                                .child("YOUR WORKSPACE"),
                        )
                        .child(
                            div()
                                .text_size(px(26.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("CV library"),
                        )
                        .child(div().text_size(px(13.5)).text_color(theme.secondary).child(
                            "A complete career record. A considered CV for every opportunity.",
                        )),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "import-master-top-btn",
                            if workspace.import_loading {
                                "Reading CV…"
                            } else {
                                "Import DOCX / PDF"
                            },
                            Some(IconName::Download),
                            ButtonVariant::Secondary,
                            workspace.import_loading,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_import_cv_picker(cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "new-master-top-btn",
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
                ),
        )
        // Toolbar: Filter pills, Search, Sort
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(12.0))
                .pb(px(4.0))
                // Filters
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .p(px(3.0))
                        .rounded(px(6.0))
                        .bg(theme.input)
                        .border_1()
                        .border_color(theme.border_soft)
                        .child(render_filter_pill(
                            "filter-all",
                            "All CVs",
                            workspace.documents.len(),
                            workspace.library_filter == LibraryFilter::All,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.library_filter = LibraryFilter::All;
                                cx.notify();
                            }),
                        ))
                        .child(render_filter_pill(
                            "filter-masters",
                            "Masters",
                            masters.len(),
                            workspace.library_filter == LibraryFilter::Masters,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.library_filter = LibraryFilter::Masters;
                                cx.notify();
                            }),
                        ))
                        .child(render_filter_pill(
                            "filter-variations",
                            "Variations",
                            variations.len(),
                            workspace.library_filter == LibraryFilter::Variations,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.library_filter = LibraryFilter::Variations;
                                cx.notify();
                            }),
                        )),
                )
                // Search and Sort controls
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(10.0))
                        // Search bar
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .h(px(34.0))
                                .px(px(10.0))
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .overflow_hidden()
                                .child(render_icon(IconName::Search, 14.0, theme.muted))
                                .child(
                                    div()
                                        .w(if is_narrow { px(130.0) } else { px(180.0) })
                                        .min_w_0()
                                        .child(workspace.search_input.clone()),
                                )
                                .when(!workspace.library_query.is_empty(), |el| {
                                    el.child(render_icon_button(
                                        "clear-search-btn",
                                        IconName::Close,
                                        "Clear search",
                                        false,
                                        theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.clear_search(cx);
                                            cx.notify();
                                        }),
                                    ))
                                }),
                        )
                        // Sort toggle button
                        .child(
                            div()
                                .id("sort-btn")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .h(px(34.0))
                                .px(px(12.0))
                                .rounded(px(5.0))
                                .bg(theme.input)
                                .border_1()
                                .border_color(theme.border)
                                .hover(|s| s.bg(theme.hover))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.library_sort = match this.library_sort {
                                        LibrarySort::LastEdited => LibrarySort::Name,
                                        LibrarySort::Name => LibrarySort::LastEdited,
                                    };
                                    cx.notify();
                                }))
                                .child(
                                    div().text_size(px(12.0)).text_color(theme.secondary).child(
                                        match workspace.library_sort {
                                            LibrarySort::LastEdited => "Sort: Last edited",
                                            LibrarySort::Name => "Sort: Name",
                                        },
                                    ),
                                ),
                        ),
                ),
        )
        // Library Table Frame
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .overflow_hidden()
                // Column Headings
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(36.0))
                        .px(px(16.0))
                        .bg(theme.surface_soft)
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted)
                        .child(div().w(px(28.0)).flex_shrink_0())
                        .child(div().flex_1().min_w_0().child("Name"))
                        .when(show_type_col, |r| r.child(div().w(px(150.0)).flex_shrink_0().child("Type & ATS")))
                        .when(show_date_col, |r| r.child(div().w(px(110.0)).flex_shrink_0().child("Last edited")))
                        .child(div().w(actions_w).flex_shrink_0().text_right().child("Actions")),
                )
                // Content rows
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .when(
                            workspace.library_filter == LibraryFilter::Variations,
                            |el| {
                                let mut list = el;
                                for doc in &visible_variations {
                                    let pending = workspace
                                        .proposals
                                        .get(&doc.id)
                                        .map(|v| v.len())
                                        .unwrap_or(0);
                                    list = list.child(render_variation_row(
                                        doc,
                                        true,
                                        Some(doc.id) == workspace.active_doc_id,
                                        workspace.default_cv_id == Some(doc.id),
                                        pending,
                                        show_type_col,
                                        show_date_col,
                                        is_narrow,
                                        actions_w,
                                        theme,
                                        cx,
                                    ));
                                }
                                list
                            },
                        )
                        .when(
                            workspace.library_filter != LibraryFilter::Variations,
                            |el| {
                                let mut list = el;
                                for root in &visible_masters {
                                    let root_variations: Vec<&CvDocument> = variations
                                        .iter()
                                        .filter(|v| v.parent_id == Some(root.id))
                                        .cloned()
                                        .collect();
                                    let total_root_variations = root_variations.len();

                                    let mut matching_children: Vec<&CvDocument> = root_variations
                                        .into_iter()
                                        .filter(|v| matches(root) || matches(v))
                                        .collect();
                                    sort_docs(&mut matching_children);

                                    let is_collapsed =
                                        workspace.collapsed_masters.contains(&root.id);
                                    let is_current = Some(root.id) == workspace.active_doc_id;

                                    list = list.child(render_master_card(
                                        workspace,
                                        root,
                                        &matching_children,
                                        total_root_variations,
                                        is_collapsed,
                                        is_current,
                                        workspace.library_filter == LibraryFilter::All,
                                        workspace.active_doc_id,
                                        theme,
                                        cx,
                                    ));
                                }
                                list
                            },
                        )
                        .when(!has_results, |el| {
                            if workspace.documents.is_empty() {
                                el.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_center()
                                        .py(px(52.0))
                                        .px(px(24.0))
                                        .gap(px(14.0))
                                        .child(render_brand_logo_badge(76.0, 68.0, 18.0, theme))
                                        .child(
                                            div()
                                                .text_size(px(17.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.text)
                                                .text_center()
                                                .child("No CVs in your workspace yet"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.0))
                                                .text_color(theme.secondary)
                                                .max_w(px(440.0))
                                                .text_center()
                                                .child("Click \"New master CV\" to start fresh, or \"Import DOCX / PDF\" to import an existing CV with guided verification."),
                                        ),
                                )
                            } else {
                                let (icon, title, subtitle) = if workspace.library_filter
                                    == LibraryFilter::Variations
                                    && variations.is_empty()
                                    && query.is_empty()
                                {
                                    (
                                        IconName::Branch,
                                        "No variations yet",
                                        "Create a tailored variation from any master CV to target a specific role.",
                                    )
                                } else {
                                    (
                                        IconName::Search,
                                        "No matching CVs found",
                                        "Try adjusting your search query or switching filters.",
                                    )
                                };
                                el.child(render_empty_state(icon, title, subtitle, theme))
                            }
                        }),
                ),
        )
}

fn render_filter_pill(
    id: &'static str,
    label: &'static str,
    count: usize,
    is_active: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(4.0))
        .bg(if is_active {
            theme.surface
        } else {
            theme.transparent
        })
        .text_size(px(12.0))
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
        .hover(|s| s.text_color(theme.text))
        .on_click(on_click)
        .child(label)
        .child(
            div()
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted)
                .child(count.to_string()),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_master_card(
    workspace: &WorkspaceView,
    root: &CvDocument,
    children: &[&CvDocument],
    total_variations: usize,
    is_collapsed: bool,
    is_current: bool,
    show_variations: bool,
    active_doc_id: Option<uuid::Uuid>,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let root_id = root.id;
    let root_id_for_var = root.id;
    let root_id_for_open = root.id;
    let root_id_for_toggle = root.id;
    let root_id_for_del = root.id;
    let root_id_for_default = root.id;
    let is_default = workspace.default_cv_id == Some(root.id);
    let show_date_col = !workspace.is_compact();
    let show_type_col = !workspace.is_narrow();
    let is_narrow = workspace.is_narrow();
    let actions_w = if is_narrow { px(236.0) } else { px(300.0) };

    let mut card = div()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(theme.border_soft)
        .when(is_current, |s| s.bg(theme.hover));

    // Master row
    card = card.child(
        div()
            .flex()
            .items_center()
            .h(px(54.0))
            .px(px(16.0))
            // Expand/collapse disclosure
            .child(
                div()
                    .w(px(28.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .when(show_variations, |el| {
                        el.child(
                            div()
                                .id(el_id(format!("disc-{}", root_id)))
                                .cursor_pointer()
                                .p(px(4.0))
                                .rounded(px(4.0))
                                .hover(|s| s.bg(theme.hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_master_collapse(root_id_for_toggle);
                                    cx.notify();
                                }))
                                .child(render_icon(
                                    if is_collapsed {
                                        IconName::Chevron
                                    } else {
                                        IconName::Down
                                    },
                                    13.0,
                                    if is_collapsed {
                                        theme.muted
                                    } else {
                                        theme.text
                                    },
                                )),
                        )
                    }),
            )
            // Title, document icon & section info (entire block including side icon opens the CV)
            .child(
                div()
                    .id(el_id(format!("master-title-{}", root.id)))
                    .flex_1()
                    .min_w_0()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .overflow_hidden()
                    .hover(|s| s.opacity(0.86))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_document(root_id_for_open, WorkspaceViewPage::Editor, cx);
                        cx.notify();
                    }))
                    .child(div().flex_shrink_0().child(render_icon(IconName::File, 18.0, theme.accent)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_size(px(14.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .child(root.title.clone()),
                                    )
                                    .when(is_default, |el| {
                                        el.child(
                                            div()
                                                .flex_shrink_0()
                                                .px(px(6.0))
                                                .py(px(1.0))
                                                .rounded(px(4.0))
                                                .bg(theme.add_bg)
                                                .text_color(theme.add_text)
                                                .text_size(px(10.0))
                                                .font_weight(FontWeight::BOLD)
                                                .child("Default"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted)
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(format!(
                                        "{} sections · {} {}",
                                        root.body.sections.len(),
                                        total_variations,
                                        if total_variations == 1 {
                                            "variation"
                                        } else {
                                            "variations"
                                        }
                                    )),
                            ),
                    ),
            )
            // Master badge
            .when(show_type_col, |row| {
                row.child(
                    div().w(px(150.0)).flex_shrink_0().flex().items_center().gap(px(5.0)).child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .px(px(7.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(theme.accent_soft)
                            .text_color(theme.accent_text)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(render_icon(IconName::Shield, 11.0, theme.accent_text))
                            .child("Master"),
                    ),
                )
            })
            // Last edited date
            .when(show_date_col, |row| {
                row.child(
                    div()
                        .w(px(110.0))
                        .flex_shrink_0()
                        .text_size(px(12.0))
                        .text_color(theme.secondary)
                        .child(format_date(&root.updated_at)),
                )
            })
            // Row actions: Create variation, consistent Default slot, Open editor, Delete
            .child(
                div()
                    .w(actions_w)
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(4.0))
                    .child(render_button(
                        format!("var-btn-{}", root.id),
                        if is_narrow { "New" } else { "Variation" },
                        Some(IconName::Plus),
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.open_variation_dialog(Some(root_id_for_var), cx);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .w(if is_narrow { px(68.0) } else { px(96.0) })
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(render_button(
                                format!("default-master-{}", root.id),
                                if is_default {
                                    "Default"
                                } else if is_narrow {
                                    "Def"
                                } else {
                                    "Set default"
                                },
                                if is_default {
                                    Some(IconName::Check)
                                } else {
                                    None
                                },
                                if is_default {
                                    ButtonVariant::Secondary
                                } else {
                                    ButtonVariant::Ghost
                                },
                                false,
                                theme,
                                cx.listener(move |this, _, _, cx| {
                                    this.set_default_cv(root_id_for_default, cx);
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(render_icon_button(
                        format!("open-master-{}", root.id),
                        IconName::Arrow,
                        "Edit master CV",
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.open_document(root_id, WorkspaceViewPage::Editor, cx);
                            cx.notify();
                        }),
                    ))
                    .child(render_icon_button(
                        format!("del-master-{}", root.id),
                        IconName::Trash,
                        "Delete master CV",
                        false,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.open_delete_cv_dialog(root_id_for_del);
                            cx.notify();
                        }),
                    )),
            ),
    );

    // Variations under this master (when expanded)
    if show_variations && !is_collapsed {
        let mut var_list = div()
            .flex()
            .flex_col()
            .bg(theme.surface_soft)
            .border_t_1()
            .border_color(theme.border_soft);

        for child in children {
            let pending = workspace
                .proposals
                .get(&child.id)
                .map(|v| v.len())
                .unwrap_or(0);
            var_list = var_list.child(render_variation_row(
                child,
                false,
                Some(child.id) == active_doc_id,
                workspace.default_cv_id == Some(child.id),
                pending,
                show_type_col,
                show_date_col,
                is_narrow,
                actions_w,
                theme,
                cx,
            ));
        }

        if children.is_empty() {
            var_list = var_list.child(
                div()
                    .py(px(12.0))
                    .px(px(48.0))
                    .text_size(px(12.0))
                    .text_color(theme.muted)
                    .child("No variations yet. Create one for a specific role."),
            );
        }

        card = card.child(var_list);
    }

    card
}

#[allow(clippy::too_many_arguments)]
fn render_variation_row(
    doc: &CvDocument,
    is_detached: bool,
    is_current: bool,
    is_default: bool,
    pending_proposals: usize,
    show_type_col: bool,
    show_date_col: bool,
    is_narrow: bool,
    actions_w: Pixels,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let doc_id = doc.id;
    let doc_id_open = doc.id;
    let doc_id_del = doc.id;
    let doc_id_default = doc.id;
    let job = doc.job_context.as_ref();
    let company_name = job
        .map(|j| j.company_name.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let target_role = job
        .map(|j| j.target_role.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let subtitle = match (company_name.is_empty(), target_role.is_empty()) {
        (false, false) => format!("{company_name} · {target_role}"),
        (false, true) => company_name.to_string(),
        (true, false) => target_role.to_string(),
        (true, true) => "Job-specific CV".to_string(),
    };
    let ats_score = job.and_then(|j| j.ats_match_score);

    div()
        .flex()
        .items_center()
        .h(px(48.0))
        .px(px(16.0))
        .when(is_current, |s| s.bg(theme.hover))
        .border_b_1()
        .border_color(theme.border_soft)
        // Indent / branch icon (clickable to open variation)
        .child(
            div()
                .id(el_id(format!("var-icon-{}", doc.id)))
                .w(px(28.0))
                .h(px(28.0))
                .flex_shrink_0()
                .cursor_pointer()
                .rounded(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(theme.hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_document(doc_id_open, WorkspaceViewPage::Editor, cx);
                    cx.notify();
                }))
                .child(render_icon(IconName::Branch, 14.0, theme.accent)),
        )
        // Variation title and role
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .id(el_id(format!("var-title-{}", doc.id)))
                .cursor_pointer()
                .flex()
                .flex_col()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_document(doc_id_open, WorkspaceViewPage::Editor, cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(doc.title.clone()),
                        )
                        .when(is_default, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(theme.add_bg)
                                    .text_color(theme.add_text)
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Default"),
                            )
                        })
                        .when(pending_proposals > 0, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(theme.accent_soft)
                                    .text_color(theme.accent_text)
                                    .text_size(px(10.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child(format!("{pending_proposals} pending")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.secondary)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(if is_detached {
                            format!("{subtitle} · Variation")
                        } else {
                            subtitle
                        }),
                ),
        )
        // Variation & ATS score badges
        .when(show_type_col, |row| {
            row.child(
                div()
                    .w(px(150.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .px(px(7.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(theme.input)
                            .text_color(theme.secondary)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Variation"),
                    )
                    .when_some(ats_score, |el, score| {
                        let is_high = score >= 75;
                        el.child(
                            div()
                                .px(px(6.0))
                                .py(px(2.0))
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
                                .text_size(px(10.5))
                                .font_weight(FontWeight::BOLD)
                                .child(format!("{score}% ATS")),
                        )
                    }),
            )
        })
        // Last edited
        .when(show_date_col, |row| {
            row.child(
                div()
                    .w(px(110.0))
                    .flex_shrink_0()
                    .text_size(px(12.0))
                    .text_color(theme.secondary)
                    .child(format_date(&doc.updated_at)),
            )
        })
        // Actions: Open button & Delete button
        .child(
            div()
                .w(actions_w)
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(4.0))
                .child(
                    div()
                        .w(if is_narrow { px(68.0) } else { px(96.0) })
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .child(render_button(
                            format!("default-var-{}", doc.id),
                            if is_default {
                                "Default"
                            } else if is_narrow {
                                "Def"
                            } else {
                                "Set default"
                            },
                            if is_default {
                                Some(IconName::Check)
                            } else {
                                None
                            },
                            if is_default {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            },
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.set_default_cv(doc_id_default, cx);
                                cx.notify();
                            }),
                        )),
                )
                .child(render_icon_button(
                    format!("open-var-{}", doc.id),
                    IconName::Arrow,
                    "Open variation in editor",
                    false,
                    theme,
                    cx.listener(move |this, _, _, cx| {
                        this.open_document(doc_id, WorkspaceViewPage::Editor, cx);
                        cx.notify();
                    }),
                ))
                .child(render_icon_button(
                    format!("del-var-{}", doc.id),
                    IconName::Trash,
                    "Delete variation",
                    false,
                    theme,
                    cx.listener(move |this, _, _, cx| {
                        this.open_delete_cv_dialog(doc_id_del);
                        cx.notify();
                    }),
                )),
        )
}

fn format_date(dt: &DateTime<Utc>) -> String {
    dt.format("%d %b %Y").to_string()
}
