//! Tuta Mail carbon/crimson theme tokens and reusable styles.
//!
//! **Owner:** Sub-Agent 3 (GPUI Desktop & Tuta Theme Engineer).
//!
//! Defines dark and light color palettes adapted from Tuta Mail's Android
//! visual identity, plus a typography scale and helper functions that
//! return styled GPUI `Div` builders for cards, buttons, inputs, and badges.
//!
//! This module is dependency-free beyond `gpui`.

use gpui::{div, px, rgb, Div, Pixels, SharedString, Styled};
use gpui::prelude::*;

// ---------------------------------------------------------------------------
// Color tokens
// ---------------------------------------------------------------------------

/// All color tokens for a single theme variant.
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    /// Ultra-clean zero-noise backdrop.
    pub bg_canvas: u32,
    /// Elevated card / sidebar surface.
    pub bg_surface: u32,
    /// Recessed interactive / input field.
    pub bg_input: u32,
    /// Primary accent — buttons, active indicators.
    pub tuta_crimson: u32,
    /// Hover / focus accent.
    pub tuta_crimson_bright: u32,
    /// PII badge background.
    pub pii_badge_bg: u32,
    /// PII badge text.
    pub pii_badge_text: u32,
    /// Diff-added background.
    pub diff_add_bg: u32,
    /// Diff-added text.
    pub diff_add_text: u32,
    /// Diff-removed background.
    pub diff_del_bg: u32,
    /// Diff-removed text.
    pub diff_del_text: u32,
    /// Structural hairline border.
    pub border_subtle: u32,
    /// Primary foreground text.
    pub text_primary: u32,
    /// Secondary / muted text.
    pub text_secondary: u32,
    /// Disabled / placeholder text.
    pub text_disabled: u32,
    /// Text on crimson buttons.
    pub text_on_accent: u32,
    /// Dark primary navigation background.
    pub nav_bg: u32,
    /// Navigation foreground.
    pub nav_text: u32,
    /// Muted navigation labels.
    pub nav_muted: u32,
    /// Active navigation item background.
    pub nav_active: u32,
    /// Soft accent surface (chips, selected rows).
    pub accent_soft: u32,
}

/// Default editorial light theme, ported from the Figma Make design.
pub const DARK: ThemeColors = ThemeColors {
    bg_canvas: 0xF3F2EF,
    bg_surface: 0xFFFFFF,
    bg_input: 0xF8F7F5,
    tuta_crimson: 0x840010,
    tuta_crimson_bright: 0xA30014,
    pii_badge_bg: 0xF2E6E8,
    pii_badge_text: 0x71101E,
    diff_add_bg: 0xE8EEE9,
    diff_add_text: 0x477054,
    diff_del_bg: 0xF8E9EB,
    diff_del_text: 0xA3212E,
    border_subtle: 0xDFDCD6,
    text_primary: 0x242321,
    text_secondary: 0x55524C,
    text_disabled: 0x96928B,
    text_on_accent: 0xFFFFFF,
    nav_bg: 0x171817,
    nav_text: 0xF0F0EE,
    nav_muted: 0xA6A7A4,
    nav_active: 0x2A2B2A,
    accent_soft: 0xF2E6E8,
};

/// Light theme variant, aligned with the same editorial palette.
pub const LIGHT: ThemeColors = ThemeColors {
    bg_canvas: 0xF3F2EF,
    bg_surface: 0xFFFFFF,
    bg_input: 0xF8F7F5,
    tuta_crimson: 0x840010,
    tuta_crimson_bright: 0xA30014,
    pii_badge_bg: 0xF2E6E8,
    pii_badge_text: 0x71101E,
    diff_add_bg: 0xE8EEE9,
    diff_add_text: 0x477054,
    diff_del_bg: 0xF8E9EB,
    diff_del_text: 0xA3212E,
    border_subtle: 0xDFDCD6,
    text_primary: 0x242321,
    text_secondary: 0x55524C,
    text_disabled: 0x96928B,
    text_on_accent: 0xFFFFFF,
    nav_bg: 0x171817,
    nav_text: 0xF0F0EE,
    nav_muted: 0xA6A7A4,
    nav_active: 0x2A2B2A,
    accent_soft: 0xF2E6E8,
};

/// UI font family bundled with the app.
pub const UI_FONT: &str = "Inter";
/// Serif family used for editor previews.
pub const SERIF_FONT: &str = "Source Serif 4";

// ---------------------------------------------------------------------------
// Active theme accessor
// ---------------------------------------------------------------------------

/// Returns the active color palette. The ported Figma design uses the light
/// editorial palette by default.
pub fn colors() -> &'static ThemeColors {
    &DARK
}

// ---------------------------------------------------------------------------
// Typography scale
// ---------------------------------------------------------------------------

/// Standardized type sizes used throughout the UI.
#[derive(Debug, Clone, Copy)]
pub struct TypographyScale {
    /// Window / page title.
    pub title: Pixels,
    /// Section headings.
    pub heading: Pixels,
    /// Subheadings / labels.
    pub subheading: Pixels,
    /// Body text.
    pub body: Pixels,
    /// Small captions / metadata.
    pub caption: Pixels,
    /// Tiny badges / counters.
    pub badge: Pixels,
}

/// The standard VanguardCV typography scale.
pub const TYPOGRAPHY: TypographyScale = TypographyScale {
    title: px(20.0),
    heading: px(16.0),
    subheading: px(14.0),
    body: px(13.0),
    caption: px(11.0),
    badge: px(10.0),
};

/// Returns the active typography scale.
pub fn typography() -> &'static TypographyScale {
    &TYPOGRAPHY
}

// ---------------------------------------------------------------------------
// Reusable style helpers
// ---------------------------------------------------------------------------

/// Elevated card panel (sidebar panel, section card, etc.).
pub fn card() -> Div {
    let c = colors();
    div()
        .bg(rgb(c.bg_surface))
        .rounded(px(8.0))
        .border_1()
        .border_color(rgb(c.border_subtle))
        .p(px(12.0))
}

/// Primary crimson button.
pub fn button_primary(label: impl Into<SharedString>) -> Div {
    let c = colors();
    div()
        .flex()
        .items_center()
        .justify_center()
        .px(px(16.0))
        .py(px(6.0))
        .bg(rgb(c.tuta_crimson))
        .rounded(px(6.0))
        .text_color(rgb(c.text_on_accent))
        .text_size(typography().body)
        .cursor_pointer()
        .hover(|s| s.bg(rgb(c.tuta_crimson_bright)))
        .child(label.into())
}

/// Ghost / outline button.
pub fn button_ghost(label: impl Into<SharedString>) -> Div {
    let c = colors();
    div()
        .flex()
        .items_center()
        .justify_center()
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(c.border_subtle))
        .text_color(rgb(c.text_secondary))
        .text_size(typography().body)
        .cursor_pointer()
        .hover(|s| s.bg(rgb(c.bg_input)).text_color(rgb(c.text_primary)))
        .child(label.into())
}

/// Input field container.
pub fn input_field() -> Div {
    let c = colors();
    div()
        .bg(rgb(c.bg_input))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(c.border_subtle))
        .px(px(10.0))
        .py(px(6.0))
        .text_color(rgb(c.text_primary))
        .text_size(typography().body)
}

/// Small PII shield badge.
pub fn pii_badge(label: impl Into<SharedString>) -> Div {
    let c = colors();
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(8.0))
        .py(px(2.0))
        .bg(rgb(c.pii_badge_bg))
        .rounded(px(4.0))
        .text_color(rgb(c.pii_badge_text))
        .text_size(typography().badge)
        .child(label.into())
}

/// Hairline horizontal separator.
pub fn hairline() -> Div {
    let c = colors();
    div()
        .w_full()
        .h(px(1.0))
        .bg(rgb(c.border_subtle))
}

/// Section header bar (used in the sidebar and editor).
pub fn section_header(title: impl Into<SharedString>) -> Div {
    let c = colors();
    div()
        .flex()
        .items_center()
        .px(px(12.0))
        .py(px(8.0))
        .text_size(typography().caption)
        .text_color(rgb(c.text_secondary))
        .child(title.into())
}

/// A small colored indicator dot (e.g. diff status).
pub fn indicator_dot(color_hex: u32) -> Div {
    div()
        .size(px(8.0))
        .rounded_full()
        .bg(rgb(color_hex))
        .flex_shrink_0()
}

/// Diff-added inline span style container.
pub fn diff_added() -> Div {
    let c = colors();
    div()
        .bg(rgb(c.diff_add_bg))
        .text_color(rgb(c.diff_add_text))
        .rounded(px(2.0))
        .px(px(2.0))
}

/// Diff-removed inline span style container.
pub fn diff_removed() -> Div {
    let c = colors();
    div()
        .bg(rgb(c.diff_del_bg))
        .text_color(rgb(c.diff_del_text))
        .rounded(px(2.0))
        .px(px(2.0))
}

/// Utility: creates a themed tooltip-like label.
pub fn tooltip_label(text: impl Into<SharedString>) -> Div {
    let c = colors();
    div()
        .px(px(8.0))
        .py(px(4.0))
        .bg(rgb(c.bg_surface))
        .border_1()
        .border_color(rgb(c.border_subtle))
        .rounded(px(4.0))
        .text_size(typography().caption)
        .text_color(rgb(c.text_primary))
        .shadow_sm()
        .child(text.into())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_canvas_is_correct() {
        assert_eq!(DARK.bg_canvas, 0xF3F2EF);
    }

    #[test]
    fn light_theme_canvas_is_correct() {
        assert_eq!(LIGHT.bg_canvas, 0xF3F2EF);
    }

    #[test]
    fn dark_crimson_values() {
        assert_eq!(DARK.tuta_crimson, 0x840010);
        assert_eq!(DARK.tuta_crimson_bright, 0xA30014);
    }

    #[test]
    fn pii_badge_colors() {
        assert_eq!(DARK.pii_badge_bg, 0xF2E6E8);
        assert_eq!(DARK.pii_badge_text, 0x71101E);
    }

    #[test]
    fn diff_colors() {
        assert_eq!(DARK.diff_add_bg, 0xE8EEE9);
        assert_eq!(DARK.diff_add_text, 0x477054);
        assert_eq!(DARK.diff_del_bg, 0xF8E9EB);
        assert_eq!(DARK.diff_del_text, 0xA3212E);
    }

    #[test]
    fn hairline_border_value() {
        assert_eq!(DARK.border_subtle, 0xDFDCD6);
        assert_eq!(LIGHT.border_subtle, 0xDFDCD6);
    }

    #[test]
    fn typography_scale_values() {
        let t = typography();
        assert_eq!(t.title, px(20.0));
        assert_eq!(t.body, px(13.0));
        assert_eq!(t.caption, px(11.0));
        assert_eq!(t.badge, px(10.0));
    }

    #[test]
    fn colors_returns_dark_by_default() {
        let c = colors();
        assert_eq!(c.bg_canvas, DARK.bg_canvas);
    }
}
