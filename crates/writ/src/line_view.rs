//! Line view component for rendering individual lines.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, IntoElement, MouseButton, MouseDownEvent, Rgba,
    SharedString, StyledText, TextRun, Window, canvas, div, img, point, prelude::*, px, rems,
};

use crate::lines::{LineInfo, LineKind};
use crate::render::StyledRegion;

/// Callback type for click events - receives the buffer offset where the click occurred.
pub type ClickCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Represents a resolved image source for rendering.
enum ImageSource {
    Url(String),
    Path(PathBuf),
}

/// A view component for rendering a single line.
pub struct LineView<'a> {
    /// The line info
    line: &'a LineInfo,
    /// The full buffer text
    text: &'a str,
    /// Current cursor position in the buffer
    cursor_offset: usize,
    /// Inline styles for this line (bold, italic, code, etc.)
    inline_styles: Vec<StyledRegion>,
    /// Theme colors
    code_color: Rgba,
    text_color: Rgba,
    cursor_color: Rgba,
    link_color: Rgba,
    /// Base path for resolving relative image paths (directory containing the markdown file)
    base_path: Option<PathBuf>,
    /// Callback when line is clicked
    on_click: Option<ClickCallback>,
}

impl<'a> LineView<'a> {
    /// Create a new line view.
    pub fn new(
        line: &'a LineInfo,
        text: &'a str,
        cursor_offset: usize,
        inline_styles: Vec<StyledRegion>,
        code_color: Rgba,
        text_color: Rgba,
        cursor_color: Rgba,
        link_color: Rgba,
        base_path: Option<PathBuf>,
    ) -> Self {
        Self {
            line,
            text,
            cursor_offset,
            inline_styles,
            code_color,
            text_color,
            cursor_color,
            link_color,
            base_path,
            on_click: None,
        }
    }

    /// Set the click callback for this line.
    pub fn on_click(mut self, callback: ClickCallback) -> Self {
        self.on_click = Some(callback);
        self
    }

    /// Resolve an image path to an absolute path or URL.
    ///
    /// - URLs (http://, https://) are returned as-is
    /// - Absolute paths are returned as-is
    /// - Relative paths are resolved against the base_path (markdown file's directory)
    fn resolve_image_source(&self, image_path: &str) -> ImageSource {
        // Check if it's a URL
        if image_path.starts_with("http://") || image_path.starts_with("https://") {
            return ImageSource::Url(image_path.to_string());
        }

        let path = Path::new(image_path);

        // If it's an absolute path, use it directly
        if path.is_absolute() {
            return ImageSource::Path(path.to_path_buf());
        }

        // It's a relative path - resolve against base_path
        if let Some(ref base) = self.base_path {
            let resolved = base.join(path);
            return ImageSource::Path(resolved);
        }

        // No base path available, try as-is (might fail)
        ImageSource::Path(path.to_path_buf())
    }

    /// Check if the cursor is on this line.
    fn cursor_on_line(&self) -> bool {
        let range = &self.line.range;
        // Cursor is on this line if it's within the range, or at the end for empty lines
        if range.start == range.end {
            // Empty line - cursor is here if it equals the position
            self.cursor_offset == range.start
        } else {
            self.cursor_offset >= range.start && self.cursor_offset <= range.end
        }
    }

    /// Get the content range, accounting for hidden markers.
    fn content_range(&self) -> Range<usize> {
        let range = &self.line.range;

        // If cursor is on this line, show markers; otherwise hide them
        if self.cursor_on_line() {
            range.clone()
        } else if let Some(marker_range) = &self.line.marker_range {
            // Hide the marker
            marker_range.end..range.end
        } else {
            range.clone()
        }
    }

    /// Build the display text and highlights.
    fn build_styled_content(&self) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
        let content_range = self.content_range();

        if content_range.start >= content_range.end {
            return (String::new(), Vec::new());
        }

        let mut display_text = String::new();
        let mut highlights = Vec::new();

        // Collect all boundary points from inline styles
        let mut boundaries: Vec<usize> = vec![content_range.start, content_range.end];

        for region in &self.inline_styles {
            // Only include boundaries within our content range
            if region.full_range.end > content_range.start
                && region.full_range.start < content_range.end
            {
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

                if cursor_inside {
                    boundaries.push(region.full_range.start.max(content_range.start));
                    boundaries.push(region.full_range.end.min(content_range.end));
                } else {
                    boundaries.push(region.full_range.start.max(content_range.start));
                    boundaries.push(region.content_range.start.max(content_range.start));
                    boundaries.push(region.content_range.end.min(content_range.end));
                    boundaries.push(region.full_range.end.min(content_range.end));
                }
            }
        }

        // Filter, sort, dedup
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

            // Check if this range should be hidden (inline marker when cursor outside)
            let mut is_hidden = false;
            for region in &self.inline_styles {
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

            for region in &self.inline_styles {
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
                    if region.link_url.is_some() {
                        highlight.color = Some(self.link_color.into());
                        highlight.underline = Some(gpui::UnderlineStyle {
                            thickness: px(1.0),
                            color: Some(self.link_color.into()),
                            wavy: false,
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
    fn compute_visual_cursor_pos(&self, display_text: &str) -> Option<usize> {
        if !self.cursor_on_line() {
            return None;
        }

        let content_range = self.content_range();

        // For empty lines, cursor is at position 0
        if content_range.start >= content_range.end {
            return Some(0);
        }

        // Calculate how much text is hidden before the cursor
        let mut hidden_before_cursor = 0usize;

        // Account for hidden block marker
        if !self.cursor_on_line() {
            if let Some(marker_range) = &self.line.marker_range {
                if self.cursor_offset > marker_range.end {
                    hidden_before_cursor += marker_range.end - marker_range.start;
                }
            }
        }

        // Account for hidden inline markers
        for region in &self.inline_styles {
            let cursor_inside = self.cursor_offset >= region.full_range.start
                && self.cursor_offset <= region.full_range.end;

            if !cursor_inside {
                // Opening marker hidden
                let opening_start = region.full_range.start.max(content_range.start);
                let opening_end = region.content_range.start.min(content_range.end);
                if opening_end > opening_start && self.cursor_offset > opening_end {
                    hidden_before_cursor += opening_end - opening_start;
                }

                // Closing marker hidden
                let closing_start = region.content_range.end.max(content_range.start);
                let closing_end = region.full_range.end.min(content_range.end);
                if closing_end > closing_start && self.cursor_offset > closing_end {
                    hidden_before_cursor += closing_end - closing_start;
                }
            }
        }

        // Visual position = buffer position relative to content start, minus hidden chars
        let buffer_pos_in_content = self.cursor_offset.saturating_sub(content_range.start);
        let visual_pos = buffer_pos_in_content.saturating_sub(hidden_before_cursor);

        Some(visual_pos.min(display_text.len()))
    }

    /// Render the cursor overlay.
    fn render_cursor(&self, cursor_pos: usize, text_layout: gpui::TextLayout) -> impl IntoElement {
        let cursor_color = self.cursor_color;

        canvas(
            move |_bounds, _window, _cx| text_layout.position_for_index(cursor_pos),
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
        .size_full()
    }
}

impl IntoElement for LineView<'_> {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        let line_number = self.line.line_number;
        let line_range = self.line.range.clone();

        // Handle image-only lines specially
        if let Some(ref image_url) = self.line.image_url {
            let alt_text = self.line.image_alt.clone().unwrap_or_default();

            // Resolve the image source (URL, absolute path, or relative path)
            let image_source = self.resolve_image_source(image_url);

            // Create the image element based on the resolved source
            let create_image = |source: &ImageSource, alt: String| -> gpui::Img {
                match source {
                    ImageSource::Url(url) => img(url.clone())
                        .max_w_full()
                        .with_fallback(move || div().child(alt.clone()).into_any_element()),
                    ImageSource::Path(path) => img(path.clone())
                        .max_w_full()
                        .with_fallback(move || div().child(alt.clone()).into_any_element()),
                }
            };

            if self.cursor_on_line() {
                // Cursor on line: show text AND image
                let (display_text, highlights) = self.build_styled_content();
                let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

                let display_text = if display_text.is_empty() {
                    " ".to_string()
                } else {
                    display_text
                };

                let shared_text: SharedString = display_text.into();
                let styled_text = StyledText::new(shared_text).with_highlights(highlights);
                let text_layout = styled_text.layout().clone();

                let mut line_div = div()
                    .id(("line", line_number))
                    .relative()
                    .child(create_image(&image_source, alt_text.clone()))
                    .child(styled_text);

                // Add cursor overlay
                if let Some(cursor_pos) = visual_cursor_pos {
                    line_div = line_div.child(self.render_cursor(cursor_pos, text_layout.clone()));
                }

                // Add click handler for cursor positioning
                if let Some(ref on_click) = self.on_click {
                    let on_click = on_click.clone();
                    let layout_for_click = text_layout;
                    let content_range = self.content_range();

                    line_div = line_div.on_mouse_down(
                        MouseButton::Left,
                        move |event: &MouseDownEvent, window, cx| {
                            let visual_index =
                                match layout_for_click.index_for_position(event.position) {
                                    Ok(idx) => idx,
                                    Err(idx) => idx,
                                };
                            let buffer_offset = content_range.start + visual_index;
                            let buffer_offset = buffer_offset.min(line_range.end);
                            on_click(buffer_offset, window, cx);
                        },
                    );
                }

                return line_div;
            } else {
                // Cursor not on line: show only the image, hide text
                return div()
                    .id(("line", line_number))
                    .w_full()
                    .overflow_hidden()
                    .child(create_image(&image_source, alt_text));
            }
        }

        // Build styled content
        let (display_text, highlights) = self.build_styled_content();
        let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

        // For blank lines, we need some content for the cursor to attach to
        let display_text = if display_text.is_empty() && self.cursor_on_line() {
            " ".to_string() // Placeholder for cursor positioning
        } else if display_text.is_empty() {
            "\u{200B}".to_string() // Zero-width space to maintain line height
        } else {
            display_text
        };

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_highlights(highlights);
        let text_layout = styled_text.layout().clone();

        // Base div with line-specific styling
        let mut line_div = match &self.line.kind {
            LineKind::Heading(level) => {
                let base = div()
                    .id(("line", line_number))
                    .relative()
                    .font_weight(FontWeight::BOLD);
                match level {
                    1 => base.text_size(rems(2.0)),
                    2 => base.text_size(rems(1.75)),
                    3 => base.text_size(rems(1.5)),
                    4 => base.text_size(rems(1.25)),
                    5 => base.text_size(rems(1.1)),
                    _ => base,
                }
            }
            LineKind::BlockQuote => {
                div()
                    .id(("line", line_number))
                    .relative()
                    .pl_3()
                    .border_l_2()
                    .border_color(gpui::rgb(0x6272a4)) // Dracula comment color
            }
            LineKind::CodeBlock { .. } => {
                div()
                    .id(("line", line_number))
                    .relative()
                    .pl_2()
                    .bg(gpui::rgb(0x282a36)) // Dracula background, slightly different
            }
            _ => div().id(("line", line_number)).relative(),
        };

        line_div = line_div.child(styled_text);

        // Add cursor overlay if cursor is on this line
        if let Some(cursor_pos) = visual_cursor_pos {
            line_div = line_div.child(self.render_cursor(cursor_pos, text_layout.clone()));
        }

        // Add click handler
        if let Some(ref on_click) = self.on_click {
            let on_click = on_click.clone();
            let layout_for_click = text_layout;
            let content_range = self.content_range();

            // Extract link regions for Ctrl/Cmd+click handling
            let link_regions: Vec<_> = self
                .inline_styles
                .iter()
                .filter_map(|region| {
                    region
                        .link_url
                        .as_ref()
                        .map(|url| (region.content_range.clone(), url.clone()))
                })
                .collect();

            // Extract hidden marker regions for visual-to-buffer conversion
            // Each entry is (opening_start, opening_end, closing_start, closing_end)
            let hidden_regions: Vec<_> = self
                .inline_styles
                .iter()
                .map(|region| {
                    let opening_start = region.full_range.start.max(content_range.start);
                    let opening_end = region.content_range.start.min(content_range.end);
                    let closing_start = region.content_range.end.max(content_range.start);
                    let closing_end = region.full_range.end.min(content_range.end);
                    (opening_start, opening_end, closing_start, closing_end)
                })
                .collect();

            line_div = line_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // Convert visual index to buffer offset, accounting for hidden markers
                    let buffer_offset = {
                        if content_range.start >= content_range.end {
                            content_range.start
                        } else {
                            let mut buffer_pos = content_range.start;
                            let mut visible_count = 0usize;

                            while buffer_pos < content_range.end && visible_count < visual_index {
                                // Check if this position is inside a hidden marker region
                                let mut is_hidden = false;
                                for &(opening_start, opening_end, closing_start, closing_end) in
                                    &hidden_regions
                                {
                                    if (buffer_pos >= opening_start && buffer_pos < opening_end)
                                        || (buffer_pos >= closing_start && buffer_pos < closing_end)
                                    {
                                        is_hidden = true;
                                        break;
                                    }
                                }

                                if !is_hidden {
                                    visible_count += 1;
                                }
                                buffer_pos += 1;
                            }

                            buffer_pos.min(content_range.end)
                        }
                    };
                    let buffer_offset = buffer_offset.min(line_range.end);

                    // Check for Ctrl/Cmd+click on a link
                    if event.modifiers.control || event.modifiers.platform {
                        for (range, url) in &link_regions {
                            if buffer_offset >= range.start && buffer_offset <= range.end {
                                // Open the URL
                                let _ = open::that(url);
                                return;
                            }
                        }
                    }

                    on_click(buffer_offset, window, cx);
                },
            );
        }

        line_div
    }
}
