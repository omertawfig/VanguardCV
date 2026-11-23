use std::sync::atomic::{AtomicU8, Ordering};

use gpui::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

static ACTIVE_THEME_MODE: AtomicU8 = AtomicU8::new(0);

/// Sets the active UI theme mode globally so standalone entities like
/// [`crate::ui::text_input::TextInput`] automatically adapt to dark/light mode.
pub fn set_active_theme_mode(mode: ThemeMode) {
    ACTIVE_THEME_MODE.store(
        match mode {
            ThemeMode::Dark => 0,
            ThemeMode::Light => 1,
        },
        Ordering::Relaxed,
    );
}

/// Returns the currently active UI theme mode.
pub fn active_theme_mode() -> ThemeMode {
    match ACTIVE_THEME_MODE.load(Ordering::Relaxed) {
        1 => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

/// Returns the [`Theme`] palette matching the currently active UI theme mode.
pub fn active_theme() -> Theme {
    match active_theme_mode() {
        ThemeMode::Dark => Theme::dark(),
        ThemeMode::Light => Theme::light(),
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub mode: ThemeMode,
    pub canvas: Rgba,
    pub surface: Rgba,
    pub surface_soft: Rgba,
    pub input: Rgba,
    pub hover: Rgba,
    pub border: Rgba,
    pub border_soft: Rgba,
    pub text: Rgba,
    pub secondary: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub accent_soft: Rgba,
    pub accent_text: Rgba,
    pub add_bg: Rgba,
    pub add_text: Rgba,
    pub del_bg: Rgba,
    pub del_text: Rgba,
    pub paper_bg: Rgba,
    pub paper_text: Rgba,
    pub paper_border: Rgba,
    pub transparent: Rgba,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            canvas: rgb(0x101010),
            surface: rgb(0x1c1c1c),
            surface_soft: rgb(0x151515),
            input: rgb(0x252525),
            hover: rgb(0x242424),
            border: rgb(0x2e2e2e),
            border_soft: rgb(0x242424),
            text: rgb(0xeeeeef),
            secondary: rgb(0xaaaab0),
            muted: rgb(0x8b8b93),
            accent: rgb(0x840010),
            accent_hover: rgb(0xb50018),
            accent_soft: rgb(0x2a1215),
            accent_text: rgb(0xe4a0aa),
            add_bg: rgb(0x132e1e),
            add_text: rgb(0x7dcea1),
            del_bg: rgb(0x301418),
            del_text: rgb(0xf29aa3),
            paper_bg: rgb(0xffffff),
            paper_text: rgb(0x1a1a1a),
            paper_border: rgb(0x3a3a3a),
            transparent: rgba(0x00000000),
        }
    }

    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            canvas: rgb(0xf6f6f6),
            surface: rgb(0xffffff),
            surface_soft: rgb(0xf0f0f0),
            input: rgb(0xf0f0f0),
            hover: rgb(0xf5f5f5),
            border: rgb(0xe2e2e2),
            border_soft: rgb(0xe9e9e9),
            text: rgb(0x252528),
            secondary: rgb(0x63636a),
            muted: rgb(0x6b6b73),
            accent: rgb(0x840010),
            accent_hover: rgb(0xa30014),
            accent_soft: rgb(0xfdecee),
            accent_text: rgb(0x9c2539),
            add_bg: rgb(0xe6f6eb),
            add_text: rgb(0x2b7247),
            del_bg: rgb(0xffefef),
            del_text: rgb(0xae3444),
            paper_bg: rgb(0xffffff),
            paper_text: rgb(0x1a1a1a),
            paper_border: rgb(0xd0d0d0),
            transparent: rgba(0x00000000),
        }
    }
}
