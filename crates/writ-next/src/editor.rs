use gpui::{
    App, Context, CursorStyle, FocusHandle, Focusable, FontStyle, FontWeight, HighlightStyle,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, StyledText, TextRun,
    Window, canvas, div, point, prelude::*, px,
};

use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::render::{
    TextStyle, buffer_to_visual_offset, compute_render_spans, visual_to_buffer_offset,
};
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
                self.buffer.insert(self.cursor.offset, "\n");
                self.cursor = Cursor::new(self.cursor.offset + 1);
                cx.notify();
            }
            "tab" => {
                self.buffer.insert(self.cursor.offset, "    ");
                self.cursor = Cursor::new(self.cursor.offset + 4);
                cx.notify();
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

    /// Handle mouse click to position cursor.
    fn on_click(&mut self, visual_index: usize, _window: &mut Window, cx: &mut Context<Self>) {
        // Convert visual index to buffer offset using render spans
        // Use current cursor position to compute spans (matches what's displayed)
        let spans = compute_render_spans(&self.buffer, self.cursor.offset);
        let buffer_offset = visual_to_buffer_offset(&spans, visual_index);

        self.cursor = Cursor::new(buffer_offset.min(self.buffer.len_bytes()));
        cx.notify();
    }

    /// Build styled text from render spans.
    fn build_styled_text(
        &self,
        theme: &Theme,
    ) -> (String, Vec<(std::ops::Range<usize>, HighlightStyle)>) {
        let spans = compute_render_spans(&self.buffer, self.cursor.offset);

        let mut text = String::new();
        let mut highlights = Vec::new();

        for span in spans {
            let start = text.len();
            text.push_str(&span.text);
            let end = text.len();

            if span.style != TextStyle::default() {
                let mut highlight = HighlightStyle::default();

                if span.style.bold {
                    highlight.font_weight = Some(FontWeight::BOLD);
                }
                if span.style.italic {
                    highlight.font_style = Some(FontStyle::Italic);
                }
                if span.style.code {
                    highlight.color = Some(theme.green.into());
                }
                if span.style.strikethrough {
                    highlight.strikethrough = Some(gpui::StrikethroughStyle {
                        thickness: px(1.0),
                        color: Some(theme.foreground.into()),
                    });
                }

                highlights.push((start..end, highlight));
            }
        }

        (text, highlights)
    }

    /// Calculate the visual cursor position given render spans.
    fn visual_cursor_position(&self) -> usize {
        let spans = compute_render_spans(&self.buffer, self.cursor.offset);
        buffer_to_visual_offset(&spans, self.cursor.offset)
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
        let (text, highlights) = self.build_styled_text(theme);
        let visual_cursor_pos = self.visual_cursor_position();

        let cursor_color = theme.purple;
        let text_color = theme.foreground;

        // Create the styled text with highlights
        let display_text: SharedString = text.clone().into();
        let styled_text = StyledText::new(display_text.clone()).with_highlights(highlights);
        let text_layout = styled_text.layout().clone();

        div()
            .id("editor")
            .track_focus(&self.focus_handle)
            .key_context("Editor")
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .font_family("Iosevka Aile")
            .text_color(text_color)
            .cursor(CursorStyle::IBeam)
            .child(styled_text)
            .child(
                canvas(
                    {
                        let text_layout = text_layout.clone();
                        move |_bounds, _window, _cx| {
                            // Calculate cursor position from layout
                            text_layout.position_for_index(visual_cursor_pos)
                        }
                    },
                    move |_bounds, cursor_pos, window: &mut Window, cx| {
                        if let Some(pos) = cursor_pos {
                            // Get text metrics
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            // Paint cursor glyph
                            let cursor_char: SharedString = "\u{258F}".into(); // LEFT ONE EIGHTH BLOCK
                            let cursor_font_size = font_size * 1.4;
                            let cursor_run = TextRun {
                                len: cursor_char.len(),
                                font: text_style.font(),
                                color: cursor_color.into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };

                            let shaped_cursor = window.text_system().shape_line(
                                cursor_char,
                                cursor_font_size,
                                &[cursor_run],
                                None,
                            );

                            // Center cursor vertically
                            let cursor_height = cursor_font_size * 1.2;
                            let y_offset = (line_height - cursor_height) / 2.0;
                            let cursor_pos = point(pos.x, pos.y + y_offset);
                            let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, {
                let text_layout = text_layout.clone();
                let text_len = text.len();
                cx.listener(move |editor, event: &MouseDownEvent, window, cx| {
                    let index = match text_layout.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx.min(text_len),
                    };
                    editor.on_click(index, window, cx);
                })
            })
    }
}
