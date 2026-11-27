mod content;
mod document;
mod privacy;
mod skills;

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::ui::components::ui::{IconName, render_icon_button};
use crate::ui::theme::{RADIUS_LG, Theme};
use crate::ui::views::import_modal::render_import_cv_modal;
use crate::ui::workspace::{Dialog, WorkspaceView};

use self::content::{
    render_add_section_modal, render_edit_bullet_modal, render_edit_category_modal,
    render_edit_entry_modal, render_edit_summary_modal, render_rename_section_modal,
    render_rewrite_bullet_modal,
};
use self::document::{
    render_compare_modal, render_confirm_delete_cv_modal, render_confirm_reset_app_modal,
    render_document_menu_modal, render_export_modal, render_new_master_modal,
    render_new_variation_modal, render_promote_modal, render_rename_modal,
};
use self::privacy::{
    render_edit_contact_modal, render_edit_custom_field_modal, render_edit_social_modal,
    render_payload_modal,
};
use self::skills::render_edit_skill_modal;
pub fn render_modal_overlay(
    workspace: &WorkspaceView,
    cx: &mut Context<WorkspaceView>,
) -> Option<Stateful<Div>> {
    let dialog = workspace.active_dialog.as_ref()?;
    let theme = &workspace.theme;

    let content = match dialog {
        Dialog::NewMaster => render_new_master_modal(workspace, theme, cx),
        Dialog::ImportCv => render_import_cv_modal(workspace, theme, cx),
        Dialog::NewVariation { root_id } => {
            render_new_variation_modal(workspace, *root_id, theme, cx)
        }
        Dialog::Compare => render_compare_modal(workspace, theme, cx),
        Dialog::PayloadPreview => render_payload_modal(workspace, theme, cx),
        Dialog::ExportPdf => render_export_modal(workspace, theme, cx),
        Dialog::PromoteItem => render_promote_modal(workspace, theme, cx),
        Dialog::EditSummary { .. } => render_edit_summary_modal(workspace, theme, cx),
        Dialog::RenameSection { section_id } => {
            render_rename_section_modal(workspace, *section_id, theme, cx)
        }
        Dialog::EditEntry { is_new, .. } => render_edit_entry_modal(workspace, *is_new, theme, cx),
        Dialog::EditBullet { is_new, .. } => {
            render_edit_bullet_modal(workspace, *is_new, theme, cx)
        }
        Dialog::EditCategory { is_new, .. } => {
            render_edit_category_modal(workspace, *is_new, theme, cx)
        }
        Dialog::AddSection => render_add_section_modal(workspace, theme, cx),
        Dialog::RenameCv => render_rename_modal(workspace, theme, cx),
        Dialog::EditSkill { is_new, read_only } => {
            render_edit_skill_modal(workspace, *is_new, *read_only, theme, cx)
        }
        Dialog::EditContact => render_edit_contact_modal(workspace, theme, cx),
        Dialog::EditSocialLink { link_id } => {
            render_edit_social_modal(workspace, link_id.is_some(), theme, cx)
        }
        Dialog::EditCustomField { field_id } => {
            render_edit_custom_field_modal(workspace, field_id.is_some(), theme, cx)
        }
        Dialog::DocumentMenu => render_document_menu_modal(workspace, theme, cx),
        Dialog::ConfirmDeleteCv { doc_id } => {
            render_confirm_delete_cv_modal(workspace, *doc_id, theme, cx)
        }
        Dialog::ConfirmResetApp => render_confirm_reset_app_modal(workspace, theme, cx),
        Dialog::RewriteBullet {
            section_id,
            bullet_id,
        } => render_rewrite_bullet_modal(workspace, *section_id, *bullet_id, theme, cx),
    };

    Some(
        div()
            .id("modal-overlay-backdrop")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.backdrop)
            .on_scroll_wheel(|_, _, cx| {
                cx.stop_propagation();
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_dialog();
                    cx.notify();
                }),
            )
            .child(content),
    )
}

pub(super) fn modal_frame(
    workspace: &WorkspaceView,
    title: &str,
    description: Option<&str>,
    wide: bool,
    theme: &Theme,
    cx: &mut Context<WorkspaceView>,
    body: Div,
) -> Div {
    let target_w = if wide { 660.0 } else { 500.0 };
    let modal_w = (workspace.viewport_width - 32.0).clamp(320.0, target_w);
    let modal_max_h = (workspace.viewport_height - 48.0).clamp(300.0, 700.0);

    div()
        .w(px(modal_w))
        .max_h(px(modal_max_h))
        .flex()
        .flex_col()
        .rounded(px(RADIUS_LG))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border_soft)
        .shadow_lg()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        // Header
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(12.0))
                .p(px(18.0))
                .border_b_1()
                .border_color(theme.border_soft)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .text_size(px(16.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.text)
                                .child(title.to_string()),
                        )
                        .when_some(description, |el, desc| {
                            el.child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(theme.secondary)
                                    .child(desc.to_string()),
                            )
                        }),
                )
                .child(render_icon_button(
                    "close-modal-btn",
                    IconName::Close,
                    "Close dialog",
                    false,
                    theme,
                    cx.listener(|this, _, _, cx| {
                        this.close_dialog();
                        cx.notify();
                    }),
                )),
        )
        // Body
        .child(
            div()
                .id("modal-body-scroll")
                .flex_1()
                .w_full()
                .min_h_0()
                .min_w_0()
                .p(px(18.0))
                .overflow_y_scroll()
                .child(body),
        )
}

