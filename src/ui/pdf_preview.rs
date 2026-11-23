//! Live rasterized PDF preview viewport.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Phase 1: Renders rasterized preview pages from `services.pdf` on a
//! background task and displays them in the right pane. Falls back to a
//! branded placeholder if page rendering fails.

use std::sync::Arc;

use gpui::{
    Context, EventEmitter, ImageSource, IntoElement, MouseButton, MouseDownEvent, RenderImage,
    SharedString, Window, div, img, prelude::*, px, rgb,
};
use image::Frame;

use crate::contracts::{CvDocument, RenderedPage, Services};
use crate::theme;

// ---------------------------------------------------------------------------
// Preview view
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfPreviewEvent {
    ExportRequested,
}

pub struct PdfPreviewView {
    services: Services,
    /// The currently rendered page images (BGRA-converted).
    rendered_images: Vec<Arc<RenderImage>>,
    /// Error message if rendering failed.
    error: Option<String>,
    /// Whether a render task is in flight.
    loading: bool,
    /// The document that was last requested for rendering.
    last_doc_version: Option<(uuid::Uuid, u32)>,
}

impl EventEmitter<PdfPreviewEvent> for PdfPreviewView {}

impl PdfPreviewView {
    pub fn new(services: Services, _cx: &mut Context<Self>) -> Self {
        Self {
            services,
            rendered_images: Vec::new(),
            error: None,
            loading: false,
            last_doc_version: None,
        }
    }

    /// Renders the document preview, bypassing the version deduplication check.
    /// Used for real-time live typing updates.
    pub fn render_document_live(&mut self, doc: &CvDocument, cx: &mut Context<Self>) {
        self.last_doc_version = None;
        self.render_document(doc, cx);
    }

    /// Triggers a background render of the document's PDF preview pages.
    pub fn render_document(&mut self, doc: &CvDocument, cx: &mut Context<Self>) {
        // Deduplicate: skip if we already rendered this version.
        let key = (doc.id, doc.version);
        if self.last_doc_version == Some(key) && !self.rendered_images.is_empty() {
            return;
        }
        self.last_doc_version = Some(key);
        self.loading = true;
        self.error = None;
        cx.notify();

        let pii = doc.pii_header.clone();
        let body = doc.body.clone();
        let pdf_compiler = self.services.pdf.clone();

        cx.spawn(async move |this, cx| {
            // Rasterize on GPUI's background executor, never the UI thread.
            let executor = cx.background_executor().clone();
            let result = executor
                .spawn(async move { pdf_compiler.render_preview_pages(&pii, &body, 3) })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(pages) => {
                        view.rendered_images = pages
                            .into_iter()
                            .filter_map(|page| rendered_page_to_image(&page))
                            .collect();
                        if view.rendered_images.is_empty() {
                            view.error = Some("PDF rendered 0 pages.".to_string());
                        }
                    }
                    Err(err) => {
                        tracing::warn!("PDF preview render failed: {err}");
                        view.error = Some(format!("Preview error: {err}"));
                        view.rendered_images.clear();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Clears the preview (when no document is selected).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.rendered_images.clear();
        self.error = None;
        self.loading = false;
        self.last_doc_version = None;
        cx.notify();
    }
}

/// Converts a `RenderedPage` (RGBA) to a GPUI `RenderImage` (BGRA).
fn rendered_page_to_image(page: &RenderedPage) -> Option<Arc<RenderImage>> {
    if page.rgba.len() != (page.width_px * page.height_px * 4) as usize {
        return None;
    }
    // GPUI expects BGRA; swap R and B channels.
    let mut bgra = page.rgba.clone();
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2); // R <-> B
    }

    let img_buf = image::RgbaImage::from_raw(page.width_px, page.height_px, bgra)?;
    let frame = Frame::new(img_buf);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

impl Render for PdfPreviewView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme::colors();
        let t = theme::typography();

        let mut container = div()
            .id("pdf-preview-scroll")
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(c.bg_canvas))
            .overflow_y_scroll();

        // Header
        container = container.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .py(px(8.0))
                .child(
                    div()
                        .text_size(t.subheading)
                        .text_color(rgb(c.text_primary))
                        .child("📄 PDF Preview"),
                )
                .child(
                    div()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                                cx.emit(PdfPreviewEvent::ExportRequested);
                            }),
                        )
                        .child(theme::button_primary("Export PDF")),
                ),
        );

        container = container.child(theme::hairline());

        if self.loading {
            container = container.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_grow()
                    .py(px(40.0))
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.text_secondary))
                            .child("Rendering preview…"),
                    ),
            );
        } else if let Some(err) = &self.error {
            container = container.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_grow()
                    .gap(px(8.0))
                    .py(px(40.0))
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.diff_del_text))
                            .child(SharedString::from(err.clone())),
                    )
                    .child(
                        div()
                            .text_size(t.caption)
                            .text_color(rgb(c.text_disabled))
                            .child("The PDF compiler may not be fully implemented yet."),
                    ),
            );
        } else if self.rendered_images.is_empty() {
            // Branded placeholder
            container = container.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .flex_grow()
                    .gap(px(12.0))
                    .py(px(60.0))
                    .child(
                        div()
                            .text_size(t.title)
                            .text_color(rgb(c.tuta_crimson))
                            .child("VanguardCV"),
                    )
                    .child(
                        div()
                            .text_size(t.body)
                            .text_color(rgb(c.text_disabled))
                            .child("Select a CV to preview its PDF rendering."),
                    ),
            );
        } else {
            // Render page images
            let mut pages_container = div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.0))
                .p(px(12.0));

            for (i, render_img) in self.rendered_images.iter().enumerate() {
                let page_label: SharedString = format!("Page {}", i + 1).into();
                pages_container = pages_container.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            img(ImageSource::Render(render_img.clone()))
                                .w_full()
                                .max_w(px(640.0))
                                .border_1()
                                .border_color(rgb(c.border_subtle))
                                .rounded(px(4.0)),
                        )
                        .child(
                            div()
                                .text_size(t.caption)
                                .text_color(rgb(c.text_disabled))
                                .child(page_label),
                        ),
                );
            }

            container = container.child(pages_container);
        }

        container
    }
}
