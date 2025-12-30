use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Context, CursorStyle, FocusHandle, Focusable, Font, IntoElement, KeyDownEvent,
    ScrollHandle, Window, div, font, prelude::*,
};

use crate::buffer::Buffer;
use crate::cursor::{Cursor, Selection};
use crate::highlight::Highlighter;
use crate::line_view::{CheckboxCallback, ClickCallback, DragCallback, LineView};
use crate::lines::{LineKind, extract_inline_styles, extract_lines};
use crate::theme::Theme;
use crate::title_bar::FileInfo;

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
    /// Syntax highlighter for code blocks
    highlighter: Highlighter,
}

impl Editor {
    /// Create a new editor with the given content.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        let buffer: Buffer = content.parse().unwrap_or_default();
        let focus_handle = cx.focus_handle();

        Self {
            buffer,
            selection: Selection::new(0, 0),
            focus_handle,
            scroll_handle: ScrollHandle::new(),
            highlighter: Highlighter::new(),
        }
    }

    /// Get the buffer contents.
    pub fn text(&self) -> String {
        self.buffer.text()
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

        // Use the layer-based continuation - each layer contributes its continuation text
        // Pass the buffer text so continuation can include leading indentation
        let text = self.buffer.text();
        let continuation = line_info.continuation(&text);
        if continuation.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{}", continuation)
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
        let LineKind::ListItem {
            checked: Some(is_checked),
            ..
        } = line.kind()
        else {
            return;
        };

        // Find the checkbox pattern in the line
        let buffer_text = self.buffer.text();
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

        self.sync_dirty_state(cx);
        cx.notify();
    }

    /// Sync dirty state from buffer to FileInfo global.
    /// Call this after any buffer modification.
    fn sync_dirty_state(&self, cx: &mut Context<Self>) {
        let file_info = cx.global::<FileInfo>();
        let buffer_dirty = self.buffer.is_dirty();
        if file_info.dirty != buffer_dirty {
            cx.set_global(FileInfo {
                path: file_info.path.clone(),
                dirty: buffer_dirty,
            });
        }
    }

    /// Save the buffer to the file.
    fn save(&mut self, cx: &mut Context<Self>) {
        let file_info = cx.global::<FileInfo>();
        let content = self.buffer.text();
        if std::fs::write(&file_info.path, &content).is_ok() {
            self.buffer.mark_clean();
            self.sync_dirty_state(cx);
            cx.notify();
        }
    }

    /// Compute syntax highlights for all code blocks in the document.
    ///
    /// This parses each code block once and returns all highlights with
    /// buffer-relative offsets. The caller can then filter by line range.
    fn compute_code_block_highlights(
        &mut self,
        lines: &[crate::lines::LineInfo],
        buffer_text: &str,
        theme: &Theme,
    ) -> Vec<(crate::highlight::HighlightSpan, gpui::Rgba)> {
        let mut all_highlights = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            // Look for start of a code block (fence line with language)
            if let LineKind::CodeBlock {
                language: Some(ref lang),
                is_fence: true,
            } = lines[i].kind()
            {
                let lang = lang.clone();
                let block_start = i;
                i += 1;

                // Collect all content lines until closing fence
                let mut code_content = String::new();
                let mut content_start_offset: Option<usize> = None;

                while i < lines.len() {
                    match &lines[i].kind() {
                        LineKind::CodeBlock { is_fence: true, .. } => {
                            // Closing fence - end of block
                            i += 1;
                            break;
                        }
                        LineKind::CodeBlock {
                            is_fence: false, ..
                        } => {
                            // Content line
                            if content_start_offset.is_none() {
                                content_start_offset = Some(lines[i].range.start);
                            }
                            code_content.push_str(&buffer_text[lines[i].range.clone()]);
                            code_content.push('\n');
                            i += 1;
                        }
                        _ => {
                            // Unexpected - shouldn't happen, but break to be safe
                            break;
                        }
                    }
                }

                // Parse and highlight the entire code block
                if let Some(start_offset) = content_start_offset {
                    let spans = self.highlighter.highlight(&code_content, &lang);

                    // Convert to buffer-relative offsets and add colors
                    for mut span in spans {
                        span.range.start += start_offset;
                        span.range.end += start_offset;
                        let color = theme.color_for_highlight(span.highlight_id);
                        all_highlights.push((span, color));
                    }
                }

                let _ = block_start; // suppress unused warning
            } else {
                i += 1;
            }
        }

        all_highlights
    }

    /// Handle a key down event.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let extend = keystroke.modifiers.shift;

        // Handle special keys
        match keystroke.key.as_str() {
            "backspace" => {
                if !self.selection.is_collapsed() {
                    // Delete selection
                    let range = self.selection.range();
                    let cursor_before = self.cursor().offset;
                    self.buffer.delete(range.clone(), cursor_before);
                    self.selection = Selection::new(range.start, range.start);
                    cx.notify();
                } else if self.cursor().offset > 0 {
                    let cursor_before = self.cursor().offset;
                    let new_cursor = self.cursor().move_left(&self.buffer);
                    self.buffer
                        .delete(new_cursor.offset..cursor_before, cursor_before);
                    self.selection = Selection::new(new_cursor.offset, new_cursor.offset);
                    cx.notify();
                }
            }
            "delete" => {
                if !self.selection.is_collapsed() {
                    // Delete selection
                    let range = self.selection.range();
                    let cursor_before = self.cursor().offset;
                    self.buffer.delete(range.clone(), cursor_before);
                    self.selection = Selection::new(range.start, range.start);
                    cx.notify();
                } else if self.cursor().offset < self.buffer.len_bytes() {
                    let cursor_before = self.cursor().offset;
                    let next = self.cursor().move_right(&self.buffer);
                    self.buffer
                        .delete(cursor_before..next.offset, cursor_before);
                    cx.notify();
                }
            }
            "left" => {
                let new_cursor = self.cursor().move_left(&self.buffer);
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "right" => {
                let new_cursor = self.cursor().move_right(&self.buffer);
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "up" => {
                let new_cursor = self.cursor().move_up(&self.buffer);
                self.move_cursor(new_cursor, extend);
                cx.notify();
            }
            "down" => {
                let new_cursor = self.cursor().move_down(&self.buffer);
                self.move_cursor(new_cursor, extend);
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
                // Delete selection first if any
                let cursor_before = self.cursor().offset;
                let insert_pos = if !self.selection.is_collapsed() {
                    let range = self.selection.range();
                    self.buffer.delete(range.clone(), cursor_before);
                    range.start
                } else {
                    cursor_before
                };

                // Smart Enter (Shift+Enter): continue the current line type
                let insert_text = if keystroke.modifiers.shift {
                    self.compute_smart_enter_text(insert_pos)
                } else {
                    "\n".to_string()
                };

                self.buffer.insert(insert_pos, &insert_text, insert_pos);
                let new_pos = insert_pos + insert_text.len();
                self.selection = Selection::new(new_pos, new_pos);
                cx.notify();
            }
            "tab" => {
                // Delete selection first if any
                let cursor_before = self.cursor().offset;
                let insert_pos = if !self.selection.is_collapsed() {
                    let range = self.selection.range();
                    self.buffer.delete(range.clone(), cursor_before);
                    range.start
                } else {
                    cursor_before
                };
                self.buffer.insert(insert_pos, "    ", insert_pos);
                self.selection = Selection::new(insert_pos + 4, insert_pos + 4);
                cx.notify();
            }
            "a" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                // Select all
                self.selection = Selection::select_all(&self.buffer);
                cx.notify();
            }
            "c" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                // Copy selection to clipboard
                if !self.selection.is_collapsed() {
                    let range = self.selection.range();
                    let text = &self.buffer.text()[range];
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                }
            }
            "x" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                // Cut selection to clipboard
                if !self.selection.is_collapsed() {
                    let range = self.selection.range();
                    let cursor_before = self.cursor().offset;
                    let text = &self.buffer.text()[range.clone()];
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                    self.buffer.delete(range.clone(), cursor_before);
                    self.selection = Selection::new(range.start, range.start);
                    cx.notify();
                }
            }
            "v" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                // Paste from clipboard, replacing selection if any
                if let Some(clipboard_item) = cx.read_from_clipboard()
                    && let Some(text) = clipboard_item.text()
                {
                    let cursor_before = self.cursor().offset;
                    let insert_pos = if !self.selection.is_collapsed() {
                        let range = self.selection.range();
                        self.buffer.delete(range.clone(), cursor_before);
                        range.start
                    } else {
                        cursor_before
                    };
                    self.buffer.insert(insert_pos, &text, insert_pos);
                    let new_pos = insert_pos + text.len();
                    self.selection = Selection::new(new_pos, new_pos);
                    cx.notify();
                }
            }
            "z" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                if keystroke.modifiers.shift {
                    // Redo: Ctrl+Shift+Z
                    if let Some(cursor_pos) = self.buffer.redo() {
                        self.selection = Selection::new(cursor_pos, cursor_pos);
                        cx.notify();
                    }
                } else {
                    // Undo: Ctrl+Z
                    if let Some(cursor_pos) = self.buffer.undo() {
                        self.selection = Selection::new(cursor_pos, cursor_pos);
                        cx.notify();
                    }
                }
            }
            "y" if keystroke.modifiers.control => {
                // Redo: Ctrl+Y (alternative)
                if let Some(cursor_pos) = self.buffer.redo() {
                    self.selection = Selection::new(cursor_pos, cursor_pos);
                    cx.notify();
                }
            }
            "s" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                // Save file
                self.save(cx);
            }
            _ => {
                // Insert printable characters, replacing selection if any
                if let Some(key_char) = &keystroke.key_char {
                    let cursor_before = self.cursor().offset;
                    let insert_pos = if !self.selection.is_collapsed() {
                        let range = self.selection.range();
                        self.buffer.delete(range.clone(), cursor_before);
                        range.start
                    } else {
                        cursor_before
                    };
                    self.buffer.insert(insert_pos, key_char, insert_pos);
                    let new_pos = insert_pos + key_char.len();
                    self.selection = Selection::new(new_pos, new_pos);
                    cx.notify();
                }
            }
        }

        // Scroll cursor into view after any cursor movement
        // Use on_next_frame to ensure scroll happens after view is laid out with new content
        let cursor_line = self.buffer.byte_to_line(self.selection.head);
        let scroll_handle = self.scroll_handle.clone();
        window.on_next_frame(move |_, _| {
            scroll_handle.scroll_to_item(cursor_line);
        });

        // Sync dirty state to FileInfo global so title bar updates
        self.sync_dirty_state(cx);
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let text_color = theme.foreground;
        let cursor_color = theme.purple;
        let link_color = theme.cyan;
        let selection_color = theme.selection;
        let border_color = theme.comment;
        let fence_color = theme.comment;
        let fence_lang_color = theme.green;
        let cursor_offset = self.selection.head;
        let selection_range = if self.selection.is_collapsed() {
            None
        } else {
            Some(self.selection.range())
        };

        // Platform-specific font defaults
        // Windows: Segoe UI / Consolas
        // macOS: system font / Menlo
        // Linux/other: Liberation Sans / Liberation Mono
        #[cfg(target_os = "windows")]
        let (text_font, code_font): (Font, Font) = (font("Segoe UI"), font("Consolas"));

        #[cfg(target_os = "macos")]
        let (text_font, code_font): (Font, Font) = (font(".AppleSystemUIFont"), font("Menlo"));

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let (text_font, code_font): (Font, Font) =
            (font("Liberation Sans"), font("Liberation Mono"));

        // Get the base path for resolving relative image paths
        let file_info = cx.global::<FileInfo>();
        let base_path: Option<PathBuf> = file_info.path.parent().map(|p| p.to_path_buf());

        // Extract lines from the buffer
        let lines = extract_lines(&self.buffer);
        let buffer_text = self.buffer.text();

        // Pre-compute syntax highlights for all code blocks
        // We parse each code block once and store highlights with buffer-relative offsets
        let code_block_highlights = self.compute_code_block_highlights(&lines, &buffer_text, theme);

        // Create click callback that updates cursor position
        let entity = cx.entity().clone();
        let on_click: ClickCallback =
            Rc::new(move |buffer_offset, shift_held, click_count, _window, cx| {
                entity.update(cx, |editor, cx| {
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
                // Extend selection to drag position (keep anchor, move head)
                editor.selection = editor.selection.extend_to(buffer_offset);
                cx.notify();
            });
        });

        // Create checkbox toggle callback
        let entity = cx.entity().clone();
        let on_checkbox: CheckboxCallback = Rc::new(move |line_number, _window, cx| {
            entity.update(cx, |editor, cx| {
                editor.toggle_checkbox(line_number, cx);
            });
        });

        // Find code block ranges (start_line_idx, end_line_idx) to determine if cursor is in a code block
        // For incomplete blocks (no closing fence), end is None
        let mut code_block_ranges: Vec<(usize, Option<usize>)> = Vec::new();
        {
            let mut i = 0;
            while i < lines.len() {
                if let LineKind::CodeBlock { is_fence: true, .. } = lines[i].kind() {
                    let start = i;
                    i += 1;
                    let mut found_close = false;
                    // Find closing fence
                    while i < lines.len() {
                        if let LineKind::CodeBlock { is_fence: true, .. } = lines[i].kind() {
                            code_block_ranges.push((start, Some(i)));
                            i += 1;
                            found_close = true;
                            break;
                        }
                        i += 1;
                    }
                    // Incomplete block - no closing fence found
                    if !found_close {
                        code_block_ranges.push((start, None));
                    }
                } else {
                    i += 1;
                }
            }
        }

        // Helper: check if cursor is inside a code block (by line index)
        let cursor_line = self.buffer.byte_to_line(cursor_offset);
        let cursor_in_code_block_range = |line_idx: usize| -> bool {
            for (start, end) in &code_block_ranges {
                let block_end = end.unwrap_or(lines.len().saturating_sub(1));
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
        let line_views: Vec<_> = lines
            .iter()
            .enumerate()
            .map(|(line_idx, line)| {
                // For fence lines, check if cursor is in this code block
                let is_fence = matches!(line.kind(), LineKind::CodeBlock { is_fence: true, .. });
                let cursor_in_block = cursor_in_code_block_range(line_idx);

                // Note: We don't skip fence lines anymore - they render as empty lines
                // to maintain consistent line indexing for scroll_to_item

                let inline_styles = extract_inline_styles(&self.buffer, line);

                // Filter pre-computed highlights to those overlapping this line
                let code_highlights: Vec<_> = code_block_highlights
                    .iter()
                    .filter(|(span, _)| {
                        // Include highlights that overlap with this line's range
                        span.range.start < line.range.end && span.range.end > line.range.start
                    })
                    .cloned()
                    .collect();

                // Show block markers for fence lines when cursor is in the code block
                let show_block_markers = is_fence && cursor_in_block;

                LineView::new(
                    line,
                    &buffer_text,
                    cursor_offset,
                    inline_styles,
                    text_color,
                    cursor_color,
                    link_color,
                    selection_color,
                    border_color,
                    fence_color,
                    fence_lang_color,
                    selection_range.clone(),
                    text_font.clone(),
                    code_font.clone(),
                    base_path.clone(),
                    code_highlights,
                    show_block_markers,
                )
                .on_click(on_click.clone())
                .on_drag(on_drag.clone())
                .on_checkbox(on_checkbox.clone())
            })
            .collect();

        div()
            .id("editor")
            .track_focus(&self.focus_handle)
            .key_context("Editor")
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle)
            .font(text_font.clone())
            .text_color(text_color)
            .cursor(CursorStyle::IBeam)
            .children(line_views)
    }
}
