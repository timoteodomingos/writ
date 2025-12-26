use std::rc::Rc;

use gpui::{
    App, Bounds, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent, Rgba,
    SharedString, StyledText, TextLayout, TextRun, Window, canvas, fill, prelude::*, px, rems,
    size,
};

use crate::document::BlockKind;
use crate::theme::Theme;

/// Callback type for click events, receives character index
pub type ClickCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Callback type for layout reporting
pub type LayoutCallback = Rc<dyn Fn(TextLayout, &mut Window, &mut App)>;

/// A view for rendering a single block of text with cursor
pub struct Block {
    pub block_idx: usize,
    pub kind: BlockKind,
    pub plain_text: String,
    pub highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    pub text_color: Rgba,
    pub marker_color: Rgba,
    /// Cursor offset if this block contains the cursor
    pub cursor_offset: Option<usize>,
    /// Pending block marker (e.g. "## " for heading)
    pub pending_block_marker: Option<String>,
    /// Pending inline marker (e.g. "**" for bold)
    pub pending_inline_marker: Option<String>,
    /// Callback when block is clicked, receives character index
    pub on_click: Option<ClickCallback>,
    /// Callback to report layout after prepaint
    pub on_layout: Option<LayoutCallback>,
}

impl Block {
    /// Create an editor Block from a document Block
    pub fn from_document_block(
        block_idx: usize,
        doc_block: &crate::document::Block,
        theme: &Theme,
    ) -> Self {
        let plain_text: String = doc_block
            .text
            .chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect();
        let highlights = doc_block.text.to_highlights(theme);

        Self {
            block_idx,
            kind: doc_block.kind.clone(),
            plain_text,
            highlights,
            text_color: theme.foreground,
            marker_color: theme.comment,
            cursor_offset: None,
            pending_block_marker: None,
            pending_inline_marker: None,
            on_click: None,
            on_layout: None,
        }
    }

    /// Set cursor offset for this block
    pub fn with_cursor_offset(mut self, offset: usize) -> Self {
        self.cursor_offset = Some(offset);
        self
    }

    /// Set pending block marker
    pub fn with_pending_block_marker(mut self, marker: String) -> Self {
        self.pending_block_marker = Some(marker);
        self
    }

    /// Set pending inline marker
    pub fn with_pending_inline_marker(mut self, marker: String) -> Self {
        self.pending_inline_marker = Some(marker);
        self
    }

    /// Set click callback
    pub fn on_click(mut self, callback: ClickCallback) -> Self {
        self.on_click = Some(callback);
        self
    }

    /// Set layout callback
    pub fn on_layout(mut self, callback: LayoutCallback) -> Self {
        self.on_layout = Some(callback);
        self
    }
}

impl IntoElement for Block {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        let text_len = self.plain_text.len();

        // Create styled text element and get its layout handle
        let styled_text = StyledText::new(self.plain_text).with_highlights(self.highlights);
        let text_layout = styled_text.layout().clone();
        let layout_for_prepaint = text_layout.clone();

        let cursor_offset = self.cursor_offset;
        let pending_block_marker = self.pending_block_marker.clone();
        let pending_inline_marker = self.pending_inline_marker.clone();
        let on_layout = self.on_layout.clone();
        let on_click = self.on_click.clone();
        let layout_for_click = layout_for_prepaint.clone();

        // Extract heading level from kind
        let heading_level = match &self.kind {
            BlockKind::Heading { level, .. } => Some(*level),
            _ => None,
        };

        // Apply heading font size if this is a heading block
        let base_div = gpui::div().id(("block", self.block_idx)).relative();

        let mut sized_div = match heading_level {
            Some(1) => base_div.text_size(rems(2.0)),
            Some(2) => base_div.text_size(rems(1.75)),
            Some(3) => base_div.text_size(rems(1.5)),
            Some(4) => base_div.text_size(rems(1.25)),
            Some(5) => base_div.text_size(rems(1.1)),
            Some(6) => base_div.text_size(rems(1.0)),
            _ => base_div,
        };
        if heading_level.is_some() {
            sized_div = sized_div.font_weight(FontWeight::BOLD);
        }

        sized_div
            .child(styled_text)
            .child(
                canvas(
                    // Prepaint: report layout and calculate cursor position
                    move |_bounds, window, cx| {
                        // Report layout via callback
                        if let Some(on_layout) = &on_layout {
                            on_layout(layout_for_prepaint.clone(), window, cx);
                        }

                        // Calculate cursor position if this block has the cursor
                        cursor_offset.and_then(|offset| text_layout.position_for_index(offset))
                    },
                    // Paint: draw the cursor and pending markers
                    move |_bounds, cursor_pos, window: &mut gpui::Window, cx| {
                        if let Some(pos) = cursor_pos {
                            // Get line height from window
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            let mut cursor_x = pos.x;

                            // Paint pending block marker (dimmed) at start of line if present
                            if let Some(ref block_marker) = pending_block_marker {
                                let marker_text: SharedString = block_marker.clone().into();
                                let run = TextRun {
                                    len: block_marker.len(),
                                    font: text_style.font(),
                                    color: self.marker_color.into(),
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

                                // Paint at start of line
                                let _ = shaped_marker.paint(pos, line_height, window, cx);

                                // Move cursor position after the block marker
                                cursor_x = pos.x + shaped_marker.width;
                            }

                            // Paint pending inline marker (dimmed) if present
                            if let Some(ref inline_marker) = pending_inline_marker {
                                // Shape the marker text
                                let marker_text: SharedString = inline_marker.clone().into();
                                let run = TextRun {
                                    len: inline_marker.len(),
                                    font: text_style.font(),
                                    color: self.marker_color.into(),
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
                            window.paint_quad(fill(cursor_bounds, self.text_color));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx: &mut App| {
                    if let Some(on_click) = &on_click {
                        // Use the layout from the styled text to get character index
                        let char_index = match layout_for_click.index_for_position(event.position) {
                            Ok(idx) => idx,
                            Err(idx) => idx.min(text_len),
                        };
                        on_click(char_index, window, cx);
                    }
                },
            )
    }
}
