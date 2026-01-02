mod action;
mod config;
mod theme;

pub use action::{Direction, EditorAction};
pub use config::EditorConfig;
pub use theme::EditorTheme;

use std::rc::Rc;

use gpui::{
    App, Context, CursorStyle, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    ModifiersChangedEvent, ReadGlobal, ScrollHandle, Window, div, font, prelude::*,
};

use crate::title_bar::FileInfo;

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::line::{CheckboxCallback, ClickCallback, DragCallback, HoverCallback, Line, LineTheme};
use crate::lines::extract_inline_styles;
use crate::marker::{LineMarkers, find_container_indent_from_lines};

type CodeBlockRange = (usize, Option<usize>);

/// A markdown editor component with live inline rendering.
///
/// The editor hides markdown syntax (like `**`, `#`, `-`) when the cursor
/// is elsewhere, showing only the styled result. When you move the cursor
/// into styled text, the syntax reappears for editing.
///
/// # Example
///
/// ```ignore
/// let editor = cx.new(|cx| Editor::new("# Hello, world!", cx));
/// ```
pub struct Editor {
    buffer: Buffer,
    selection: Selection,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    cursor_child_index: Option<usize>,
    scroll_to_cursor_pending: bool,
    input_blocked: bool,
    streaming_mode: bool,
    config: EditorConfig,
    /// Whether mouse is over a checkbox.
    hovering_checkbox: bool,
    /// Whether mouse is over a link (regardless of Ctrl state).
    hovering_link_region: bool,
    /// Whether Ctrl/Cmd is currently held.
    ctrl_held: bool,
}

impl Editor {
    /// Create a new editor with the given content and default configuration.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        Self::with_config(content, EditorConfig::default(), cx)
    }

    /// Create a new editor with the given content and configuration.
    pub fn with_config(content: &str, config: EditorConfig, cx: &mut Context<Self>) -> Self {
        let buffer: Buffer = content.parse().unwrap_or_default();
        let focus_handle = cx.focus_handle();

        let scroll_handle = ScrollHandle::new();

        Self {
            buffer,
            selection: Selection::new(0, 0),
            focus_handle,
            scroll_handle,
            cursor_child_index: None,
            scroll_to_cursor_pending: false,
            input_blocked: false,
            streaming_mode: false,
            config,
            hovering_checkbox: false,
            hovering_link_region: false,
            ctrl_held: false,
        }
    }

    /// Returns the buffer contents as a string.
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Returns the length of the buffer in bytes.
    pub fn len(&self) -> usize {
        self.buffer.len_bytes()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.len_bytes() == 0
    }

    /// Replace the entire buffer contents, resetting cursor to the start.
    pub fn set_text(&mut self, content: &str, cx: &mut Context<Self>) {
        self.buffer = content.parse().unwrap_or_default();
        self.selection = Selection::new(0, 0);
        cx.notify();
    }

    /// Insert text at the current cursor position.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.insert_text(text);
        cx.notify();
    }

    /// Append text to the end of the buffer and move cursor to the end.
    ///
    /// Useful for streaming content from an AI or other source.
    pub fn append(&mut self, text: &str, cx: &mut Context<Self>) {
        let end = self.buffer.len_bytes();
        self.buffer.insert(end, text, end);
        let new_end = self.buffer.len_bytes();
        self.selection = Selection::new(new_end, new_end);
        cx.notify();
    }

    /// Append text and scroll to keep the cursor visible.
    pub fn append_and_scroll(&mut self, text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.append(text, cx);
        self.scroll_handle.scroll_to_bottom();
    }

    fn cursor(&self) -> Cursor {
        self.selection.cursor()
    }

    fn move_cursor(&mut self, new_cursor: Cursor, extend: bool) {
        if extend {
            self.selection = self.selection.extend_to(new_cursor.offset);
        } else {
            self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
        }
    }

    fn find_line_at(&self, byte_pos: usize) -> Option<(usize, &LineMarkers)> {
        let idx = self.buffer.byte_to_line(byte_pos);
        self.buffer.lines().get(idx).map(|line| (idx, line))
    }

    fn perform_pending_scroll(&mut self, margin: gpui::Pixels) {
        if !self.scroll_to_cursor_pending {
            return;
        }

        let Some(child_ix) = self.cursor_child_index else {
            return;
        };

        let Some(item_bounds) = self.scroll_handle.bounds_for_item(child_ix) else {
            return;
        };

        self.scroll_to_cursor_pending = false;

        let viewport = self.scroll_handle.bounds();
        let offset = self.scroll_handle.offset();

        let item_top = item_bounds.origin.y + offset.y;
        let item_bottom = item_top + item_bounds.size.height;

        let visible_top = viewport.origin.y + margin;
        let visible_bottom = viewport.origin.y + viewport.size.height - margin;

        if item_top < visible_top {
            let new_offset_y = viewport.origin.y - item_bounds.origin.y + margin;
            self.scroll_handle
                .set_offset(gpui::point(offset.x, new_offset_y));
        } else if item_bottom > visible_bottom {
            let new_offset_y = viewport.origin.y + viewport.size.height
                - item_bounds.origin.y
                - item_bounds.size.height
                - margin;
            self.scroll_handle
                .set_offset(gpui::point(offset.x, new_offset_y));
        }
    }

    fn request_scroll_to_cursor(&mut self) {
        self.scroll_to_cursor_pending = true;
    }

    fn compute_smart_enter_text(&self, cursor_pos: usize) -> String {
        let lines = self.buffer.lines();
        let cursor_line_idx = self.buffer.byte_to_line(cursor_pos);

        let Some(line_info) = lines.get(cursor_line_idx) else {
            return "\n".to_string();
        };

        let buffer_text = self.buffer.text();
        let continuation = line_info.continuation(&buffer_text);
        if continuation.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{}", continuation)
        }
    }

    fn compute_smart_tab_indent(&self, cursor_pos: usize) -> Option<String> {
        find_container_indent_from_lines(self.buffer.lines(), cursor_pos)
            .map(|width| " ".repeat(width))
    }

    fn smart_tab(&mut self) {
        if let Some(indent) = self.compute_smart_tab_indent(self.cursor().offset) {
            let cursor_offset = self.cursor().offset;
            let line_start = self.cursor().move_to_line_start(&self.buffer).offset;
            self.buffer.insert(line_start, &indent, cursor_offset);
            let new_offset = cursor_offset + indent.len();
            self.selection = Selection::new(new_offset, new_offset);
        }
    }

    fn toggle_checkbox(&mut self, line_number: usize, cx: &mut Context<Self>) {
        let lines = self.buffer.lines();
        let Some(line) = lines.get(line_number) else {
            return;
        };

        let buffer_text = self.buffer.text();
        let Some(is_checked) = line.checkbox() else {
            return;
        };

        let line_text = &buffer_text[line.range.clone()];
        let checkbox_pattern = if is_checked { "[x]" } else { "[ ]" };
        let alt_pattern = if is_checked { "[X]" } else { "" };

        let checkbox_offset = line_text.find(checkbox_pattern).or_else(|| {
            if !alt_pattern.is_empty() {
                line_text.find(alt_pattern)
            } else {
                None
            }
        });

        let Some(relative_offset) = checkbox_offset else {
            return;
        };

        let checkbox_content_start = line.range.start + relative_offset + 1;
        let checkbox_content_end = checkbox_content_start + 1;
        let new_content = if is_checked { " " } else { "x" };
        let cursor_before = self.cursor().offset;

        self.buffer.replace(
            checkbox_content_start..checkbox_content_end,
            new_content,
            cursor_before,
        );

        self.selection = Selection::new(cursor_before, cursor_before);

        cx.notify();
    }

    fn insert_text(&mut self, text: &str) {
        let cursor_before = self.cursor().offset;
        let insert_pos = if !self.selection.is_collapsed() {
            let range = self.selection.range();
            self.buffer.delete(range.clone(), cursor_before);
            range.start
        } else {
            cursor_before
        };
        self.buffer.insert(insert_pos, text, insert_pos);
        let new_pos = insert_pos + text.len();
        self.selection = Selection::new(new_pos, new_pos);
    }

    fn delete_backward(&mut self) {
        if !self.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset > 0 {
            let cursor_pos = self.cursor().offset;

            if let Some(delete_range) = self.smart_backspace_range(cursor_pos) {
                self.buffer.delete(delete_range.clone(), cursor_pos);
                self.selection = Selection::new(delete_range.start, delete_range.start);
            } else {
                let new_cursor = self.cursor().move_left(&self.buffer);
                self.buffer
                    .delete(new_cursor.offset..cursor_pos, cursor_pos);
                self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
            }
        }
    }

    fn smart_backspace_range(&self, cursor_pos: usize) -> Option<std::ops::Range<usize>> {
        let (_, line) = self.find_line_at(cursor_pos)?;

        for marker in &line.markers {
            if cursor_pos == marker.range.end {
                return Some(marker.range.clone());
            }
        }

        None
    }

    fn delete_forward(&mut self) {
        if !self.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset < self.buffer.len_bytes() {
            let cursor_before = self.cursor().offset;
            let next = self.cursor().move_right(&self.buffer);
            self.buffer
                .delete(cursor_before..next.offset, cursor_before);
        }
    }

    fn delete_selection(&mut self) {
        let range = self.selection.range();
        let cursor_before = self.cursor().offset;
        self.buffer.delete(range.clone(), cursor_before);
        self.selection = Selection::new(range.start, range.start);
    }

    fn move_in_direction(&mut self, direction: Direction, extend: bool) {
        let new_cursor = match direction {
            Direction::Left => self.smart_move_left(),
            Direction::Right => self.smart_move_right(),
            Direction::Up => self.cursor().move_up(&self.buffer),
            Direction::Down => self.cursor().move_down(&self.buffer),
        };
        self.move_cursor(new_cursor, extend);
    }

    fn smart_move_left(&self) -> Cursor {
        let cursor_pos = self.cursor().offset;
        if cursor_pos == 0 {
            return self.cursor();
        }

        if let Some((line_idx, line)) = self.find_line_at(cursor_pos)
            && let Some(marker_range) = line.marker_range()
            && cursor_pos <= marker_range.end
        {
            if line_idx > 0 {
                let prev_line = &self.buffer.lines()[line_idx - 1];
                return Cursor::new(prev_line.range.end);
            } else {
                return Cursor::new(marker_range.end);
            }
        }

        self.cursor().move_left(&self.buffer)
    }

    fn smart_move_right(&self) -> Cursor {
        let cursor_pos = self.cursor().offset;
        let len = self.buffer.len_bytes();
        if cursor_pos >= len {
            return self.cursor();
        }

        if let Some((_, line)) = self.find_line_at(cursor_pos)
            && let Some(marker_range) = line.marker_range()
        {
            if cursor_pos == line.range.start {
                return Cursor::new(marker_range.end);
            }
            if cursor_pos > marker_range.start && cursor_pos < marker_range.end {
                return Cursor::new(marker_range.end);
            }
        }

        self.cursor().move_right(&self.buffer)
    }

    fn compute_code_block_ranges(lines: &[LineMarkers]) -> Vec<CodeBlockRange> {
        let mut ranges = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            if lines[i].is_fence() {
                let start = i;
                i += 1;
                let mut found_close = false;

                while i < lines.len() {
                    if lines[i].is_fence() {
                        ranges.push((start, Some(i)));
                        i += 1;
                        found_close = true;
                        break;
                    }
                    i += 1;
                }

                if !found_close {
                    ranges.push((start, None));
                }
            } else {
                i += 1;
            }
        }

        ranges
    }

    fn cursor_in_code_block(&self) -> bool {
        let lines = self.buffer.lines();
        let cursor_line = self.buffer.byte_to_line(self.cursor().offset);
        let ranges = Self::compute_code_block_ranges(lines);

        for (start, end) in ranges {
            let block_end = end.unwrap_or(lines.len().saturating_sub(1));
            if cursor_line >= start && cursor_line <= block_end {
                return true;
            }
        }
        false
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.input_blocked {
            return;
        }

        let keystroke = &event.keystroke;
        let extend = keystroke.modifiers.shift;

        match keystroke.key.as_str() {
            "backspace" => {
                self.delete_backward();
                cx.notify();
            }
            "delete" => {
                self.delete_forward();
                cx.notify();
            }
            "left" => {
                self.move_in_direction(Direction::Left, extend);
                cx.notify();
            }
            "right" => {
                self.move_in_direction(Direction::Right, extend);
                cx.notify();
            }
            "up" => {
                self.move_in_direction(Direction::Up, extend);
                cx.notify();
            }
            "down" => {
                self.move_in_direction(Direction::Down, extend);
                cx.notify();
            }
            "home" => {
                let new_cursor = if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor().move_to_start()
                } else {
                    self.cursor().move_to_line_start(&self.buffer)
                };
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "end" => {
                let new_cursor = if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor().move_to_end(&self.buffer)
                } else {
                    self.cursor().move_to_line_end(&self.buffer)
                };
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "enter" => {
                if keystroke.modifiers.shift {
                    let text = self.compute_smart_enter_text(self.cursor().offset);
                    self.insert_text(&text);
                } else {
                    self.insert_text("\n");
                }
                cx.notify();
            }
            "tab" => {
                if self.cursor_in_code_block() {
                    self.insert_text("    ");
                } else {
                    self.smart_tab();
                }
                cx.notify();
            }
            "a" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                self.selection = Selection::select_all(&self.buffer);
                cx.notify();
            }
            "c" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if !self.selection.is_collapsed() {
                    let range = self.selection.range();
                    let text = &self.buffer.text()[range];
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                }
            }
            "x" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if !self.selection.is_collapsed() {
                    let range = self.selection.range();
                    let text = self.buffer.text()[range].to_string();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.delete_selection();
                    cx.notify();
                }
            }
            "v" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if let Some(clipboard_item) = cx.read_from_clipboard()
                    && let Some(text) = clipboard_item.text()
                {
                    self.insert_text(&text);
                    cx.notify();
                }
            }
            "z" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if keystroke.modifiers.shift {
                    if let Some(cursor_pos) = self.buffer.redo() {
                        self.selection = Selection::new(cursor_pos, cursor_pos);
                        cx.notify();
                    }
                } else if let Some(cursor_pos) = self.buffer.undo() {
                    self.selection = Selection::new(cursor_pos, cursor_pos);
                    cx.notify();
                }
            }
            "y" if keystroke.modifiers.control => {
                if let Some(cursor_pos) = self.buffer.redo() {
                    self.selection = Selection::new(cursor_pos, cursor_pos);
                    cx.notify();
                }
            }
            "s" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                self.save(cx);
            }

            _ => {
                if let Some(key_char) = &keystroke.key_char {
                    if key_char == " " && !self.cursor_in_code_block() {
                        let cursor = self.cursor();
                        let line_start = cursor.move_to_line_start(&self.buffer).offset;
                        if cursor.offset == line_start {
                            return;
                        }
                    }
                    self.insert_text(key_char);
                    cx.notify();
                }
            }
        }
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ctrl_held = event.modifiers.control || event.modifiers.platform;
        if self.ctrl_held != ctrl_held {
            self.ctrl_held = ctrl_held;
            cx.notify();
        }
    }

    /// Block or unblock user input. Useful during demos or streaming.
    pub fn set_input_blocked(&mut self, blocked: bool) {
        self.input_blocked = blocked;
    }

    /// Returns true if user input is currently blocked.
    pub fn is_input_blocked(&self) -> bool {
        self.input_blocked
    }

    /// Enter streaming mode: block input and move cursor to end.
    ///
    /// Call this before appending streamed content, then call
    /// [`end_streaming`](Self::end_streaming) when done.
    pub fn begin_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming_mode = true;
        self.input_blocked = true;
        let end = self.buffer.len_bytes();
        self.selection = Selection::new(end, end);
        cx.notify();
    }

    /// Exit streaming mode and re-enable user input.
    pub fn end_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming_mode = false;
        self.input_blocked = false;
        cx.notify();
    }

    /// Returns true if currently in streaming mode.
    pub fn is_streaming(&self) -> bool {
        self.streaming_mode
    }

    /// Returns the current cursor position as a byte offset.
    pub fn cursor_position(&self) -> usize {
        self.selection.head
    }

    /// Returns the current selection range, or None if the cursor is collapsed.
    pub fn selection_range(&self) -> Option<std::ops::Range<usize>> {
        if self.selection.is_collapsed() {
            None
        } else {
            Some(self.selection.range())
        }
    }

    /// Set the cursor position to the given byte offset.
    pub fn set_cursor(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.buffer.len_bytes());
        self.selection = Selection::new(offset, offset);
        cx.notify();
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.buffer.len_bytes();
        self.selection = Selection::new(end, end);
        cx.notify();
    }

    /// Move the cursor to the start of the buffer.
    pub fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.selection = Selection::new(0, 0);
        cx.notify();
    }

    /// Returns true if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    /// Mark the buffer as clean (no unsaved changes).
    pub fn mark_clean(&mut self) {
        self.buffer.mark_clean();
    }

    /// Save the buffer to the file specified in FileInfo.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let file_info = FileInfo::global(cx);
        let path = file_info.path.clone();
        let content = self.buffer.text();

        if let Err(e) = std::fs::write(&path, &content) {
            eprintln!("Failed to save file: {}", e);
            return;
        }

        self.buffer.mark_clean();
        cx.notify();
    }

    /// Returns true if there are actions to undo.
    pub fn can_undo(&self) -> bool {
        self.buffer.can_undo()
    }

    /// Returns true if there are actions to redo.
    pub fn can_redo(&self) -> bool {
        self.buffer.can_redo()
    }

    /// Undo the last action.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.buffer.undo() {
            self.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.buffer.redo() {
            self.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }

    /// Execute an editor action programmatically.
    ///
    /// This is useful for scripted demos or external control of the editor.
    pub fn execute(&mut self, action: EditorAction, _window: &mut Window, cx: &mut Context<Self>) {
        match action {
            EditorAction::Type(c) => {
                self.insert_text(&c.to_string());
            }
            EditorAction::Enter => {
                self.insert_text("\n");
            }
            EditorAction::ShiftEnter => {
                let text = self.compute_smart_enter_text(self.cursor().offset);
                self.insert_text(&text);
            }
            EditorAction::Tab => {
                self.smart_tab();
            }
            EditorAction::Backspace => {
                self.delete_backward();
            }
            EditorAction::Move(direction) => {
                self.move_in_direction(direction, false);
            }
        }
        cx.notify();
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sync dirty state with FileInfo global for title bar
        let file_info = FileInfo::global(cx);
        if file_info.dirty != self.buffer.is_dirty() {
            cx.set_global(FileInfo {
                path: file_info.path.clone(),
                dirty: self.buffer.is_dirty(),
            });
        }

        let theme = self.config.theme.clone();
        let line_theme = LineTheme {
            text_color: theme.foreground,
            cursor_color: theme.purple,
            link_color: theme.cyan,
            selection_color: theme.selection,
            border_color: theme.comment,
            code_color: theme.pink,
            fence_color: theme.comment,
            fence_lang_color: theme.green,
            text_font: font(&self.config.text_font),
            code_font: font(&self.config.code_font),
        };
        let cursor_offset = self.selection.head;
        let selection_range = if self.selection.is_collapsed() {
            None
        } else {
            Some(self.selection.range())
        };

        let buffer_text = self.buffer.text();

        let entity = cx.entity().clone();
        let on_click: ClickCallback =
            Rc::new(move |buffer_offset, shift_held, click_count, _window, cx| {
                entity.update(cx, |editor, cx| {
                    if editor.input_blocked {
                        return;
                    }
                    if shift_held {
                        editor.selection = editor.selection.extend_to(buffer_offset);
                    } else {
                        match click_count {
                            2 => {
                                editor.selection =
                                    Selection::select_word_at(buffer_offset, &editor.buffer);
                            }
                            3 => {
                                editor.selection =
                                    Selection::select_line_at(buffer_offset, &editor.buffer);
                            }
                            _ => {
                                editor.selection = Selection::new(buffer_offset, buffer_offset);
                            }
                        }
                    }
                    cx.notify();
                });
            });

        let entity = cx.entity().clone();
        let on_drag: DragCallback = Rc::new(move |buffer_offset, _window, cx| {
            entity.update(cx, |editor, cx| {
                if editor.input_blocked {
                    return;
                }
                editor.selection = editor.selection.extend_to(buffer_offset);
                cx.notify();
            });
        });

        let entity = cx.entity().clone();
        let on_checkbox: CheckboxCallback = Rc::new(move |line_number, _window, cx| {
            entity.update(cx, |editor, cx| {
                if editor.input_blocked {
                    return;
                }
                editor.toggle_checkbox(line_number, cx);
            });
        });

        let entity = cx.entity().clone();
        let on_hover: HoverCallback = Rc::new(
            move |hovering_checkbox, hovering_link_region, _window, cx| {
                entity.update(cx, |editor, cx| {
                    if editor.hovering_checkbox != hovering_checkbox
                        || editor.hovering_link_region != hovering_link_region
                    {
                        editor.hovering_checkbox = hovering_checkbox;
                        editor.hovering_link_region = hovering_link_region;
                        cx.notify();
                    }
                });
            },
        );

        let lines = self.buffer.lines().to_vec();
        let cursor_line = self.buffer.byte_to_line(cursor_offset);
        let cursor_child_index = Some(cursor_line + 1);

        let line_views: Vec<_> = lines
            .iter()
            .map(|line| {
                let inline_styles = extract_inline_styles(&self.buffer, line);

                let code_highlights: Vec<_> = self
                    .buffer
                    .code_highlights_for_range(line.range.clone())
                    .iter()
                    .map(|span| (span.clone(), theme.color_for_highlight(span.highlight_id)))
                    .collect();

                Line::new(
                    line,
                    &buffer_text,
                    cursor_offset,
                    inline_styles,
                    line_theme.clone(),
                    selection_range.clone(),
                    code_highlights,
                    self.config.base_path.clone(),
                )
                .on_click(on_click.clone())
                .on_drag(on_drag.clone())
                .on_checkbox(on_checkbox.clone())
                .on_hover(on_hover.clone())
            })
            .collect();

        let cursor_moved = self.cursor_child_index != cursor_child_index;
        self.cursor_child_index = cursor_child_index;

        if cursor_moved {
            self.request_scroll_to_cursor();
        }

        let margin = self.config.padding_y.to_pixels(window.rem_size());
        self.perform_pending_scroll(margin);

        let top_spacer = div().h(self.config.padding_y);
        let bottom_spacer = div().h(self.config.padding_y);

        div()
            .id("editor")
            .track_focus(&self.focus_handle)
            .key_context("Editor")
            .on_key_down(cx.listener(Self::on_key_down))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle)
            .px(self.config.padding_x)
            .font(line_theme.text_font.clone())
            .text_color(line_theme.text_color)
            .cursor(
                if self.hovering_checkbox || (self.hovering_link_region && self.ctrl_held) {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::IBeam
                },
            )
            .child(top_spacer)
            .children(line_views)
            .child(bottom_spacer)
    }
}
