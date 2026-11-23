use crate::ui::theme::{self, Theme};
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
            .rounded(px(4.0))
            .bg(t.surface)
            .border_1()
            .border_color(t.border)
            .shadow_md()
            .text_size(px(11.5))
            .text_color(t.text)
            .child(self.text.clone())
    }
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
    pub fn asset_path(&self) -> &'static str {
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
        .rounded(px(5.0))
        .text_size(px(13.0))
        .line_height(px(18.0))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .border_1();

    let text_color = match variant {
        ButtonVariant::Primary => rgb(0xffffff),
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
                    .hover(|s| s.bg(theme.accent_hover).border_color(theme.accent_hover));
            }
            ButtonVariant::Secondary => {
                el = el
                    .bg(theme.input)
                    .border_color(theme.border)
                    .text_color(text_color)
                    .hover(|s| s.bg(theme.hover).border_color(theme.secondary));
            }
            ButtonVariant::Ghost => {
                el = el
                    .bg(gpui::transparent_black())
                    .border_color(gpui::transparent_black())
                    .text_color(text_color)
                    .hover(|s| s.bg(theme.hover).text_color(theme.text));
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
        .rounded(px(5.0))
        .border_1()
        .when(!tooltip_text.is_empty(), move |btn| {
            btn.tooltip(move |_window, cx| {
                let text = tooltip_text.clone();
                cx.new(|_| SimpleTooltip { text }).into()
            })
        });

    if disabled {
        el.cursor_not_allowed()
            .opacity(0.35)
            .border_color(theme.transparent)
            .child(render_icon(icon, 15.0, theme.muted))
    } else {
        el.cursor_pointer()
            .border_color(theme.transparent)
            .hover(|s| s.bg(theme.hover).border_color(theme.border))
            .on_click(on_click)
            .child(render_icon(icon, 15.0, theme.secondary))
    }
}

pub fn render_toggle(
    id: impl Into<SharedString>,
    checked: bool,
    theme: &Theme,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element_id = ElementId::Name(id.into());
    let bg_color = if checked { theme.accent } else { theme.input };
    let border_color = if checked { theme.accent } else { theme.border };

    div()
        .id(element_id)
        .cursor_pointer()
        .flex()
        .flex_shrink_0()
        .items_center()
        .w(px(32.0))
        .h(px(18.0))
        .rounded(px(9.0))
        .bg(bg_color)
        .border_1()
        .border_color(border_color)
        .px(px(2.0))
        .on_click(on_toggle)
        .child(
            div()
                .size(px(12.0))
                .rounded(px(6.0))
                .bg(if checked {
                    rgb(0xffffff)
                } else {
                    theme.secondary
                })
                .when(checked, |s| s.ml(px(14.0)))
                .when(!checked, |s| s.ml(px(0.0))),
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
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .text_center()
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(theme.secondary)
                .max_w(px(420.0))
                .text_center()
                .child(description.to_string()),
        )
}

pub fn render_diff_text(before: &str, after: &str, variant: &str, theme: &Theme) -> Div {
    let original: Vec<&str> = before.split_inclusive(char::is_whitespace).collect();
    let proposed: Vec<&str> = after.split_inclusive(char::is_whitespace).collect();

    let mut prefix = 0;
    while prefix < original.len() && prefix < proposed.len() && original[prefix] == proposed[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < (original.len() - prefix)
        && suffix < (proposed.len() - prefix)
        && original[original.len() - suffix - 1] == proposed[proposed.len() - suffix - 1]
    {
        suffix += 1;
    }

    let tokens = if variant == "before" {
        &original
    } else {
        &proposed
    };
    let end = if tokens.len() >= suffix {
        tokens.len() - suffix
    } else {
        prefix
    };

    let prefix_len: usize = tokens[..prefix.min(tokens.len())]
        .iter()
        .map(|s| s.len())
        .sum();
    let middle_len: usize = if prefix <= end && end <= tokens.len() {
        tokens[prefix..end].iter().map(|s| s.len()).sum()
    } else {
        0
    };
    let full_text = if variant == "before" {
        before.to_string()
    } else {
        after.to_string()
    };

    let mut highlights = Vec::new();
    let mid_end = (prefix_len + middle_len).min(full_text.len());
    if prefix_len < mid_end
        && full_text.is_char_boundary(prefix_len)
        && full_text.is_char_boundary(mid_end)
    {
        let (bg, fg) = if variant == "before" {
            (theme.del_bg, theme.del_text)
        } else {
            (theme.add_bg, theme.add_text)
        };
        highlights.push((
            prefix_len..mid_end,
            HighlightStyle {
                color: Some(fg.into()),
                background_color: Some(bg.into()),
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
        ));
    }

    let styled = if highlights.is_empty() {
        StyledText::new(full_text)
    } else {
        StyledText::new(full_text).with_highlights(highlights)
    };

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .text_size(px(12.5))
        .line_height(px(19.0))
        .text_color(theme.text)
        .child(styled)
}
