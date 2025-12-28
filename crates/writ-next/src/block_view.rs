//! Block view component for rendering individual blocks.

use std::ops::Range;

use gpui::{
    FontStyle, FontWeight, HighlightStyle, IntoElement, Rgba, SharedString, StyledText, div,
    prelude::*, px, rems,
};

use crate::blocks::RenderBlock;
use crate::render::StyledRegion;

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
}

impl<'a> BlockView<'a> {
    /// Create a new block view.
    pub fn new(
        block: &'a RenderBlock,
        text: &'a str,
        cursor_offset: usize,
        code_color: Rgba,
        text_color: Rgba,
    ) -> Self {
        Self {
            block,
            text,
            cursor_offset,
            code_color,
            text_color,
        }
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

    /// Render a paragraph block.
    fn render_paragraph(&self, range: &Range<usize>, inline_styles: &[StyledRegion]) -> gpui::Div {
        let (display_text, highlights) = self.build_styled_content(range.clone(), inline_styles);
        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_highlights(highlights);

        div().child(styled_text)
    }

    /// Render a heading block with appropriate font size and marker hiding.
    fn render_heading(
        &self,
        level: u8,
        _range: &Range<usize>,
        marker_range: &Range<usize>,
        content_range: &Range<usize>,
        inline_styles: &[StyledRegion],
    ) -> gpui::Div {
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

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_highlights(highlights);

        // Apply font size and bold based on heading level
        let base_div = div().font_weight(FontWeight::BOLD);

        match level {
            1 => base_div.text_size(rems(2.0)).child(styled_text),
            2 => base_div.text_size(rems(1.75)).child(styled_text),
            3 => base_div.text_size(rems(1.5)).child(styled_text),
            4 => base_div.text_size(rems(1.25)).child(styled_text),
            5 => base_div.text_size(rems(1.1)).child(styled_text),
            _ => base_div.child(styled_text),
        }
    }
}

impl IntoElement for BlockView<'_> {
    type Element = gpui::Div;

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
                inline_styles,
                ..
            } => {
                // For now, render list items like paragraphs
                self.render_paragraph(range, inline_styles)
            }

            RenderBlock::BlockQuote { range, .. } => {
                // For now, just show the text
                let text = &self.text[range.clone()];
                let shared_text: SharedString = text.to_string().into();
                let styled_text = StyledText::new(shared_text);
                div().child(styled_text)
            }

            RenderBlock::CodeBlock { range, .. } => {
                // For now, just show the text
                let text = &self.text[range.clone()];
                let shared_text: SharedString = text.to_string().into();
                let styled_text = StyledText::new(shared_text);
                div().child(styled_text)
            }
        }
    }
}
