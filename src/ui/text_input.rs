//! Reusable text input view for GPUI.
//!
//! Adapts GPUI's input example into a reusable [`TextInput`] entity with
//! focus, caret, selection, backspace/delete, arrow/home/end/page-up/page-down/select-all,
//! copy/cut/paste, mouse selection, placeholder, password masking, read-only mode,
//! multiline wrapping with vertical scrolling & scrollbar indicator, single-line horizontal
//! scrolling, `ContentMask` clipping, dynamic dark/light theming, and an on_change callback.
//!
//! The underlying [`TextBuffer`] encapsulates pure editing logic and can be
//! unit-tested without a window or display server.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window, actions, div, fill, point,
    prelude::*, px, relative, rgba, size,
};

use crate::ui::theme;

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        PageUp,
        PageDown,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Newline,
    ]
);

/// Registers default keybindings for text inputs on macOS and Linux/Windows.
pub fn bind_default_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("up", Up, Some("TextInput")),
        KeyBinding::new("down", Down, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("shift-up", SelectUp, Some("TextInput")),
        KeyBinding::new("shift-down", SelectDown, Some("TextInput")),
        KeyBinding::new("pageup", PageUp, Some("TextInput")),
        KeyBinding::new("pagedown", PageDown, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
        KeyBinding::new("enter", Newline, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
    ]);
}

// ---------------------------------------------------------------------------
// Pure TextBuffer (Unit-testable without Window)
// ---------------------------------------------------------------------------

/// Pure in-memory text editing buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuffer {
    pub content: String,
    pub selected_range: Range<usize>,
    pub selection_reversed: bool,
    pub marked_range: Option<Range<usize>>,
    pub is_multiline: bool,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new("")
    }
}

impl TextBuffer {
    pub fn new(content: impl Into<String>) -> Self {
        let content = content.into();
        let len = content.len();
        Self {
            content,
            selected_range: len..len,
            selection_reversed: false,
            marked_range: None,
            is_multiline: false,
        }
    }

    pub fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub fn move_to(&mut self, offset: usize) {
        let clamped = offset.min(self.content.len());
        self.selected_range = clamped..clamped;
        self.selection_reversed = false;
    }

    pub fn select_to(&mut self, offset: usize) {
        let clamped = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = clamped;
        } else {
            self.selected_range.end = clamped;
        }

        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
    }

    pub fn select_all(&mut self) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
    }

    pub fn move_left(&mut self, select: bool) {
        let target = if self.selected_range.is_empty() || select {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };

        if select {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
    }

    pub fn move_right(&mut self, select: bool) {
        let target = if self.selected_range.is_empty() || select {
            self.next_boundary(self.cursor_offset())
        } else {
            self.selected_range.end
        };

        if select {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
    }

    pub fn move_home(&mut self, select: bool) {
        let target = if self.is_multiline {
            let cursor = self.cursor_offset().min(self.content.len());
            self.content[..cursor]
                .rfind('\n')
                .map(|idx| idx + 1)
                .unwrap_or(0)
        } else {
            0
        };
        if select {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
    }

    pub fn move_end(&mut self, select: bool) {
        let target = if self.is_multiline {
            let cursor = self.cursor_offset().min(self.content.len());
            self.content[cursor..]
                .find('\n')
                .map(|rel| cursor + rel)
                .unwrap_or(self.content.len())
        } else {
            self.content.len()
        };
        if select {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
    }

    pub fn replace_range(&mut self, range: Range<usize>, new_text: &str) {
        let start = range.start.min(self.content.len());
        let end = range.end.min(self.content.len()).max(start);

        let sanitized = if self.is_multiline {
            new_text.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            new_text.replace("\r\n", " ").replace(['\n', '\r'], " ")
        };

        let mut next = String::with_capacity(self.content.len() + sanitized.len() - (end - start));
        next.push_str(&self.content[..start]);
        next.push_str(&sanitized);
        next.push_str(&self.content[end..]);

        self.content = next;
        let new_cursor = start + sanitized.len();
        self.selected_range = new_cursor..new_cursor;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    pub fn insert(&mut self, text: &str) {
        let range = self.selected_range.clone();
        self.replace_range(range, text);
    }

    pub fn backspace(&mut self) -> bool {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            if cursor == 0 {
                return false;
            }
            let prev = self.previous_boundary(cursor);
            self.replace_range(prev..cursor, "");
            true
        } else {
            let range = self.selected_range.clone();
            self.replace_range(range, "");
            true
        }
    }

    pub fn delete(&mut self) -> bool {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            if cursor >= self.content.len() {
                return false;
            }
            let next = self.next_boundary(cursor);
            self.replace_range(cursor..next, "");
            true
        } else {
            let range = self.selected_range.clone();
            self.replace_range(range, "");
            true
        }
    }

    pub fn cut(&mut self) -> Option<String> {
        if self.selected_range.is_empty() {
            None
        } else {
            let text = self.content[self.selected_range.clone()].to_string();
            let range = self.selected_range.clone();
            self.replace_range(range, "");
            Some(text)
        }
    }

    pub fn copy(&self) -> Option<String> {
        if self.selected_range.is_empty() {
            None
        } else {
            Some(self.content[self.selected_range.clone()].to_string())
        }
    }

    pub fn paste(&mut self, text: &str) {
        let range = self.selected_range.clone();
        self.replace_range(range, text);
    }

    pub fn reset(&mut self) {
        self.content.clear();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.content = text.into();
        let len = self.content.len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    // --- Boundary and UTF-16 conversions ---

    pub fn previous_boundary(&self, offset: usize) -> usize {
        let offset = offset.min(self.content.len());
        self.content
            .char_indices()
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    pub fn next_boundary(&self, offset: usize) -> usize {
        let offset = offset.min(self.content.len());
        self.content
            .char_indices()
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    pub fn offset_from_utf16(&self, offset_utf16: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset_utf16 {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    pub fn offset_to_utf16(&self, offset_utf8: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset_utf8 {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }
}

// ---------------------------------------------------------------------------
// Visual Line Layout
// ---------------------------------------------------------------------------

pub(crate) struct VisualLine {
    /// Byte range inside the underlying display text (or original content if unmasked).
    byte_range: Range<usize>,
    shaped: ShapedLine,
}

/// Maps a byte offset in `content` to a byte offset in masked display text (`•` = 3 bytes).
fn content_to_masked_byte(content: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(content.len());
    let char_count = content[..clamped].chars().count();
    char_count * '•'.len_utf8()
}

/// Maps a byte offset in masked display text back to a byte offset in `content`.
fn masked_to_content_byte(content: &str, masked_byte: usize) -> usize {
    let char_idx = masked_byte / '•'.len_utf8();
    content
        .char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(content.len())
}

// ---------------------------------------------------------------------------
// TextInput Entity
// ---------------------------------------------------------------------------

pub type ChangeCallback =
    Arc<dyn Fn(&str, &mut Window, &mut Context<TextInput>) + Send + Sync + 'static>;

/// Emitted by [`TextInput`] when content changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputEvent {
    Change(String),
}

/// A reusable text input field supporting focus, caret, selections, keyboard
/// navigation, clipboard actions, password masking, read-only mode, multiline
/// wrapping with vertical scrolling, single-line horizontal scrolling, and callbacks.
pub struct TextInput {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) buffer: TextBuffer,
    pub(crate) placeholder: SharedString,
    pub(crate) masked: bool,
    pub(crate) read_only: bool,
    pub(crate) last_lines: Vec<VisualLine>,
    pub(crate) last_line_height: Pixels,
    pub(crate) scroll_x: Pixels,
    pub(crate) scroll_y: Pixels,
    pub(crate) max_scroll_x: Pixels,
    pub(crate) max_scroll_y: Pixels,
    pub(crate) pending_scroll_to_cursor: bool,
    pub(crate) last_bounds: Option<Bounds<Pixels>>,
    pub(crate) is_selecting: bool,
    pub(crate) on_change: Option<ChangeCallback>,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            buffer: TextBuffer::default(),
            placeholder: SharedString::default(),
            masked: false,
            read_only: false,
            last_lines: Vec::new(),
            last_line_height: px(20.0),
            scroll_x: px(0.0),
            scroll_y: px(0.0),
            max_scroll_x: px(0.0),
            max_scroll_y: px(0.0),
            pending_scroll_to_cursor: false,
            last_bounds: None,
            is_selecting: false,
            on_change: None,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.buffer.set_text(content);
        self
    }

    pub fn with_value(self, content: impl Into<String>) -> Self {
        self.with_content(content)
    }

    pub fn set_value(&mut self, text: impl Into<String>) {
        self.buffer.set_text(text);
        self.scroll_x = px(0.0);
        self.scroll_y = px(0.0);
        self.pending_scroll_to_cursor = false;
    }

    pub fn with_multiline(mut self, multiline: bool) -> Self {
        self.buffer.is_multiline = multiline;
        self
    }

    pub fn with_masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            cx.notify();
        }
    }

    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        if self.masked != masked {
            self.masked = masked;
            cx.notify();
        }
    }

    pub fn is_masked(&self) -> bool {
        self.masked
    }

    pub fn on_change(
        mut self,
        callback: impl Fn(&str, &mut Window, &mut Context<Self>) + Send + Sync + 'static,
    ) -> Self {
        self.on_change = Some(Arc::new(callback));
        self
    }

    pub fn set_on_change(
        &mut self,
        callback: impl Fn(&str, &mut Window, &mut Context<Self>) + Send + Sync + 'static,
    ) {
        self.on_change = Some(Arc::new(callback));
    }

    pub fn text(&self) -> &str {
        &self.buffer.content
    }

    pub fn content(&self) -> &str {
        self.text()
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        if self.buffer.content != text {
            self.buffer.set_text(text);
            self.scroll_x = px(0.0);
            self.scroll_y = px(0.0);
            self.pending_scroll_to_cursor = false;
            cx.notify();
        }
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.buffer.reset();
        self.scroll_x = px(0.0);
        self.scroll_y = px(0.0);
        self.pending_scroll_to_cursor = false;
        cx.notify();
    }

    fn notify_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.buffer.content.clone();
        cx.emit(TextInputEvent::Change(text.clone()));
        if let Some(cb) = self.on_change.clone() {
            cb(&text, window, cx);
        }
    }

    fn move_vertical(&mut self, row_delta: isize, select: bool) {
        if !self.buffer.is_multiline || self.last_lines.is_empty() {
            if row_delta < 0 {
                self.buffer.move_home(select);
            } else {
                self.buffer.move_end(select);
            }
            self.pending_scroll_to_cursor = true;
            return;
        }

        let cursor = self.buffer.cursor_offset();
        let display_cursor = if self.masked {
            content_to_masked_byte(&self.buffer.content, cursor)
        } else {
            cursor
        };

        let (cur_row, cur_local) = locate_offset_in_lines(&self.last_lines, display_cursor);
        let cur_x = self
            .last_lines
            .get(cur_row)
            .map(|l| l.shaped.x_for_index(cur_local))
            .unwrap_or(px(0.0));

        let last_row = self.last_lines.len().saturating_sub(1);
        let target_row = (cur_row as isize + row_delta).clamp(0, last_row as isize) as usize;

        let target_offset = if target_row == cur_row {
            if row_delta < 0 {
                0
            } else {
                self.buffer.content.len()
            }
        } else {
            let visual = &self.last_lines[target_row];
            let local_idx = visual.shaped.closest_index_for_x(cur_x);
            let display_byte = visual.byte_range.start + local_idx;
            if self.masked {
                masked_to_content_byte(&self.buffer.content, display_byte)
            } else {
                display_byte.min(self.buffer.content.len())
            }
        };

        if select {
            self.buffer.select_to(target_offset);
        } else {
            self.buffer.move_to(target_offset);
        }
        self.pending_scroll_to_cursor = true;
    }

    // --- Action handlers ---

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(false);
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(false);
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false);
        cx.notify();
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false);
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(true);
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(true);
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true);
        cx.notify();
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true);
        cx.notify();
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self
            .last_bounds
            .map(|b| {
                ((b.size.height / self.last_line_height.max(px(1.0))).floor() as isize - 1).max(1)
            })
            .unwrap_or(5);
        self.move_vertical(-rows, false);
        cx.notify();
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self
            .last_bounds
            .map(|b| {
                ((b.size.height / self.last_line_height.max(px(1.0))).floor() as isize - 1).max(1)
            })
            .unwrap_or(5);
        self.move_vertical(rows, false);
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_home(false);
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_end(false);
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.buffer.is_multiline {
            self.buffer.insert("\n");
            self.pending_scroll_to_cursor = true;
            self.notify_change(window, cx);
            cx.notify();
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.buffer.backspace() {
            self.pending_scroll_to_cursor = true;
            self.notify_change(window, cx);
            cx.notify();
        }
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.buffer.delete() {
            self.pending_scroll_to_cursor = true;
            self.notify_change(window, cx);
            cx.notify();
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.buffer.paste(&text);
            self.pending_scroll_to_cursor = true;
            self.notify_change(window, cx);
            cx.notify();
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        if let Some(text) = self.buffer.copy() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.masked {
            return;
        }
        if let Some(text) = self.buffer.cut() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.pending_scroll_to_cursor = true;
            self.notify_change(window, cx);
            cx.notify();
        }
    }

    // --- Mouse & Scroll handlers ---

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lh = self.last_line_height.max(px(18.0));
        let delta_px = match event.delta {
            ScrollDelta::Lines(lines) => point(lines.x * lh * 2.0, lines.y * lh * 2.5),
            ScrollDelta::Pixels(pixels) => pixels,
        };

        if self.buffer.is_multiline {
            if self.max_scroll_y > px(0.0) {
                let next_y = (self.scroll_y - delta_px.y).clamp(px(0.0), self.max_scroll_y);
                if next_y != self.scroll_y {
                    self.scroll_y = next_y;
                    self.pending_scroll_to_cursor = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }
        } else if self.max_scroll_x > px(0.0) && (delta_px.x != px(0.0) || event.modifiers.shift) {
            let horiz = if delta_px.x != px(0.0) {
                delta_px.x
            } else {
                delta_px.y
            };
            let next_x = (self.scroll_x - horiz).clamp(px(0.0), self.max_scroll_x);
            if next_x != self.scroll_x {
                self.scroll_x = next_x;
                self.pending_scroll_to_cursor = false;
                cx.stop_propagation();
                cx.notify();
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        window.focus(&self.focus_handle);

        let index = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.buffer.select_to(index);
        } else {
            self.buffer.move_to(index);
        }
        self.pending_scroll_to_cursor = true;
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let index = self.index_for_mouse_position(event.position);
            self.buffer.select_to(index);
            self.pending_scroll_to_cursor = true;
            cx.notify();
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.buffer.content.is_empty() || self.last_lines.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };

        let lh = self.last_line_height.max(px(1.0));
        let rel_y = if self.buffer.is_multiline {
            position.y - bounds.top() + self.scroll_y
        } else {
            px(0.0)
        };
        if rel_y < px(0.0) {
            return 0;
        }

        let row = (rel_y / lh).floor().max(0.0) as usize;
        if row >= self.last_lines.len() {
            return self.buffer.content.len();
        }

        let visual = &self.last_lines[row];
        let local_x = position.x - bounds.left() + self.scroll_x;
        let local_idx = visual.shaped.closest_index_for_x(local_x);
        let display_byte = visual.byte_range.start + local_idx;

        if self.masked {
            masked_to_content_byte(&self.buffer.content, display_byte)
        } else {
            display_byte.min(self.buffer.content.len())
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// ---------------------------------------------------------------------------
// EntityInputHandler for platform text input / IME
// ---------------------------------------------------------------------------

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.buffer.range_from_utf16(&range_utf16);
        actual_range.replace(self.buffer.range_to_utf16(&range));
        let start = range.start.min(self.buffer.content.len());
        let end = range.end.min(self.buffer.content.len()).max(start);
        Some(self.buffer.content[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.buffer.selected_range),
            reversed: self.buffer.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.buffer
            .marked_range
            .as_ref()
            .map(|range| self.buffer.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.buffer.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.buffer.range_from_utf16(r))
            .or_else(|| self.buffer.marked_range.clone())
            .unwrap_or_else(|| self.buffer.selected_range.clone());

        self.buffer.replace_range(range, new_text);
        self.pending_scroll_to_cursor = true;
        self.notify_change(window, cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.buffer.range_from_utf16(r))
            .or_else(|| self.buffer.marked_range.clone())
            .unwrap_or_else(|| self.buffer.selected_range.clone());

        let start = range.start.min(self.buffer.content.len());
        self.buffer.replace_range(range, new_text);
        let inserted_end = self
            .buffer
            .cursor_offset()
            .max(start)
            .min(self.buffer.content.len());

        if inserted_end > start {
            self.buffer.marked_range = Some(start..inserted_end);
        } else {
            self.buffer.marked_range = None;
        }

        if let Some(new_sel) = new_selected_range_utf16 {
            let inserted = &self.buffer.content[start..inserted_end];
            let utf16_to_utf8 = |target_u16: usize| -> usize {
                let mut u8_off = 0usize;
                let mut u16_off = 0usize;
                for ch in inserted.chars() {
                    if u16_off >= target_u16 {
                        break;
                    }
                    u16_off += ch.len_utf16();
                    u8_off += ch.len_utf8();
                }
                u8_off
            };
            let sel_start = start + utf16_to_utf8(new_sel.start);
            let sel_end = (start + utf16_to_utf8(new_sel.end)).max(sel_start);
            self.buffer.selected_range = sel_start..sel_end;
        }

        self.pending_scroll_to_cursor = true;
        self.notify_change(window, cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.last_lines.is_empty() {
            return None;
        }
        let range = self.buffer.range_from_utf16(&range_utf16);
        let start = range.start.min(self.buffer.content.len());
        let end = range.end.min(self.buffer.content.len()).max(start);

        let d_start = if self.masked {
            content_to_masked_byte(&self.buffer.content, start)
        } else {
            start
        };
        let d_end = if self.masked {
            content_to_masked_byte(&self.buffer.content, end)
        } else {
            end
        };

        let (start_row, start_local) = locate_offset_in_lines(&self.last_lines, d_start);
        let (end_row, end_local) = locate_offset_in_lines(&self.last_lines, d_end);
        let start_line = &self.last_lines[start_row];
        let end_line = &self.last_lines[end_row];
        let start_y = bounds.top() + self.last_line_height * (start_row as f32) - self.scroll_y;
        let end_y = bounds.top() + self.last_line_height * (end_row as f32) - self.scroll_y;

        Some(Bounds::from_corners(
            point(
                bounds.left() + start_line.shaped.x_for_index(start_local) - self.scroll_x,
                start_y,
            ),
            point(
                bounds.left() + end_line.shaped.x_for_index(end_local) - self.scroll_x,
                end_y + self.last_line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let utf8_index = self.index_for_mouse_position(point);
        Some(self.buffer.offset_to_utf16(utf8_index))
    }
}

// ---------------------------------------------------------------------------
// TextElement
// ---------------------------------------------------------------------------

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    lines: Vec<VisualLine>,
    line_height: Pixels,
    scroll_x: Pixels,
    scroll_y: Pixels,
    max_scroll_x: Pixels,
    max_scroll_y: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    scrollbar_thumb: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Splits `text` into wrapped visual lines that fit within `max_width`,
/// preserving explicit `\n` breaks and word boundaries when possible.
fn wrap_text_into_visual_lines(
    text: &str,
    max_width: Pixels,
    font_size: Pixels,
    base_run: &TextRun,
    window: &mut Window,
) -> Vec<VisualLine> {
    let mut lines = Vec::new();
    let effective_max = max_width.max(px(40.0));

    let mut para_start = 0usize;
    loop {
        let rel_nl = text[para_start..].find('\n');
        let para_end = rel_nl.map(|p| para_start + p).unwrap_or(text.len());

        if para_start == para_end {
            // Empty paragraph line
            let run = TextRun {
                len: 0,
                ..base_run.clone()
            };
            let shaped =
                window
                    .text_system()
                    .shape_line(SharedString::from(""), font_size, &[run], None);
            lines.push(VisualLine {
                byte_range: para_start..para_end,
                shaped,
            });
        } else {
            let mut seg_start = para_start;
            while seg_start < para_end {
                let slice = &text[seg_start..para_end];
                let run = TextRun {
                    len: slice.len(),
                    ..base_run.clone()
                };
                let full_shaped = window.text_system().shape_line(
                    SharedString::from(slice.to_string()),
                    font_size,
                    &[run],
                    None,
                );

                if full_shaped.width <= effective_max {
                    lines.push(VisualLine {
                        byte_range: seg_start..para_end,
                        shaped: full_shaped,
                    });
                    break;
                }

                let mut split_rel = full_shaped.closest_index_for_x(effective_max);
                if split_rel == 0 {
                    // Advance at least one character to avoid infinite loop
                    split_rel = slice
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(slice.len());
                } else if split_rel < slice.len() {
                    // Prefer breaking on whitespace so words don't get cut mid-word
                    if let Some((ws_pos, ws_ch)) = slice[..split_rel]
                        .char_indices()
                        .rev()
                        .find(|(_, c)| c.is_whitespace())
                    {
                        let candidate = ws_pos + ws_ch.len_utf8();
                        if candidate > 0 {
                            split_rel = candidate;
                        }
                    }
                }

                let seg_end = (seg_start + split_rel).min(para_end);
                let seg_slice = &text[seg_start..seg_end];
                let seg_run = TextRun {
                    len: seg_slice.len(),
                    ..base_run.clone()
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(seg_slice.to_string()),
                    font_size,
                    &[seg_run],
                    None,
                );
                lines.push(VisualLine {
                    byte_range: seg_start..seg_end,
                    shaped,
                });
                seg_start = seg_end;
            }
        }

        if let Some(rel) = rel_nl {
            para_start = para_start + rel + 1;
            if para_start > text.len() {
                break;
            }
        } else {
            break;
        }
    }

    lines
}

/// Finds `(row_idx, local_byte_offset)` for a byte position in `lines`.
fn locate_offset_in_lines(lines: &[VisualLine], byte_offset: usize) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }
    for (row, line) in lines.iter().enumerate() {
        if byte_offset <= line.byte_range.end {
            let local = byte_offset.saturating_sub(line.byte_range.start);
            let max_local = line.byte_range.end - line.byte_range.start;
            return (row, local.min(max_local));
        }
    }
    let last_row = lines.len() - 1;
    let last = &lines[last_row];
    (last_row, last.byte_range.end - last.byte_range.start)
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let is_multiline = self.input.read(cx).buffer.is_multiline;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if is_multiline {
            style.size.height = relative(1.).into();
        } else {
            style.size.height = window.line_height().into();
        }
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.buffer.content.clone();
        let selected_range = input.buffer.selected_range.clone();
        let cursor = input.buffer.cursor_offset();
        let is_multiline = input.buffer.is_multiline;
        let masked = input.masked;
        let placeholder = input.placeholder.clone();
        let marked_range = input.buffer.marked_range.clone();
        let current_scroll_x = input.scroll_x;
        let current_scroll_y = input.scroll_y;
        let pending_scroll_to_cursor = input.pending_scroll_to_cursor;

        let style = window.text_style();
        let t = theme::active_theme();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let is_placeholder = content.is_empty();
        let (display_string, text_color) = if is_placeholder {
            (placeholder.to_string(), t.muted)
        } else if masked {
            ("•".repeat(content.chars().count()), t.text)
        } else {
            (content.clone(), t.text)
        };

        let base_run = TextRun {
            len: display_string.len(),
            font: style.font(),
            color: text_color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let display_cursor = if is_placeholder {
            0
        } else if masked {
            content_to_masked_byte(&content, cursor)
        } else {
            cursor
        };

        let display_sel = if is_placeholder {
            0..0
        } else if masked {
            content_to_masked_byte(&content, selected_range.start)
                ..content_to_masked_byte(&content, selected_range.end)
        } else {
            selected_range.clone()
        };

        let selection_color = rgba(0xB5001844);

        if is_multiline {
            // Reserve a few pixels on the right for the vertical scrollbar thumb.
            let wrap_width = (bounds.size.width - px(10.0)).max(px(40.0));
            let lines = wrap_text_into_visual_lines(
                &display_string,
                wrap_width,
                font_size,
                &base_run,
                window,
            );

            let total_height = line_height * (lines.len() as f32);
            let viewport_height = bounds.size.height.max(line_height);
            let max_scroll_y = (total_height - viewport_height).max(px(0.0));
            let mut scroll_y = current_scroll_y.clamp(px(0.0), max_scroll_y);

            let (cursor_row, cursor_local) = locate_offset_in_lines(&lines, display_cursor);
            if pending_scroll_to_cursor {
                let cursor_top = line_height * (cursor_row as f32);
                let cursor_bottom = cursor_top + line_height;
                if cursor_top < scroll_y {
                    scroll_y = cursor_top;
                } else if cursor_bottom > scroll_y + viewport_height {
                    scroll_y = (cursor_bottom - viewport_height).min(max_scroll_y);
                }
            }

            let (cursor_quad, selections) = if display_sel.is_empty() {
                let x = lines
                    .get(cursor_row)
                    .map(|l| l.shaped.x_for_index(cursor_local))
                    .unwrap_or(px(0.0));
                let y = bounds.top() + line_height * (cursor_row as f32) - scroll_y;
                (
                    Some(fill(
                        Bounds::new(point(bounds.left() + x, y), size(px(1.5), line_height)),
                        t.accent_hover,
                    )),
                    Vec::new(),
                )
            } else {
                let mut quads = Vec::new();
                for (row, line) in lines.iter().enumerate() {
                    let seg_start = line.byte_range.start;
                    let seg_end = line.byte_range.end;
                    if display_sel.end <= seg_start || display_sel.start >= seg_end {
                        continue;
                    }
                    let sel_l = display_sel.start.max(seg_start) - seg_start;
                    let sel_r = display_sel.end.min(seg_end) - seg_start;
                    let x1 = line.shaped.x_for_index(sel_l);
                    let x2 = line.shaped.x_for_index(sel_r);
                    let y = bounds.top() + line_height * (row as f32) - scroll_y;
                    quads.push(fill(
                        Bounds::from_corners(
                            point(bounds.left() + x1, y),
                            point(bounds.left() + x2.max(x1 + px(4.0)), y + line_height),
                        ),
                        selection_color,
                    ));
                }
                (None, quads)
            };

            let scrollbar_thumb = if max_scroll_y > px(0.0) && total_height > px(0.0) {
                let ratio = (viewport_height / total_height).clamp(0.12, 1.0);
                let thumb_h = (viewport_height * ratio).max(px(20.0));
                let track_h = (viewport_height - thumb_h).max(px(1.0));
                let scroll_frac = (scroll_y / max_scroll_y).clamp(0.0, 1.0);
                let thumb_y = bounds.top() + track_h * scroll_frac;
                let thumb_x = bounds.right() - px(5.0);
                let mut thumb_color = t.muted;
                thumb_color.a = 0.55;
                Some(fill(
                    Bounds::new(point(thumb_x, thumb_y), size(px(4.0), thumb_h)),
                    thumb_color,
                ))
            } else {
                None
            };

            PrepaintState {
                lines,
                line_height,
                scroll_x: px(0.0),
                scroll_y,
                max_scroll_x: px(0.0),
                max_scroll_y,
                cursor: cursor_quad,
                selections,
                scrollbar_thumb,
            }
        } else {
            let runs = if !is_placeholder
                && !masked
                && let Some(marked) = marked_range.as_ref()
            {
                vec![
                    TextRun {
                        len: marked.start.min(display_string.len()),
                        ..base_run.clone()
                    },
                    TextRun {
                        len: (marked.end.saturating_sub(marked.start)).min(display_string.len()),
                        underline: Some(gpui::UnderlineStyle {
                            color: Some(base_run.color),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                        ..base_run.clone()
                    },
                    TextRun {
                        len: display_string.len().saturating_sub(marked.end),
                        ..base_run.clone()
                    },
                ]
                .into_iter()
                .filter(|r| r.len > 0)
                .collect()
            } else {
                vec![base_run]
            };

            let shaped = window.text_system().shape_line(
                SharedString::from(display_string.clone()),
                font_size,
                &runs,
                None,
            );

            let cursor_x = shaped.x_for_index(display_cursor);
            let max_w = bounds.size.width.max(px(16.0));
            let max_scroll_x = (shaped.width - max_w + px(12.0)).max(px(0.0));
            let mut scroll_x = current_scroll_x.clamp(px(0.0), max_scroll_x);

            if pending_scroll_to_cursor {
                if cursor_x < scroll_x + px(4.0) {
                    scroll_x = (cursor_x - px(8.0)).max(px(0.0));
                } else if cursor_x > scroll_x + max_w - px(6.0) {
                    scroll_x = (cursor_x - max_w + px(10.0)).min(max_scroll_x);
                }
            }

            let (cursor_quad, selections) = if display_sel.is_empty() {
                (
                    Some(fill(
                        Bounds::new(
                            point(bounds.left() + cursor_x - scroll_x, bounds.top()),
                            size(px(1.5), bounds.bottom() - bounds.top()),
                        ),
                        t.accent_hover,
                    )),
                    Vec::new(),
                )
            } else {
                let x1 = (shaped.x_for_index(display_sel.start) - scroll_x).max(px(0.0));
                let x2 = (shaped.x_for_index(display_sel.end) - scroll_x).min(bounds.size.width);
                let quad = fill(
                    Bounds::from_corners(
                        point(bounds.left() + x1, bounds.top()),
                        point(bounds.left() + x2.max(x1), bounds.bottom()),
                    ),
                    selection_color,
                );
                (None, vec![quad])
            };

            PrepaintState {
                lines: vec![VisualLine {
                    byte_range: 0..display_string.len(),
                    shaped,
                }],
                line_height,
                scroll_x,
                scroll_y: px(0.0),
                max_scroll_x,
                max_scroll_y: px(0.0),
                cursor: cursor_quad,
                selections,
                scrollbar_thumb: None,
            }
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        let line_height = prepaint.line_height;
        let scroll_x = prepaint.scroll_x;
        let scroll_y = prepaint.scroll_y;
        let max_scroll_x = prepaint.max_scroll_x;
        let max_scroll_y = prepaint.max_scroll_y;
        let lines = std::mem::take(&mut prepaint.lines);
        let selections = std::mem::take(&mut prepaint.selections);
        let cursor = prepaint.cursor.take();
        let scrollbar_thumb = prepaint.scrollbar_thumb.take();
        let is_focused = focus_handle.is_focused(window);
        let is_read_only = self.input.read(cx).read_only;

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for selection in selections {
                window.paint_quad(selection);
            }

            for (row, line) in lines.iter().enumerate() {
                let y = bounds.top() + line_height * (row as f32) - scroll_y;
                if y + line_height >= bounds.top() && y <= bounds.bottom() {
                    let origin = point(bounds.left() - scroll_x, y);
                    let _ = line.shaped.paint(origin, line_height, window, cx);
                }
            }

            if is_focused && !is_read_only && let Some(cursor_quad) = cursor {
                window.paint_quad(cursor_quad);
            }

            if let Some(thumb) = scrollbar_thumb {
                window.paint_quad(thumb);
            }
        });

        self.input.update(cx, |input, _cx| {
            input.last_lines = lines;
            input.last_line_height = line_height;
            input.scroll_x = scroll_x;
            input.scroll_y = scroll_y;
            input.max_scroll_x = max_scroll_x;
            input.max_scroll_y = max_scroll_y;
            input.pending_scroll_to_cursor = false;
            input.last_bounds = Some(bounds);
        });
    }
}

// ---------------------------------------------------------------------------
// Render implementation
// ---------------------------------------------------------------------------

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme::active_theme();
        let is_multiline = self.buffer.is_multiline;

        let mut container = div()
            .flex()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .key_context("TextInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .text_size(px(13.0))
            .text_color(t.text);

        if is_multiline {
            container = container.h_full().items_start().py(px(4.0));
        } else {
            container = container.items_center();
        }

        container
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: cx.entity() })
    }
}

// ---------------------------------------------------------------------------
// Unit tests for pure editing operations
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_cursor() {
        let mut buf = TextBuffer::new("");
        buf.insert("Hello");
        assert_eq!(buf.content, "Hello");
        assert_eq!(buf.cursor_offset(), 5);

        buf.insert(" World");
        assert_eq!(buf.content, "Hello World");
        assert_eq!(buf.cursor_offset(), 11);
    }

    #[test]
    fn test_backspace_and_delete() {
        let mut buf = TextBuffer::new("Hello");
        assert!(buf.backspace());
        assert_eq!(buf.content, "Hell");
        assert_eq!(buf.cursor_offset(), 4);

        buf.move_to(0);
        assert!(buf.delete());
        assert_eq!(buf.content, "ell");
        assert_eq!(buf.cursor_offset(), 0);

        // Backspace at 0 does nothing
        assert!(!buf.backspace());
        assert_eq!(buf.content, "ell");

        // Delete at end does nothing
        buf.move_to(buf.content.len());
        assert!(!buf.delete());
    }

    #[test]
    fn test_selection_and_replace() {
        let mut buf = TextBuffer::new("Hello World");
        buf.selected_range = 0..5;
        buf.insert("Hi");
        assert_eq!(buf.content, "Hi World");
        assert_eq!(buf.cursor_offset(), 2);
    }

    #[test]
    fn test_cut_copy_paste() {
        let mut buf = TextBuffer::new("VanguardCV");
        buf.selected_range = 8..10;
        let copied = buf.copy();
        assert_eq!(copied.as_deref(), Some("CV"));
        assert_eq!(buf.content, "VanguardCV");

        let cut = buf.cut();
        assert_eq!(cut.as_deref(), Some("CV"));
        assert_eq!(buf.content, "Vanguard");

        buf.paste(" Studio");
        assert_eq!(buf.content, "Vanguard Studio");
    }

    #[test]
    fn test_arrows_and_boundaries() {
        let mut buf = TextBuffer::new("abc");
        buf.move_to(3);
        buf.move_left(false);
        assert_eq!(buf.cursor_offset(), 2);
        buf.move_left(false);
        assert_eq!(buf.cursor_offset(), 1);
        buf.move_right(false);
        assert_eq!(buf.cursor_offset(), 2);

        buf.move_home(false);
        assert_eq!(buf.cursor_offset(), 0);
        buf.move_end(false);
        assert_eq!(buf.cursor_offset(), 3);
    }

    #[test]
    fn test_select_all() {
        let mut buf = TextBuffer::new("Select Me");
        buf.select_all();
        assert_eq!(buf.selected_range, 0..9);
        buf.backspace();
        assert_eq!(buf.content, "");
    }

    #[test]
    fn test_utf16_conversions() {
        // "🦀" is 4 bytes in UTF-8, 2 code units in UTF-16
        let buf = TextBuffer::new("Rust 🦀 Rock");
        assert_eq!(buf.offset_to_utf16(5), 5); // start of crab
        assert_eq!(buf.offset_to_utf16(9), 7); // after crab
        assert_eq!(buf.offset_from_utf16(7), 9);
    }

    #[test]
    fn test_multiline_sanitization() {
        let mut buf = TextBuffer::new("");
        buf.is_multiline = false;
        buf.insert("line1\nline2");
        assert_eq!(buf.content, "line1 line2");

        let mut multi = TextBuffer::new("");
        multi.is_multiline = true;
        multi.insert("line1\nline2");
        assert_eq!(multi.content, "line1\nline2");
    }
}
