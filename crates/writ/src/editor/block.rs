use std::rc::Rc;

use gpui::{
    App, BorderStyle, Bounds, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent,
    Pixels, Rgba, SharedString, StyledText, TextLayout, TextRun, Window, canvas, prelude::*, px,
    quad, rems, size,
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
    pub selection_color: Rgba,
    pub cursor_color: Rgba,
    /// Cursor offset if this block contains the cursor
    pub cursor_offset: Option<usize>,
    /// Pending block marker (e.g. "## " for heading)
    pub pending_block_marker: Option<String>,
    /// Pending inline marker (e.g. "**" for bold)
    pub pending_inline_marker: Option<String>,
    /// Active styles indicator (e.g. "BI" for bold+italic)
    pub active_styles_indicator: Option<String>,
    /// Ranges of link marker characters to highlight in marker color
    pub link_marker_ranges: Vec<std::ops::Range<usize>>,
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
            selection_color: theme.selection,
            cursor_color: theme.purple,
            cursor_offset: None,
            pending_block_marker: None,
            pending_inline_marker: None,
            active_styles_indicator: None,
            link_marker_ranges: Vec::new(),
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

    /// Set active styles indicator
    pub fn with_active_styles_indicator(mut self, indicator: String) -> Self {
        self.active_styles_indicator = Some(indicator);
        self
    }

    /// Set link marker ranges (characters to highlight in marker color)
    pub fn with_link_marker_ranges(mut self, ranges: Vec<std::ops::Range<usize>>) -> Self {
        self.link_marker_ranges = ranges;
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

        // Combine regular highlights with link marker highlights
        let mut all_highlights = self.highlights;
        for range in self.link_marker_ranges {
            all_highlights.push((
                range,
                HighlightStyle {
                    color: Some(self.marker_color.into()),
                    ..Default::default()
                },
            ));
        }

        // Create styled text element and get its layout handle
        let styled_text = StyledText::new(self.plain_text).with_highlights(all_highlights);
        let text_layout = styled_text.layout().clone();
        let layout_for_prepaint = text_layout.clone();

        let cursor_offset = self.cursor_offset;
        let pending_block_marker = self.pending_block_marker.clone();
        let pending_inline_marker = self.pending_inline_marker.clone();
        let active_styles_indicator = self.active_styles_indicator.clone();
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

                            // Paint cursor using a font glyph for consistent rendering
                            let cursor_char: SharedString = "▏".into(); // U+258F LEFT ONE EIGHTH BLOCK
                            let cursor_font_size = font_size * 1.4; // Larger to ensure full height
                            let base_font = text_style.font();
                            let cursor_font = gpui::Font {
                                family: base_font.family.clone(),
                                features: base_font.features.clone(),
                                fallbacks: base_font.fallbacks.clone(),
                                weight: FontWeight::BOLD,
                                style: base_font.style,
                            };
                            let cursor_run = TextRun {
                                len: cursor_char.len(),
                                font: cursor_font,
                                color: self.cursor_color.into(),
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

                            // Center the cursor vertically on the text line
                            let cursor_height = cursor_font_size * 1.2; // Approximate line height
                            let y_offset = (line_height - cursor_height) / 2.0;
                            let cursor_pos = gpui::point(cursor_x, pos.y + y_offset);
                            let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);

                            // Paint active styles indicator as floating tooltip above cursor
                            if let Some(ref indicator) = active_styles_indicator {
                                let base_font = text_style.font();

                                // Use smaller font for tooltip
                                let tooltip_font_size = font_size * 0.75;
                                let tooltip_line_height = tooltip_font_size * 1.2;

                                // First, measure total width of indicator by shaping all chars
                                let mut shaped_chars = Vec::new();
                                let mut total_width = px(0.0);
                                let padding_x = px(4.0);
                                let padding_y = px(1.0);
                                let tooltip_gap = px(4.0); // Gap between tooltip and cursor
                                let border_width = px(1.0);
                                let corner_radius = px(3.0);

                                for ch in indicator.chars() {
                                    let (weight, style, strikethrough) = match ch {
                                        'B' => (FontWeight::BOLD, gpui::FontStyle::Normal, None),
                                        'I' => {
                                            (FontWeight::default(), gpui::FontStyle::Italic, None)
                                        }
                                        'S' => (
                                            FontWeight::default(),
                                            gpui::FontStyle::Normal,
                                            Some(gpui::StrikethroughStyle {
                                                thickness: px(1.0),
                                                color: Some(self.text_color.into()),
                                            }),
                                        ),
                                        _ => (FontWeight::default(), gpui::FontStyle::Normal, None),
                                    };

                                    let font = gpui::Font {
                                        family: base_font.family.clone(),
                                        features: base_font.features.clone(),
                                        fallbacks: base_font.fallbacks.clone(),
                                        weight,
                                        style,
                                    };

                                    let char_text: SharedString = ch.to_string().into();
                                    let run = TextRun {
                                        len: ch.len_utf8(),
                                        font,
                                        color: self.text_color.into(),
                                        background_color: None,
                                        underline: None,
                                        strikethrough,
                                    };

                                    let shaped_char = window.text_system().shape_line(
                                        char_text,
                                        tooltip_font_size,
                                        &[run],
                                        None,
                                    );

                                    total_width += shaped_char.width;
                                    shaped_chars.push(shaped_char);
                                }

                                // Calculate tooltip position (above cursor, centered on cursor)
                                let tooltip_height = tooltip_line_height + padding_y * 2.0;
                                let tooltip_width = total_width + padding_x * 2.0;
                                let tooltip_x = cursor_x - tooltip_width / 2.0 + px(1.0); // Center on cursor
                                let tooltip_y = pos.y - tooltip_height - tooltip_gap;

                                // Paint background with rounded corners and border
                                let bg_bounds = Bounds::new(
                                    gpui::point(tooltip_x, tooltip_y),
                                    size(tooltip_width, tooltip_height),
                                );
                                window.paint_quad(quad(
                                    bg_bounds,
                                    corner_radius,
                                    self.marker_color,
                                    gpui::Edges::<Pixels> {
                                        top: border_width,
                                        right: border_width,
                                        bottom: border_width,
                                        left: border_width,
                                    },
                                    self.selection_color,
                                    BorderStyle::Solid,
                                ));

                                // Paint each styled character
                                let mut current_x = tooltip_x + padding_x;
                                let text_y = tooltip_y + padding_y;

                                for shaped_char in shaped_chars {
                                    let _ = shaped_char.paint(
                                        gpui::point(current_x, text_y),
                                        tooltip_line_height,
                                        window,
                                        cx,
                                    );

                                    current_x += shaped_char.width;
                                }
                            }
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
