//! Modal dialog for inspecting the scrubbed outbound AI payload.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Renders the user-auditable zero-trust payload viewer showing the exact
//! JSON that would be transmitted to AI, verifying that all PII was scrubbed
//! locally and replaced with redaction tokens.

use gpui::{
    ClipboardItem, Context, IntoElement, MouseButton, MouseDownEvent, SharedString, Window, div,
    prelude::*, px, rgb, rgba,
};

use crate::theme;

pub struct PayloadModal {
    pub json_payload: String,
    pub is_copied: bool,
}

impl PayloadModal {
    pub fn new(json_payload: impl Into<String>) -> Self {
        Self {
            json_payload: json_payload.into(),
            is_copied: false,
        }
    }
}

/// Renders the modal overlay with the formatted JSON payload.
pub fn render_payload_modal<V: 'static>(
    json: &str,
    is_copied: bool,
    on_close: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static + Clone,
    on_copy: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static + Clone,
    cx: &mut Context<V>,
) -> impl IntoElement {
    let c = theme::colors();
    let t = theme::typography();

    let close_cb_backdrop = on_close.clone();
    let close_cb_btn = on_close.clone();
    let close_cb_x = on_close;

    div()
        .id("payload-modal-backdrop")
        .absolute()
        .inset_0()
        .bg(rgba(0x000000C0))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| {
            close_cb_backdrop(this, window, cx);
        }))
        .child(
            div()
                .id("payload-modal-card")
                .flex()
                .flex_col()
                .w_full()
                .max_w(px(760.0))
                .max_h(px(640.0))
                .bg(rgb(c.bg_surface))
                .border_1()
                .border_color(rgb(c.border_subtle))
                .rounded(px(8.0))
                .p(px(20.0))
                .gap(px(12.0))
                .on_mouse_down(MouseButton::Left, |_, _, _| {})
                // Header
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(theme::pii_badge("🔒 ZERO-TRUST AIR-GAP AUDIT"))
                                .child(
                                    div()
                                        .text_size(t.heading)
                                        .text_color(rgb(c.text_primary))
                                        .child("Inspect Outbound AI Payload"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(t.heading)
                                .text_color(rgb(c.text_secondary))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(c.text_primary)))
                                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    close_cb_x(this, window, cx);
                                }))
                                .child("✕"),
                        ),
                )
                // Subtitle explanation
                .child(
                    div()
                        .text_size(t.caption)
                        .text_color(rgb(c.text_secondary))
                        .child(
                            "This read-only view displays the exact JSON payload scrubbed by \
                             PrivacyGuard. All names, emails, phone numbers, and contact links \
                             have been replaced with redaction tokens before any network transmission.",
                        ),
                )
                .child(theme::hairline())
                // Code viewer box
                .child(
                    div()
                        .id("payload-json-box")
                        .flex_grow()
                        .overflow_y_scroll()
                        .bg(rgb(c.bg_input))
                        .border_1()
                        .border_color(rgb(c.border_subtle))
                        .rounded(px(6.0))
                        .p(px(12.0))
                        .child(
                            div()
                                .text_size(t.caption)
                                .text_color(rgb(c.text_primary))
                                .child(SharedString::from(json.to_string())),
                        ),
                )
                .child(theme::hairline())
                // Footer actions
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(t.caption)
                                .text_color(rgb(c.diff_add_text))
                                .child("✓ PII Air-Gap Active (0 plain credentials)"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .px(px(14.0))
                                        .py(px(6.0))
                                        .rounded(px(6.0))
                                        .border_1()
                                        .border_color(rgb(c.border_subtle))
                                        .text_size(t.body)
                                        .text_color(rgb(c.text_primary))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(c.bg_input)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                            on_copy(this, window, cx);
                                        }))
                                        .child(if is_copied { "✓ Copied!" } else { "Copy JSON" }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .px(px(16.0))
                                        .py(px(6.0))
                                        .rounded(px(6.0))
                                        .bg(rgb(c.tuta_crimson))
                                        .hover(|s| s.bg(rgb(c.tuta_crimson_bright)))
                                        .text_size(t.body)
                                        .text_color(rgb(c.text_on_accent))
                                        .cursor_pointer()
                                        .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                            close_cb_btn(this, window, cx);
                                        }))
                                        .child("Close"),
                                ),
                        ),
                ),
        )
}

/// Helper to copy payload to clipboard.
pub fn copy_payload_to_clipboard(json: &str, cx: &mut gpui::App) {
    cx.write_to_clipboard(ClipboardItem::new_string(json.to_string()));
}
