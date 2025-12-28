//! Block view component for rendering individual blocks.

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent, Rgba,
    SharedString, StyledText, TextRun, Window, canvas, div, point, prelude::*, px, rems,
};

use crate::blocks::RenderBlock;
use crate::render::StyledRegion;

/// Convert a visual character index to a buffer offset, accounting for hidden markers.
fn visual_to_buffer_offset(
    range: &Range<usize>,
    visual_index: usize,
    inline_styles: &[StyledRegion],
    cursor_offset: usize,
) -> usize {
    // Walk through the range, tracking visible vs hidden characters
    let mut buffer_pos = range.start;
    let mut visual_pos = 0;

    while buffer_pos < range.end && visual_pos < visual_index {
        // Check if this position is in a hidden marker
        let mut is_hidden = false;
        for region in inline_styles {
            let cursor_inside =
                cursor_offset >= region.full_range.start && cursor_offset <= region.full_range.end;

            if !cursor_inside {
                // Opening marker
                if buffer_pos >= region.full_range.start && buffer_pos < region.content_range.start
                {
                    is_hidden = true;
                    break;
                }
                // Closing marker
                if buffer_pos >= region.content_range.end && buffer_pos < region.full_range.end {
                    is_hidden = true;
                    break;
                }
            }
        }

        if !is_hidden {
            visual_pos += 1;
        }
        buffer_pos += 1;
    }

    buffer_pos.min(range.end)
}

/// Callback type for click events - receives the buffer offset where the click occurred.
pub type ClickCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A view component for rendering a single block.
pub struct BlockView<'a> {
    /// The block to render
    block: &'a RenderBlock,
    /// The full buffer text (for extracting block content)
    text: &'a str,
    /// Current cursor position in the buffer
    cursor_offset: usize,
    /// Theme colors
    code_color: Rgba,
    text_color: Rgba,
    cursor_color: Rgba,
    /// Unique ID for this block (index in block list)
    block_index: usize,
    /// Callback when block is clicked
    on_click: Option<ClickCallback>,
}

impl<'a> BlockView<'a> {
    /// Create a new block view.
    pub fn new(
        block: &'a RenderBlock,
        text: &'a str,
        cursor_offset: usize,
        code_color: Rgba,
        text_color: Rgba,
        cursor_color: Rgba,
        block_index: usize,
    ) -> Self {
        Self {
            block,
            text,
            cursor_offset,
            code_color,
            text_color,
            cursor_color,
            block_index,
            on_click: None,
        }
    }

    /// Set the click callback for this block.
    pub fn on_click(mut self, callback: ClickCallback) -> Self {
        self.on_click = Some(callback);
        self
    }

    /// Check if the cursor is within this block.
    fn cursor_in_block(&self) -> bool {
        self.block.contains_cursor(self.cursor_offset)
    }

    /// Build the display text and highlights for a paragraph or similar block.
    fn build_styled_content(
        &self,
        content_range: Range<usize>,
        inline_styles: &[StyledRegion],
    ) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
        // Build text and highlights, handling marker visibility
        let mut display_text = String::new();
        let mut highlights = Vec::new();

        // Collect all boundary points
        let mut boundaries: Vec<usize> = vec![content_range.start, content_range.end];

        for region in inline_styles {
            let cursor_inside = self.cursor_offset >= region.full_range.start
                && self.cursor_offset <= region.full_range.end;

            if cursor_inside {
                boundaries.push(region.full_range.start);
                boundaries.push(region.full_range.end);
            } else {
                boundaries.push(region.full_range.start);
                boundaries.push(region.content_range.start);
                boundaries.push(region.content_range.end);
                boundaries.push(region.full_range.end);
            }
        }

        // Filter to content range and sort
        boundaries.retain(|&b| b >= content_range.start && b <= content_range.end);
        boundaries.sort();
        boundaries.dedup();

        // Build spans
        for window in boundaries.windows(2) {
            let start = window[0];
            let end = window[1];

            if start >= end {
                continue;
            }

            // Check if this range should be hidden (marker area when cursor outside)
            let mut is_hidden = false;
            for region in inline_styles {
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

                if !cursor_inside {
                    let in_opening =
                        start >= region.full_range.start && end <= region.content_range.start;
                    let in_closing =
                        start >= region.content_range.end && end <= region.full_range.end;

                    if in_opening || in_closing {
                        is_hidden = true;
                        break;
                    }
                }
            }

            if is_hidden {
                continue;
            }

            // Add text
            let display_start = display_text.len();
            display_text.push_str(&self.text[start..end]);
            let display_end = display_text.len();

            // Compute merged style
            let mut has_style = false;
            let mut highlight = HighlightStyle::default();

            for region in inline_styles {
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

                let style_range = if cursor_inside {
                    &region.full_range
                } else {
                    &region.content_range
                };

                if style_range.start <= start && end <= style_range.end {
                    has_style = true;
                    if region.style.bold {
                        highlight.font_weight = Some(FontWeight::BOLD);
                    }
                    if region.style.italic {
                        highlight.font_style = Some(FontStyle::Italic);
                    }
                    if region.style.code {
                        highlight.color = Some(self.code_color.into());
                    }
                    if region.style.strikethrough {
                        highlight.strikethrough = Some(gpui::StrikethroughStyle {
                            thickness: px(1.0),
                            color: Some(self.text_color.into()),
                        });
                    }
                }
            }

            if has_style {
                highlights.push((display_start..display_end, highlight));
            }
        }

        (display_text, highlights)
    }

    /// Compute the visual cursor position within the displayed text.
    /// Returns None if cursor is not in this block's range.
    fn compute_visual_cursor_pos(
        &self,
        range: &Range<usize>,
        display_text: &str,
        inline_styles: &[StyledRegion],
    ) -> Option<usize> {
        if !self.cursor_in_block() {
            return None;
        }

        // Calculate how much text is hidden before the cursor
        let mut hidden_before_cursor = 0usize;

        for region in inline_styles {
            let cursor_inside = self.cursor_offset >= region.full_range.start
                && self.cursor_offset <= region.full_range.end;

            if !cursor_inside {
                // Opening marker hidden
                let opening_start = region.full_range.start.max(range.start);
                let opening_end = region.content_range.start.min(range.end);
                if opening_end > opening_start && self.cursor_offset > opening_end {
                    hidden_before_cursor += opening_end - opening_start;
                }

                // Closing marker hidden
                let closing_start = region.content_range.end.max(range.start);
                let closing_end = region.full_range.end.min(range.end);
                if closing_end > closing_start && self.cursor_offset > closing_end {
                    hidden_before_cursor += closing_end - closing_start;
                }
            }
        }

        // Visual position = buffer position relative to block start, minus hidden chars
        let buffer_pos_in_block = self.cursor_offset.saturating_sub(range.start);
        let visual_pos = buffer_pos_in_block.saturating_sub(hidden_before_cursor);

        Some(visual_pos.min(display_text.len()))
    }

    /// Render a paragraph block.
    fn render_paragraph(
        &self,
        range: &Range<usize>,
        inline_styles: &[StyledRegion],
    ) -> gpui::Stateful<gpui::Div> {
        let (display_text, highlights) = self.build_styled_content(range.clone(), inline_styles);
        let visual_cursor_pos = self.compute_visual_cursor_pos(range, &display_text, inline_styles);

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_highlights(highlights);
        let text_layout = styled_text.layout().clone();

        let cursor_color = self.cursor_color;

        let mut block_div = div()
            .id(("block", self.block_index))
            .relative()
            .child(styled_text);

        // Add cursor overlay if cursor is in this block
        if let Some(cursor_pos) = visual_cursor_pos {
            block_div = block_div.child(
                canvas(
                    {
                        let text_layout = text_layout.clone();
                        move |_bounds, _window, _cx| text_layout.position_for_index(cursor_pos)
                    },
                    move |_bounds, cursor_pos_result, window: &mut Window, cx| {
                        if let Some(pos) = cursor_pos_result {
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            let cursor_char: SharedString = "\u{258F}".into();
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

                            let cursor_height = cursor_font_size * 1.2;
                            let y_offset = (line_height - cursor_height) / 2.0;
                            let cursor_pos = point(pos.x, pos.y + y_offset);
                            let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        }

        // Add click handler
        if let Some(on_click) = &self.on_click {
            let on_click = on_click.clone();
            let layout_for_click = text_layout.clone();
            let block_range = range.clone();
            let click_inline_styles: Vec<StyledRegion> = inline_styles.to_vec();
            let current_cursor = self.cursor_offset;

            block_div = block_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    // Get visual character index from click position
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // Convert visual index to buffer offset
                    let buffer_offset = visual_to_buffer_offset(
                        &block_range,
                        visual_index,
                        &click_inline_styles,
                        current_cursor,
                    );

                    on_click(buffer_offset, window, cx);
                },
            );
        }

        block_div
    }

    /// Render a heading block with appropriate font size and marker hiding.
    fn render_heading(
        &self,
        level: u8,
        _range: &Range<usize>,
        marker_range: &Range<usize>,
        content_range: &Range<usize>,
        inline_styles: &[StyledRegion],
    ) -> gpui::Stateful<gpui::Div> {
        let cursor_in_block = self.cursor_in_block();

        // Build display text - hide marker if cursor is outside this block
        let mut display_text = String::new();
        let mut highlights = Vec::new();

        if cursor_in_block {
            // Show the full heading including marker
            // First add the marker
            display_text.push_str(&self.text[marker_range.clone()]);

            // Then add content with inline styles
            let (content_text, mut content_highlights) =
                self.build_styled_content(content_range.clone(), inline_styles);

            // Adjust highlight offsets for the marker prefix
            let offset = display_text.len();
            for (range, _) in &mut content_highlights {
                range.start += offset;
                range.end += offset;
            }

            display_text.push_str(&content_text);
            highlights.extend(content_highlights);
        } else {
            // Hide marker, just show content
            let (content_text, content_highlights) =
                self.build_styled_content(content_range.clone(), inline_styles);
            display_text = content_text;
            highlights = content_highlights;
        }

        // Trim trailing newline for display
        let display_text = display_text.trim_end_matches('\n').to_string();

        // Compute visual cursor position for heading
        let visual_cursor_pos = if self.cursor_in_block() {
            // For headings, cursor position is relative to what's displayed
            let buffer_pos = self.cursor_offset;
            let display_start = if cursor_in_block {
                marker_range.start // showing marker
            } else {
                content_range.start // hiding marker
            };
            let pos_in_display = buffer_pos.saturating_sub(display_start);
            Some(pos_in_display.min(display_text.len()))
        } else {
            None
        };

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_highlights(highlights);
        let text_layout = styled_text.layout().clone();

        let cursor_color = self.cursor_color;

        // Apply font size and bold based on heading level
        let base_div = div()
            .id(("block", self.block_index))
            .relative()
            .font_weight(FontWeight::BOLD);

        let mut sized_div = match level {
            1 => base_div.text_size(rems(2.0)),
            2 => base_div.text_size(rems(1.75)),
            3 => base_div.text_size(rems(1.5)),
            4 => base_div.text_size(rems(1.25)),
            5 => base_div.text_size(rems(1.1)),
            _ => base_div,
        };

        sized_div = sized_div.child(styled_text);

        // Add cursor overlay if cursor is in this block
        if let Some(cursor_pos) = visual_cursor_pos {
            sized_div = sized_div.child(
                canvas(
                    {
                        let text_layout = text_layout.clone();
                        move |_bounds, _window, _cx| text_layout.position_for_index(cursor_pos)
                    },
                    move |_bounds, cursor_pos_result, window: &mut Window, cx| {
                        if let Some(pos) = cursor_pos_result {
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            let cursor_char: SharedString = "\u{258F}".into();
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

                            let cursor_height = cursor_font_size * 1.2;
                            let y_offset = (line_height - cursor_height) / 2.0;
                            let cursor_pos = point(pos.x, pos.y + y_offset);
                            let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        }

        // Add click handler
        if let Some(on_click) = &self.on_click {
            let on_click = on_click.clone();
            let layout_for_click = text_layout.clone();
            let click_marker_range = marker_range.clone();
            let click_content_range = content_range.clone();
            let cursor_in = cursor_in_block;

            sized_div = sized_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    // Get visual character index from click position
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // Convert visual index to buffer offset
                    // For headings, if cursor is in block, marker is shown; otherwise hidden
                    let buffer_offset = if cursor_in {
                        // Marker is visible, so visual index maps directly to buffer
                        click_marker_range.start + visual_index
                    } else {
                        // Marker is hidden, so visual index is relative to content
                        click_content_range.start + visual_index
                    };

                    on_click(buffer_offset.min(click_content_range.end), window, cx);
                },
            );
        }

        sized_div
    }

    /// Render a list item block with marker hiding.
    fn render_list_item(
        &self,
        _range: &Range<usize>,
        marker_range: &Range<usize>,
        content_range: &Range<usize>,
        inline_styles: &[StyledRegion],
    ) -> gpui::Stateful<gpui::Div> {
        let cursor_in_block = self.cursor_in_block();

        // Build display text - hide marker if cursor is outside this block
        let mut display_text = String::new();
        let mut highlights = Vec::new();

        if cursor_in_block {
            // Show the full list item including marker
            display_text.push_str(&self.text[marker_range.clone()]);

            // Then add content with inline styles
            let (content_text, mut content_highlights) =
                self.build_styled_content(content_range.clone(), inline_styles);

            // Adjust highlight offsets for the marker prefix
            let offset = display_text.len();
            for (range, _) in &mut content_highlights {
                range.start += offset;
                range.end += offset;
            }

            display_text.push_str(&content_text);
            highlights.extend(content_highlights);
        } else {
            // Hide marker, show bullet point and content
            display_text.push_str("• ");
            let bullet_offset = display_text.len();

            let (content_text, mut content_highlights) =
                self.build_styled_content(content_range.clone(), inline_styles);

            // Adjust highlight offsets
            for (range, _) in &mut content_highlights {
                range.start += bullet_offset;
                range.end += bullet_offset;
            }

            display_text.push_str(&content_text);
            highlights.extend(content_highlights);
        }

        // Trim trailing newline for display
        let display_text = display_text.trim_end_matches('\n').to_string();

        // Compute visual cursor position
        let visual_cursor_pos = if self.cursor_in_block() {
            let buffer_pos = self.cursor_offset;
            let display_start = if cursor_in_block {
                marker_range.start
            } else {
                content_range.start
            };
            let pos_in_display = buffer_pos.saturating_sub(display_start);
            Some(pos_in_display.min(display_text.len()))
        } else {
            None
        };

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_highlights(highlights);
        let text_layout = styled_text.layout().clone();

        let cursor_color = self.cursor_color;

        let mut block_div = div()
            .id(("block", self.block_index))
            .relative()
            .child(styled_text);

        // Add cursor overlay if cursor is in this block
        if let Some(cursor_pos) = visual_cursor_pos {
            block_div = block_div.child(
                canvas(
                    {
                        let text_layout = text_layout.clone();
                        move |_bounds, _window, _cx| text_layout.position_for_index(cursor_pos)
                    },
                    move |_bounds, cursor_pos_result, window: &mut Window, cx| {
                        if let Some(pos) = cursor_pos_result {
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            let cursor_char: SharedString = "\u{258F}".into();
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

                            let cursor_height = cursor_font_size * 1.2;
                            let y_offset = (line_height - cursor_height) / 2.0;
                            let cursor_pos = point(pos.x, pos.y + y_offset);
                            let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        }

        // Add click handler for list items
        if let Some(on_click) = &self.on_click {
            let on_click = on_click.clone();
            let layout_for_click = text_layout.clone();
            let click_marker_range = marker_range.clone();
            let click_content_range = content_range.clone();
            let cursor_in = cursor_in_block;

            block_div = block_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // For list items, if cursor is in block, marker is shown; otherwise bullet is shown
                    let buffer_offset = if cursor_in {
                        click_marker_range.start + visual_index
                    } else {
                        // "• " is 4 bytes, so subtract that from visual index
                        let adjusted = visual_index.saturating_sub(4);
                        click_content_range.start + adjusted
                    };

                    on_click(buffer_offset.min(click_content_range.end), window, cx);
                },
            );
        }

        block_div
    }

    /// Render a blockquote block with marker hiding.
    fn render_blockquote(
        &self,
        range: &Range<usize>,
        marker_range: &Range<usize>,
        nested_blocks: &[RenderBlock],
    ) -> gpui::Stateful<gpui::Div> {
        let cursor_in_block = self.cursor_in_block();

        // For blockquotes, we need to render nested blocks
        // For simplicity, just render the text with/without marker for now

        // Build display text
        let mut display_text = String::new();

        if cursor_in_block {
            // Show everything including > marker
            display_text.push_str(&self.text[range.clone()]);
        } else {
            // Hide marker, indent with a bar
            // For nested blocks, we'd recursively render them
            // For now, just show content after marker
            let content_start = marker_range.end;
            if content_start < range.end {
                display_text.push_str(&self.text[content_start..range.end]);
            }
        }

        // Trim trailing newline
        let display_text = display_text.trim_end_matches('\n').to_string();

        // Compute visual cursor position
        let visual_cursor_pos = if self.cursor_in_block() {
            let buffer_pos = self.cursor_offset;
            let display_start = if cursor_in_block {
                range.start
            } else {
                marker_range.end
            };
            let pos_in_display = buffer_pos.saturating_sub(display_start);
            Some(pos_in_display.min(display_text.len()))
        } else {
            None
        };

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text);
        let text_layout = styled_text.layout().clone();

        let cursor_color = self.cursor_color;

        // Style blockquote with left border
        let mut block_div = div()
            .id(("block", self.block_index))
            .relative()
            .pl_3() // padding left
            .border_l_2() // left border
            .border_color(gpui::rgb(0x6272a4)) // Dracula comment color
            .child(styled_text);

        // Add cursor overlay if cursor is in this block
        if let Some(cursor_pos) = visual_cursor_pos {
            block_div = block_div.child(
                canvas(
                    {
                        let text_layout = text_layout.clone();
                        move |_bounds, _window, _cx| text_layout.position_for_index(cursor_pos)
                    },
                    move |_bounds, cursor_pos_result, window: &mut Window, cx| {
                        if let Some(pos) = cursor_pos_result {
                            let text_style = window.text_style();
                            let font_size = text_style.font_size.to_pixels(window.rem_size());
                            let line_height = text_style
                                .line_height
                                .to_pixels(font_size.into(), window.rem_size());

                            let cursor_char: SharedString = "\u{258F}".into();
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

                            let cursor_height = cursor_font_size * 1.2;
                            let y_offset = (line_height - cursor_height) / 2.0;
                            let cursor_pos = point(pos.x, pos.y + y_offset);
                            let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        }

        // Mark nested_blocks as used (will be used for proper nested rendering later)
        let _ = nested_blocks;

        // Add click handler for blockquotes
        if let Some(on_click) = &self.on_click {
            let on_click = on_click.clone();
            let layout_for_click = text_layout.clone();
            let click_range = range.clone();
            let click_marker_range = marker_range.clone();
            let cursor_in = cursor_in_block;

            block_div = block_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // For blockquotes, if cursor is in block, full text shown; otherwise marker hidden
                    let buffer_offset = if cursor_in {
                        click_range.start + visual_index
                    } else {
                        click_marker_range.end + visual_index
                    };

                    on_click(buffer_offset.min(click_range.end), window, cx);
                },
            );
        }

        block_div
    }
}

impl IntoElement for BlockView<'_> {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        match self.block {
            RenderBlock::Paragraph {
                range,
                inline_styles,
            } => self.render_paragraph(range, inline_styles),

            RenderBlock::Heading {
                level,
                range,
                marker_range,
                content_range,
                inline_styles,
            } => self.render_heading(*level, range, marker_range, content_range, inline_styles),

            RenderBlock::ListItem {
                range,
                marker_range,
                content_range,
                inline_styles,
                ..
            } => self.render_list_item(range, marker_range, content_range, inline_styles),

            RenderBlock::BlockQuote {
                range,
                marker_range,
                nested_blocks,
            } => self.render_blockquote(range, marker_range, nested_blocks),

            RenderBlock::CodeBlock { range, .. } => {
                // For now, just show the text
                let text = &self.text[range.clone()];
                let shared_text: SharedString = text.to_string().into();
                let styled_text = StyledText::new(shared_text);
                div().id(("block", self.block_index)).child(styled_text)
            }
        }
    }
}
