use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::ui::components::ui::{
    ButtonVariant, IconName, el_id, render_button, render_icon, render_icon_button, render_toggle,
};
use crate::ui::theme::Theme;
use crate::ui::views::editor::render_drag_handle;
use crate::ui::workspace::{
    EditorDragKind, EditorDragPayload, WorkspaceView, WorkspaceViewPage,
};

pub fn render_privacy_view(workspace: &WorkspaceView, cx: &mut Context<WorkspaceView>) -> Div {
    let theme = &workspace.theme;
    let pad_x = workspace.page_padding_x();
    let Some(doc) = workspace.active_document() else {
        return render_privacy_no_document_state(workspace, pad_x, theme, cx);
    };
    let header = &doc.pii_header;
    let (_, redaction_count) = workspace.build_payload_preview();
    let stack_columns = workspace.content_width() < 860.0;

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
                .when(workspace.content_width() < 560.0, |el| el.flex_col())
                .when(workspace.content_width() >= 560.0, |el| el.flex_row().items_start().justify_between())
                .gap(px(12.0))
                .child(
                    div()
                        .when(workspace.content_width() >= 560.0, |el| el.flex_1())
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(26.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("Personal information"),
                        )
                        .child(
                            div()
                                .text_size(px(13.5))
                                .text_color(theme.secondary)
                                .child(
                                    "Your identity stays separate from the career content AI can see.",
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(5.0))
                        .bg(theme.accent_soft)
                        .text_color(theme.accent_text)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(render_icon(IconName::Lock, 12.0, theme.accent_text))
                        .child("Local only"),
                ),
        )
        // Two-column layout (stacks vertically on compact/narrow windows)
        .child(
            div()
                .flex()
                .w_full()
                .when(stack_columns, |el| el.flex_col())
                .when(!stack_columns, |el| el.items_start())
                .min_w_0()
                .gap(px(20.0))
                // Left Column: Configuration stack
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        // Contact details card
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(20.0))
                                .rounded(px(6.0))
                                .bg(theme.surface)
                                .border_1()
                                .border_color(theme.border)
                                .gap(px(16.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
                                        .gap(px(10.0))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(180.0))
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(px(15.0))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.text)
                                                        .child("Contact details"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .text_color(theme.muted)
                                                        .child("Included in your PDF. Excluded from the AI payload preview."),
                                                ),
                                        )
                                        .child(render_button(
                                            "edit-contact-btn",
                                            "Edit fields",
                                            Some(IconName::Edit),
                                            ButtonVariant::Ghost,
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.open_edit_contact_dialog(cx);
                                                cx.notify();
                                            }),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(10.0))
                                        .child(render_field_row(
                                            "Full name",
                                            if header.full_name.is_empty() {
                                                "Not set"
                                            } else {
                                                &header.full_name
                                            },
                                            theme,
                                        ))
                                        .child(render_field_row(
                                            "Headline",
                                            header.headline_job_title.as_deref().unwrap_or("Not set"),
                                            theme,
                                        ))
                                        .child(render_field_row(
                                            "Email",
                                            header.email.as_deref().unwrap_or("Not set"),
                                            theme,
                                        ))
                                        .child(render_field_row(
                                            "Phone",
                                            header.phone.as_deref().unwrap_or("Not set"),
                                            theme,
                                        ))
                                        .child(render_field_row(
                                            "Location",
                                            header.location.as_deref().unwrap_or("Not set"),
                                            theme,
                                        )),
                                ),
                        )
                        // Social links card
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(20.0))
                                .rounded(px(6.0))
                                .bg(theme.surface)
                                .border_1()
                                .border_color(theme.border)
                                .gap(px(16.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
                                        .gap(px(10.0))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(180.0))
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(px(15.0))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.text)
                                                        .child("Social & portfolio links"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .text_color(theme.muted)
                                                        .child("Drag the six-dot handle to reorder links in your PDF header. Toggle off per document if needed."),
                                                ),
                                        )
                                        .child(render_button(
                                            "add-social-btn",
                                            "Add link",
                                            Some(IconName::Plus),
                                            ButtonVariant::Ghost,
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.open_add_social_dialog(cx);
                                                cx.notify();
                                            }),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.0))
                                        .children(header.social_links.iter().map(|link| {
                                            let link_id = link.id;
                                            let link_id_del = link.id;
                                            let link_clone = link.clone();
                                            let link_display = if !link.display_text.is_empty() {
                                                link.display_text.clone()
                                            } else {
                                                link.url.clone()
                                            };
                                            let link_drag_kind = EditorDragKind::SocialLink {
                                                link_id: link.id,
                                            };
                                            let link_is_dragging = workspace.active_drag_item.as_ref()
                                                == Some(&link_drag_kind);
                                            let link_target_move = link_drag_kind.clone();
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .flex_wrap()
                                                .gap(px(8.0))
                                                .px(px(12.0))
                                                .py(px(10.0))
                                                .rounded(px(6.0))
                                                .border_1()
                                                .border_color(if link_is_dragging {
                                                    theme.accent
                                                } else {
                                                    theme.border_soft
                                                })
                                                .bg(if link_is_dragging {
                                                    theme.accent_soft
                                                } else {
                                                    theme.input
                                                })
                                                .when(link_is_dragging, |s| {
                                                    s.opacity(0.72).shadow_md()
                                                })
                                                .when(!link.enabled && !link_is_dragging, |s| {
                                                    s.opacity(0.6)
                                                })
                                                .on_drag_move::<EditorDragPayload>(cx.listener(
                                                    move |this,
                                                          event: &DragMoveEvent<EditorDragPayload>,
                                                          _window,
                                                          cx| {
                                                        let dragged = event.drag(cx).kind.clone();
                                                        this.handle_editor_drag_move(
                                                            &dragged,
                                                            &link_target_move,
                                                            event.event.position,
                                                            event.bounds,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .on_drop::<EditorDragPayload>(cx.listener(
                                                    |this, _, _window, cx| {
                                                        this.finish_editor_drag(cx);
                                                    },
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(160.0))
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.0))
                                                        .child(render_drag_handle(
                                                            format!("drag-link-{}", link.id),
                                                            EditorDragPayload {
                                                                kind: link_drag_kind,
                                                                label: link.platform.clone(),
                                                                sublabel: Some(link_display.clone()),
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
                                                                        .text_size(px(13.0))
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(theme.text)
                                                                        .child(
                                                                            link.platform.clone(),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(12.0))
                                                                        .text_color(theme.secondary)
                                                                        .child(link_display),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.0))
                                                        .child(render_toggle(
                                                            format!("toggle-link-{}", link.id),
                                                            link.enabled,
                                                            theme,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.toggle_social_link_enabled(
                                                                    link_id, cx,
                                                                );
                                                                cx.notify();
                                                            }),
                                                        ))
                                                        .child(render_icon_button(
                                                            format!("edit-link-{}", link.id),
                                                            IconName::Edit,
                                                            "Edit link",
                                                            false,
                                                            theme,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.open_edit_social_dialog(
                                                                    &link_clone,
                                                                    cx,
                                                                );
                                                                cx.notify();
                                                            }),
                                                        ))
                                                        .child(render_icon_button(
                                                            format!("del-link-{}", link.id),
                                                            IconName::Trash,
                                                            "Delete link",
                                                            false,
                                                            theme,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.delete_social_link(
                                                                    link_id_del,
                                                                    cx,
                                                                );
                                                                cx.notify();
                                                            }),
                                                        )),
                                                )
                                        }))
                                        .when(header.social_links.is_empty(), |el| {
                                            el.child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(theme.muted)
                                                    .child("No social links added yet."),
                                            )
                                        }),
                                ),
                        )
                        // Custom header fields
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(20.0))
                                .rounded(px(6.0))
                                .bg(theme.surface)
                                .border_1()
                                .border_color(theme.border)
                                .gap(px(16.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
                                        .gap(px(10.0))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(180.0))
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(px(15.0))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.text)
                                                        .child("Custom header fields"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .text_color(theme.muted)
                                                        .child("Add clearance, citizenship, or work authorization. Drag to reorder in your PDF header."),
                                                ),
                                        )
                                        .child(render_button(
                                            "add-field-btn",
                                            "Add field",
                                            Some(IconName::Plus),
                                            ButtonVariant::Ghost,
                                            false,
                                            theme,
                                            cx.listener(|this, _, _, cx| {
                                                this.open_add_custom_field_dialog(cx);
                                                cx.notify();
                                            }),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(8.0))
                                        .children(header.custom_fields.iter().map(|field| {
                                            let field_id = field.id;
                                            let field_id_del = field.id;
                                            let field_clone = field.clone();
                                            let field_drag_kind = EditorDragKind::CustomHeaderField {
                                                field_id: field.id,
                                            };
                                            let field_is_dragging =
                                                workspace.active_drag_item.as_ref()
                                                    == Some(&field_drag_kind);
                                            let field_target_move = field_drag_kind.clone();
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .flex_wrap()
                                                .gap(px(8.0))
                                                .px(px(12.0))
                                                .py(px(10.0))
                                                .rounded(px(6.0))
                                                .border_1()
                                                .border_color(if field_is_dragging {
                                                    theme.accent
                                                } else {
                                                    theme.border_soft
                                                })
                                                .bg(if field_is_dragging {
                                                    theme.accent_soft
                                                } else {
                                                    theme.input
                                                })
                                                .when(field_is_dragging, |s| {
                                                    s.opacity(0.72).shadow_md()
                                                })
                                                .when(!field.enabled && !field_is_dragging, |s| {
                                                    s.opacity(0.6)
                                                })
                                                .on_drag_move::<EditorDragPayload>(cx.listener(
                                                    move |this,
                                                          event: &DragMoveEvent<EditorDragPayload>,
                                                          _window,
                                                          cx| {
                                                        let dragged = event.drag(cx).kind.clone();
                                                        this.handle_editor_drag_move(
                                                            &dragged,
                                                            &field_target_move,
                                                            event.event.position,
                                                            event.bounds,
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .on_drop::<EditorDragPayload>(cx.listener(
                                                    |this, _, _window, cx| {
                                                        this.finish_editor_drag(cx);
                                                    },
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(160.0))
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.0))
                                                        .child(render_drag_handle(
                                                            format!("drag-field-{}", field.id),
                                                            EditorDragPayload {
                                                                kind: field_drag_kind,
                                                                label: field.label.clone(),
                                                                sublabel: Some(field.value.clone()),
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
                                                                        .text_size(px(13.0))
                                                                        .font_weight(
                                                                            FontWeight::SEMIBOLD,
                                                                        )
                                                                        .text_color(theme.text)
                                                                        .child(format!(
                                                                            "{}:",
                                                                            field.label
                                                                        )),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(12.0))
                                                                        .text_color(theme.secondary)
                                                                        .child(field.value.clone()),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.0))
                                                        .child(render_toggle(
                                                            format!("toggle-field-{}", field.id),
                                                            field.enabled,
                                                            theme,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.toggle_custom_field_enabled(
                                                                    field_id, cx,
                                                                );
                                                                cx.notify();
                                                            }),
                                                        ))
                                                        .child(render_icon_button(
                                                            format!("edit-field-{}", field.id),
                                                            IconName::Edit,
                                                            "Edit custom field",
                                                            false,
                                                            theme,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.open_edit_custom_field_dialog(
                                                                    &field_clone,
                                                                    cx,
                                                                );
                                                                cx.notify();
                                                            }),
                                                        ))
                                                        .child(render_icon_button(
                                                            format!("del-field-{}", field.id),
                                                            IconName::Trash,
                                                            "Delete custom field",
                                                            false,
                                                            theme,
                                                            cx.listener(move |this, _, _, cx| {
                                                                this.delete_custom_field(
                                                                    field_id_del,
                                                                    cx,
                                                                );
                                                                cx.notify();
                                                            }),
                                                        )),
                                                )
                                        }))
                                        .when(header.custom_fields.is_empty(), |el| {
                                            el.child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(theme.muted)
                                                    .child("No custom header fields added yet."),
                                            )
                                        }),
                                ),
                        ),
                )
                // Right Column: Air-Gap guarantees
                .child(
                    div()
                        .when(!stack_columns, |el| el.w(px(320.0)).flex_shrink_0())
                        .when(stack_columns, |el| el.w_full())
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(16.0))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(18.0))
                                .rounded(px(6.0))
                                .bg(theme.surface)
                                .border_1()
                                .border_color(theme.border)
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(10.0))
                                        .child(render_icon(IconName::Shield, 18.0, theme.accent))
                                        .child(
                                            div()
                                                .text_size(px(14.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(theme.text)
                                                .child("Zero-trust PII air gap"),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .line_height(px(18.0))
                                        .text_color(theme.secondary)
                                        .child("Personal information is kept strictly local on your machine. When AI analyzes your CV, your name, contact data, and custom identifiers are scrubbed completely before any transmission."),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
                                        .gap(px(8.0))
                                        .p(px(10.0))
                                        .rounded(px(5.0))
                                        .bg(theme.input)
                                        .border_1()
                                        .border_color(theme.border_soft)
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(theme.secondary)
                                                .child("Live outbound audit"),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .px(px(7.0))
                                                .py(px(2.0))
                                                .rounded(px(4.0))
                                                .bg(theme.add_bg)
                                                .text_color(theme.add_text)
                                                .text_size(px(11.0))
                                                .font_weight(FontWeight::BOLD)
                                                .child(if redaction_count == 0 {
                                                    "0 PII leaks in body".to_string()
                                                } else {
                                                    format!(
                                                        "{redaction_count} {} scrubbed",
                                                        if redaction_count == 1 {
                                                            "token"
                                                        } else {
                                                            "tokens"
                                                        }
                                                    )
                                                }),
                                        ),
                                )
                                .child(render_button(
                                    "inspect-payload-side-btn",
                                    "Inspect outbound payload",
                                    Some(IconName::Lock),
                                    ButtonVariant::Secondary,
                                    false,
                                    theme,
                                    cx.listener(|this, _, _, cx| {
                                        this.open_payload_dialog();
                                        cx.notify();
                                    }),
                                )),
                        ),
                ),
        )
}

fn render_field_row(label: &str, value: &str, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .min_w_0()
        .py(px(6.0))
        .border_b_1()
        .border_color(theme.border_soft)
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.secondary)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_right()
                .text_size(px(13.0))
                .text_color(theme.text)
                .child(value.to_string()),
        )
}

fn render_privacy_no_document_state(
    workspace: &WorkspaceView,
    pad_x: Pixels,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
) -> Div {
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
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(26.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.text)
                        .child("Personal information"),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .text_color(theme.secondary)
                        .child("Your identity stays separate from the career content AI can see."),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .p(px(24.0))
                .rounded(px(6.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(render_icon(IconName::Shield, 18.0, theme.accent))
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child("Select a CV to edit its contact header"),
                        ),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .line_height(px(19.0))
                        .text_color(theme.secondary)
                        .child("Contact details, social links, and custom header fields are stored per CV and stay strictly local. Choose a CV below or open one from CV library."),
                )
                .when(!workspace.documents.is_empty(), |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .children(workspace.documents.iter().map(|doc| {
                                let doc_id = doc.id;
                                let is_variation = doc.parent_id.is_some();
                                div()
                                    .id(el_id(format!("privacy-pick-{}", doc.id)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.0))
                                    .px(px(12.0))
                                    .py(px(9.0))
                                    .rounded(px(5.0))
                                    .bg(theme.input)
                                    .border_1()
                                    .border_color(theme.border_soft)
                                    .hover(|s| s.bg(theme.hover))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_document(doc_id, cx);
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(render_icon(
                                                if is_variation {
                                                    IconName::Branch
                                                } else {
                                                    IconName::Shield
                                                },
                                                14.0,
                                                theme.accent,
                                            ))
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
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(px(11.0))
                                            .text_color(theme.muted)
                                            .child(if is_variation { "Variation" } else { "Master" }),
                                    )
                            })),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .pt(px(4.0))
                        .child(render_button(
                            "privacy-go-library-btn",
                            "Go to CV library",
                            Some(IconName::Folder),
                            ButtonVariant::Primary,
                            false,
                            theme,
                            cx.listener(|this, _, _, cx| {
                                this.navigate_to(WorkspaceViewPage::Library, cx);
                                cx.notify();
                            }),
                        )),
                ),
        )
}