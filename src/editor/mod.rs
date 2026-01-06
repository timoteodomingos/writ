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
use crate::marker::{LineMarkers, MarkerKind};
use crate::paste::{PasteContext, transform_paste};

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
/// Context about the line at the cursor, used by smart editing actions.
pub struct LineContext<'a> {
    /// Current cursor byte offset.
    pub cursor_offset: usize,
    /// Index of the current line.
    pub line_idx: usize,
    /// The current line's markers.
    pub line: &'a LineMarkers,
    /// Whether content after markers is empty (whitespace only).
    pub is_empty: bool,
    /// Whether this line has any container markers.
    pub has_container: bool,
    /// The previous line, if any.
    pub prev_line: Option<&'a LineMarkers>,
}

/// Core editing state that can be used without GPUI context.
/// This contains the buffer and selection, and all editing logic.
pub struct EditorState {
    pub buffer: Buffer,
    pub selection: Selection,
}

impl EditorState {
    pub fn new(content: &str) -> Self {
        let buffer: Buffer = content.parse().unwrap_or_default();
        Self {
            buffer,
            selection: Selection::new(0, 0),
        }
    }

    pub fn cursor(&self) -> Cursor {
        self.selection.cursor()
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Set cursor position by byte offset.
    pub fn set_cursor(&mut self, offset: usize) {
        let offset = offset.min(self.buffer.len_bytes());
        self.selection = Selection::new(offset, offset);
    }

    /// Move cursor left by one character.
    pub fn move_left(&mut self) {
        let new_cursor = self.cursor().move_left(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor right by one character.
    pub fn move_right(&mut self) {
        let new_cursor = self.cursor().move_right(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor up by one line.
    pub fn move_up(&mut self) {
        let new_cursor = self.cursor().move_up(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor down by one line.
    pub fn move_down(&mut self) {
        let new_cursor = self.cursor().move_down(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor to start of current line.
    pub fn move_to_line_start(&mut self) {
        let new_cursor = self.cursor().move_to_line_start(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Move cursor to end of current line.
    pub fn move_to_line_end(&mut self) {
        let new_cursor = self.cursor().move_to_line_end(&self.buffer);
        self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
    }

    /// Insert text at the current cursor position.
    pub fn insert_text(&mut self, text: &str) {
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

    fn find_line_at(&self, byte_pos: usize) -> Option<(usize, &LineMarkers)> {
        let idx = self.buffer.byte_to_line(byte_pos);
        self.buffer.lines().get(idx).map(|line| (idx, line))
    }

    /// Check if the cursor is inside a code block (between opening and closing fences,
    /// or after an opening fence with no closing fence yet).
    fn cursor_in_code_block(&self) -> bool {
        let lines = self.buffer.lines();
        let cursor_line = self.buffer.byte_to_line(self.cursor().offset);

        let mut i = 0;
        while i < lines.len() {
            // Check for opening fence
            let is_opening_fence = lines[i].markers.iter().any(|m| {
                matches!(
                    m.kind,
                    MarkerKind::CodeBlockFence {
                        is_opening: true,
                        ..
                    }
                )
            });

            if is_opening_fence {
                let start = i;
                i += 1;
                let mut found_close = false;
                while i < lines.len() {
                    // Check for closing fence
                    let is_closing_fence = lines[i].markers.iter().any(|m| {
                        matches!(
                            m.kind,
                            MarkerKind::CodeBlockFence {
                                is_opening: false,
                                ..
                            }
                        )
                    });
                    if is_closing_fence {
                        // Found a complete code block
                        if cursor_line > start && cursor_line < i {
                            return true;
                        }
                        i += 1;
                        found_close = true;
                        break;
                    }
                    i += 1;
                }
                // Incomplete code block (no closing fence) - cursor is inside if after start
                if !found_close && cursor_line > start {
                    return true;
                }
            } else {
                i += 1;
            }
        }
        false
    }

    /// Check if a line has content after its markers.
    /// Lines with code fences are always considered to have content.
    fn line_has_content(&self, line: &LineMarkers) -> bool {
        if line.is_fence() {
            return true;
        }
        let content_start = line
            .marker_range()
            .map(|r| r.end)
            .unwrap_or(line.range.start);
        !self
            .buffer
            .slice_cow(content_start..line.range.end)
            .trim()
            .is_empty()
    }

    /// Get context about the line at the cursor.
    /// Returns None if the cursor is not on a valid line.
    fn line_context(&self) -> Option<LineContext<'_>> {
        let cursor_offset = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_offset);
        let lines = self.buffer.lines();
        let line = lines.get(line_idx)?;

        let is_empty = !self.line_has_content(line);

        let prev_line = if line_idx > 0 {
            lines.get(line_idx - 1)
        } else {
            None
        };

        Some(LineContext {
            cursor_offset,
            line_idx,
            line,
            is_empty,
            has_container: line.has_container(),
            prev_line,
        })
    }

    /// Auto-insert space after `>` if it just became a blockquote marker.
    /// Returns true if a space was inserted.
    pub fn maybe_complete_blockquote_marker(&mut self) -> bool {
        let cursor_pos = self.cursor().offset;
        if cursor_pos == 0 {
            return false;
        }

        if self.buffer.byte_at(cursor_pos - 1) != Some(b'>') {
            return false;
        }

        if self.buffer.byte_at(cursor_pos) == Some(b' ') {
            return false;
        }

        let lines = self.buffer.lines();
        let line_idx = self.buffer.byte_to_line(cursor_pos);
        let Some(line) = lines.get(line_idx) else {
            return false;
        };

        let has_blockquote = line
            .markers
            .iter()
            .any(|m| matches!(m.kind, MarkerKind::BlockQuote));

        if !has_blockquote {
            return false;
        }

        self.insert_text(" ");
        true
    }

    /// Try to insert a space. Returns false if space should be ignored
    /// (at line start, or at blockquote content start outside code blocks).
    pub fn try_insert_space(&mut self) -> bool {
        if self.cursor_in_code_block() {
            self.insert_text(" ");
            return true;
        }

        let cursor = self.cursor();
        let line_start = cursor.move_to_line_start(&self.buffer).offset;

        // Ignore space at line start or at blockquote content start
        if cursor.offset == line_start || self.cursor_at_blockquote_content_start() {
            return false;
        }

        self.insert_text(" ");
        true
    }

    /// Check if cursor is at the content start of a blockquote-only line.
    /// Used to prevent inserting spaces/tabs at the "beginning" of blockquote content.
    fn cursor_at_blockquote_content_start(&self) -> bool {
        let cursor_pos = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_pos);
        let lines = self.buffer.lines();
        let Some(line) = lines.get(line_idx) else {
            return false;
        };

        // Only applies to blockquote-only lines (no lists)
        if !line.is_blockquote_only() {
            return false;
        }

        // Check if cursor is at the content start (right after marker)
        if let Some(marker_range) = line.marker_range() {
            cursor_pos == marker_range.end
        } else {
            false
        }
    }

    /// Smart tab: adds structure based on context.
    /// Tab: cycle forward through nesting states based on context (up to 2 lines above).
    /// States depend on context type:
    /// - List: empty → indent → indent+marker → empty
    /// - Blockquote: empty → marker → empty
    /// - Nested (> -): empty → outer → outer+inner → outer+indent → empty
    pub fn tab(&mut self) {
        let Some((states, current_idx)) = self.tab_cycle_state() else {
            return;
        };

        if states.len() <= 1 {
            return;
        }

        let next_idx = (current_idx + 1) % states.len();
        self.set_line_prefix(&states[next_idx]);
    }

    /// Shift+Tab: cycle backward through nesting states.
    fn shift_tab_cycle(&mut self) {
        let Some((states, current_idx)) = self.tab_cycle_state() else {
            return;
        };

        if states.len() <= 1 {
            return;
        }

        let prev_idx = if current_idx == 0 {
            states.len() - 1
        } else {
            current_idx - 1
        };
        self.set_line_prefix(&states[prev_idx]);
    }

    /// Determine the tab cycle states and current position based on context.
    /// Returns (states, current_index) or None if no context.
    fn tab_cycle_state(&self) -> Option<(Vec<String>, usize)> {
        let ctx = self.line_context()?;

        // Find context line with at most 1 blank line between (0 or 1 blank lines allowed)
        let context_line = self.find_context_line(ctx.line_idx, 1)?;

        // Get the current line's prefix (everything before content)
        let current_prefix = self.current_line_prefix(ctx.line);

        // Build states based on context
        let states = self.build_cycle_states(&context_line);

        // Find current state in the cycle
        let current_idx = states
            .iter()
            .position(|s| s == &current_prefix)
            .unwrap_or(0);

        Some((states, current_idx))
    }

    /// Find the context line (with markers) within max_lines above current.
    fn find_context_line(&self, line_idx: usize, max_lines: usize) -> Option<LineMarkers> {
        if line_idx == 0 {
            return None;
        }

        let lines = self.buffer.lines();
        let mut blank_count = 0;

        for i in (0..line_idx).rev() {
            let line = &lines[i];
            let text = self.buffer.slice_cow(line.range.clone());

            if !line.markers.is_empty() {
                // Found a container line
                if blank_count <= max_lines {
                    return Some(line.clone());
                } else {
                    return None;
                }
            }

            if text.trim().is_empty() {
                blank_count += 1;
                if blank_count > max_lines {
                    return None;
                }
            } else {
                // Non-empty non-container line breaks the search
                return None;
            }
        }
        None
    }

    /// Get the current prefix of a line (markers/indent before content).
    fn current_line_prefix(&self, line: &LineMarkers) -> String {
        if line.markers.is_empty() {
            String::new()
        } else {
            // Markers are innermost-first, so first() has the latest end position
            let prefix_end = line.markers.first().unwrap().range.end;
            self.buffer
                .slice_cow(line.range.start..prefix_end)
                .into_owned()
        }
    }

    /// Build the cycle states based on context line type.
    fn build_cycle_states(&self, context: &LineMarkers) -> Vec<String> {
        let mut states = vec![String::new()]; // Always start with empty

        let has_list = context.markers.iter().any(|m| {
            matches!(
                m.kind,
                MarkerKind::ListItem { .. } | MarkerKind::TaskList { .. }
            )
        });
        let has_blockquote = context
            .markers
            .iter()
            .any(|m| matches!(m.kind, MarkerKind::BlockQuote));

        if has_blockquote && has_list {
            // Nested context (e.g., "> - item")
            // States: empty → "> " → "> - " → ">   " → empty
            let outer = self.blockquote_prefix(context);
            let full = context.continuation_rope(self.buffer.rope());
            let indent = context.nested_paragraph_indent(self.buffer.rope());

            states.push(outer);
            if full != states.last().unwrap().as_str() {
                states.push(full);
            }
            if !indent.is_empty() && indent != states.last().unwrap().as_str() {
                states.push(indent);
            }
        } else if has_list {
            // List context: empty → "  " → "  - " → empty
            // nested_paragraph_indent gives us the indent ("  ")
            // For the full nested marker, we need indent + list marker
            let indent = context.nested_paragraph_indent(self.buffer.rope());
            let list_marker = context.continuation_rope(self.buffer.rope());
            let mut full = indent.clone();
            full.push_str(&list_marker);

            states.push(indent);
            states.push(full);
        } else if has_blockquote {
            // Blockquote only: empty → "> " → empty
            let marker = context.continuation_rope(self.buffer.rope());
            if !marker.is_empty() {
                states.push(marker);
            }
        }

        states
    }

    /// Extract the blockquote prefix from a nested context.
    fn blockquote_prefix(&self, context: &LineMarkers) -> String {
        let mut result = String::new();
        for marker in &context.markers {
            if matches!(marker.kind, MarkerKind::BlockQuote) {
                result.push_str(&self.buffer.slice_cow(marker.range.clone()));
            }
        }
        result
    }

    /// Set the line prefix, replacing current markers.
    fn set_line_prefix(&mut self, new_prefix: &str) {
        let Some(ctx) = self.line_context() else {
            return;
        };

        let line_start = ctx.line.range.start;
        let current_prefix_end = if ctx.line.markers.is_empty() {
            line_start
        } else {
            // Markers are innermost-first, so first() has the latest end position
            ctx.line.markers.first().unwrap().range.end
        };

        // Delete current prefix and insert new one
        if current_prefix_end > line_start {
            self.buffer
                .delete(line_start..current_prefix_end, self.cursor().offset);
        }

        if !new_prefix.is_empty() {
            self.buffer.insert(line_start, new_prefix, line_start);
        }

        // Update cursor position
        let new_cursor = line_start + new_prefix.len();
        self.selection = Selection::new(new_cursor, new_cursor);
    }

    /// Smart enter: creates paragraph break or exits container on empty line.
    /// Enter: just insert a raw newline. No magic.
    pub fn enter(&mut self) {
        self.insert_text("\n");
    }

    /// Shift+Enter: continue container (add markers from current line).
    pub fn shift_enter(&mut self) {
        let Some(ctx) = self.line_context() else {
            self.insert_text("\n");
            return;
        };

        // Get the continuation (all markers) for this line
        let continuation = ctx.line.continuation_rope(self.buffer.rope());
        self.insert_text("\n");
        if !continuation.is_empty() {
            self.insert_text(&continuation);
        }
    }

    /// Shift+Alt+Enter: create indented continuation (for nested paragraphs).
    /// For lists: newline + indent (no list marker)
    /// For blockquotes alone: newline + indent (exits blockquote)
    /// For nested (e.g. `> - item`): newline + outer markers + indent
    pub fn shift_alt_enter(&mut self) {
        let indent = {
            let Some(ctx) = self.line_context() else {
                self.insert_text("\n");
                return;
            };

            // Check if line has only blockquote markers (no list markers)
            let has_list = ctx.line.markers.iter().any(|m| {
                matches!(
                    m.kind,
                    MarkerKind::ListItem { .. } | MarkerKind::TaskList { .. }
                )
            });
            let has_blockquote = ctx
                .line
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::BlockQuote));

            if has_blockquote && !has_list {
                // Pure blockquote: exit with just indent
                "  ".to_string()
            } else {
                // Lists or nested: use nested_paragraph_indent which keeps outer containers
                ctx.line.nested_paragraph_indent(self.buffer.rope())
            }
        };

        self.insert_text("\n");
        if !indent.is_empty() {
            self.insert_text(&indent);
        }
    }

    /// Shift+Tab: cycle backward through nesting states.
    pub fn shift_tab(&mut self) {
        self.shift_tab_cycle();
    }

    fn backspace_range_with_type(
        &self,
        cursor_pos: usize,
    ) -> Option<(std::ops::Range<usize>, bool)> {
        let (_, line) = self.find_line_at(cursor_pos)?;

        for marker in &line.markers {
            if cursor_pos == marker.range.end {
                let is_indent = matches!(marker.kind, MarkerKind::Indent);
                return Some((marker.range.clone(), is_indent));
            }
        }

        None
    }

    /// Delete backward (backspace). Simple: delete one unit.
    /// Markers and indents are atomic - deleted as a whole.
    pub fn delete_backward(&mut self) {
        if !self.selection.is_collapsed() {
            self.delete_selection();
            return;
        }

        if self.cursor().offset == 0 {
            return;
        }

        let cursor_pos = self.cursor().offset;

        // Check if we're at a marker position - if so, delete the marker atomically
        if let Some((marker_range, _is_indent)) = self.backspace_range_with_type(cursor_pos) {
            self.buffer.delete(marker_range.clone(), cursor_pos);
            self.selection = Selection::new(marker_range.start, marker_range.start);
            return;
        }

        // Otherwise, just delete one character (including newlines)
        // Use move_left to handle atomic marker jumping for cursor
        let new_pos = cursor_pos - 1;
        self.buffer.delete(new_pos..cursor_pos, cursor_pos);
        self.selection = Selection::new(new_pos, new_pos);
    }

    fn delete_selection(&mut self) {
        let range = self.selection.range();
        let cursor_before = self.cursor().offset;
        self.buffer.delete(range.clone(), cursor_before);
        self.selection = Selection::new(range.start, range.start);
    }

    /// Delete the character after the cursor, or the selection if active.
    pub fn delete_forward(&mut self) {
        if !self.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset < self.buffer.len_bytes() {
            let cursor_before = self.cursor().offset;
            let next = self.cursor().move_right(&self.buffer);
            self.buffer
                .delete(cursor_before..next.offset, cursor_before);
        }
    }
}

pub struct Editor {
    state: EditorState,
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
        let focus_handle = cx.focus_handle();
        let scroll_handle = ScrollHandle::new();

        Self {
            state: EditorState::new(content),
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
        self.state.buffer.text()
    }

    /// Returns the length of the buffer in bytes.
    pub fn len(&self) -> usize {
        self.state.buffer.len_bytes()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.state.buffer.len_bytes() == 0
    }

    /// Replace the entire buffer contents, resetting cursor to the start.
    pub fn set_text(&mut self, content: &str, cx: &mut Context<Self>) {
        self.state.buffer = content.parse().unwrap_or_default();
        self.state.selection = Selection::new(0, 0);
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
        let end = self.state.buffer.len_bytes();
        self.state.buffer.insert(end, text, end);
        let new_end = self.state.buffer.len_bytes();
        self.state.selection = Selection::new(new_end, new_end);
        cx.notify();
    }

    /// Append text and scroll to keep the cursor visible.
    pub fn append_and_scroll(&mut self, text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.append(text, cx);
        self.scroll_handle.scroll_to_bottom();
    }

    fn cursor(&self) -> Cursor {
        self.state.selection.cursor()
    }

    fn move_cursor(&mut self, new_cursor: Cursor, extend: bool) {
        if extend {
            self.state.selection = self.state.selection.extend_to(new_cursor.offset);
        } else {
            self.state.selection = Selection::new(new_cursor.offset, new_cursor.offset);
        }
    }

    fn perform_pending_scroll(&mut self) {
        if !self.scroll_to_cursor_pending {
            return;
        }

        let Some(child_ix) = self.cursor_child_index else {
            return;
        };

        // Use scroll_to_item which defers scrolling to prepaint when bounds are known
        self.scroll_handle.scroll_to_item(child_ix);
        self.scroll_to_cursor_pending = false;
    }

    fn request_scroll_to_cursor(&mut self) {
        self.scroll_to_cursor_pending = true;
    }

    fn tab(&mut self) {
        self.state.tab();
    }

    fn shift_tab(&mut self) {
        self.state.shift_tab();
    }

    fn toggle_checkbox(&mut self, line_number: usize, cx: &mut Context<Self>) {
        let lines = self.state.buffer.lines();
        let Some(line) = lines.get(line_number) else {
            return;
        };

        let Some(is_checked) = line.checkbox() else {
            return;
        };

        let line_text = self.state.buffer.slice_cow(line.range.clone());
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

        self.state.buffer.replace(
            checkbox_content_start..checkbox_content_end,
            new_content,
            cursor_before,
        );

        self.state.selection = Selection::new(cursor_before, cursor_before);

        cx.notify();
    }

    fn insert_text(&mut self, text: &str) {
        self.state.insert_text(text);
    }

    fn delete_backward(&mut self) {
        self.state.delete_backward();
    }

    fn delete_forward(&mut self) {
        self.state.delete_forward();
    }

    fn move_in_direction(&mut self, direction: Direction, extend: bool) {
        let new_cursor = match direction {
            Direction::Left => self.cursor().move_left(&self.state.buffer),
            Direction::Right => self.cursor().move_right(&self.state.buffer),
            Direction::Up => self.cursor().move_up(&self.state.buffer),
            Direction::Down => self.cursor().move_down(&self.state.buffer),
        };
        self.move_cursor(new_cursor, extend);
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
                    self.cursor().move_to_line_start(&self.state.buffer)
                };
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "end" => {
                let new_cursor = if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor().move_to_end(&self.state.buffer)
                } else {
                    self.cursor().move_to_line_end(&self.state.buffer)
                };
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "enter" => {
                if keystroke.modifiers.shift && keystroke.modifiers.alt {
                    // Shift+Alt+Enter: indented continuation (nested paragraph)
                    self.state.shift_alt_enter();
                } else if keystroke.modifiers.shift {
                    // Shift+Enter: continue container (add markers)
                    self.state.shift_enter();
                } else {
                    // Enter: raw newline
                    self.state.enter();
                }
                cx.notify();
            }
            "tab" => {
                if self.state.cursor_in_code_block() {
                    self.insert_text("    ");
                } else if keystroke.modifiers.shift {
                    self.shift_tab();
                } else {
                    self.tab();
                }
                cx.notify();
            }
            "a" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                self.state.selection = Selection::select_all(&self.state.buffer);
                cx.notify();
            }
            "c" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if !self.state.selection.is_collapsed() {
                    let range = self.state.selection.range();
                    let text = self.state.buffer.slice_cow(range).into_owned();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
            }
            "x" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if !self.state.selection.is_collapsed() {
                    let range = self.state.selection.range();
                    let text = self.state.buffer.slice_cow(range).into_owned();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.state.delete_selection();
                    cx.notify();
                }
            }
            "v" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if let Some(clipboard_item) = cx.read_from_clipboard()
                    && let Some(text) = clipboard_item.text()
                {
                    // Context-aware paste: transform content based on cursor position
                    let ctx =
                        PasteContext::from_buffer(&self.state.buffer, self.state.cursor().offset);
                    let transformed = transform_paste(&text, &ctx);
                    self.insert_text(&transformed);
                    cx.notify();
                }
            }
            "z" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if keystroke.modifiers.shift {
                    if let Some(cursor_pos) = self.state.buffer.redo() {
                        self.state.selection = Selection::new(cursor_pos, cursor_pos);
                        cx.notify();
                    }
                } else if let Some(cursor_pos) = self.state.buffer.undo() {
                    self.state.selection = Selection::new(cursor_pos, cursor_pos);
                    cx.notify();
                }
            }
            "y" if keystroke.modifiers.control => {
                if let Some(cursor_pos) = self.state.buffer.redo() {
                    self.state.selection = Selection::new(cursor_pos, cursor_pos);
                    cx.notify();
                }
            }
            "s" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                self.save(cx);
            }

            _ => {
                if let Some(key_char) = &keystroke.key_char {
                    if key_char == " " {
                        if !self.state.try_insert_space() {
                            return;
                        }
                    } else {
                        self.insert_text(key_char);
                    }

                    // Auto-insert space after blockquote marker if needed
                    if key_char == ">" {
                        self.state.maybe_complete_blockquote_marker();
                    }

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
        let end = self.state.buffer.len_bytes();
        self.state.selection = Selection::new(end, end);
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
        self.state.selection.head
    }

    /// Returns the current selection range, or None if the cursor is collapsed.
    pub fn selection_range(&self) -> Option<std::ops::Range<usize>> {
        if self.state.selection.is_collapsed() {
            None
        } else {
            Some(self.state.selection.range())
        }
    }

    /// Set the cursor position to the given byte offset.
    pub fn set_cursor(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.state.buffer.len_bytes());
        self.state.selection = Selection::new(offset, offset);
        cx.notify();
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.state.buffer.len_bytes();
        self.state.selection = Selection::new(end, end);
        cx.notify();
    }

    /// Move the cursor to the start of the buffer.
    pub fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.state.selection = Selection::new(0, 0);
        cx.notify();
    }

    /// Returns true if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.state.buffer.is_dirty()
    }

    /// Mark the buffer as clean (no unsaved changes).
    pub fn mark_clean(&mut self) {
        self.state.buffer.mark_clean();
    }

    /// Save the buffer to the file specified in FileInfo.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let file_info = FileInfo::global(cx);
        let path = file_info.path.clone();
        let content = self.state.buffer.text();

        if let Err(e) = std::fs::write(&path, &content) {
            eprintln!("Failed to save file: {}", e);
            return;
        }

        self.state.buffer.mark_clean();
        cx.notify();
    }

    /// Returns true if there are actions to undo.
    pub fn can_undo(&self) -> bool {
        self.state.buffer.can_undo()
    }

    /// Returns true if there are actions to redo.
    pub fn can_redo(&self) -> bool {
        self.state.buffer.can_redo()
    }

    /// Undo the last action.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.state.buffer.undo() {
            self.state.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.state.buffer.redo() {
            self.state.selection = Selection::new(cursor_pos, cursor_pos);
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
                self.state.enter();
            }
            EditorAction::ShiftEnter => {
                self.state.shift_enter();
            }
            EditorAction::ShiftAltEnter => {
                self.state.shift_alt_enter();
            }
            EditorAction::Tab => {
                self.tab();
            }
            EditorAction::ShiftTab => {
                self.shift_tab();
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
        if file_info.dirty != self.state.buffer.is_dirty() {
            cx.set_global(FileInfo {
                path: file_info.path.clone(),
                dirty: self.state.buffer.is_dirty(),
            });
        }

        let theme = self.config.theme.clone();
        let code_font = font(&self.config.code_font);

        // Measure the width of a monospace character for precise indent padding.
        // We shape a single space character and get its width.
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let measure_run = gpui::TextRun {
            len: 1,
            font: code_font.clone(),
            color: gpui::transparent_black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(" ".into(), font_size, &[measure_run], None);
        let monospace_char_width = shaped.width;

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
            code_font,
            monospace_char_width,
        };
        let cursor_offset = self.state.selection.head;
        let selection_range = if self.state.selection.is_collapsed() {
            None
        } else {
            Some(self.state.selection.range())
        };

        let entity = cx.entity().clone();
        let on_click: ClickCallback =
            Rc::new(move |buffer_offset, shift_held, click_count, _window, cx| {
                entity.update(cx, |editor, cx| {
                    if editor.input_blocked {
                        return;
                    }
                    if shift_held {
                        editor.state.selection = editor.state.selection.extend_to(buffer_offset);
                    } else {
                        match click_count {
                            2 => {
                                editor.state.selection =
                                    Selection::select_word_at(buffer_offset, &editor.state.buffer);
                            }
                            3 => {
                                editor.state.selection =
                                    Selection::select_line_at(buffer_offset, &editor.state.buffer);
                            }
                            _ => {
                                editor.state.selection =
                                    Selection::new(buffer_offset, buffer_offset);
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
                editor.state.selection = editor.state.selection.extend_to(buffer_offset);
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

        let lines = self.state.buffer.lines().to_vec();
        let cursor_line = self.state.buffer.byte_to_line(cursor_offset);

        // Determine which lines should be collapsed (hidden empty lines between list items)
        let is_collapsed: Vec<bool> = lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                // Never collapse if cursor is on this line or selection includes it
                if idx == cursor_line {
                    return false;
                }
                if let Some(ref range) = selection_range
                    && line.range.start < range.end
                    && line.range.end > range.start
                {
                    return false;
                }
                // Collapse single empty line between sibling list items
                if !self.state.buffer.is_line_empty(idx) || idx == 0 || idx >= lines.len() - 1 {
                    return false;
                }
                let prev = &lines[idx - 1];
                let next = &lines[idx + 1];
                // Both adjacent lines must have list markers
                if !prev.has_list_marker() || !next.has_list_marker() {
                    return false;
                }
                // They must be related (siblings or parent-child) in the list structure
                self.state.buffer.are_related_list_items(idx - 1, idx + 1)
            })
            .collect();

        // Compute cursor_child_index accounting for collapsed lines
        // +1 for the top spacer, then count non-collapsed lines before cursor
        let cursor_line_clamped = cursor_line.min(is_collapsed.len());
        let collapsed_before_cursor = is_collapsed[..cursor_line_clamped]
            .iter()
            .filter(|&&c| c)
            .count();
        let cursor_child_index = Some(cursor_line_clamped - collapsed_before_cursor + 1);

        // Pre-compute inline styles and code highlights for each line
        let line_data: Vec<_> = lines
            .iter()
            .map(|line| {
                let inline_styles = self.state.buffer.inline_styles_for_range(&line.range);
                let code_highlights: Vec<_> = self
                    .state
                    .buffer
                    .code_highlights_for_range(line.range.clone())
                    .iter()
                    .map(|span| (span.clone(), theme.color_for_highlight(span.highlight_id)))
                    .collect();
                (inline_styles, code_highlights)
            })
            .collect();

        // Clone the rope once - Rope::clone() is O(1) due to internal Arc sharing
        let rope = self.state.buffer.rope().clone();
        let line_views: Vec<_> = lines
            .iter()
            .zip(line_data)
            .zip(is_collapsed.iter())
            .filter(|&(_, &collapsed)| !collapsed)
            .map(|((line, (inline_styles, code_highlights)), _)| {
                Line::new(
                    line,
                    rope.clone(),
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

        self.perform_pending_scroll();

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Trim leading newline from raw string literals for readability.
    /// Allows writing:
    /// ```
    /// r#"
    /// - item one
    /// - item two
    /// "#
    /// ```
    fn trim_raw(s: &str) -> &str {
        s.strip_prefix('\n').unwrap_or(s)
    }

    /// Helper to create an EditorState with cursor at a specific position.
    /// The cursor position is indicated by | in the input string.
    fn editor_with_cursor(input: &str) -> EditorState {
        let input = trim_raw(input);
        let cursor_pos = input
            .find('|')
            .expect("Input must contain | for cursor position");
        let content = input.replace('|', "");
        let mut state = EditorState::new(&content);
        state.set_cursor(cursor_pos);
        state
    }

    /// Helper to check editor state matches expected content with cursor.
    fn assert_editor_eq(state: &EditorState, expected: &str) {
        let expected = trim_raw(expected);
        let text = state.text();
        let cursor = state.cursor().offset;
        let mut actual = String::new();
        actual.push_str(&text[..cursor]);
        actual.push('|');
        actual.push_str(&text[cursor..]);
        assert_eq!(actual, expected);
    }

    mod cursor_movement_tests {
        use super::*;

        #[test]
        fn move_left() {
            let mut state = editor_with_cursor("hel|lo");
            state.move_left();
            assert_editor_eq(&state, "he|llo");
        }

        #[test]
        fn move_left_at_start() {
            let mut state = editor_with_cursor("|hello");
            state.move_left();
            assert_editor_eq(&state, "|hello");
        }

        #[test]
        fn move_right() {
            let mut state = editor_with_cursor("he|llo");
            state.move_right();
            assert_editor_eq(&state, "hel|lo");
        }

        #[test]
        fn move_right_at_end() {
            let mut state = editor_with_cursor("hello|");
            state.move_right();
            assert_editor_eq(&state, "hello|");
        }

        #[test]
        fn move_up() {
            let mut state = editor_with_cursor("line one\nline |two\nline three");
            state.move_up();
            assert_editor_eq(&state, "line |one\nline two\nline three");
        }

        #[test]
        fn move_up_from_first_line() {
            let mut state = editor_with_cursor("hel|lo\nworld");
            state.move_up();
            assert_editor_eq(&state, "|hello\nworld");
        }

        #[test]
        fn move_down() {
            let mut state = editor_with_cursor("line |one\nline two\nline three");
            state.move_down();
            assert_editor_eq(&state, "line one\nline |two\nline three");
        }

        #[test]
        fn move_down_from_last_line() {
            let mut state = editor_with_cursor("hello\nwor|ld");
            state.move_down();
            assert_editor_eq(&state, "hello\nworld|");
        }

        #[test]
        fn move_up_preserves_column() {
            let mut state = editor_with_cursor("short\nlonger line|");
            state.move_up();
            assert_editor_eq(&state, "short|\nlonger line");
        }

        #[test]
        fn move_to_line_start() {
            let mut state = editor_with_cursor("hello\nwor|ld");
            state.move_to_line_start();
            assert_editor_eq(&state, "hello\n|world");
        }

        #[test]
        fn move_to_line_end() {
            let mut state = editor_with_cursor("hello\nwor|ld");
            state.move_to_line_end();
            assert_editor_eq(&state, "hello\nworld|");
        }
    }

    // ========================================================================
    // New "raw markdown" behavior tests
    // These test the simplified, non-controlling editing paradigm.
    // ========================================================================

    mod raw_enter_tests {
        use super::*;

        // --- Enter: always raw \n ---

        #[test]
        fn enter_on_paragraph_inserts_newline() {
            let mut state = editor_with_cursor("Hello world|");
            state.enter();
            assert_editor_eq(&state, "Hello world\n|");
        }

        #[test]
        fn enter_on_heading_inserts_newline() {
            let mut state = editor_with_cursor("# Hello|");
            state.enter();
            assert_editor_eq(&state, "# Hello\n|");
        }

        #[test]
        fn enter_on_list_item_inserts_newline_no_marker() {
            let mut state = editor_with_cursor("- item one|");
            state.enter();
            assert_editor_eq(&state, "- item one\n|");
        }

        #[test]
        fn enter_on_blockquote_inserts_newline_no_marker() {
            let mut state = editor_with_cursor("> quote|");
            state.enter();
            assert_editor_eq(&state, "> quote\n|");
        }

        #[test]
        fn enter_on_nested_container_inserts_newline_no_markers() {
            let mut state = editor_with_cursor("> - item|");
            state.enter();
            assert_editor_eq(&state, "> - item\n|");
        }

        #[test]
        fn enter_on_empty_list_item_inserts_newline_keeps_marker() {
            let mut state = editor_with_cursor("- item one\n- |");
            state.enter();
            assert_editor_eq(&state, "- item one\n- \n|");
        }

        #[test]
        fn enter_on_empty_blockquote_inserts_newline_keeps_marker() {
            let mut state = editor_with_cursor("> quote one\n> |");
            state.enter();
            assert_editor_eq(&state, "> quote one\n> \n|");
        }

        #[test]
        fn enter_in_code_block_inserts_newline() {
            let mut state = editor_with_cursor("```rust\nlet x = 1;|");
            state.enter();
            assert_editor_eq(&state, "```rust\nlet x = 1;\n|");
        }

        #[test]
        fn enter_on_code_fence_inserts_newline() {
            let mut state = editor_with_cursor("```rust|");
            state.enter();
            assert_editor_eq(&state, "```rust\n|");
        }

        #[test]
        fn enter_preserves_soft_wrap_style() {
            // Adjacent lines without blank line between them
            let mut state = editor_with_cursor("First sentence.\nSecond sentence.|");
            state.enter();
            assert_editor_eq(&state, "First sentence.\nSecond sentence.\n|");
        }

        // --- Shift+Enter: continue container ---

        #[test]
        fn shift_enter_on_list_item_continues_list() {
            let mut state = editor_with_cursor("- item one|");
            state.shift_enter();
            assert_editor_eq(&state, "- item one\n- |");
        }

        #[test]
        fn shift_enter_on_blockquote_continues_blockquote() {
            let mut state = editor_with_cursor("> quote|");
            state.shift_enter();
            assert_editor_eq(&state, "> quote\n> |");
        }

        #[test]
        fn shift_enter_on_nested_container_continues_all() {
            let mut state = editor_with_cursor("> - item|");
            state.shift_enter();
            assert_editor_eq(&state, "> - item\n> - |");
        }

        #[test]
        fn shift_enter_on_paragraph_just_inserts_newline() {
            let mut state = editor_with_cursor("Hello world|");
            state.shift_enter();
            assert_editor_eq(&state, "Hello world\n|");
        }

        #[test]
        fn shift_enter_on_heading_just_inserts_newline() {
            let mut state = editor_with_cursor("# Hello|");
            state.shift_enter();
            assert_editor_eq(&state, "# Hello\n|");
        }

        // --- Shift+Alt+Enter: indented continuation ---

        #[test]
        fn shift_alt_enter_on_list_item_creates_indent() {
            let mut state = editor_with_cursor("- item one|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "- item one\n  |");
        }

        #[test]
        fn shift_alt_enter_on_blockquote_creates_indent_outside() {
            let mut state = editor_with_cursor("> quote|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "> quote\n  |");
        }

        #[test]
        fn shift_alt_enter_on_nested_container_creates_indent_inside() {
            let mut state = editor_with_cursor("> - item|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "> - item\n>   |");
        }

        #[test]
        fn shift_alt_enter_on_paragraph_just_inserts_newline() {
            let mut state = editor_with_cursor("Hello world|");
            state.shift_alt_enter();
            assert_editor_eq(&state, "Hello world\n|");
        }
    }

    mod raw_backspace_tests {
        use super::*;

        #[test]
        fn backspace_deletes_char() {
            let mut state = editor_with_cursor("hello|");
            state.delete_backward();
            assert_editor_eq(&state, "hell|");
        }

        #[test]
        fn backspace_at_line_start_joins_lines() {
            let mut state = editor_with_cursor("line one\n|line two");
            state.delete_backward();
            assert_editor_eq(&state, "line one|line two");
        }

        #[test]
        fn backspace_deletes_entire_list_marker() {
            let mut state = editor_with_cursor("- |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }

        #[test]
        fn backspace_deletes_innermost_marker_first() {
            let mut state = editor_with_cursor("> - |");
            state.delete_backward();
            assert_editor_eq(&state, "> |");
        }

        #[test]
        fn backspace_then_deletes_outer_marker() {
            let mut state = editor_with_cursor("> |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }

        #[test]
        fn backspace_deletes_entire_indent() {
            // Indent after list item is atomic - need context for tree-sitter to recognize it
            let mut state = editor_with_cursor("- item\n  |text");
            state.delete_backward();
            assert_editor_eq(&state, "- item\n|text");
        }

        #[test]
        fn backspace_in_middle_of_text_deletes_char() {
            let mut state = editor_with_cursor("- item o|ne");
            state.delete_backward();
            assert_editor_eq(&state, "- item |ne");
        }

        #[test]
        fn backspace_on_empty_line_after_list_joins() {
            let mut state = editor_with_cursor("- item one\n|");
            state.delete_backward();
            assert_editor_eq(&state, "- item one|");
        }

        #[test]
        fn backspace_sequence_through_markers_and_join() {
            // Start: "- item one\n- |"
            // Backspace 1: delete "- " marker -> "- item one\n|"
            // Backspace 2: join lines -> "- item one|"
            let mut state = editor_with_cursor("- item one\n- |");
            state.delete_backward();
            assert_editor_eq(&state, "- item one\n|");
            state.delete_backward();
            assert_editor_eq(&state, "- item one|");
        }

        #[test]
        fn backspace_with_content_after_cursor_deletes_marker() {
            let mut state = editor_with_cursor("- |two");
            state.delete_backward();
            assert_editor_eq(&state, "|two");
        }
    }

    mod raw_tab_tests {
        use super::*;

        // --- Tab cycling through states ---

        #[test]
        fn tab_on_empty_line_after_list_adds_indent() {
            let mut state = editor_with_cursor("- item\n|");
            state.tab();
            assert_editor_eq(&state, "- item\n  |");
        }

        #[test]
        fn tab_twice_after_list_adds_marker() {
            let mut state = editor_with_cursor("- item\n|");
            state.tab();
            state.tab();
            assert_editor_eq(&state, "- item\n  - |");
        }

        #[test]
        fn tab_three_times_after_list_cycles_back() {
            let mut state = editor_with_cursor("- item\n|");
            state.tab();
            state.tab();
            state.tab();
            assert_editor_eq(&state, "- item\n|");
        }

        #[test]
        fn tab_with_blank_line_between_still_works() {
            let mut state = editor_with_cursor("- item\n\n|");
            state.tab();
            assert_editor_eq(&state, "- item\n\n  |");
        }

        #[test]
        fn tab_with_two_blank_lines_does_nothing() {
            let mut state = editor_with_cursor("- item\n\n\n|");
            state.tab();
            assert_editor_eq(&state, "- item\n\n\n|");
        }

        #[test]
        fn tab_on_blockquote_context_adds_marker() {
            let mut state = editor_with_cursor("> quote\n|");
            state.tab();
            assert_editor_eq(&state, "> quote\n> |");
        }

        #[test]
        fn tab_twice_on_blockquote_context_cycles_back() {
            let mut state = editor_with_cursor("> quote\n|");
            state.tab();
            state.tab();
            assert_editor_eq(&state, "> quote\n|");
        }

        #[test]
        fn tab_on_nested_context_cycles_through_all_states() {
            let mut state = editor_with_cursor("> - item\n|");

            // Tab 1: add outer marker
            state.tab();
            assert_editor_eq(&state, "> - item\n> |");

            // Tab 2: add inner marker
            state.tab();
            assert_editor_eq(&state, "> - item\n> - |");

            // Tab 3: indent (no inner marker)
            state.tab();
            assert_editor_eq(&state, "> - item\n>   |");

            // Tab 4: cycle back to start
            state.tab();
            assert_editor_eq(&state, "> - item\n|");
        }

        // --- Shift+Tab cycling backwards ---

        #[test]
        fn shift_tab_cycles_backwards() {
            let mut state = editor_with_cursor("- item\n|");
            state.shift_tab();
            // Should go backwards: empty -> marker+indent -> indent -> empty
            // First shift_tab goes to "  - |"
            assert_editor_eq(&state, "- item\n  - |");
        }

        #[test]
        fn shift_tab_from_indent_removes_indent() {
            let mut state = editor_with_cursor("- item\n  |");
            state.shift_tab();
            assert_editor_eq(&state, "- item\n|");
        }

        #[test]
        fn shift_tab_from_nested_marker_goes_to_indent() {
            let mut state = editor_with_cursor("- item\n  - |");
            state.shift_tab();
            assert_editor_eq(&state, "- item\n  |");
        }
    }

    mod raw_cursor_movement_tests {
        use super::*;

        #[test]
        fn move_left_through_marker_is_atomic() {
            let mut state = editor_with_cursor("- |item");
            state.move_left();
            assert_editor_eq(&state, "|- item");
        }

        #[test]
        fn move_right_through_marker_is_atomic() {
            let mut state = editor_with_cursor("|- item");
            state.move_right();
            assert_editor_eq(&state, "- |item");
        }

        #[test]
        fn move_left_through_nested_markers_one_at_a_time() {
            let mut state = editor_with_cursor("> - |item");
            state.move_left();
            assert_editor_eq(&state, "> |- item");
            state.move_left();
            assert_editor_eq(&state, "|> - item");
        }

        #[test]
        fn move_left_does_not_skip_blank_lines() {
            let mut state = editor_with_cursor("line one\n\n|line three");
            state.move_left();
            assert_editor_eq(&state, "line one\n|\nline three");
        }

        #[test]
        fn move_left_from_blank_line_goes_to_previous() {
            let mut state = editor_with_cursor("line one\n|\nline three");
            state.move_left();
            assert_editor_eq(&state, "line one|\n\nline three");
        }

        #[test]
        fn move_up_maintains_column_in_content_area() {
            let mut state = editor_with_cursor("- item one\n- item |two");
            state.move_up();
            assert_editor_eq(&state, "- item |one\n- item two");
        }
    }
}
