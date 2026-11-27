use std::sync::atomic::{AtomicU8, Ordering};

use gpui::*;

// Shared typography scale (minimum 11px for desktop readability)
pub const TEXT_XS: f32 = 11.0;
pub const TEXT_SM: f32 = 12.0;
pub const TEXT_BASE: f32 = 13.0;
pub const TEXT_LG: f32 = 16.0;

// Shared corner-radius scale
pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 8.0;

// Cross-platform monospace family
pub const FONT_MONO: &str = "monospace";

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
    /// Higher-contrast border (>= 3:1 WCAG 1.4.11) for interactive controls
    /// such as inputs, secondary buttons, checkboxes, and toggles.
    pub border_control: Rgba,
    /// High-contrast focus indicator for active text inputs.
    pub focus_ring: Rgba,
    /// Soft translucent outer focus halo around active text inputs.
    pub focus_halo: Rgba,
    pub text: Rgba,
    pub secondary: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub accent_soft: Rgba,
    pub accent_text: Rgba,
    /// Foreground color used directly on solid `accent` fills.
    pub on_accent: Rgba,
    pub add_bg: Rgba,
    pub add_text: Rgba,
    pub del_bg: Rgba,
    pub del_text: Rgba,
    /// Code / JSON payload viewer surface & foreground (both themes >= 7:1).
    pub code_bg: Rgba,
    pub code_text: Rgba,
    /// Modal overlay backdrop scrim.
    pub backdrop: Rgba,
    /// Text selection highlight inside `TextInput`.
    pub selection_bg: Rgba,
    /// Brand emblem badge tokens.
    pub brand_badge_bg: Rgba,
    pub brand_badge_border: Rgba,
    pub brand_badge_accent: Rgba,
    /// A4 paper preview sheet & ink tokens (always dark-on-white print ink).
    pub paper_bg: Rgba,
    pub paper_text: Rgba,
    pub paper_body: Rgba,
    pub paper_secondary: Rgba,
    pub paper_muted: Rgba,
    pub paper_divider: Rgba,
    pub paper_link: Rgba,
    pub paper_rule: Rgba,
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
            border_control: rgb(0x6f6f76),
            focus_ring: rgb(0xe44b5a),
            focus_halo: rgba(0xe44b5a36),
            text: rgb(0xeeeeef),
            secondary: rgb(0xaaaab0),
            muted: rgb(0x8b8b93),
            accent: rgb(0x840010),
            accent_hover: rgb(0xb50018),
            accent_soft: rgb(0x2a1215),
            accent_text: rgb(0xe4a0aa),
            on_accent: rgb(0xffffff),
            add_bg: rgb(0x132e1e),
            add_text: rgb(0x7dcea1),
            del_bg: rgb(0x301418),
            del_text: rgb(0xf29aa3),
            code_bg: rgb(0x111315),
            code_text: rgb(0x8be0b0),
            backdrop: rgba(0x000000b0),
            selection_bg: rgba(0xb5001844),
            brand_badge_bg: rgb(0x231a1c),
            brand_badge_border: rgba(0xd9384858),
            brand_badge_accent: rgb(0xe44b5a),
            paper_bg: rgb(0xffffff),
            paper_text: rgb(0x1a1a1a),
            paper_body: rgb(0x333333),
            paper_secondary: rgb(0x444444),
            paper_muted: rgb(0x555555),
            paper_divider: rgb(0x888888),
            paper_link: rgb(0x1a4575),
            paper_rule: rgb(0x222222),
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
            border_control: rgb(0x82828a),
            focus_ring: rgb(0xa30014),
            focus_halo: rgba(0xa3001426),
            text: rgb(0x252528),
            secondary: rgb(0x63636a),
            muted: rgb(0x6b6b73),
            accent: rgb(0x840010),
            accent_hover: rgb(0xa30014),
            accent_soft: rgb(0xfdecee),
            accent_text: rgb(0x9c2539),
            on_accent: rgb(0xffffff),
            add_bg: rgb(0xe6f6eb),
            add_text: rgb(0x2b7247),
            del_bg: rgb(0xffefef),
            del_text: rgb(0xae3444),
            code_bg: rgb(0xf4f6f5),
            code_text: rgb(0x1d5c38),
            backdrop: rgba(0x0000007a),
            selection_bg: rgba(0xa3001433),
            brand_badge_bg: rgb(0xfdecee),
            brand_badge_border: rgba(0x84001038),
            brand_badge_accent: rgb(0x840010),
            paper_bg: rgb(0xffffff),
            paper_text: rgb(0x1a1a1a),
            paper_body: rgb(0x333333),
            paper_secondary: rgb(0x444444),
            paper_muted: rgb(0x555555),
            paper_divider: rgb(0x888888),
            paper_link: rgb(0x1a4575),
            paper_rule: rgb(0x222222),
            paper_border: rgb(0xd0d0d0),
            transparent: rgba(0x00000000),
        }
    }
}
