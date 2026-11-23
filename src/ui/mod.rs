//! GPUI desktop application and Tuta Mail theme.
//!
//! The integration entry point expected by `src/main.rs` is:
//!
//! ```ignore
//! pub fn run(services: crate::contracts::Services) -> anyhow::Result<()>
//! ```

#![allow(
    clippy::too_many_arguments,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::unnecessary_sort_by,
    clippy::unneeded_struct_pattern,
    clippy::needless_borrows_for_generic_args
)]

pub mod assets;
pub mod components;
pub mod text_input;
pub mod theme;
pub mod views;
pub mod workspace;

use gpui::{
    App, Application, Bounds, KeyBinding, SharedString, Size, TitlebarOptions, Window,
    WindowBounds, WindowOptions, prelude::*, px, size,
};

use crate::contracts::Services;
use assets::VanguardAssets;
use workspace::{
    AcceptAllProposals, CloseModal, ExportPdfShortcut, NewVariation, ToggleAiPanel, WorkspaceView,
};

const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
const INTER_BOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");
const SOURCE_SERIF_REGULAR: &[u8] = include_bytes!("../../assets/fonts/SourceSerif4-Regular.otf");
const SOURCE_SERIF_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/SourceSerif4-Semibold.otf");
const SOURCE_SERIF_BOLD: &[u8] = include_bytes!("../../assets/fonts/SourceSerif4-Bold.otf");

/// Boots the GPUI application, opens the main VanguardCV window, and runs
/// the event loop until the window is closed.
pub fn run(services: Services) -> anyhow::Result<()> {
    Application::new()
        .with_assets(VanguardAssets)
        .run(move |cx: &mut App| {
            use std::borrow::Cow;
            cx.text_system()
                .add_fonts(vec![
                    Cow::Borrowed(INTER_REGULAR),
                    Cow::Borrowed(INTER_MEDIUM),
                    Cow::Borrowed(INTER_SEMIBOLD),
                    Cow::Borrowed(INTER_BOLD),
                    Cow::Borrowed(SOURCE_SERIF_REGULAR),
                    Cow::Borrowed(SOURCE_SERIF_SEMIBOLD),
                    Cow::Borrowed(SOURCE_SERIF_BOLD),
                ])
                .expect("bundled UI fonts must load");

            text_input::bind_default_keys(cx);
            cx.bind_keys([
                KeyBinding::new("ctrl-n", NewVariation, None),
                KeyBinding::new("cmd-n", NewVariation, None),
                KeyBinding::new("ctrl-e", ExportPdfShortcut, None),
                KeyBinding::new("cmd-e", ExportPdfShortcut, None),
                KeyBinding::new("ctrl-p", ToggleAiPanel, None),
                KeyBinding::new("cmd-p", ToggleAiPanel, None),
                KeyBinding::new("ctrl-enter", AcceptAllProposals, None),
                KeyBinding::new("cmd-enter", AcceptAllProposals, None),
                KeyBinding::new("escape", CloseModal, None),
            ]);

            // Preferred window size: 1280×820; freely resizable down to 560×440.
            let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);

            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(Size {
                        width: px(560.0),
                        height: px(440.0),
                    }),
                    titlebar: Some(TitlebarOptions {
                        title: Some(SharedString::from("VanguardCV")),
                        ..Default::default()
                    }),
                    focus: true,
                    show: true,
                    is_movable: true,
                    is_resizable: true,
                    ..Default::default()
                },
                {
                    let svc = services.clone();
                    move |window: &mut Window, cx: &mut App| {
                        let view = cx.new(|cx| WorkspaceView::new(svc, cx));
                        let handle = view.read(cx).focus_handle.clone();
                        window.focus(&handle);
                        view
                    }
                },
            );

            match result {
                Ok(_handle) => {
                    tracing::info!("VanguardCV window opened");
                }
                Err(err) => {
                    tracing::error!("Failed to open VanguardCV window: {err}");
                }
            }

            cx.activate(true);
        });

    Ok(())
}
