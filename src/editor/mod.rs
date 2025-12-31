mod action;
mod config;
mod theme;

pub use action::{Direction, EditorAction};
pub use config::EditorConfig;
pub use theme::EditorTheme;

use std::rc::Rc;

use gpui::{
    App, Context, CursorStyle, FocusHandle, Focusable, IntoElement, KeyDownEvent, ScrollHandle,
    Window, div, font, prelude::*,
};

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::line_view::{
    CheckboxCallback, ClickCallback, DragCallback, LineView, LineViewTheme, LinkHoverCallback,
};
use crate::lines::{LineInfo, LineKind, extract_inline_styles, extract_lines};

/// Code block range: (start_line_idx, end_line_idx or None for incomplete blocks)
type CodeBlockRange = (usize, Option<usize>);

/// The main editor component.
pub struct Editor {
    /// The text buffer
    buffer: Buffer,
    /// Current selection (anchor and head). When collapsed, acts as cursor.
    selection: Selection,
    /// Focus handle for keyboard input
    focus_handle: FocusHandle,
    /// Scroll handle for scrolling cursor into view
    scroll_handle: ScrollHandle,
    /// Child index of the cursor line in the scroll container (accounts for spacer and filtered lines)
    cursor_child_index: Option<usize>,
    /// Whether a scroll to cursor is pending (set when cursor moves, cleared after scroll)
    scroll_to_cursor_pending: bool,
    /// Whether user input is blocked (for demo mode)
    input_blocked: bool,
    /// Whether streaming mode is active
    streaming_mode: bool,
    /// Editor configuration (theme, fonts, etc.)
    config: EditorConfig,
    /// Whether mouse is hovering over a link with Ctrl/Cmd held
    hovering_link: bool,
}

impl Editor {
    /// Create a new editor with the given content and default configuration.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        Self::with_config(content, EditorConfig::default(), cx)
    }

    /// Create a new editor with custom configuration.
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
            hovering_link: false,
        }
    }

    /// Get the buffer contents.
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Get the buffer length in bytes.
    pub fn len(&self) -> usize {
        self.buffer.len_bytes()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.len_bytes() == 0
    }

    /// Replace the entire buffer contents.
    ///
    /// This resets cursor to position 0 and clears undo history.
    pub fn set_text(&mut self, content: &str, cx: &mut Context<Self>) {
        self.buffer = content.parse().unwrap_or_default();
        self.selection = Selection::new(0, 0);
        cx.notify();
    }

    /// Insert text at the current cursor position.
    ///
    /// If there's a selection, it will be replaced.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.insert_text(text);
        cx.notify();
    }

    /// Append text at the end of the buffer.
    ///
    /// This is optimized for streaming scenarios where text arrives incrementally.
    /// The cursor is moved to the end after appending.
    pub fn append(&mut self, text: &str, cx: &mut Context<Self>) {
        let end = self.buffer.len_bytes();
        self.buffer.insert(end, text, end);
        let new_end = self.buffer.len_bytes();
        self.selection = Selection::new(new_end, new_end);
        cx.notify();
    }

    /// Append text and scroll to keep the end visible.
    ///
    /// Useful for streaming scenarios where you want to follow new content.
    pub fn append_and_scroll(&mut self, text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        self.append(text, cx);
        self.scroll_handle.scroll_to_bottom();
    }

    /// Helper to get cursor position (selection head).
    fn cursor(&self) -> Cursor {
        self.selection.cursor()
    }

    /// Helper to move cursor and update selection.
    /// If extend is true, extends selection; otherwise collapses to new position.
    fn move_cursor(&mut self, new_cursor: Cursor, extend: bool) {
        if extend {
            self.selection = self.selection.extend_to(new_cursor.offset);
        } else {
            self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
        }
    }

    /// Perform scroll to cursor if pending and bounds are available.
    /// Called from render() - uses bounds from the previous layout.
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

        // Clear the pending flag now that we have bounds
        self.scroll_to_cursor_pending = false;

        let viewport = self.scroll_handle.bounds();
        let offset = self.scroll_handle.offset();

        // Calculate item position relative to viewport
        let item_top = item_bounds.origin.y + offset.y;
        let item_bottom = item_top + item_bounds.size.height;

        let visible_top = viewport.origin.y + margin;
        let visible_bottom = viewport.origin.y + viewport.size.height - margin;

        // Check if we need to scroll
        if item_top < visible_top {
            // Item is above visible area - scroll up
            let new_offset_y = viewport.origin.y - item_bounds.origin.y + margin;
            self.scroll_handle
                .set_offset(gpui::point(offset.x, new_offset_y));
        } else if item_bottom > visible_bottom {
            // Item is below visible area - scroll down
            let new_offset_y = viewport.origin.y + viewport.size.height
                - item_bounds.origin.y
                - item_bounds.size.height
                - margin;
            self.scroll_handle
                .set_offset(gpui::point(offset.x, new_offset_y));
        }
    }

    /// Mark that we need to scroll to cursor on next render.
    fn request_scroll_to_cursor(&mut self) {
        self.scroll_to_cursor_pending = true;
    }

    /// Compute the text to insert for Smart Enter (Shift+Enter).
    ///
    /// This continues the current line type at the same nesting level:
    /// - Unordered list: `\n` + indentation + `- `
    /// - Ordered list: `\n` + indentation + `1. ` (normalization fixes the number)
    /// - Task list: `\n` + indentation + `- [ ] `
    /// - Blockquote: `\n` + same `> ` prefix(es)
    /// - Code block/regular text: just `\n`
    fn compute_smart_enter_text(&self, cursor_pos: usize) -> String {
        let lines = extract_lines(&self.buffer);
        let cursor_line_idx = self.buffer.byte_to_line(cursor_pos);

        // Find the line info for the current line
        let Some(line_info) = lines.get(cursor_line_idx) else {
            return "\n".to_string();
        };

        // Use pre-computed continuation from LineInfo
        if line_info.continuation.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{}", line_info.continuation)
        }
    }

    /// Toggle the checkbox on a given line.
    ///
    /// Finds `[ ]` or `[x]`/`[X]` in the line and toggles it.
    /// The cursor stays where it was.
    fn toggle_checkbox(&mut self, line_number: usize, cx: &mut Context<Self>) {
        let lines = extract_lines(&self.buffer);
        let Some(line) = lines.get(line_number) else {
            return;
        };

        // Only toggle task list items
        let buffer_text = self.buffer.text();
        let LineKind::ListItem {
            checked: Some(is_checked),
            ..
        } = &line.kind
        else {
            return;
        };
        let is_checked = *is_checked;

        // Find the checkbox pattern in the line
        let line_text = &buffer_text[line.range.clone()];

        // Find the position of [ ] or [x]/[X] in the line
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

        // Calculate the absolute buffer position of the checkbox content (the x or space)
        let checkbox_content_start = line.range.start + relative_offset + 1; // skip '['
        let checkbox_content_end = checkbox_content_start + 1; // just the 'x' or ' '

        // Toggle the content
        let new_content = if is_checked { " " } else { "x" };
        let cursor_before = self.cursor().offset;

        self.buffer.replace(
            checkbox_content_start..checkbox_content_end,
            new_content,
            cursor_before,
        );

        // Keep cursor where it was (don't update selection)
        // The replace returns a new position, but we want to keep the old one
        // unless the cursor was inside the checkbox area
        self.selection = Selection::new(cursor_before, cursor_before);

        cx.notify();
    }

    // =========================================================================
    // Core editing operations - used by both keyboard handler and execute()
    // =========================================================================

    /// Insert text at cursor, replacing selection if any.
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

    /// Delete backward (backspace behavior).
    fn delete_backward(&mut self) {
        if !self.selection.is_collapsed() {
            self.delete_selection();
        } else if self.cursor().offset > 0 {
            let cursor_before = self.cursor().offset;
            let new_cursor = self.cursor().move_left(&self.buffer);
            self.buffer
                .delete(new_cursor.offset..cursor_before, cursor_before);
            self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
        }
    }

    /// Delete forward (delete key behavior).
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

    /// Delete the current selection.
    fn delete_selection(&mut self) {
        let range = self.selection.range();
        let cursor_before = self.cursor().offset;
        self.buffer.delete(range.clone(), cursor_before);
        self.selection = Selection::new(range.start, range.start);
    }

    /// Move cursor in a direction, optionally extending selection.
    fn move_in_direction(&mut self, direction: Direction, extend: bool) {
        let new_cursor = match direction {
            Direction::Left => self.cursor().move_left(&self.buffer),
            Direction::Right => self.cursor().move_right(&self.buffer),
            Direction::Up => self.cursor().move_up(&self.buffer),
            Direction::Down => self.cursor().move_down(&self.buffer),
        };
        self.move_cursor(new_cursor, extend);
    }

    /// Compute code block ranges from lines.
    /// Returns (start_line_idx, end_line_idx or None for incomplete blocks).
    fn compute_code_block_ranges(lines: &[LineInfo]) -> Vec<CodeBlockRange> {
        let mut ranges = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            if let LineKind::CodeBlock { is_fence: true, .. } = &lines[i].kind {
                let start = i;
                i += 1;
                let mut found_close = false;

                // Find closing fence
                while i < lines.len() {
                    if let LineKind::CodeBlock { is_fence: true, .. } = &lines[i].kind {
                        ranges.push((start, Some(i)));
                        i += 1;
                        found_close = true;
                        break;
                    }
                    i += 1;
                }

                // Incomplete block - no closing fence found
                if !found_close {
                    ranges.push((start, None));
                }
            } else {
                i += 1;
            }
        }

        ranges
    }

    /// Handle a key down event.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Block user input during demo mode
        if self.input_blocked {
            return;
        }

        let keystroke = &event.keystroke;
        let extend = keystroke.modifiers.shift;

        // Handle special keys
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
                self.insert_text("    ");
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
                // Save is handled by the application, not the editor component
            }
            _ => {
                if let Some(key_char) = &keystroke.key_char {
                    self.insert_text(key_char);
                    cx.notify();
                }
            }
        }
    }

    /// Block user input (for demo mode).
    pub fn set_input_blocked(&mut self, blocked: bool) {
        self.input_blocked = blocked;
    }

    /// Check if user input is currently blocked.
    pub fn is_input_blocked(&self) -> bool {
        self.input_blocked
    }

    // =========================================================================
    // Streaming mode - for AI chat applications
    // =========================================================================

    /// Begin streaming mode.
    ///
    /// In streaming mode:
    /// - User input is blocked
    /// - Cursor is pinned to end of document
    /// - Appends are optimized for frequent small updates
    pub fn begin_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming_mode = true;
        self.input_blocked = true;
        // Move cursor to end
        let end = self.buffer.len_bytes();
        self.selection = Selection::new(end, end);
        cx.notify();
    }

    /// End streaming mode and restore normal editing.
    pub fn end_streaming(&mut self, cx: &mut Context<Self>) {
        self.streaming_mode = false;
        self.input_blocked = false;
        cx.notify();
    }

    /// Check if currently in streaming mode.
    pub fn is_streaming(&self) -> bool {
        self.streaming_mode
    }

    // =========================================================================
    // State queries
    // =========================================================================

    /// Get the current cursor position (byte offset).
    pub fn cursor_position(&self) -> usize {
        self.selection.head
    }

    /// Get the current selection range, or None if collapsed (just a cursor).
    pub fn selection_range(&self) -> Option<std::ops::Range<usize>> {
        if self.selection.is_collapsed() {
            None
        } else {
            Some(self.selection.range())
        }
    }

    /// Set the cursor position.
    pub fn set_cursor(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.buffer.len_bytes());
        self.selection = Selection::new(offset, offset);
        cx.notify();
    }

    /// Move cursor to end of document.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.buffer.len_bytes();
        self.selection = Selection::new(end, end);
        cx.notify();
    }

    /// Move cursor to start of document.
    pub fn move_to_start(&mut self, cx: &mut Context<Self>) {
        self.selection = Selection::new(0, 0);
        cx.notify();
    }

    /// Check if the buffer has been modified since last `mark_clean()`.
    pub fn is_dirty(&self) -> bool {
        self.buffer.is_dirty()
    }

    /// Mark the buffer as clean (call after saving).
    pub fn mark_clean(&mut self) {
        self.buffer.mark_clean();
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.buffer.can_undo()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.buffer.can_redo()
    }

    /// Undo the last edit.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.buffer.undo() {
            self.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }

    /// Redo the last undone edit.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor_pos) = self.buffer.redo() {
            self.selection = Selection::new(cursor_pos, cursor_pos);
            cx.notify();
        }
    }

    /// Execute an editor action programmatically.
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
        let theme = self.config.theme.clone();
        let line_theme = LineViewTheme {
            text_color: theme.foreground,
            cursor_color: theme.purple,
            link_color: theme.cyan,
            selection_color: theme.selection,
            border_color: theme.comment,
            fence_color: theme.comment,
            fence_lang_color: theme.green,
            code_color: theme.pink,
            text_font: font(&self.config.text_font),
            code_font: font(&self.config.code_font),
        };
        let cursor_offset = self.selection.head;
        let selection_range = if self.selection.is_collapsed() {
            None
        } else {
            Some(self.selection.range())
        };

        // Get the base path for resolving relative image paths
        let base_path = self.config.base_path.clone();

        let buffer_text = self.buffer.text();

        // Create click callback that updates cursor position
        let entity = cx.entity().clone();
        let on_click: ClickCallback =
            Rc::new(move |buffer_offset, shift_held, click_count, _window, cx| {
                entity.update(cx, |editor, cx| {
                    // Block user input during demo mode
                    if editor.input_blocked {
                        return;
                    }
                    if shift_held {
                        // Extend selection to click position
                        editor.selection = editor.selection.extend_to(buffer_offset);
                    } else {
                        match click_count {
                            2 => {
                                // Double-click: select word
                                editor.selection =
                                    Selection::select_word_at(buffer_offset, &editor.buffer);
                            }
                            3 => {
                                // Triple-click: select line
                                editor.selection =
                                    Selection::select_line_at(buffer_offset, &editor.buffer);
                            }
                            _ => {
                                // Single click: collapse selection to click position
                                editor.selection = Selection::new(buffer_offset, buffer_offset);
                            }
                        }
                    }
                    cx.notify();
                });
            });

        // Create drag callback that extends selection during mouse drag
        let entity = cx.entity().clone();
        let on_drag: DragCallback = Rc::new(move |buffer_offset, _window, cx| {
            entity.update(cx, |editor, cx| {
                // Block user input during demo mode
                if editor.input_blocked {
                    return;
                }
                // Extend selection to drag position (keep anchor, move head)
                editor.selection = editor.selection.extend_to(buffer_offset);
                cx.notify();
            });
        });

        // Create checkbox toggle callback
        let entity = cx.entity().clone();
        let on_checkbox: CheckboxCallback = Rc::new(move |line_number, _window, cx| {
            entity.update(cx, |editor, cx| {
                // Block user input during demo mode
                if editor.input_blocked {
                    return;
                }
                editor.toggle_checkbox(line_number, cx);
            });
        });

        // Create link hover callback
        let entity = cx.entity().clone();
        let on_link_hover: LinkHoverCallback = Rc::new(move |hovering, _window, cx| {
            entity.update(cx, |editor, cx| {
                if editor.hovering_link != hovering {
                    editor.hovering_link = hovering;
                    cx.notify();
                }
            });
        });

        // Extract lines fresh on each render (tree-sitter walking is fast)
        let lines = extract_lines(&self.buffer);
        let code_block_ranges = Self::compute_code_block_ranges(&lines);
        let cursor_line = self.buffer.byte_to_line(cursor_offset);
        let num_lines = lines.len();

        // Helper: check if cursor is inside a code block (by line index)
        let cursor_in_code_block_range = |line_idx: usize| -> bool {
            for (start, end) in &code_block_ranges {
                let block_end = end.unwrap_or(num_lines.saturating_sub(1));
                // Check if this fence line belongs to a block that contains the cursor
                if line_idx >= *start
                    && line_idx <= block_end
                    && cursor_line >= *start
                    && cursor_line <= block_end
                {
                    return true;
                }
            }
            false
        };

        // Build line views with click and drag handling
        // Skip fence lines when cursor is outside the code block (they're hidden)
        // Track child index for cursor line (accounting for filtered lines and top spacer)
        let mut cursor_child_index: Option<usize> = None;
        let mut child_index = 1; // Start at 1 to account for top_spacer

        let line_views: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(line_idx, line)| {
                // For fence lines, check if cursor is in this code block
                let is_fence = matches!(&line.kind, LineKind::CodeBlock { is_fence: true, .. });
                let cursor_in_block = cursor_in_code_block_range(line_idx);

                // Skip fence lines when cursor is outside the code block
                if is_fence && !cursor_in_block {
                    return None;
                }

                // Track the child index for the cursor line
                if line_idx == cursor_line {
                    cursor_child_index = Some(child_index);
                }
                child_index += 1;

                // Show block markers for fence lines when cursor is in the code block
                let show_block_markers = is_fence && cursor_in_block;

                // Extract inline styles for this line
                let inline_styles = extract_inline_styles(&self.buffer, line);

                // Get code highlights for this line's range
                let code_highlights: Vec<_> = self
                    .buffer
                    .code_highlights_for_range(line.range.clone())
                    .iter()
                    .map(|span| (span.clone(), theme.color_for_highlight(span.highlight_id)))
                    .collect();

                let line_view = LineView::new(
                    line,
                    &buffer_text,
                    cursor_offset,
                    inline_styles,
                    line_theme.clone(),
                    selection_range.clone(),
                    base_path.clone(),
                    code_highlights,
                    show_block_markers,
                )
                .on_click(on_click.clone())
                .on_drag(on_drag.clone())
                .on_checkbox(on_checkbox.clone())
                .on_link_hover(on_link_hover.clone());

                Some(line_view)
            })
            .collect();

        // Check if cursor moved to a different child
        let cursor_moved = self.cursor_child_index != cursor_child_index;
        self.cursor_child_index = cursor_child_index;

        // If cursor moved, request scroll on next render (when bounds will be available)
        if cursor_moved {
            self.request_scroll_to_cursor();
        }

        // Perform any pending scroll (uses bounds from previous layout)
        let margin = self.config.padding_y.to_pixels(window.rem_size());
        self.perform_pending_scroll(margin);

        // Create spacer elements for vertical padding (these scroll with content)
        let top_spacer = div().h(self.config.padding_y);
        let bottom_spacer = div().h(self.config.padding_y);

        div()
            .id("editor")
            .track_focus(&self.focus_handle)
            .key_context("Editor")
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle)
            .px(self.config.padding_x)
            .font(line_theme.text_font.clone())
            .text_color(line_theme.text_color)
            .cursor(if self.hovering_link {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            })
            .child(top_spacer)
            .children(line_views)
            .child(bottom_spacer)
    }
}
