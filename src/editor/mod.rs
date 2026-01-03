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

    /// Delete a range and adjust cursor position accordingly.
    /// If deleting before the cursor, the cursor shifts back.
    /// If deleting at or after the cursor, the cursor moves to the start of deleted range.
    fn delete_and_adjust(&mut self, range: std::ops::Range<usize>) {
        let cursor_offset = self.cursor().offset;
        self.buffer.delete(range.clone(), cursor_offset);
        let deleted_len = range.end - range.start;
        let new_pos = if range.end <= cursor_offset {
            cursor_offset - deleted_len
        } else {
            range.start
        };
        self.selection = Selection::new(new_pos, new_pos);
    }

    /// Insert text at a position and adjust cursor accordingly.
    /// If inserting before the cursor, the cursor shifts forward.
    /// If inserting at or after the cursor, the cursor moves to end of inserted text.
    fn insert_at(&mut self, pos: usize, text: &str) {
        let cursor_offset = self.cursor().offset;
        self.buffer.insert(pos, text, cursor_offset);
        let new_pos = if pos <= cursor_offset {
            cursor_offset + text.len()
        } else {
            pos + text.len()
        };
        self.selection = Selection::new(new_pos, new_pos);
    }

    /// Get context about the line at the cursor.
    /// Returns None if the cursor is not on a valid line.
    fn line_context(&self) -> Option<LineContext<'_>> {
        let cursor_offset = self.cursor().offset;
        let line_idx = self.buffer.byte_to_line(cursor_offset);
        let lines = self.buffer.lines();
        let line = lines.get(line_idx)?;

        let content_start = line
            .marker_range()
            .map(|r| r.end)
            .unwrap_or(line.range.start);

        let is_empty = self
            .buffer
            .slice_cow(content_start..line.range.end)
            .trim()
            .is_empty();

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

    /// Check if cursor is at end of a "pending marker" (e.g., `*|` or `1.|`)
    /// that would become a list marker if we added a space.
    fn is_pending_marker(&self, cursor_pos: usize) -> bool {
        let line_idx = self.buffer.byte_to_line(cursor_pos);
        let lines = self.buffer.lines();
        let Some(current_line) = lines.get(line_idx) else {
            return false;
        };

        if !current_line.markers.is_empty() {
            return false;
        }

        let line_start = current_line.range.start;
        if cursor_pos < line_start {
            return false;
        }

        let line_to_cursor = self.buffer.slice_cow(line_start..cursor_pos);

        if line_to_cursor == "*" || line_to_cursor == "-" || line_to_cursor == "+" {
            return true;
        }

        if let Some(before_dot) = line_to_cursor.strip_suffix('.')
            && !before_dot.is_empty()
            && before_dot.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }

        false
    }

    fn complete_pending_marker(&mut self) {
        self.insert_text(" ");
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

    fn compute_smart_enter_text(&self, cursor_pos: usize) -> String {
        let lines = self.buffer.lines();
        let cursor_line_idx = self.buffer.byte_to_line(cursor_pos);

        let Some(line_info) = lines.get(cursor_line_idx) else {
            return "\n".to_string();
        };

        let continuation = line_info.continuation_rope(self.buffer.rope());
        if continuation.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{}", continuation)
        }
    }

    /// Smart tab: adds structure based on context.
    pub fn smart_tab(&mut self) {
        let Some(ctx) = self.line_context() else {
            return;
        };

        let line_start = ctx.line.range.start;

        if !ctx.line.markers.is_empty() {
            // Check if we can nest deeper (only one level beyond previous line)
            if let Some(prev) = ctx.prev_line
                && ctx.line.marker_width() >= prev.marker_width() + 2
            {
                return;
            }
            self.insert_at(line_start, "  ");
        } else if ctx.line_idx > 0 {
            // Find the nearest container to nest under
            let lines = self.buffer.lines();
            let mut container_line = None;
            for i in (0..ctx.line_idx).rev() {
                let line = &lines[i];
                if !line.markers.is_empty() {
                    container_line = Some(line);
                    break;
                }
                if !self.buffer.slice_cow(line.range.clone()).trim().is_empty() {
                    break;
                }
            }

            if let Some(container) = container_line {
                let prev_line = &lines[ctx.line_idx - 1];
                let is_after_blank = self
                    .buffer
                    .slice_cow(prev_line.range.clone())
                    .trim()
                    .is_empty();

                if is_after_blank {
                    // After blank line - indent as nested block
                    let indent = " ".repeat(container.marker_width());
                    self.insert_at(line_start, &indent);
                } else {
                    // Adjacent to container - add list marker
                    let continuation = container.continuation_rope(self.buffer.rope());
                    if !continuation.is_empty() {
                        self.insert_at(line_start, &continuation);
                    }
                }
            }
        }
    }

    /// Smart enter: creates sibling or exits container.
    pub fn smart_enter(&mut self) {
        if self.is_pending_marker(self.cursor().offset) {
            self.complete_pending_marker();
            let text = self.compute_smart_enter_text(self.cursor().offset);
            self.insert_text(&text);
            return;
        }

        let Some(ctx) = self.line_context() else {
            self.insert_text("\n");
            return;
        };

        // Check if line has an opening code fence marker
        let has_opening_fence = ctx.line.markers.iter().any(|m| {
            matches!(
                m.kind,
                MarkerKind::CodeBlockFence {
                    is_opening: true,
                    ..
                }
            )
        });

        // Check if line has a closing code fence marker
        let has_closing_fence = ctx.line.markers.iter().any(|m| {
            matches!(
                m.kind,
                MarkerKind::CodeBlockFence {
                    is_opening: false,
                    ..
                }
            )
        });

        // Check if we're inside a code block (between fences)
        let in_code_block = self.cursor_in_code_block();

        if has_opening_fence || in_code_block {
            // Opening fence or inside code block: just insert newline with any outer container
            // continuations (e.g., blockquote markers), but not the fence itself
            let continuation = ctx.line.continuation_without_fence();
            if continuation.is_empty() {
                self.insert_text("\n");
            } else {
                self.insert_text(&format!("\n{}", continuation));
            }
        } else if has_closing_fence {
            // Closing fence: create paragraph break to start new content
            let continuation = ctx.line.continuation_without_fence();
            if continuation.is_empty() {
                self.insert_text("\n\n");
            } else {
                self.insert_text(&format!("\n\n{}", continuation));
            }
        } else if !ctx.has_container {
            // Paragraph
            if ctx.is_empty && !ctx.line.markers.is_empty() {
                // Empty nested paragraph - exit by removing indent
                self.delete_and_adjust(ctx.line.markers[0].range.clone());
            } else if ctx.is_empty {
                // Already on empty line - just add one newline
                self.insert_text("\n");
            } else {
                // Create paragraph break
                // Only preserve Indent markers as indent, not other single markers like CodeBlockFence
                let indent = ctx.line.indent_only_rope(self.buffer.rope());
                self.insert_text(&format!("\n\n{}", indent));
            }
        } else if ctx.is_empty {
            // Empty container line - exit ALL markers
            let marker_range = ctx.line.marker_range().unwrap_or(ctx.line.range.clone());

            // Check if previous line is an empty continuation (e.g., just ">")
            // This handles: "> hey\n>\n> |" -> Enter -> "> hey\n\n|"
            let prev_empty_range = ctx.prev_line.and_then(|prev| {
                let prev_content_start = prev
                    .marker_range()
                    .map(|r| r.end)
                    .unwrap_or(prev.range.start);
                let prev_is_empty = self
                    .buffer
                    .slice_cow(prev_content_start..prev.range.end)
                    .trim()
                    .is_empty();
                if prev_is_empty && !prev.markers.is_empty() {
                    Some(prev.range.start..marker_range.end)
                } else {
                    None
                }
            });

            if let Some(delete_range) = prev_empty_range {
                self.delete_and_adjust(delete_range);
            } else {
                self.delete_and_adjust(marker_range);
            }

            self.insert_at(self.cursor().offset, "\n");
        } else {
            // Line has content
            // Check if this is a blockquote-only line (no list markers) - create paragraph break
            if ctx.line.is_blockquote_only() {
                // Paragraph break within blockquote: "> text" -> "> text\n>\n> "
                let continuation = ctx.line.continuation_rope(self.buffer.rope());
                self.insert_text(&format!("\n{}\n{}", continuation.trim_end(), continuation));
            } else {
                // Create sibling (for lists, etc.)
                let text = self.compute_smart_enter_text(ctx.cursor_offset);
                self.insert_text(&text);
            }
        }
    }

    /// Smart shift-tab: removes one level of structure.
    pub fn smart_shift_tab(&mut self) {
        let Some(ctx) = self.line_context() else {
            return;
        };

        if ctx.line.markers.is_empty() {
            return;
        }

        let line_text = self.buffer.slice_cow(ctx.line.range.clone());

        if line_text.starts_with("  ") {
            // Nested - remove indentation
            let delete_start = ctx.line.range.start;
            self.delete_and_adjust(delete_start..delete_start + 2);
        } else {
            // At root level - don't remove if it would leave invalid empty line
            if ctx.is_empty && ctx.line_idx > 0 {
                return;
            }
            self.delete_and_adjust(ctx.line.markers[0].range.clone());
        }
    }

    /// Shift+Enter: same as first Enter (sibling/paragraph break), but never exits.
    /// Use this to add more items without exiting the container.
    pub fn shift_enter(&mut self) {
        if self.is_pending_marker(self.cursor().offset) {
            self.complete_pending_marker();
            let text = self.compute_smart_enter_text(self.cursor().offset);
            self.insert_text(&text);
            return;
        }

        let Some(ctx) = self.line_context() else {
            self.insert_text("\n");
            return;
        };

        // Same logic as smart_enter for content, but skip the "exit on empty" branch
        if !ctx.has_container {
            // Paragraph - create paragraph break with indent if nested
            let indent = ctx
                .line
                .indent_only_string(&self.buffer.slice_cow(ctx.line.range.clone()));
            self.insert_text(&format!("\n\n{}", indent));
        } else {
            // Container - check if blockquote-only (paragraph break) or list (sibling)
            if ctx.line.is_blockquote_only() {
                let continuation = ctx.line.continuation_rope(self.buffer.rope());
                self.insert_text(&format!("\n{}\n{}", continuation.trim_end(), continuation));
            } else {
                let text = self.compute_smart_enter_text(ctx.cursor_offset);
                self.insert_text(&text);
            }
        }
    }

    fn smart_backspace_range_with_type(
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

    /// Delete backward (backspace). Joins lines when appropriate.
    pub fn delete_backward(&mut self) {
        if !self.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset > 0 {
            let cursor_pos = self.cursor().offset;

            if let Some((delete_range, is_indent_marker)) =
                self.smart_backspace_range_with_type(cursor_pos)
            {
                let new_pos = delete_range.start;
                self.buffer.delete(delete_range, cursor_pos);

                // Also delete preceding newline to join lines (but not for Indent markers)
                if new_pos > 0 && !is_indent_marker {
                    let char_before = if new_pos > 0 {
                        self.buffer.byte_at(new_pos - 1).map(|b| b as char)
                    } else {
                        None
                    };
                    if char_before == Some('\n') {
                        self.buffer.delete(new_pos - 1..new_pos, new_pos);
                        self.selection = Selection::new(new_pos - 1, new_pos - 1);
                    } else {
                        self.selection = Selection::new(new_pos, new_pos);
                    }
                } else {
                    self.selection = Selection::new(new_pos, new_pos);
                }
            } else {
                let char_before = if cursor_pos > 0 {
                    self.buffer.byte_at(cursor_pos - 1).map(|b| b as char)
                } else {
                    None
                };

                if char_before == Some('\n') {
                    let line_idx = self.buffer.byte_to_line(cursor_pos);
                    let lines = self.buffer.lines();
                    if let Some(current_line) = lines.get(line_idx) {
                        let line_text = self.buffer.slice_cow(current_line.range.clone());
                        if line_text.trim().is_empty() {
                            let has_content_after = lines.get(line_idx + 1).is_some_and(|next| {
                                !self.buffer.slice_cow(next.range.clone()).trim().is_empty()
                            });

                            if has_content_after {
                                self.buffer.delete(cursor_pos - 1..cursor_pos, cursor_pos);
                                self.selection = Selection::new(cursor_pos - 1, cursor_pos - 1);
                                return;
                            }

                            // Collapse all trailing newlines
                            let mut delete_start = cursor_pos;
                            while delete_start > 0
                                && self.buffer.byte_at(delete_start - 1) == Some(b'\n')
                            {
                                delete_start -= 1;
                            }
                            self.buffer.delete(delete_start..cursor_pos, cursor_pos);
                            self.selection = Selection::new(delete_start, delete_start);
                            return;
                        }
                    }
                }

                // Normal backspace
                let new_cursor = self.cursor().move_left(&self.buffer);
                self.buffer
                    .delete(new_cursor.offset..cursor_pos, cursor_pos);
                self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
            }
        }
    }

    fn delete_selection(&mut self) {
        let range = self.selection.range();
        let cursor_before = self.cursor().offset;
        self.buffer.delete(range.clone(), cursor_before);
        self.selection = Selection::new(range.start, range.start);
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

    fn find_line_at(&self, byte_pos: usize) -> Option<(usize, &LineMarkers)> {
        let idx = self.state.buffer.byte_to_line(byte_pos);
        self.state.buffer.lines().get(idx).map(|line| (idx, line))
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

    fn smart_tab(&mut self) {
        self.state.smart_tab();
    }

    fn smart_shift_tab(&mut self) {
        self.state.smart_shift_tab();
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
        let cursor_before = self.cursor().offset;
        let insert_pos = if !self.state.selection.is_collapsed() {
            let range = self.state.selection.range();
            self.state.buffer.delete(range.clone(), cursor_before);
            range.start
        } else {
            cursor_before
        };
        self.state.buffer.insert(insert_pos, text, insert_pos);
        let new_pos = insert_pos + text.len();
        self.state.selection = Selection::new(new_pos, new_pos);
    }

    fn delete_backward(&mut self) {
        self.state.delete_backward();
    }

    fn delete_forward(&mut self) {
        if !self.state.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset < self.state.buffer.len_bytes() {
            let cursor_before = self.cursor().offset;
            let next = self.cursor().move_right(&self.state.buffer);
            self.state
                .buffer
                .delete(cursor_before..next.offset, cursor_before);
        }
    }

    fn delete_selection(&mut self) {
        let range = self.state.selection.range();
        let cursor_before = self.cursor().offset;
        self.state.buffer.delete(range.clone(), cursor_before);
        self.state.selection = Selection::new(range.start, range.start);
    }

    fn move_in_direction(&mut self, direction: Direction, extend: bool) {
        let new_cursor = match direction {
            Direction::Left => self.smart_move_left(),
            Direction::Right => self.smart_move_right(),
            Direction::Up => self.cursor().move_up(&self.state.buffer),
            Direction::Down => self.cursor().move_down(&self.state.buffer),
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
                let prev_line = &self.state.buffer.lines()[line_idx - 1];
                return Cursor::new(prev_line.range.end);
            } else {
                return Cursor::new(marker_range.end);
            }
        }

        self.cursor().move_left(&self.state.buffer)
    }

    fn smart_move_right(&self) -> Cursor {
        let cursor_pos = self.cursor().offset;
        let len = self.state.buffer.len_bytes();
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

        self.cursor().move_right(&self.state.buffer)
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
                if keystroke.modifiers.shift {
                    // Shift+Enter: always create sibling (maintain continuation)
                    self.state.shift_enter();
                } else {
                    // Enter: create sibling or exit container if empty
                    self.state.smart_enter();
                }
                cx.notify();
            }
            "tab" => {
                if self.state.cursor_in_code_block() {
                    self.insert_text("    ");
                } else if keystroke.modifiers.shift {
                    self.smart_shift_tab();
                } else {
                    self.smart_tab();
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
                    if key_char == " " && !self.state.cursor_in_code_block() {
                        let cursor = self.cursor();
                        let line_start = cursor.move_to_line_start(&self.state.buffer).offset;
                        if cursor.offset == line_start {
                            return;
                        }
                    }
                    self.insert_text(key_char);

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
                self.state.smart_enter();
            }
            EditorAction::ShiftEnter => {
                self.state.shift_enter();
            }
            EditorAction::Tab => {
                self.smart_tab();
            }
            EditorAction::ShiftTab => {
                self.smart_shift_tab();
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
        let cursor_child_index = Some(cursor_line + 1);

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
            .map(|(line, (inline_styles, code_highlights))| {
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

    mod smart_enter_tests {
        use super::*;

        #[test]
        fn enter_on_list_item_creates_sibling() {
            let mut state = editor_with_cursor(
                r#"
- item one|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item one
- |"#,
            );
        }

        #[test]
        fn enter_in_middle_of_list_item_splits() {
            let mut state = editor_with_cursor(
                r#"
- item o|ne"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item o
- |ne"#,
            );
        }

        #[test]
        fn enter_on_blockquote_creates_paragraph_break() {
            let mut state = editor_with_cursor(
                r#"
> quote one|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
> quote one
>
> |"#,
            );
        }

        #[test]
        fn enter_on_nested_list_in_blockquote() {
            let mut state = editor_with_cursor(
                r#"
> - item|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
> - item
> - |"#,
            );
        }

        #[test]
        fn enter_on_plain_text_creates_paragraph_break() {
            // Paragraphs get a blank line (paragraph break) on Enter
            let mut state = editor_with_cursor(
                r#"
hello world|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
hello world

|"#,
            );
        }

        #[test]
        fn enter_on_heading_creates_paragraph_break() {
            // Headings get a blank line (paragraph break) on Enter
            let mut state = editor_with_cursor(
                r#"
# Hello|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
# Hello

|"#,
            );
        }

        #[test]
        fn enter_on_nested_paragraph_creates_paragraph_break_with_indent() {
            // Nested paragraph under list item: Enter creates paragraph break but keeps indent
            let mut state = editor_with_cursor(
                r#"
- item

  paragraph|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item

  paragraph

  |"#,
            );
        }

        #[test]
        fn enter_on_empty_nested_paragraph_exits_indent() {
            // Empty nested paragraph line: Enter removes indent and exits
            let mut state = editor_with_cursor(
                r#"
- item

  paragraph

  |"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item

  paragraph

|"#,
            );
        }

        #[test]
        fn shift_enter_on_empty_list_item_creates_sibling() {
            // Shift+Enter always maintains continuation, unlike Enter which exits
            let mut state = editor_with_cursor(
                r#"
- item one
- |"#,
            );
            state.shift_enter();
            assert_editor_eq(&state, "- item one\n- \n- |");
        }

        #[test]
        fn shift_enter_on_list_item_with_content_creates_sibling() {
            let mut state = editor_with_cursor(
                r#"
- item one|"#,
            );
            state.shift_enter();
            assert_editor_eq(
                &state,
                r#"
- item one
- |"#,
            );
        }

        #[test]
        fn enter_on_pending_marker_completes_it() {
            // `*|` should become `* \n* |`
            let mut state = editor_with_cursor(
                r#"
*|"#,
            );
            state.smart_enter();
            assert_editor_eq(&state, "* \n* |");
        }

        #[test]
        fn enter_on_pending_ordered_marker_completes_it() {
            // `1.|` should become `1. \n1. |`
            let mut state = editor_with_cursor(
                r#"
1.|"#,
            );
            state.smart_enter();
            assert_editor_eq(&state, "1. \n1. |");
        }

        #[test]
        fn enter_on_empty_list_item_exits_list() {
            let mut state = editor_with_cursor(
                r#"
- item
- |"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item

|"#,
            );
        }

        #[test]
        fn enter_on_empty_nested_list_exits_all() {
            // Empty container line exits ALL markers
            let mut state = editor_with_cursor(
                r#"
> - item
> - |"#,
            );
            state.smart_enter();
            assert_editor_eq(&state, "> - item\n\n|");
        }

        #[test]
        fn enter_on_empty_blockquote_exits_blockquote() {
            let mut state = editor_with_cursor(
                r#"
> quote
> |"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
> quote

|"#,
            );
        }

        #[test]
        fn double_enter_on_blockquote_exits_and_cleans_up() {
            // First enter creates paragraph break, second enter exits and removes empty continuation
            let mut state = editor_with_cursor("> hey|");
            state.smart_enter();
            assert_editor_eq(&state, "> hey\n>\n> |");
            state.smart_enter();
            assert_editor_eq(&state, "> hey\n\n|");
        }

        #[test]
        fn enter_on_code_fence_inserts_single_newline() {
            // Opening fence: just insert newline, not paragraph break
            let mut state = editor_with_cursor(
                r#"
```rust|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
```rust
|"#,
            );
        }

        #[test]
        fn enter_on_closing_code_fence_creates_paragraph_break() {
            // Closing fence: create paragraph break to start new content
            // The closing fence needs trailing content for tree-sitter to parse it
            let mut state = editor_with_cursor(
                r#"
```rust
code
```|
after"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
```rust
code
```

|
after"#,
            );
        }

        #[test]
        fn enter_on_closing_code_fence_without_trailing_content() {
            // Closing fence without trailing content - should still create paragraph break
            let mut state = editor_with_cursor(
                r#"
```rust
code
```|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
```rust
code
```

|"#,
            );
        }

        #[test]
        fn enter_on_code_fence_in_blockquote_continues_blockquote() {
            // Code fence inside blockquote: continue the blockquote but not the fence
            let mut state = editor_with_cursor(
                r#"
> ```rust|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
> ```rust
> |"#,
            );
        }

        #[test]
        fn enter_inside_code_block_inserts_single_newline() {
            // Inside code block: just insert newline, not paragraph break
            // Need trailing newline after closing fence for tree-sitter to detect it
            let mut state = editor_with_cursor(
                r#"
```rust
let x = 1;|
```
"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
```rust
let x = 1;
|
```
"#,
            );
        }

        #[test]
        fn enter_after_code_block_creates_paragraph_break() {
            // After a complete code block, Enter should create paragraph break
            let mut state = editor_with_cursor(
                r#"
```rust
```
|"#,
            );
            // Debug
            println!("Text before: {:?}", state.text());
            println!("Lines: {:?}", state.buffer.lines());
            println!("cursor_in_code_block: {}", state.cursor_in_code_block());

            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
```rust
```

|"#,
            );
        }
    }

    mod smart_tab_tests {
        use super::*;

        #[test]
        fn tab_on_empty_line_after_list_item_adds_marker() {
            let mut state = editor_with_cursor(
                r#"
- item
|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item
- |"#,
            );
        }

        #[test]
        fn tab_twice_on_empty_line_after_list_item_nests() {
            let mut state = editor_with_cursor(
                r#"
- item
|"#,
            );
            state.smart_tab();
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item
  - |"#,
            );
        }

        #[test]
        fn tab_on_list_item_increases_nesting() {
            let mut state = editor_with_cursor(
                r#"
- item|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
  - item|"#,
            );
        }

        #[test]
        fn tab_on_empty_line_after_blockquote_adds_marker() {
            let mut state = editor_with_cursor(
                r#"
> quote
|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
> quote
> |"#,
            );
        }

        #[test]
        fn tab_on_empty_line_after_nested_list_in_blockquote() {
            let mut state = editor_with_cursor(
                r#"
> - item
|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
> - item
> - |"#,
            );
        }

        #[test]
        fn tab_on_plain_text_adjacent_to_list_adds_marker() {
            let mut state = editor_with_cursor(
                r#"
- item
text|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item
- text|"#,
            );
        }

        #[test]
        fn tab_on_empty_list_item_nests_it() {
            let mut state = editor_with_cursor(
                r#"
- item one
- |"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item one
  - |"#,
            );
        }

        #[test]
        fn tab_on_already_nested_item_does_nothing() {
            // Can only nest one level deeper than previous line
            let mut state = editor_with_cursor(
                r#"
- item
  - nested|"#,
            );
            state.smart_tab();
            // Should not change - already at max nesting
            assert_editor_eq(
                &state,
                r#"
- item
  - nested|"#,
            );
        }

        #[test]
        fn tab_on_sibling_can_nest() {
            // Sibling at same level can nest once
            let mut state = editor_with_cursor(
                r#"
- item
- sibling|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item
  - sibling|"#,
            );
            // But not twice
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item
  - sibling|"#,
            );
        }

        #[test]
        fn tab_after_blank_line_indents_as_block() {
            // After blank line, tab should indent as nested block, not add marker
            let mut state = editor_with_cursor(
                r#"
- item

|"#,
            );
            state.smart_tab();
            assert_editor_eq(
                &state,
                r#"
- item

  |"#,
            );
        }
    }

    mod smart_shift_tab_tests {
        use super::*;

        #[test]
        fn shift_tab_on_nested_list_item_unnests() {
            let mut state = editor_with_cursor(
                r#"
  - nested item|"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
- nested item|"#,
            );
        }

        #[test]
        fn shift_tab_on_list_item_removes_marker() {
            let mut state = editor_with_cursor(
                r#"
- item|"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
item|"#,
            );
        }

        #[test]
        fn shift_tab_on_nested_list_in_blockquote_removes_list() {
            let mut state = editor_with_cursor(
                r#"
> - item|"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
> item|"#,
            );
        }

        #[test]
        fn shift_tab_on_blockquote_removes_marker() {
            let mut state = editor_with_cursor(
                r#"
> item|"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
item|"#,
            );
        }

        #[test]
        fn shift_tab_on_empty_list_item_does_nothing() {
            // Would create invalid state (empty line after list item)
            let mut state = editor_with_cursor(
                r#"
- item
- |"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
- item
- |"#,
            );
        }

        #[test]
        fn shift_tab_on_empty_nested_list_item_unnests() {
            // Unnesting is valid - becomes sibling, not empty line
            let mut state = editor_with_cursor(
                r#"
- item
  - |"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
- item
- |"#,
            );
        }

        #[test]
        fn shift_tab_on_plain_text_does_nothing() {
            let mut state = editor_with_cursor(
                r#"
plain text|"#,
            );
            state.smart_shift_tab();
            assert_editor_eq(
                &state,
                r#"
plain text|"#,
            );
        }
    }

    mod blockquote_auto_space_tests {
        use super::*;

        #[test]
        fn typing_blockquote_marker_auto_inserts_space() {
            let mut state = editor_with_cursor(
                r#"
|"#,
            );
            // Simulate typing ">"
            state.insert_text(">");
            state.maybe_complete_blockquote_marker();
            assert_editor_eq(
                &state, r#"
> |"#,
            );
        }

        #[test]
        fn blockquote_marker_with_existing_space_no_double_space() {
            let mut state = editor_with_cursor(
                r#"
> |"#,
            );
            // Already has space, should not add another
            let inserted = state.maybe_complete_blockquote_marker();
            assert!(!inserted);
            assert_editor_eq(
                &state, r#"
> |"#,
            );
        }

        #[test]
        fn nested_blockquote_auto_inserts_space() {
            let mut state = editor_with_cursor(
                r#"
> |"#,
            );
            // Simulate typing another ">"
            state.insert_text(">");
            state.maybe_complete_blockquote_marker();
            assert_editor_eq(
                &state,
                r#"
> > |"#,
            );
        }

        #[test]
        fn greater_than_in_text_no_auto_space() {
            // If > is typed in the middle of text, don't auto-space
            let mut state = editor_with_cursor(
                r#"
5 |"#,
            );
            state.insert_text(">");
            let inserted = state.maybe_complete_blockquote_marker();
            assert!(!inserted);
            assert_editor_eq(
                &state, r#"
5 >|"#,
            );
        }
    }

    mod backspace_tests {
        use super::*;

        #[test]
        fn backspace_on_empty_list_item_joins_to_previous() {
            let mut state = editor_with_cursor(
                r#"
- item one
- |"#,
            );
            // Single backspace should join to previous line's content
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- item one|"#,
            );
        }

        #[test]
        fn backspace_after_enter_on_list_item_joins_to_content() {
            // Scenario: "- item one|" -> Enter -> "- item one\n- |" -> Backspace
            // Should go back to "- item one|", not "- item one\n|"
            let mut state = editor_with_cursor(
                r#"
- item one|"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item one
- |"#,
            );
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- item one|"#,
            );
        }

        #[test]
        fn backspace_after_exit_container_joins_to_content() {
            // Scenario: "- item one\n- |" -> Enter (exit) -> "- item one\n\n|" -> Backspace
            // One backspace should join directly to content
            let mut state = editor_with_cursor(
                r#"
- item one
- |"#,
            );
            state.smart_enter();
            assert_editor_eq(
                &state,
                r#"
- item one

|"#,
            );
            // One backspace joins directly to content
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- item one|"#,
            );
        }

        #[test]
        fn backspace_from_blank_line_after_list() {
            // Direct test: cursor on blank line after list item
            // One backspace should join directly to content
            let mut state = editor_with_cursor(
                r#"
- item one

|"#,
            );
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- item one|"#,
            );
        }

        #[test]
        fn backspace_on_nested_paragraph_deletes_one_newline() {
            // When on indented paragraph after blank line, backspace should only
            // delete one newline, not collapse all the way to the list item
            let mut state = editor_with_cursor(
                r#"
- item

  |paragraph"#,
            );
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- item

|paragraph"#,
            );
        }

        #[test]
        fn backspace_on_blank_line_before_nested_paragraph() {
            // Backspace on blank line between list and paragraph should
            // only delete one newline
            let mut state = editor_with_cursor(
                r#"
- item

|
  paragraph"#,
            );
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- item
|
  paragraph"#,
            );
        }

        #[test]
        fn backspace_on_empty_task_list_deletes_entire_marker() {
            // Task list marker "- [ ] " is a single marker, so backspace
            // should delete it all at once (not just the checkbox)
            let mut state = editor_with_cursor("- [ ] |");
            state.delete_backward();
            assert_editor_eq(&state, "|");
        }

        #[test]
        fn backspace_on_task_list_with_content_joins_to_previous() {
            let mut state = editor_with_cursor(
                r#"
- [ ] task one
- [ ] |"#,
            );
            state.delete_backward();
            assert_editor_eq(
                &state,
                r#"
- [ ] task one|"#,
            );
        }
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
}
