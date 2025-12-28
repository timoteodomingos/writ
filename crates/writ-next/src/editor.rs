use std::rc::Rc;

use gpui::{
    App, Context, CursorStyle, FocusHandle, Focusable, IntoElement, KeyDownEvent, Window, div,
    prelude::*,
};

use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::line_view::{ClickCallback, LineView};
use crate::lines::{extract_inline_styles, extract_lines};
use crate::theme::Theme;

/// The main editor component.
pub struct Editor {
    /// The text buffer
    buffer: Buffer,
    /// Current cursor position
    cursor: Cursor,
    /// Focus handle for keyboard input
    focus_handle: FocusHandle,
}

impl Editor {
    /// Create a new editor with the given content.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        let buffer: Buffer = content.parse().unwrap_or_default();
        let focus_handle = cx.focus_handle();

        Self {
            buffer,
            cursor: Cursor::start(),
            focus_handle,
        }
    }

    /// Get the buffer contents.
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Handle a key down event.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;

        // Handle special keys
        match keystroke.key.as_str() {
            "backspace" => {
                if self.cursor.offset > 0 {
                    let new_cursor = self.cursor.move_left(&self.buffer);
                    self.buffer.delete(new_cursor.offset..self.cursor.offset);
                    self.cursor = new_cursor;
                    cx.notify();
                }
            }
            "delete" => {
                if self.cursor.offset < self.buffer.len_bytes() {
                    let next = self.cursor.move_right(&self.buffer);
                    self.buffer.delete(self.cursor.offset..next.offset);
                    cx.notify();
                }
            }
            "left" => {
                self.cursor = self.cursor.move_left(&self.buffer);
                cx.notify();
            }
            "right" => {
                self.cursor = self.cursor.move_right(&self.buffer);
                cx.notify();
            }
            "up" => {
                self.cursor = self.cursor.move_up(&self.buffer);
                cx.notify();
            }
            "down" => {
                self.cursor = self.cursor.move_down(&self.buffer);
                cx.notify();
            }
            "home" => {
                if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor = self.cursor.move_to_start();
                } else {
                    self.cursor = self.cursor.move_to_line_start(&self.buffer);
                }
                cx.notify();
            }
            "end" => {
                if keystroke.modifiers.control || keystroke.modifiers.platform {
                    self.cursor = self.cursor.move_to_end(&self.buffer);
                } else {
                    self.cursor = self.cursor.move_to_line_end(&self.buffer);
                }
                cx.notify();
            }
            "enter" => {
                // Insert newline and move cursor after it (to the new line)
                self.buffer.insert(self.cursor.offset, "\n");
                self.cursor = Cursor::new(self.cursor.offset + 1);
                cx.notify();
            }
            "tab" => {
                self.buffer.insert(self.cursor.offset, "    ");
                self.cursor = Cursor::new(self.cursor.offset + 4);
                cx.notify();
            }
            "v" if keystroke.modifiers.control || keystroke.modifiers.platform => {
                // Paste from clipboard
                if let Some(clipboard_item) = cx.read_from_clipboard() {
                    if let Some(text) = clipboard_item.text() {
                        self.buffer.insert(self.cursor.offset, &text);
                        self.cursor = Cursor::new(self.cursor.offset + text.len());
                        cx.notify();
                    }
                }
            }
            _ => {
                // Insert printable characters
                if let Some(key_char) = &keystroke.key_char {
                    self.buffer.insert(self.cursor.offset, key_char);
                    self.cursor = Cursor::new(self.cursor.offset + key_char.len());
                    cx.notify();
                }
            }
        }
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
        let code_color = theme.green;
        let cursor_color = theme.purple;
        let link_color = theme.cyan;
        let cursor_offset = self.cursor.offset;

        // Extract lines from the buffer
        let lines = extract_lines(&self.buffer);
        let buffer_text = self.buffer.text();

        // Create click callback that updates cursor position
        let entity = cx.entity().clone();
        let on_click: ClickCallback = Rc::new(move |buffer_offset, _window, cx| {
            entity.update(cx, |editor, cx| {
                editor.cursor = Cursor::new(buffer_offset);
                cx.notify();
            });
        });

        // Build line views with click handling
        let line_views: Vec<_> = lines
            .iter()
            .map(|line| {
                let inline_styles = extract_inline_styles(&self.buffer, line);
                LineView::new(
                    line,
                    &buffer_text,
                    cursor_offset,
                    inline_styles,
                    code_color,
                    text_color,
                    cursor_color,
                    link_color,
                )
                .on_click(on_click.clone())
            })
            .collect();

        div()
            .id("editor")
            .track_focus(&self.focus_handle)
            .key_context("Editor")
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .overflow_scroll()
            .font_family("Iosevka Aile")
            .text_color(text_color)
            .cursor(CursorStyle::IBeam)
            .children(line_views)
    }
}
