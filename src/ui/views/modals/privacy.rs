use gpui::*;

use crate::ui::components::ui::{ButtonVariant, IconName, render_button, render_icon, render_input_box};
use crate::ui::theme::{FONT_MONO, RADIUS_MD, TEXT_SM, Theme};
use crate::ui::workspace::WorkspaceView;

use super::modal_frame;
pub(super) fn render_payload_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let (payload_json, redaction_count) = workspace.build_payload_preview();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .p(px(8.0))
                .rounded(px(6.0))
                .bg(theme.input)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(div().flex_shrink_0().pt(px(1.5)).child(render_icon(IconName::Shield, 14.0, theme.accent)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.0))
                                .text_color(theme.secondary)
                                .child("Personal contact header excluded by Zero-Trust scrubber"),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .px(px(7.0))
                        .py(px(1.5))
                        .rounded(px(4.0))
                        .bg(theme.add_bg)
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.add_text)
                        .child(format!(
                            "{redaction_count} PII {} redacted",
                            if redaction_count == 1 {
                                "token"
                            } else {
                                "tokens"
                            }
                        )),
                ),
        )
        .child(
            div()
                .id("payload-code-scroll")
                .w_full()
                .h(px((workspace.viewport_height - 270.0).clamp(160.0, 340.0)))
                .min_w_0()
                .p(px(12.0))
                .rounded(px(RADIUS_MD))
                .bg(theme.code_bg)
                .border_1()
                .border_color(theme.border_control)
                .overflow_y_scroll()
                .text_size(px(TEXT_SM))
                .font_family(FONT_MONO)
                .text_color(theme.code_text)
                .child(payload_json.clone()),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .pt(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .child("Exact outbound payload queued for AI processing."),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(render_button(
                            "copy-payload-btn",
                            "Copy JSON",
                            Some(IconName::Copy),
                            ButtonVariant::Secondary,
                            false,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    payload_json.clone(),
                                ));
                                this.notify("Anonymized payload copied to clipboard.");
                                cx.notify();
                            }),
                        ))
                        .child(render_button(
                            "done-payload-btn",
                            "Done",
                            None,
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.close_dialog();
                                cx.notify();
                            }),
                        )),
                ),
        );

    modal_frame(
        workspace,
        "AI payload preview",
        Some("The anonymized content prepared from your current CV. Zero PII is transmitted."),
        true,
        theme,
        cx,
        body,
    )
}


pub(super) fn render_edit_contact_modal(
    workspace: &WorkspaceView,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Full name"),
                )
                .child(
                    render_input_box(workspace.pii_name_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Professional headline"),
                )
                .child(
                    render_input_box(workspace.pii_headline_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Email address"),
                        )
                        .child(
                            render_input_box(workspace.pii_email_input.clone(), 34.0, theme, cx),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.secondary)
                                .child("Phone number"),
                        )
                        .child(
                            render_input_box(workspace.pii_phone_input.clone(), 34.0, theme, cx),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Location"),
                )
                .child(
                    render_input_box(workspace.pii_location_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-contact-btn",
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
                    "save-contact-btn",
                    "Save contact details",
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_contact_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        "Contact details",
        Some("Combined only with your local document."),
        false,
        theme,
        cx,
        body,
    )
}


pub(super) fn render_edit_social_modal(
    workspace: &WorkspaceView,
    is_edit: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Platform"),
                )
                .child(
                    render_input_box(workspace.social_platform_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Display label (shown in PDF)"),
                )
                .child(
                    render_input_box(workspace.social_display_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Target URL"),
                )
                .child(
                    render_input_box(workspace.social_url_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-social-btn",
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
                    "save-social-btn",
                    if is_edit { "Save link" } else { "Add link" },
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_social_link_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        if is_edit {
            "Edit social link"
        } else {
            "Add social or portfolio link"
        },
        Some("Rendered as a clickable hyperlink in your local PDF header. Never sent to AI."),
        false,
        theme,
        cx,
        body,
    )
}


pub(super) fn render_edit_custom_field_modal(
    workspace: &WorkspaceView,
    is_edit: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
    let body = div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Field label"),
                )
                .child(
                    render_input_box(workspace.custom_field_label_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.secondary)
                        .child("Value"),
                )
                .child(
                    render_input_box(workspace.custom_field_value_input.clone(), 34.0, theme, cx),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .pt(px(10.0))
                .child(render_button(
                    "cancel-field-btn",
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
                    "save-field-btn",
                    if is_edit { "Save field" } else { "Add field" },
                    Some(IconName::Check),
                    ButtonVariant::Primary,
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.save_custom_field_from_dialog(cx);
                        cx.notify();
                    }),
                )),
        );

    modal_frame(
        workspace,
        if is_edit {
            "Edit custom header field"
        } else {
            "Add custom header field"
        },
        Some("Displayed in your local PDF header and scrubbed from AI payloads."),
        false,
        theme,
        cx,
        body,
    )
}

