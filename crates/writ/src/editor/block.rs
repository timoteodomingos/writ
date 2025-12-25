use gpui::{
    App, Bounds, Entity, HighlightStyle, IntoElement, MouseButton, MouseDownEvent, SharedString,
    StyledText, TextRun, canvas, fill, prelude::*, px, size,
};
use slotmap::DefaultKey;

use super::{Editor, EditorAction};

/// A view for rendering a single block of text with cursor
pub struct Block {
    pub block_idx: usize,
    pub block_key: DefaultKey,
    pub plain_text: String,
    pub highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    pub cursor_offset: Option<usize>,
    /// Pending marker text to show at cursor (e.g., "*" or "**")
    pub pending_marker: Option<String>,
    pub foreground_color: gpui::Rgba,
    pub editor: Entity<Editor>,
}

impl IntoElement for Block {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        let text_len = self.plain_text.len();

        // Create styled text element and get its layout handle
        let styled_text = StyledText::new(self.plain_text).with_highlights(self.highlights);
        let text_layout = styled_text.layout().clone();
        let layout_for_prepaint = text_layout.clone();

        let editor_for_click = self.editor.clone();
        let block_key = self.block_key;
        let cursor_offset = self.cursor_offset;
        let pending_marker = self.pending_marker;
        let foreground_color = self.foreground_color;

        gpui::div()
            .id(("block", self.block_idx))
            .relative()
            .child(styled_text)
            .child(
                canvas(
                    // Prepaint: store layout and calculate cursor position
                    move |_bounds, _window, cx| {
                        // Store layout for click handling
                        self.editor.update(cx, |ed, _| {
                            ed.block_layouts
                                .insert(block_key, layout_for_prepaint.clone());
                        });

                        // Calculate cursor position if this block has the cursor
                        let cursor_pos =
                            cursor_offset.and_then(|offset| text_layout.position_for_index(offset));

                        // Return data for paint phase
                        (cursor_pos, pending_marker.clone())
                    },
                    // Paint: draw the cursor and pending marker
                    move |_bounds, (cursor_pos, pending_marker), window: &mut gpui::Window, cx| {
                        if let Some(pos) = cursor_pos {
                            // Get line height from window
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            // Paint pending marker (dimmed) if present
                            let mut cursor_x = pos.x;
                            if let Some(ref marker) = pending_marker {
                                // Shape the marker text
                                let marker_text: SharedString = marker.clone().into();
                                let run = TextRun {
                                    len: marker.len(),
                                    font: text_style.font(),
                                    color: gpui::Hsla {
                                        h: 0.0,
                                        s: 0.0,
                                        l: 0.5,
                                        a: 0.7,
                                    },
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                let shaped_marker = window.text_system().shape_line(
                                    marker_text,
                                    font_size,
                                    &[run],
                                    None,
                                );

                                // Paint the marker at cursor position
                                let _ = shaped_marker.paint(pos, line_height, window, cx);

                                // Move cursor position after the marker
                                cursor_x = pos.x + shaped_marker.width;
                            }

                            // Paint cursor bar after any pending marker
                            let cursor_bounds = Bounds::new(
                                gpui::point(cursor_x, pos.y),
                                size(px(2.0), line_height),
                            );
                            window.paint_quad(fill(cursor_bounds, foreground_color));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, _window, cx: &mut App| {
                    editor_for_click.update(cx, |editor, cx| {
                        if let Some(layout) = editor.block_layouts.get(block_key) {
                            let char_index = match layout.index_for_position(event.position) {
                                Ok(idx) => idx,
                                Err(idx) => idx.min(text_len),
                            };
                            editor.state.apply(EditorAction::SetCursor {
                                block_key,
                                offset: char_index,
                            });
                            cx.notify();
                        }
                    });
                },
            )
    }
}
