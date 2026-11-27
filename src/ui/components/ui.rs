use crate::ui::text_input::TextInput;
use crate::ui::theme::{self, RADIUS_MD, RADIUS_SM, TEXT_BASE, TEXT_LG, TEXT_SM, Theme};
use gpui::prelude::FluentBuilder;
use gpui::*;

pub fn el_id(name: impl Into<SharedString>) -> ElementId {
    ElementId::Name(name.into())
}

struct SimpleTooltip {
    text: SharedString,
}

impl Render for SimpleTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme::active_theme();
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(RADIUS_SM))
            .bg(t.surface)
            .border_1()
            .border_color(t.border_soft)
            .shadow_md()
            .text_size(px(TEXT_SM))
            .text_color(t.text)
            .child(self.text.clone())
    }
}

pub fn tooltip_view(text: impl Into<SharedString>, cx: &mut App) -> AnyView {
    let text = text.into();
    cx.new(|_| SimpleTooltip { text }).into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Folder,
    File,
    Edit,
    Sparkles,
    Shield,
    Settings,
    Chevron,
    Arrow,
    Branch,
    Plus,
    Search,
    Lock,
    Check,
    Close,
    Download,
    Sun,
    Moon,
    Code,
    Up,
    Down,
    Trash,
    Grip,
    Minus,
    Copy,
    Briefcase,
    Graduation,
    List,
}

impl IconName {
    fn asset_path(&self) -> &'static str {
        match self {
            IconName::Folder => "icons/folder.svg",
            IconName::File => "icons/file.svg",
            IconName::Edit => "icons/edit.svg",
            IconName::Sparkles => "icons/sparkles.svg",
            IconName::Shield => "icons/shield.svg",
            IconName::Settings => "icons/settings.svg",
            IconName::Chevron => "icons/chevron.svg",
            IconName::Arrow => "icons/arrow.svg",
            IconName::Branch => "icons/branch.svg",
            IconName::Plus => "icons/plus.svg",
            IconName::Search => "icons/search.svg",
            IconName::Lock => "icons/lock.svg",
            IconName::Check => "icons/check.svg",
            IconName::Close => "icons/close.svg",
            IconName::Download => "icons/download.svg",
            IconName::Sun => "icons/sun.svg",
            IconName::Moon => "icons/moon.svg",
            IconName::Code => "icons/code.svg",
            IconName::Up => "icons/up.svg",
            IconName::Down => "icons/down.svg",
            IconName::Trash => "icons/trash.svg",
            IconName::Grip => "icons/grip.svg",
            IconName::Minus => "icons/minus.svg",
            IconName::Copy => "icons/copy.svg",
            IconName::Briefcase => "icons/briefcase.svg",
            IconName::Graduation => "icons/graduation.svg",
            IconName::List => "icons/list.svg",
        }
    }
}

pub fn render_icon(name: IconName, size_px: f32, color: Rgba) -> impl IntoElement {
    svg()
        .path(name.asset_path())
        .size(px(size_px))
        .text_color(color)
        .flex_shrink_0()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
}

pub fn render_button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    icon: Option<IconName>,
    variant: ButtonVariant,
    disabled: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id = ElementId::Name(id.into());
    let mut el = div()
        .id(element_id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .h(px(34.0))
        .px(px(13.0))
        .rounded(px(RADIUS_MD))
        .text_size(px(TEXT_BASE))
        .line_height(px(18.0))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .border_1();

    let text_color = match variant {
        ButtonVariant::Primary => theme.on_accent,
        ButtonVariant::Secondary => theme.text,
        ButtonVariant::Ghost => theme.secondary,
    };

    if disabled {
        el = el
            .cursor_not_allowed()
            .opacity(0.45)
            .border_color(theme.border_soft)
            .bg(match variant {
                ButtonVariant::Primary => theme.accent,
                _ => theme.input,
            })
            .text_color(text_color);
    } else {
        el = el.cursor_pointer();
        match variant {
            ButtonVariant::Primary => {
                el = el
                    .bg(theme.accent)
                    .border_color(theme.accent)
                    .text_color(text_color)
                    .hover(|s| s.bg(theme.accent_hover).border_color(theme.accent_hover))
                    .active(|s| s.bg(theme.accent).opacity(0.88));
            }
            ButtonVariant::Secondary => {
                el = el
                    .bg(theme.input)
                    .border_color(theme.border_control)
                    .text_color(text_color)
                    .hover(|s| s.bg(theme.hover).border_color(theme.secondary))
                    .active(|s| s.bg(theme.surface_soft).opacity(0.9));
            }
            ButtonVariant::Ghost => {
                el = el
                    .bg(gpui::transparent_black())
                    .border_color(gpui::transparent_black())
                    .text_color(text_color)
                    .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    .active(|s| s.bg(theme.surface_soft).opacity(0.85));
            }
        }
        el = el.on_click(on_click);
    }

    if let Some(ic) = icon {
        el = el.child(render_icon(ic, 15.0, text_color));
    }
    el.child(label.into())
}

pub fn render_icon_button(
    id: impl Into<SharedString>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    disabled: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id = ElementId::Name(id.into());
    let tooltip_text: SharedString = tooltip.into();
    let el = div()
        .id(element_id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(30.0))
        .rounded(px(RADIUS_MD))
        .border_1()
        .when(!tooltip_text.is_empty(), move |btn| {
            btn.tooltip(move |_window, cx| tooltip_view(tooltip_text.clone(), cx))
        });

    if disabled {
        el.cursor_not_allowed()
            .opacity(0.35)
            .border_color(theme.transparent)
            .child(render_icon(icon, 15.0, theme.muted))
    } else {
        el.cursor_pointer()
            .border_color(theme.transparent)
            .hover(|s| s.bg(theme.hover).border_color(theme.border_control))
            .active(|s| s.bg(theme.surface_soft).opacity(0.85))
            .on_click(on_click)
            .child(render_icon(icon, 15.0, theme.secondary))
    }
}

/// Renders a toggle switch with a >= 24px vertical click target and hover/press cues.
pub fn render_toggle(
    id: impl Into<SharedString>,
    checked: bool,
    theme: &Theme,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id = ElementId::Name(id.into());
    let bg_color = if checked { theme.accent } else { theme.input };
    let border_color = if checked {
        theme.accent_hover
    } else {
        theme.border_control
    };
    let hover_bg = theme.hover;

    div()
        .id(element_id)
        .cursor_pointer()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .min_w(px(36.0))
        .h(px(24.0))
        .px(px(2.0))
        .rounded(px(RADIUS_MD))
        .hover(move |s| s.bg(hover_bg))
        .active(|s| s.opacity(0.86))
        .on_click(on_toggle)
        .child(
            div()
                .flex()
                .items_center()
                .w(px(32.0))
                .h(px(18.0))
                .rounded_full()
                .bg(bg_color)
                .border_1()
                .border_color(border_color)
                .px(px(2.0))
                .child(
                    div()
                        .size(px(12.0))
                        .rounded_full()
                        .bg(if checked {
                            theme.on_accent
                        } else {
                            theme.secondary
                        })
                        .when(checked, |s| s.ml(px(14.0)))
                        .when(!checked, |s| s.ml(px(0.0))),
                ),
        )
}

/// Renders a compact visual checkbox wrapped in a 24x24px clickable hit area
/// with WCAG-compliant control border contrast, crisp edge definition, and
/// tactile hover/press feedback.
pub fn render_checkbox(
    id: impl Into<SharedString>,
    checked: bool,
    box_size: f32,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id = ElementId::Name(id.into());
    let hover_bg = theme.hover;
    let check_icon_size = (box_size * 0.68).round().clamp(10.0, 12.0);

    div()
        .id(element_id)
        .flex_shrink_0()
        .cursor_pointer()
        .size(px(24.0))
        .rounded(px(RADIUS_MD))
        .flex()
        .items_center()
        .justify_center()
        .hover(move |s| s.bg(hover_bg))
        .active(|s| s.opacity(0.82))
        .on_click(on_click)
        .child(
            div()
                .size(px(box_size))
                .rounded(px(RADIUS_SM))
                .border_1()
                .border_color(if checked {
                    theme.accent_hover
                } else {
                    theme.border_control
                })
                .bg(if checked { theme.accent } else { theme.input })
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |el| {
                    el.child(render_icon(
                        IconName::Check,
                        check_icon_size,
                        theme.on_accent,
                    ))
                }),
        )
}

/// Non-shifting 2px outer translucent focus halo paired with `theme.focus_ring`.
fn focus_ring_shadow(theme: &Theme) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: theme.focus_halo.into(),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(2.0),
    }]
}

/// Renders a single-line text input container with optional leading icon,
/// optional trailing action (e.g. a clear button), unified corner radius,
/// WCAG-compliant control border contrast, hover cue, and clean outer focus ring.
///
/// The returned `Div` does not force `w_full()` so callers inside flex rows can
/// chain `.flex_1()`, `.w(px(...))`, or `.w_full()` without flex-basis conflicts.
pub fn render_decorated_input_box(
    input: Entity<TextInput>,
    height_px: f32,
    leading_icon: Option<IconName>,
    trailing: Option<AnyElement>,
    input_width_px: Option<f32>,
    theme: &Theme,
    cx: &App,
) -> Div {
    let focused = input.read(cx).is_focused_cached();
    let hover_border = theme.muted;

    let input_slot = match input_width_px {
        Some(w) => div().w(px(w)).min_w_0().child(input),
        None => div().flex_1().min_w_0().child(input),
    };

    div()
        .h(px(height_px))
        .px(px(10.0))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(RADIUS_MD))
        .bg(theme.input)
        .border_1()
        .border_color(if focused {
            theme.focus_ring
        } else {
            theme.border_control
        })
        .when(focused, |el| el.shadow(focus_ring_shadow(theme)))
        .when(!focused, |el| {
            el.hover(move |s| s.border_color(hover_border))
        })
        .overflow_hidden()
        .when_some(leading_icon, |el, ic| {
            el.child(render_icon(ic, 14.0, theme.muted))
        })
        .child(input_slot)
        .when_some(trailing, |el, trailing_el| el.child(trailing_el))
}

/// Renders a full-width single-line text input container with WCAG-compliant
/// control border contrast, unified corner radius, subtle hover cue, and clean
/// outer focus ring.
pub fn render_input_box(input: Entity<TextInput>, height_px: f32, theme: &Theme, cx: &App) -> Div {
    render_decorated_input_box(input, height_px, None, None, None, theme, cx).w_full()
}

/// Renders a multi-line text input container with WCAG-compliant control border
/// contrast, unified corner radius, subtle hover cue, and clean outer focus ring.
pub fn render_multiline_input_box(
    input: Entity<TextInput>,
    height_px: f32,
    theme: &Theme,
    cx: &App,
) -> Div {
    let focused = input.read(cx).is_focused_cached();
    let hover_border = theme.muted;
    div()
        .h(px(height_px))
        .px(px(10.0))
        .py(px(6.0))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .rounded(px(RADIUS_MD))
        .bg(theme.input)
        .border_1()
        .border_color(if focused {
            theme.focus_ring
        } else {
            theme.border_control
        })
        .when(focused, |el| el.shadow(focus_ring_shadow(theme)))
        .when(!focused, |el| {
            el.hover(move |s| s.border_color(hover_border))
        })
        .overflow_hidden()
        .child(input)
}

/// Renders a selectable toggle/choice pill with consistent hover and active states.
pub fn render_selectable_pill(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    icon: Option<IconName>,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id = ElementId::Name(id.into());
    let hover_border = theme.accent;

    let mut el = div()
        .id(element_id)
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(9.0))
        .py(px(5.0))
        .rounded(px(RADIUS_SM))
        .bg(if selected {
            theme.accent_soft
        } else {
            theme.input
        })
        .border_1()
        .border_color(if selected {
            theme.accent
        } else {
            theme.border_control
        })
        .hover(move |s| s.border_color(hover_border))
        .active(|s| s.opacity(0.86))
        .text_size(px(TEXT_SM))
        .font_weight(if selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(if selected {
            theme.accent_text
        } else {
            theme.secondary
        })
        .on_click(on_click);

    if let Some(ic) = icon {
        el = el.child(render_icon(
            ic,
            11.0,
            if selected { theme.accent } else { theme.muted },
        ));
    }
    el.child(label.into())
}

/// Renders the VanguardCV brand emblem inside an elevated, high-contrast framed
/// tile so the logo reads bigger and bolder in both dark and light themes
/// without modifying the underlying artwork.
pub fn render_brand_logo_badge(tile_size: f32, logo_size: f32, radius: f32, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(tile_size))
        .rounded(px(radius))
        .bg(theme.brand_badge_bg)
        .border_1()
        .border_color(theme.brand_badge_border)
        .child(
            img("branding/logo.png")
                .size(px(logo_size))
                .object_fit(ObjectFit::Contain),
        )
}

pub fn render_empty_state(icon: IconName, title: &str, description: &str, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .py(px(48.0))
        .px(px(24.0))
        .gap(px(10.0))
        .child(render_icon(icon, 30.0, theme.muted))
        .child(
            div()
                .text_size(px(TEXT_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .text_center()
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(TEXT_BASE))
                .text_color(theme.secondary)
                .max_w(px(420.0))
                .text_center()
                .child(description.to_string()),
        )
}

/// Computes the byte range `(start, end)` of the changed middle span inside
/// `before` (when `is_before == true`) or `after` without allocating intermediate
/// token `Vec`s.
fn diff_highlight_range(before: &str, after: &str, is_before: bool) -> Option<(usize, usize)> {
    let mut prefix = 0usize;
    for (a, b) in before
        .split_inclusive(char::is_whitespace)
        .zip(after.split_inclusive(char::is_whitespace))
    {
        if a == b {
            prefix += a.len();
        } else {
            break;
        }
    }

    let rem_before = &before[prefix..];
    let rem_after = &after[prefix..];

    // Match identical trailing bytes from the end, then align to the start of a
    // whole whitespace-delimited token so highlights stay word-aligned.
    let raw_suffix = rem_before
        .as_bytes()
        .iter()
        .rev()
        .zip(rem_after.as_bytes().iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let mut suffix = 0usize;
    if raw_suffix > 0 {
        let mut cut = rem_before.len() - raw_suffix;
        while cut < rem_before.len() && !rem_before.is_char_boundary(cut) {
            cut += 1;
        }
        // Advance past any partial word unless the match starts at a word boundary.
        let starts_at_boundary = cut == 0
            || rem_before[..cut]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if !starts_at_boundary {
            if let Some(ws_pos) = rem_before[cut..].find(char::is_whitespace) {
                let ws_char_len = rem_before[cut + ws_pos..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
                cut += ws_pos + ws_char_len;
            } else {
                cut = rem_before.len();
            }
        }
        let candidate = rem_before.len().saturating_sub(cut);
        if candidate <= rem_after.len()
            && rem_after.is_char_boundary(rem_after.len() - candidate)
            && rem_before[rem_before.len() - candidate..]
                == rem_after[rem_after.len() - candidate..]
        {
            suffix = candidate;
        }
    }

    let target = if is_before { before } else { after };
    let end = target.len().saturating_sub(suffix);
    if prefix < end && target.is_char_boundary(prefix) && target.is_char_boundary(end) {
        Some((prefix, end))
    } else {
        None
    }
}

pub fn render_diff_text(before: &str, after: &str, variant: &str, theme: &Theme) -> Div {
    let is_before = variant == "before";
    let full_text = if is_before {
        before.to_string()
    } else {
        after.to_string()
    };

    let styled = if let Some((start, end)) = diff_highlight_range(before, after, is_before) {
        let (bg, fg) = if is_before {
            (theme.del_bg, theme.del_text)
        } else {
            (theme.add_bg, theme.add_text)
        };
        StyledText::new(full_text).with_highlights([(
            start..end,
            HighlightStyle {
                color: Some(fg.into()),
                background_color: Some(bg.into()),
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
        )])
    } else {
        StyledText::new(full_text)
    };

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .text_size(px(TEXT_SM))
        .line_height(px(19.0))
        .text_color(theme.text)
        .child(styled)
}

#[cfg(test)]
mod tests {
    use super::diff_highlight_range;

    #[test]
    fn diff_highlight_aligns_to_changed_words() {
        let before = "Led backend platform team";
        let after = "Led distributed systems team";
        let b_range = diff_highlight_range(before, after, true).unwrap();
        let a_range = diff_highlight_range(before, after, false).unwrap();
        assert_eq!(&before[b_range.0..b_range.1], "backend platform ");
        assert_eq!(&after[a_range.0..a_range.1], "distributed systems ");
    }

    #[test]
    fn diff_highlight_identical_returns_none() {
        assert_eq!(diff_highlight_range("Same text", "Same text", true), None);
        assert_eq!(diff_highlight_range("Same text", "Same text", false), None);
    }
}
