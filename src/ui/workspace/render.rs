use super::*;

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        self.viewport_width = f32::from(vp.width);
        self.viewport_height = f32::from(vp.height);

        // If a drag-reorder ended outside a drop target, finalize and persist cleanly.
        if self.active_drag_item.is_some() && !cx.has_active_drag() {
            self.finish_editor_drag(cx);
        }

        // Schedule automatic toast dismissal for any new notification
        if self.notice.is_some() && self.notice_generation != self.scheduled_notice_generation {
            self.scheduled_notice_generation = self.notice_generation;
            let notice_gen = self.notice_generation;
            let timeout = if self.notice_is_error {
                Duration::from_millis(6000)
            } else {
                Duration::from_millis(3500)
            };
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(timeout).await;
                let _ = this.update(cx, |view, cx| {
                    if view.notice_generation == notice_gen && view.notice.is_some() {
                        view.notice = None;
                        view.notice_is_error = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }

        let theme = self.theme.clone();
        let view = self.current_view;
        let active_title = self.active_document().map(|d| d.title.clone());
        let is_dirty = self.debounced_saver.is_dirty();
        let show_sidebar = self.is_sidebar_visible();
        let is_compact = self.is_compact();
        let is_narrow = self.is_narrow();
        let toast_max_w = (self.viewport_width - 32.0).clamp(240.0, 440.0);

        div()
            .relative()
            .flex()
            .size_full()
            .font_family("Inter")
            .track_focus(&self.focus_handle)
            .bg(theme.canvas)
            .text_color(theme.text)
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.preview_pan_state.is_some() {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.update_preview_pan(event.position, cx);
                    } else {
                        this.end_preview_pan();
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    if this.preview_pan_state.is_some() {
                        this.end_preview_pan();
                        cx.notify();
                    }
                    if this.active_drag_item.is_some() {
                        this.finish_editor_drag(cx);
                    }
                }),
            )
            .on_drop::<EditorDragPayload>(cx.listener(|this, _, _window, cx| {
                this.finish_editor_drag(cx);
            }))
            .on_action(cx.listener(|this, _: &NewVariation, _window, cx| {
                if this.active_dialog.is_none() {
                    this.open_variation_dialog(None, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ExportPdfShortcut, _window, cx| {
                if this.active_dialog.is_none() {
                    this.open_export_dialog();
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleAiPanel, _window, cx| {
                if this.active_dialog.is_none() {
                    match this.current_view {
                        WorkspaceViewPage::Editor => {
                            this.toggle_split_preview(cx);
                        }
                        WorkspaceViewPage::Preview => {
                            this.navigate_to(WorkspaceViewPage::Editor, cx);
                        }
                        _ => {
                            if this.active_document().is_some() {
                                this.navigate_to(WorkspaceViewPage::Preview, cx);
                            } else {
                                this.notify("Select a CV from the library to open PDF preview.");
                            }
                        }
                    }
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &AcceptAllProposals, _window, cx| {
                if this.active_dialog.is_none() {
                    this.accept_all_proposals(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CloseModal, _window, cx| {
                if this.active_dialog.is_some() {
                    this.close_dialog();
                    cx.notify();
                }
            }))
            // Sidebar (responsive: auto-hides below 860px, toggleable anytime)
            .when(show_sidebar, |el| el.child(render_sidebar(self, cx)))
            // Main shell
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .h_full()
                    .min_w_0()
                    .overflow_hidden()
                    // Topbar
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.0))
                            .h(px(54.0))
                            .px(if is_narrow { px(12.0) } else { px(20.0) })
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(theme.surface_soft)
                            // Sidebar toggle + Breadcrumbs
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .text_size(px(13.0))
                                    .overflow_hidden()
                                    .child(render_icon_button(
                                        "toggle-sidebar-btn",
                                        IconName::List,
                                        if show_sidebar {
                                            "Hide sidebar"
                                        } else {
                                            "Show sidebar"
                                        },
                                        false,
                                        &theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_sidebar();
                                            cx.notify();
                                        }),
                                    ))
                                    .when(!show_sidebar, |el| {
                                        el.child(
                                            div()
                                                .id("topbar-brand-btn")
                                                .cursor_pointer()
                                                .flex_shrink_0()
                                                .rounded(px(8.0))
                                                .hover(|s| s.opacity(0.88))
                                                .tooltip(|_window, cx| {
                                                    tooltip_view("Go to CV library", cx)
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.navigate_to(
                                                        WorkspaceViewPage::Library,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }))
                                                .child(render_brand_logo_badge(
                                                    32.0,
                                                    27.0,
                                                    8.0,
                                                    &theme,
                                                )),
                                        )
                                    })
                                    .when(!is_narrow, |el| {
                                        el.child(
                                            div()
                                                .id("breadcrumb-workspace")
                                                .cursor_pointer()
                                                .flex_shrink_0()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .text_color(theme.secondary)
                                                .hover(|s| s.text_color(theme.text))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.navigate_to(
                                                        WorkspaceViewPage::Library,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }))
                                                .child(render_icon(
                                                    IconName::Folder,
                                                    14.0,
                                                    theme.secondary,
                                                ))
                                                .child("Workspace"),
                                        )
                                        .child(render_icon(IconName::Chevron, 11.0, theme.muted))
                                    })
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(view.display_name()),
                                    )
                                    .when_some(
                                        if !is_narrow
                                            && matches!(
                                                view,
                                                WorkspaceViewPage::Editor
                                                    | WorkspaceViewPage::Preview
                                                    | WorkspaceViewPage::Privacy
                                            )
                                        {
                                            active_title
                                        } else {
                                            None
                                        },
                                        |el, title| {
                                            el.child(render_icon(
                                                IconName::Chevron,
                                                11.0,
                                                theme.muted,
                                            ))
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .text_ellipsis()
                                                    .text_color(theme.secondary)
                                                    .child(title),
                                            )
                                        },
                                    ),
                            )
                            // Topbar right
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .when(!is_compact, |el| {
                                        el.child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(6.0))
                                                .text_size(px(12.0))
                                                .text_color(theme.muted)
                                                .child(render_icon(
                                                    IconName::Lock,
                                                    12.0,
                                                    theme.accent,
                                                ))
                                                .child(if is_dirty {
                                                    "Saving…"
                                                } else {
                                                    "Saved locally"
                                                }),
                                        )
                                        .child(div().w(px(1.0)).h(px(18.0)).bg(theme.border))
                                    })
                                    .when(
                                        matches!(
                                            view,
                                            WorkspaceViewPage::Editor | WorkspaceViewPage::Preview
                                        ),
                                        |el| {
                                            el.child(render_button(
                                                "topbar-export-btn",
                                                if self.is_exporting_pdf {
                                                    "Exporting…"
                                                } else if is_narrow {
                                                    "Export"
                                                } else {
                                                    "Export PDF"
                                                },
                                                Some(IconName::Download),
                                                ButtonVariant::Secondary,
                                                self.is_exporting_pdf,
                                                &theme,
                                                cx.listener(|this, _, _, cx| {
                                                    this.open_export_dialog();
                                                    cx.notify();
                                                }),
                                            ))
                                        },
                                    )
                                    .child(render_icon_button(
                                        "theme-toggle-btn",
                                        if theme.mode == ThemeMode::Dark {
                                            IconName::Sun
                                        } else {
                                            IconName::Moon
                                        },
                                        "Toggle dark/light theme",
                                        false,
                                        &theme,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_theme();
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    // View Content (split live PDF preview in Editor on wide viewports)
                    .child(if self.is_split_preview_active() {
                        let preview_w = self.split_preview_pane_width();
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_row()
                            .overflow_hidden()
                            .child(
                                div()
                                    .id(el_id(format!("main-content-scroll-{view:?}")))
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_y_scroll()
                                    .child(render_editor_view(self, cx)),
                            )
                            .child(render_live_preview_pane(self, preview_w, cx))
                            .into_any_element()
                    } else {
                        div()
                            .id(el_id(format!("main-content-scroll-{view:?}")))
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_y_scroll()
                            .child(match view {
                                WorkspaceViewPage::Library => render_library_view(self, cx),
                                WorkspaceViewPage::Jobs => render_jobs_view(self, cx),
                                WorkspaceViewPage::Editor => render_editor_view(self, cx),
                                WorkspaceViewPage::Preview => render_preview_view(self, cx),
                                WorkspaceViewPage::Skills => render_skills_view(self, cx),
                                WorkspaceViewPage::Privacy => render_privacy_view(self, cx),
                                WorkspaceViewPage::Settings => render_settings_view(self, cx),
                            })
                            .into_any_element()
                    })
                    // Status Footer
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .h(px(29.0))
                            .px(if is_narrow { px(12.0) } else { px(20.0) })
                            .border_t_1()
                            .border_color(theme.border)
                            .bg(theme.surface_soft)
                            .text_size(px(11.0))
                            .text_color(theme.muted)
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child("VanguardCV")
                                    .child("·")
                                    .child(if is_dirty {
                                        "Syncing to local SQLite…"
                                    } else {
                                        "Saved to local SQLite"
                                    }),
                            )
                            .when(!is_narrow, |el| {
                                el.child(
                                    div()
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(render_icon(IconName::Lock, 11.0, theme.muted))
                                        .child("Zero-Trust PII Air Gap Active"),
                                )
                            }),
                    ),
            )
            // Modal dialog overlay
            .when_some(render_modal_overlay(self, cx), |el, modal| el.child(modal))
            // Toast notification banner (rendered above modals so validation errors stay visible)
            .when_some(self.notice.clone(), |el, notice_text| {
                let is_err = self.notice_is_error;
                el.child(
                    div()
                        .occlude()
                        .absolute()
                        .top(px(16.0))
                        .right(px(16.0))
                        .max_w(px(toast_max_w))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .py(px(10.0))
                        .px(px(14.0))
                        .rounded(px(6.0))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(if is_err { theme.accent } else { theme.border })
                        .shadow_lg()
                        .child(render_icon(
                            if is_err {
                                IconName::Shield
                            } else {
                                IconName::Check
                            },
                            16.0,
                            if is_err {
                                theme.del_text
                            } else {
                                theme.add_text
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.0))
                                .text_color(theme.text)
                                .child(notice_text),
                        )
                        .child(render_icon_button(
                            "dismiss-notice-btn",
                            IconName::Close,
                            "Dismiss notice",
                            false,
                            &theme,
                            cx.listener(|this, _, _, cx| {
                                this.clear_notice();
                                cx.notify();
                            }),
                        )),
                )
            })
    }
}

fn move_by_id<T>(
    list: &mut Vec<T>,
    is_from: impl Fn(&T) -> bool,
    is_to: impl Fn(&T) -> bool,
) -> bool {
    let Some(from_idx) = list.iter().position(&is_from) else {
        return false;
    };
    let Some(to_idx) = list.iter().position(&is_to) else {
        return false;
    };
    if from_idx == to_idx {
        return false;
    }
    let item = list.remove(from_idx);
    list.insert(to_idx, item);
    true
}

fn index_pair_by_id<T>(
    list: &[T],
    is_from: impl Fn(&T) -> bool,
    is_to: impl Fn(&T) -> bool,
) -> Option<(usize, usize)> {
    let from_idx = list.iter().position(is_from)?;
    let to_idx = list.iter().position(is_to)?;
    if from_idx == to_idx {
        None
    } else {
        Some((from_idx, to_idx))
    }
}

pub fn drag_item_index_pair(
    body: &SanitizedCvBody,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> Option<(usize, usize)> {
    match (dragged, target) {
        (
            EditorDragKind::Section { section_id: from_id },
            EditorDragKind::Section { section_id: to_id },
        ) => index_pair_by_id(&body.sections, |s| s.id == *from_id, |s| s.id == *to_id),
        (
            EditorDragKind::TimelineEntry {
                section_id: fs,
                entry_id: from_id,
            },
            EditorDragKind::TimelineEntry {
                section_id: ts,
                entry_id: to_id,
            },
        ) if fs == ts => {
            let sec = body.sections.iter().find(|s| s.id == *fs)?;
            let SectionKind::TimelineEntries { entries } = &sec.kind else {
                return None;
            };
            index_pair_by_id(entries, |e| e.id == *from_id, |e| e.id == *to_id)
        }
        (
            EditorDragKind::Bullet {
                section_id: fs,
                entry_id: fe,
                bullet_id: from_id,
            },
            EditorDragKind::Bullet {
                section_id: ts,
                entry_id: te,
                bullet_id: to_id,
            },
        ) if fs == ts && fe == te => {
            let sec = body.sections.iter().find(|s| s.id == *fs)?;
            match (&sec.kind, fe) {
                (SectionKind::TimelineEntries { entries }, Some(eid)) => {
                    let entry = entries.iter().find(|e| e.id == *eid)?;
                    index_pair_by_id(&entry.bullets, |b| b.id == *from_id, |b| b.id == *to_id)
                }
                (SectionKind::SimpleBulletList { items }, None) => {
                    index_pair_by_id(items, |b| b.id == *from_id, |b| b.id == *to_id)
                }
                _ => None,
            }
        }
        (
            EditorDragKind::SkillCategory {
                section_id: fs,
                category_id: from_id,
            },
            EditorDragKind::SkillCategory {
                section_id: ts,
                category_id: to_id,
            },
        ) if fs == ts => {
            let sec = body.sections.iter().find(|s| s.id == *fs)?;
            let SectionKind::CategorizedInlineList { categories } = &sec.kind else {
                return None;
            };
            index_pair_by_id(categories, |c| c.id == *from_id, |c| c.id == *to_id)
        }
        _ => None,
    }
}

pub fn reorder_item_in_body(
    body: &mut SanitizedCvBody,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> bool {
    if dragged == target {
        return false;
    }
    match (dragged, target) {
        (
            EditorDragKind::Section { section_id: from_id },
            EditorDragKind::Section { section_id: to_id },
        ) => move_by_id(&mut body.sections, |s| s.id == *from_id, |s| s.id == *to_id),
        (
            EditorDragKind::TimelineEntry {
                section_id: fs,
                entry_id: from_id,
            },
            EditorDragKind::TimelineEntry {
                section_id: ts,
                entry_id: to_id,
            },
        ) if fs == ts => {
            if let Some(sec) = body.sections.iter_mut().find(|s| s.id == *fs)
                && let SectionKind::TimelineEntries { entries } = &mut sec.kind
            {
                move_by_id(entries, |e| e.id == *from_id, |e| e.id == *to_id)
            } else {
                false
            }
        }
        (
            EditorDragKind::Bullet {
                section_id: fs,
                entry_id: fe,
                bullet_id: from_id,
            },
            EditorDragKind::Bullet {
                section_id: ts,
                entry_id: te,
                bullet_id: to_id,
            },
        ) if fs == ts && fe == te => {
            if let Some(sec) = body.sections.iter_mut().find(|s| s.id == *fs) {
                match (&mut sec.kind, fe) {
                    (SectionKind::TimelineEntries { entries }, Some(eid)) => {
                        if let Some(entry) = entries.iter_mut().find(|e| e.id == *eid) {
                            move_by_id(&mut entry.bullets, |b| b.id == *from_id, |b| b.id == *to_id)
                        } else {
                            false
                        }
                    }
                    (SectionKind::SimpleBulletList { items }, None) => {
                        move_by_id(items, |b| b.id == *from_id, |b| b.id == *to_id)
                    }
                    _ => false,
                }
            } else {
                false
            }
        }
        (
            EditorDragKind::SkillCategory {
                section_id: fs,
                category_id: from_id,
            },
            EditorDragKind::SkillCategory {
                section_id: ts,
                category_id: to_id,
            },
        ) if fs == ts => {
            if let Some(sec) = body.sections.iter_mut().find(|s| s.id == *fs)
                && let SectionKind::CategorizedInlineList { categories } = &mut sec.kind
            {
                move_by_id(categories, |c| c.id == *from_id, |c| c.id == *to_id)
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn drag_item_index_pair_in_doc(
    doc: &CvDocument,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> Option<(usize, usize)> {
    match (dragged, target) {
        (
            EditorDragKind::SocialLink { link_id: from_id },
            EditorDragKind::SocialLink { link_id: to_id },
        ) => index_pair_by_id(
            &doc.pii_header.social_links,
            |l| l.id == *from_id,
            |l| l.id == *to_id,
        ),
        (
            EditorDragKind::CustomHeaderField { field_id: from_id },
            EditorDragKind::CustomHeaderField { field_id: to_id },
        ) => index_pair_by_id(
            &doc.pii_header.custom_fields,
            |f| f.id == *from_id,
            |f| f.id == *to_id,
        ),
        _ => drag_item_index_pair(&doc.body, dragged, target),
    }
}

pub fn reorder_item_in_doc(
    doc: &mut CvDocument,
    dragged: &EditorDragKind,
    target: &EditorDragKind,
) -> bool {
    if dragged == target {
        return false;
    }
    match (dragged, target) {
        (
            EditorDragKind::SocialLink { link_id: from_id },
            EditorDragKind::SocialLink { link_id: to_id },
        ) => move_by_id(
            &mut doc.pii_header.social_links,
            |l| l.id == *from_id,
            |l| l.id == *to_id,
        ),
        (
            EditorDragKind::CustomHeaderField { field_id: from_id },
            EditorDragKind::CustomHeaderField { field_id: to_id },
        ) => move_by_id(
            &mut doc.pii_header.custom_fields,
            |f| f.id == *from_id,
            |f| f.id == *to_id,
        ),
        _ => reorder_item_in_body(&mut doc.body, dragged, target),
    }
}
