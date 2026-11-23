use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::contracts::{CvDocument, CvSection, SectionKind, SkillCategoryItem, TimelineEntry};
use crate::ui::components::ui::{
    ButtonVariant, IconName, render_button, render_empty_state, render_icon, render_icon_button,
};
use crate::ui::theme::Theme;
use crate::ui::workspace::{WorkspaceView, WorkspaceViewPage};

pub fn render_preview_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
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
                "Open a CV from CV library to preview or export its PDF.",
                theme,
            ))
            .child(render_button(
                "preview-empty-go-library",
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
    let zoom = workspace.preview_zoom;
    let page_count = workspace.rendered_images.len();
    let is_rendering = workspace.is_rendering_pdf;
    let is_exporting = workspace.is_exporting_pdf;
    let pad_x = workspace.page_padding_x();
    let sidebar_offset = if workspace.is_sidebar_visible() {
        if workspace.viewport_width < 1020.0 { 204.0 } else { 228.0 }
    } else {
        0.0
    };
    let avail_paper_w = (workspace.viewport_width - sidebar_offset - f32::from(pad_x) * 2.0 - 48.0)
        .clamp(280.0, 1100.0);

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
        .gap(px(18.0))
        // Page heading
        .child(
            div()
                .flex()
                .w_full()
                .min_w_0()
                .when(workspace.content_width() < 640.0, |el| el.flex_col())
                .when(workspace.content_width() >= 640.0, |el| el.flex_row().items_start().justify_between())
                .gap(px(12.0))
                .child(
                    div()
                        .when(workspace.content_width() >= 640.0, |el| el.flex_1())
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(26.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("PDF preview"),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .text_color(theme.secondary)
                                .child(doc.title.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(render_button(
                            "preview-refresh-btn",
                            if is_rendering {
                                "Rendering…"
                            } else {
                                "Refresh preview"
                            },
                            Some(IconName::Sparkles),
                            ButtonVariant::Ghost,
                            is_rendering,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.trigger_pdf_preview_render(cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "preview-back-btn",
                            "Back to editor",
                            Some(IconName::Edit),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.navigate_to(WorkspaceViewPage::Editor, cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "preview-export-btn",
                            if is_exporting {
                                "Exporting…"
                            } else {
                                "Export PDF"
                            },
                            Some(IconName::Download),
                            ButtonVariant::Primary,
                            is_exporting,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.open_export_dialog();
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Page overflow warning (when Typst produces > 1 page)
        .when(page_count > 1, |el| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(12.0))
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(theme.del_bg)
                    .border_1()
                    .border_color(theme.del_text)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .flex()
                            .items_start()
                            .gap(px(10.0))
                            .child(div().flex_shrink_0().pt(px(2.0)).child(render_icon(IconName::File, 16.0, theme.del_text)))
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
                                            .text_color(theme.del_text)
                                            .child(format!(
                                                "CV spans {page_count} pages (exceeds 1-page target)"
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(theme.secondary)
                                            .child("Enable the Page Compressor AI skill or toggle off lower-priority achievements in the editor."),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(render_button(
                                "overflow-compress-btn",
                                if workspace.ai_state.is_running {
                                    "Compressing…"
                                } else if doc.parent_id.is_some() {
                                    "Run Page Compressor"
                                } else {
                                    "Compress via variation"
                                },
                                Some(IconName::Sparkles),
                                ButtonVariant::Primary,
                                workspace.ai_state.is_running,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.run_page_compressor(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(render_button(
                                "overflow-go-editor-btn",
                                "Trim in editor",
                                Some(IconName::Edit),
                                ButtonVariant::Secondary,
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.navigate_to(WorkspaceViewPage::Editor, cx);
                                    cx.notify();
                                }),
                            )),
                    ),
            )
        })
        // Typst error banner (if compilation failed)
        .when_some(workspace.preview_error.clone(), |el, err| {
            el.child(
                div()
                    .flex()
                    .items_start()
                    .min_w_0()
                    .gap(px(10.0))
                    .p(px(12.0))
                    .rounded(px(6.0))
                    .bg(theme.del_bg)
                    .border_1()
                    .border_color(theme.del_text)
                    .child(div().flex_shrink_0().pt(px(2.0)).child(render_icon(IconName::Shield, 16.0, theme.del_text)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.del_text)
                                    .child("Typst preview fallback active"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.secondary)
                                    .child(err),
                            ),
                    ),
            )
        })
        // Preview toolbar
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(10.0))
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(220.0))
                        .flex()
                        .items_center()
                        .flex_wrap()
                        .gap(px(8.0))
                        .text_size(px(13.0))
                        .text_color(theme.text)
                        .child(div().flex_shrink_0().child(render_icon(IconName::File, 15.0, theme.accent)))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Classic serif"),
                        )
                        .child(div().text_color(theme.muted).child("·"))
                        .child(
                            div()
                                .text_color(theme.secondary)
                                .child("A4 Typst Engine"),
                        )
                        .when(page_count > 0, |el| {
                            el.child(
                                div()
                                    .px(px(7.0))
                                    .py(px(2.0))
                                    .rounded(px(4.0))
                                    .bg(if page_count == 1 {
                                        theme.add_bg
                                    } else {
                                        theme.del_bg
                                    })
                                    .text_color(if page_count == 1 {
                                        theme.add_text
                                    } else {
                                        theme.del_text
                                    })
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!(
                                        "{page_count} {}",
                                        if page_count == 1 { "page" } else { "pages" }
                                    )),
                            )
                        })
                        .when(is_rendering, |el| {
                            el.child(
                                div()
                                    .px(px(7.0))
                                    .py(px(2.0))
                                    .rounded(px(4.0))
                                    .bg(theme.accent_soft)
                                    .text_color(theme.accent_text)
                                    .text_size(px(11.0))
                                    .child("Compiling…"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_icon_button(
                            "zoom-out-btn",
                            IconName::Minus,
                            "Zoom out",
                            zoom <= 70,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                if this.preview_zoom > 70 {
                                    this.preview_zoom -= 10;
                                    this.clamp_preview_scroll(false);
                                    cx.notify();
                                }
                            }),
                        ))
                        .child(
                            div()
                                .w(px(46.0))
                                .text_center()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text)
                                .child(format!("{zoom}%")),
                        )
                        .child(render_icon_button(
                            "zoom-in-btn",
                            IconName::Plus,
                            "Zoom in",
                            zoom >= 130,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                if this.preview_zoom < 130 {
                                    this.preview_zoom += 10;
                                    cx.notify();
                                }
                            }),
                        )),
                ),
        )
        // Paper Canvas / Rasterized PDF View
        .child({
            let scale = zoom as f32 / 100.0;
            let base_fit = avail_paper_w.min(750.0);
            let target_w = (base_fit * scale).max(260.0);
            let target_h = (target_w * std::f32::consts::SQRT_2).round();
            let inner_w = (target_w + 32.0).max(avail_paper_w);
            let is_panning = workspace.preview_pan_state.is_some_and(|p| !p.is_split);
            let h_scroll = workspace.full_preview_scroll.offset();
            let h_max = workspace.full_preview_scroll.max_offset();
            let show_hbar = f32::from(h_max.width) > 1.0;

            div()
                .relative()
                .w_full()
                .min_w_0()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border_soft)
                .bg(theme.canvas)
                .overflow_hidden()
                .child(
                    div()
                        .id("preview-paper-canvas")
                        .w_full()
                        .min_w_0()
                        .overflow_x_scroll()
                        .track_scroll(&workspace.full_preview_scroll)
                        .when(show_hbar && !is_panning, |d| d.cursor_grab())
                        .when(is_panning, |d| d.cursor_grabbing())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                if f32::from(this.full_preview_scroll.max_offset().width) > 1.0 {
                                    this.begin_preview_pan(false, event.position);
                                    cx.notify();
                                }
                            }),
                        )
                        .on_scroll_wheel(cx.listener(
                            |this, event: &ScrollWheelEvent, _window, cx| {
                                if event.modifiers.shift {
                                    let delta = event.delta.pixel_delta(px(28.0));
                                    if delta.x == px(0.0) && delta.y != px(0.0) {
                                        this.scroll_preview_by(false, delta.y, px(0.0), cx);
                                        cx.stop_propagation();
                                        return;
                                    }
                                }
                                cx.notify();
                            },
                        ))
                        .child(
                            div()
                                .w(px(inner_w))
                                .flex_shrink_0()
                                .py(px(24.0))
                                .px(px(16.0))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(20.0))
                                .when(!workspace.rendered_images.is_empty(), |el| {
                                    let mut list = el;
                                    for img_ref in &workspace.rendered_images {
                                        list = list.child(
                                            div()
                                                .w(px(target_w))
                                                .h(px(target_h))
                                                .flex_shrink_0()
                                                .rounded(px(3.0))
                                                .shadow_lg()
                                                .border_1()
                                                .border_color(theme.paper_border)
                                                .overflow_hidden()
                                                .child(
                                                    img(ImageSource::Render(img_ref.clone()))
                                                        .size_full(),
                                                ),
                                        );
                                    }
                                    list
                                })
                                .when(workspace.rendered_images.is_empty(), |el| {
                                    el.child(render_resume_paper(doc, zoom, target_w, theme))
                                }),
                        ),
                )
                .when(show_hbar, |el| {
                    let vp_w = f32::from(workspace.full_preview_scroll.bounds().size.width)
                        .max(avail_paper_w);
                    let max_x = f32::from(h_max.width).max(1.0);
                    let track_w = (vp_w - 24.0).max(80.0);
                    let ratio = (vp_w / (vp_w + max_x)).clamp(0.14, 0.92);
                    let thumb_w = (track_w * ratio).max(32.0);
                    let progress = ((-f32::from(h_scroll.x)) / max_x).clamp(0.0, 1.0);
                    let thumb_left = (track_w - thumb_w) * progress;
                    el.child(
                        div()
                            .id("full-preview-hbar")
                            .cursor_pointer()
                            .absolute()
                            .bottom(px(6.0))
                            .left(px(12.0))
                            .right(px(12.0))
                            .h(px(8.0))
                            .rounded(px(4.0))
                            .bg(theme.surface_soft)
                            .border_1()
                            .border_color(theme.border)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    let bounds = this.full_preview_scroll.bounds();
                                    let rel = (f32::from(event.position.x - bounds.left()) - 12.0)
                                        / (f32::from(bounds.size.width) - 24.0).max(1.0);
                                    let max_w = f32::from(this.full_preview_scroll.max_offset().width);
                                    let cur = this.full_preview_scroll.offset();
                                    let next_x = px(-(rel.clamp(0.0, 1.0) * max_w));
                                    this.full_preview_scroll.set_offset(point(next_x, cur.y));
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .ml(px(thumb_left))
                                    .w(px(thumb_w))
                                    .h_full()
                                    .rounded(px(4.0))
                                    .bg(theme.muted),
                            ),
                    )
                })
        })
        // Footnote
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .flex_wrap()
                .gap(px(6.0))
                .text_size(px(11.5))
                .text_color(theme.muted)
                .child(div().flex_shrink_0().child(render_icon(IconName::Lock, 12.0, theme.muted)))
                .child("Personal details are combined with your CV only in this local preview."),
        )
}

/// Compact live side-by-side Typst PDF preview pane embedded in the CV editor.
pub fn render_live_preview_pane(
    workspace: &WorkspaceView,
    pane_width: f32,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let theme = &workspace.theme;
    let Some(doc) = workspace.active_document() else {
        return div();
    };
    let zoom = workspace.split_preview_zoom;
    let page_count = workspace.rendered_images.len();
    let is_rendering = workspace.is_rendering_pdf;

    let is_panning = workspace.preview_pan_state.is_some_and(|p| p.is_split);
    let base_fit = (pane_width - 36.0).clamp(240.0, 480.0);
    let target_w = (base_fit * (zoom as f32 / 100.0)).max(220.0);
    let target_h = (target_w * std::f32::consts::SQRT_2).round();
    let inner_w = (target_w + 32.0).max(pane_width - 2.0);
    let scroll_offset = workspace.split_preview_scroll.offset();
    let scroll_max = workspace.split_preview_scroll.max_offset();
    let scroll_bounds = workspace.split_preview_scroll.bounds();
    let max_x = f32::from(scroll_max.width);
    let max_y = f32::from(scroll_max.height);
    let show_hbar = max_x > 1.0 || inner_w > pane_width;
    let show_vbar = max_y > 1.0;

    div()
        .w(px(pane_width))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .flex()
        .flex_col()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.canvas)
        .overflow_hidden()
        // Pane Header Toolbar
        .child(
            div()
                .flex_shrink_0()
                .w_full()
                .min_w_0()
                .px(px(12.0))
                .py(px(8.0))
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.surface_soft)
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
                        .gap(px(6.0))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_shrink_0()
                                .child(render_icon(IconName::File, 14.0, theme.accent)),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child("Live PDF"),
                        )
                        .when(is_rendering, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(6.0))
                                    .py(px(1.5))
                                    .rounded(px(4.0))
                                    .bg(theme.accent_soft)
                                    .text_color(theme.accent_text)
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Compiling…"),
                            )
                        })
                        .when(!is_rendering && page_count > 0, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(6.0))
                                    .py(px(1.5))
                                    .rounded(px(4.0))
                                    .bg(if page_count == 1 {
                                        theme.add_bg
                                    } else {
                                        theme.del_bg
                                    })
                                    .text_color(if page_count == 1 {
                                        theme.add_text
                                    } else {
                                        theme.del_text
                                    })
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!(
                                        "{page_count} {}",
                                        if page_count == 1 { "page" } else { "pages" }
                                    )),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(render_icon_button(
                            "split-zoom-out-btn",
                            IconName::Minus,
                            "Zoom out",
                            zoom <= 70,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                if this.split_preview_zoom > 70 {
                                    this.split_preview_zoom -= 10;
                                    this.clamp_preview_scroll(true);
                                    cx.notify();
                                }
                            }),
                        ))
                        .child(
                            div()
                                .w(px(36.0))
                                .text_center()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child(format!("{zoom}%")),
                        )
                        .child(render_icon_button(
                            "split-zoom-in-btn",
                            IconName::Plus,
                            "Zoom in",
                            zoom >= 150,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                if this.split_preview_zoom < 150 {
                                    this.split_preview_zoom += 10;
                                    cx.notify();
                                }
                            }),
                        ))
                        .child(render_icon_button(
                            "split-refresh-btn",
                            IconName::Sparkles,
                            "Refresh live preview",
                            is_rendering,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.flush_pending_now();
                                this.trigger_pdf_preview_render(cx);
                                cx.notify();
                            }),
                        ))
                        .child(render_icon_button(
                            "split-close-btn",
                            IconName::Close,
                            "Hide live preview (Ctrl+P)",
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.split_preview_open = false;
                                cx.notify();
                            }),
                        )),
                ),
        )
        // Compact Page Overflow Alert + One-Click Page Compressor
        .when(page_count > 1, |el| {
            el.child(
                div()
                    .flex_shrink_0()
                    .w_full()
                    .min_w_0()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.del_text)
                    .bg(theme.del_bg)
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_start()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .pt(px(1.5))
                                    .child(render_icon(IconName::File, 13.0, theme.del_text)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(11.5))
                                    .line_height(px(16.0))
                                    .text_color(theme.del_text)
                                    .child(format!(
                                        "CV spans {page_count} pages (exceeds 1-page target)."
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .child(render_button(
                                "split-run-compressor-btn",
                                if workspace.ai_state.is_running {
                                    "Compressing…"
                                } else if doc.parent_id.is_some() {
                                    "Run Page Compressor"
                                } else {
                                    "Compress via variation"
                                },
                                Some(IconName::Sparkles),
                                ButtonVariant::Primary,
                                workspace.ai_state.is_running,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.run_page_compressor(cx);
                                    cx.notify();
                                }),
                            )),
                    ),
            )
        })
        // Compact Typst Error Banner
        .when_some(workspace.preview_error.clone(), |el, err| {
            el.child(
                div()
                    .flex_shrink_0()
                    .w_full()
                    .min_w_0()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.del_text)
                    .bg(theme.del_bg)
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_shrink_0()
                            .pt(px(1.5))
                            .child(render_icon(IconName::Shield, 13.0, theme.del_text)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.0))
                            .line_height(px(15.0))
                            .text_color(theme.secondary)
                            .child(format!("Typst fallback: {err}")),
                    ),
            )
        })
        // Scrollable 2D A4 Paper Viewport (vertical + horizontal scroll, Shift+wheel, drag-to-pan, scrollbars)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .child(
                    div()
                        .id("editor-split-preview-scroll")
                        .size_full()
                        .overflow_scroll()
                        .track_scroll(&workspace.split_preview_scroll)
                        .when(!is_panning && (show_hbar || show_vbar), |d| d.cursor_grab())
                        .when(is_panning, |d| d.cursor_grabbing())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                this.begin_preview_pan(true, event.position);
                                cx.notify();
                            }),
                        )
                        .on_scroll_wheel(cx.listener(
                            |this, event: &ScrollWheelEvent, _window, cx| {
                                if event.modifiers.shift {
                                    let delta = event.delta.pixel_delta(px(28.0));
                                    if delta.x == px(0.0) && delta.y != px(0.0) {
                                        this.scroll_preview_by(true, delta.y, px(0.0), cx);
                                        cx.stop_propagation();
                                        return;
                                    }
                                }
                                cx.notify();
                            },
                        ))
                        .child(
                            div()
                                .w(px(inner_w))
                                .flex_shrink_0()
                                .px(px(16.0))
                                .pt(px(16.0))
                                .pb(px(28.0))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(14.0))
                                .when(!workspace.rendered_images.is_empty(), |el| {
                                    let mut list = el;
                                    for (idx, img_ref) in workspace.rendered_images.iter().enumerate() {
                                        list = list.child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .gap(px(6.0))
                                                .flex_shrink_0()
                                                .child(
                                                    div()
                                                        .w(px(target_w))
                                                        .h(px(target_h))
                                                        .flex_shrink_0()
                                                        .rounded(px(3.0))
                                                        .shadow_md()
                                                        .border_1()
                                                        .border_color(theme.paper_border)
                                                        .overflow_hidden()
                                                        .child(
                                                            img(ImageSource::Render(img_ref.clone()))
                                                                .size_full(),
                                                        ),
                                                )
                                                .when(page_count > 1, |p| {
                                                    p.child(
                                                        div()
                                                            .text_size(px(10.5))
                                                            .text_color(theme.muted)
                                                            .child(format!("Page {} of {page_count}", idx + 1)),
                                                    )
                                                }),
                                        );
                                    }
                                    list
                                })
                                .when(workspace.rendered_images.is_empty(), |el| {
                                    el.child(render_resume_paper(doc, zoom, target_w, theme))
                                }),
                        ),
                )
                .when(show_hbar && max_x > 1.0, |el| {
                    let vp_w = f32::from(scroll_bounds.size.width).max(pane_width);
                    let track_w = (vp_w - 26.0).max(60.0);
                    let ratio = (vp_w / (vp_w + max_x)).clamp(0.14, 0.92);
                    let thumb_w = (track_w * ratio).max(28.0);
                    let progress = ((-f32::from(scroll_offset.x)) / max_x).clamp(0.0, 1.0);
                    let thumb_left = (track_w - thumb_w) * progress;
                    el.child(
                        div()
                            .id("split-preview-hbar")
                            .cursor_pointer()
                            .absolute()
                            .bottom(px(5.0))
                            .left(px(10.0))
                            .right(px(16.0))
                            .h(px(8.0))
                            .rounded(px(4.0))
                            .bg(theme.surface_soft)
                            .border_1()
                            .border_color(theme.border)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    let bounds = this.split_preview_scroll.bounds();
                                    let rel = (f32::from(event.position.x - bounds.left()) - 10.0)
                                        / (f32::from(bounds.size.width) - 26.0).max(1.0);
                                    let max_w = f32::from(this.split_preview_scroll.max_offset().width);
                                    let cur = this.split_preview_scroll.offset();
                                    let next_x = px(-(rel.clamp(0.0, 1.0) * max_w));
                                    this.split_preview_scroll.set_offset(point(next_x, cur.y));
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(div().ml(px(thumb_left)).w(px(thumb_w)).h_full().rounded(px(4.0)).bg(theme.muted)),
                    )
                })
                .when(show_vbar, |el| {
                    let vp_h = f32::from(scroll_bounds.size.height).max(240.0);
                    let track_h = (vp_h - 28.0).max(60.0);
                    let ratio = (vp_h / (vp_h + max_y)).clamp(0.14, 0.92);
                    let thumb_h = (track_h * ratio).max(28.0);
                    let progress = ((-f32::from(scroll_offset.y)) / max_y).clamp(0.0, 1.0);
                    let thumb_top = (track_h - thumb_h) * progress;
                    el.child(
                        div()
                            .id("split-preview-vbar")
                            .cursor_pointer()
                            .absolute()
                            .top(px(8.0))
                            .bottom(px(20.0))
                            .right(px(4.0))
                            .w(px(7.0))
                            .rounded(px(4.0))
                            .bg(theme.surface_soft)
                            .border_1()
                            .border_color(theme.border)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    let bounds = this.split_preview_scroll.bounds();
                                    let rel = (f32::from(event.position.y - bounds.top()) - 8.0)
                                        / (f32::from(bounds.size.height) - 28.0).max(1.0);
                                    let max_h = f32::from(this.split_preview_scroll.max_offset().height);
                                    let cur = this.split_preview_scroll.offset();
                                    let next_y = px(-(rel.clamp(0.0, 1.0) * max_h));
                                    this.split_preview_scroll.set_offset(point(cur.x, next_y));
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(div().mt(px(thumb_top)).w_full().h(px(thumb_h)).rounded(px(4.0)).bg(theme.muted)),
                    )
                }),
        )
        // Compact Footer Strip
        .child(
            div()
                .flex_shrink_0()
                .w_full()
                .min_w_0()
                .px(px(12.0))
                .py(px(7.0))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.surface_soft)
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
                        .gap(px(6.0))
                        .text_size(px(11.0))
                        .text_color(theme.muted)
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_shrink_0()
                                .child(render_icon(IconName::Lock, 11.0, theme.muted)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(if show_hbar {
                                    "Drag to pan · Shift+scroll sideways"
                                } else {
                                    "A4 Typst · Updates live as you edit"
                                }),
                        ),
                )
                .child(
                    div().flex_shrink_0().child(render_button(
                        "split-full-preview-btn",
                        "Full view",
                        Some(IconName::File),
                        ButtonVariant::Ghost,
                        false,
                        theme,
                        cx.listener(|this, _, _, cx| {
                            this.navigate_to(WorkspaceViewPage::Preview, cx);
                            cx.notify();
                        }),
                    )),
                ),
        )
}

pub fn render_resume_paper(doc: &CvDocument, zoom: u32, target_width: f32, theme: &Theme) -> Div {
    let scale = (target_width / 750.0).clamp(0.45, zoom as f32 / 100.0 + 0.35);
    let paper_width = target_width;

    let header = &doc.pii_header;
    let mut contact_items = Vec::new();
    if let Some(loc) = &header.location
        && !loc.trim().is_empty()
    {
        contact_items.push(loc.as_str());
    }
    if let Some(email) = &header.email
        && !email.trim().is_empty()
    {
        contact_items.push(email.as_str());
    }
    if let Some(phone) = &header.phone
        && !phone.trim().is_empty()
    {
        contact_items.push(phone.as_str());
    }

    let enabled_links: Vec<&crate::contracts::SocialLink> = header
        .social_links
        .iter()
        .filter(|l| l.enabled && (!l.display_text.is_empty() || !l.url.is_empty()))
        .collect();

    let custom_fields: Vec<&crate::contracts::CustomHeaderField> = header
        .custom_fields
        .iter()
        .filter(|f| f.enabled && !f.value.is_empty())
        .collect();

    div()
        .w(px(paper_width))
        .min_h(px(1060.0 * scale))
        .p(px(44.0 * scale))
        .bg(theme.paper_bg)
        .text_color(theme.paper_text)
        .border_1()
        .border_color(theme.paper_border)
        .rounded(px(3.0))
        .shadow_md()
        .flex()
        .flex_col()
        .gap(px(18.0 * scale))
        // Header
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .text_center()
                .gap(px(4.0 * scale))
                .child(
                    div()
                        .text_size(px(22.0 * scale))
                        .font_weight(FontWeight::BOLD)
                        .child(if header.full_name.is_empty() {
                            "YOUR NAME".to_string()
                        } else {
                            header.full_name.to_uppercase()
                        }),
                )
                .when_some(
                    header
                        .headline_job_title
                        .as_deref()
                        .filter(|s| !s.is_empty()),
                    |el, title| {
                        el.child(
                            div()
                                .text_size(px(13.5 * scale))
                                .text_color(rgb(0x444444))
                                .child(title.to_string()),
                        )
                    },
                )
                .when(!contact_items.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.5 * scale))
                            .text_color(rgb(0x555555))
                            .child(contact_items.join("  |  ")),
                    )
                })
                .when(!enabled_links.is_empty(), |el| {
                    let mut links_row = div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_wrap()
                        .gap(px(8.0 * scale))
                        .text_size(px(11.5 * scale))
                        .text_color(rgb(0x1a4575));

                    for (idx, link) in enabled_links.iter().enumerate() {
                        if idx > 0 {
                            links_row = links_row.child(div().text_color(rgb(0x888888)).child("|"));
                        }
                        let text = if !link.display_text.is_empty() {
                            link.display_text.clone()
                        } else {
                            link.url.clone()
                        };
                        links_row = links_row.child(div().underline().child(text));
                    }
                    el.child(links_row)
                })
                .when(!custom_fields.is_empty(), |el| {
                    let custom_line: Vec<String> = custom_fields
                        .iter()
                        .map(|f| format!("{}: {}", f.label, f.value))
                        .collect();
                    el.child(
                        div()
                            .text_size(px(11.0 * scale))
                            .text_color(rgb(0x555555))
                            .child(custom_line.join("  |  ")),
                    )
                }),
        )
        // Body sections
        .children(
            doc.body
                .sections
                .iter()
                .filter(|s| s.enabled)
                .map(|sec| render_paper_section(sec, scale)),
        )
}

fn render_paper_section(sec: &CvSection, scale: f32) -> Div {
    let mut section_div = div().flex().flex_col().gap(px(8.0 * scale));

    // Section title uppercase with underlined bar
    section_div = section_div.child(
        div()
            .flex()
            .flex_col()
            .gap(px(2.0 * scale))
            .child(
                div()
                    .text_size(px(12.5 * scale))
                    .font_weight(FontWeight::BOLD)
                    .child(sec.title.to_uppercase()),
            )
            .child(div().w_full().h(px(1.0 * scale)).bg(rgb(0x222222))),
    );

    match &sec.kind {
        SectionKind::Summary { text } => {
            section_div = section_div.child(
                div()
                    .text_size(px(11.0 * scale))
                    .line_height(px(16.0 * scale))
                    .text_color(rgb(0x333333))
                    .child(text.clone()),
            );
        }
        SectionKind::TimelineEntries { entries } => {
            let mut list = div().flex().flex_col().gap(px(10.0 * scale));
            for entry in entries.iter().filter(|e| e.enabled) {
                list = list.child(render_paper_entry(entry, scale));
            }
            section_div = section_div.child(list);
        }
        SectionKind::CategorizedInlineList { categories } => {
            let mut list = div().flex().flex_col().gap(px(4.0 * scale));
            for cat in categories.iter().filter(|c| c.enabled) {
                list = list.child(render_paper_category(cat, scale));
            }
            section_div = section_div.child(list);
        }
        SectionKind::SimpleBulletList { items } => {
            let mut list = div().flex().flex_col().gap(px(4.0 * scale));
            for item in items.iter().filter(|i| i.enabled) {
                list = list.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(6.0 * scale))
                        .text_size(px(10.5 * scale))
                        .line_height(px(15.0 * scale))
                        .child(div().child("•"))
                        .child(item.text.clone()),
                );
            }
            section_div = section_div.child(list);
        }
    }

    section_div
}

fn render_paper_entry(entry: &TimelineEntry, scale: f32) -> Div {
    let mut entry_div = div().flex().flex_col().gap(px(4.0 * scale));

    let header_row = div()
        .flex()
        .items_start()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(11.5 * scale))
                        .font_weight(FontWeight::BOLD)
                        .child(entry.primary_heading.clone()),
                )
                .when_some(
                    entry.secondary_heading.as_deref().filter(|s| !s.is_empty()),
                    |el, role| el.child(div().text_size(px(11.0 * scale)).child(role.to_string())),
                ),
        )
        .child(
            div()
                .text_size(px(11.0 * scale))
                .font_weight(FontWeight::MEDIUM)
                .child(entry.date_range()),
        );

    entry_div = entry_div.child(header_row);

    let active_bullets: Vec<_> = entry.bullets.iter().filter(|b| b.enabled).collect();
    if !active_bullets.is_empty() {
        let mut bullets_list = div()
            .flex()
            .flex_col()
            .gap(px(3.0 * scale))
            .pl(px(10.0 * scale));
        for bullet in active_bullets {
            bullets_list = bullets_list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.0 * scale))
                    .text_size(px(10.5 * scale))
                    .line_height(px(15.0 * scale))
                    .child(div().child("•"))
                    .child(bullet.text.clone()),
            );
        }
        entry_div = entry_div.child(bullets_list);
    }

    entry_div
}

fn render_paper_category(cat: &SkillCategoryItem, scale: f32) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(6.0 * scale))
        .text_size(px(10.5 * scale))
        .line_height(px(15.0 * scale))
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .child(format!("{}:", cat.category_label)),
        )
        .child(div().child(cat.joined_items()))
}
