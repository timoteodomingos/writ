//! Line view component for rendering individual lines.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    App, CursorStyle, Font, FontStyle, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Rgba, ScrollAnchor, SharedString, StyledText, TextRun, Window, canvas, div,
    img, point, prelude::*, px, rems,
};

use crate::highlight::HighlightSpan;
use crate::lines::{LineInfo, LineKind};
use crate::render::StyledRegion;

/// Callback type for click events - receives the buffer offset where the click occurred,
/// whether shift was held (for extending selection), and the click count (1=single, 2=double, 3=triple).
pub type ClickCallback = Rc<dyn Fn(usize, bool, usize, &mut Window, &mut App)>;

/// Callback type for drag events - receives the buffer offset during mouse drag.
pub type DragCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Callback type for checkbox toggle - receives the line number where checkbox was clicked.
pub type CheckboxCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;

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
    text_color: Rgba,
    cursor_color: Rgba,
    link_color: Rgba,
    selection_color: Rgba,
    border_color: Rgba,
    /// Color for code fence backticks (comment color)
    fence_color: Rgba,
    /// Color for code fence language identifier (green)
    fence_lang_color: Rgba,
    /// Selection range in buffer offsets (None if collapsed/no selection)
    selection_range: Option<Range<usize>>,
    /// Font for regular text
    text_font: Font,
    /// Font for inline code
    code_font: Font,
    /// Base path for resolving relative image paths (directory containing the markdown file)
    base_path: Option<PathBuf>,
    /// Syntax highlighting spans for code blocks (with pre-computed colors)
    code_highlights: Vec<(HighlightSpan, Rgba)>,
    /// Callback when line is clicked
    on_click: Option<ClickCallback>,
    /// Callback when mouse is dragged over line (with button pressed)
    on_drag: Option<DragCallback>,
    /// Callback when checkbox is clicked (for task list items)
    on_checkbox: Option<CheckboxCallback>,
    /// Whether to force showing block markers (e.g., cursor is in code block)
    show_block_markers: bool,
    /// Scroll anchor for cursor line (attached to line containing cursor)
    scroll_anchor: Option<ScrollAnchor>,
}

impl<'a> LineView<'a> {
    /// Create a new line view.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        line: &'a LineInfo,
        text: &'a str,
        cursor_offset: usize,
        inline_styles: Vec<StyledRegion>,
        text_color: Rgba,
        cursor_color: Rgba,
        link_color: Rgba,
        selection_color: Rgba,
        border_color: Rgba,
        fence_color: Rgba,
        fence_lang_color: Rgba,
        selection_range: Option<Range<usize>>,
        text_font: Font,
        code_font: Font,
        base_path: Option<PathBuf>,
        code_highlights: Vec<(HighlightSpan, Rgba)>,
        show_block_markers: bool,
    ) -> Self {
        Self {
            line,
            text,
            cursor_offset,
            inline_styles,
            text_color,
            cursor_color,
            link_color,
            selection_color,
            border_color,
            fence_color,
            fence_lang_color,
            selection_range,
            text_font,
            code_font,
            base_path,
            code_highlights,
            on_click: None,
            on_drag: None,
            on_checkbox: None,
            show_block_markers,
            scroll_anchor: None,
        }
    }

    /// Set the scroll anchor for this line (used for cursor line).
    pub fn with_scroll_anchor(mut self, anchor: Option<ScrollAnchor>) -> Self {
        self.scroll_anchor = anchor;
        self
    }

    /// Set the click callback for this line.
    pub fn on_click(mut self, callback: ClickCallback) -> Self {
        self.on_click = Some(callback);
        self
    }

    /// Set the drag callback for this line.
    pub fn on_drag(mut self, callback: DragCallback) -> Self {
        self.on_drag = Some(callback);
        self
    }

    /// Set the checkbox toggle callback for this line.
    pub fn on_checkbox(mut self, callback: CheckboxCallback) -> Self {
        self.on_checkbox = Some(callback);
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

    /// Check if selection intersects this line.
    fn selection_on_line(&self) -> bool {
        if let Some(ref sel) = self.selection_range {
            let line_range = &self.line.range;
            // Selection intersects if it overlaps with line range
            sel.start < line_range.end && sel.end > line_range.start
        } else {
            false
        }
    }

    /// Check if we should show raw markers (cursor/selection on line, or force show).
    fn should_show_raw_markers(&self) -> bool {
        self.cursor_on_line() || self.selection_on_line() || self.show_block_markers
    }

    /// Get the content range, accounting for hidden block markers (like # for headings).
    fn content_range(&self) -> Range<usize> {
        let range = &self.line.range;

        // Block markers are shown when cursor/selection is on line
        if self.should_show_raw_markers() {
            range.clone()
        } else if let Some(marker_range) = self.line.marker_range() {
            // For ordered lists, don't hide the marker (no substitution available)
            if matches!(self.line.kind(), LineKind::ListItem { ordered: true, .. }) {
                range.clone()
            } else {
                // Hide the block marker
                marker_range.end..range.end
            }
        } else {
            range.clone()
        }
    }

    /// Check if this line should show a border (has blockquote layer).
    fn should_show_border(&self) -> bool {
        !self.should_show_raw_markers() && self.line.has_border()
    }

    /// Get the substitution prefix for this line (combined from all layers).
    fn get_marker_substitution(&self) -> Option<String> {
        // Only substitute when not showing raw markers
        if self.should_show_raw_markers() {
            return None;
        }

        let substitution = self.line.substitution();
        if substitution.is_empty() {
            None
        } else {
            Some(substitution)
        }
    }

    /// Get checkbox state if this line shows a clickable checkbox (when cursor is away).
    /// Returns Some(checked) where checked is true for ☑, false for ☐.
    fn checkbox_state(&self) -> Option<bool> {
        match self.get_marker_substitution().as_deref() {
            Some("☐ ") => Some(false),
            Some("☑ ") => Some(true),
            _ => None,
        }
    }

    /// Get the non-checkbox part of the marker substitution.
    /// For checkbox lines, this returns None (checkbox rendered separately).
    /// For other lines, returns the full substitution.
    fn get_non_checkbox_substitution(&self) -> Option<String> {
        match self.get_marker_substitution().as_deref() {
            Some("☐ ") | Some("☑ ") => None, // Checkbox rendered separately
            other => other.map(|s| s.to_string()),
        }
    }

    /// Get leading whitespace (indentation before the first marker).
    /// Returns empty string if no markers or showing raw markers.
    fn leading_whitespace(&self) -> &str {
        if self.should_show_raw_markers() {
            return "";
        }

        if let Some(marker_range) = self.line.marker_range() {
            let line_start = self.line.range.start;
            if marker_range.start > line_start {
                // There's whitespace before the first marker
                &self.text[line_start..marker_range.start]
            } else {
                ""
            }
        } else {
            ""
        }
    }

    /// Check if this line should have bold styling (headings).
    fn is_line_bold(&self) -> bool {
        matches!(self.line.kind(), LineKind::Heading(_))
    }

    /// Check if this line is inside a code block (should use mono font).
    fn is_code_block_line(&self) -> bool {
        matches!(self.line.kind(), LineKind::CodeBlock { .. })
    }

    /// Get the base text font with line-level styling (bold for headings).
    fn line_font(&self) -> Font {
        if self.is_line_bold() {
            Font {
                weight: FontWeight::BOLD,
                ..self.text_font.clone()
            }
        } else {
            self.text_font.clone()
        }
    }

    /// Get the syntax highlight color for a buffer range, if any.
    ///
    /// Returns the color of the most specific highlight span that contains this range.
    /// tree-sitter-highlight produces non-overlapping spans, so we just find
    /// the one that contains our range.
    fn get_highlight_color_for_range(&self, start: usize, end: usize) -> Option<Rgba> {
        for (span, color) in &self.code_highlights {
            // Check if this highlight span contains our range
            if span.range.start <= start && end <= span.range.end {
                return Some(*color);
            }
        }
        None
    }

    /// Check if this is a fence line (code block delimiter).
    fn is_fence_line(&self) -> bool {
        matches!(self.line.kind(), LineKind::CodeBlock { is_fence: true, .. })
    }

    /// Build styled content for fence lines (``` with optional language).
    /// Returns backticks in comment color and language in green.
    fn build_fence_content(&self) -> (String, Vec<TextRun>) {
        let line_text = &self.text[self.line.range.clone()];
        let trimmed = line_text.trim_start();
        let leading_spaces = line_text.len() - trimmed.len();

        let mut display_text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();

        // Add leading whitespace
        if leading_spaces > 0 {
            let whitespace = &line_text[..leading_spaces];
            display_text.push_str(whitespace);
            runs.push(TextRun {
                len: whitespace.len(),
                font: self.code_font.clone(),
                color: self.fence_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        // Find the backticks
        let backticks: String = trimmed.chars().take_while(|&c| c == '`').collect();
        let language = trimmed[backticks.len()..].trim();

        // Add backticks in fence color (comment)
        if !backticks.is_empty() {
            display_text.push_str(&backticks);
            runs.push(TextRun {
                len: backticks.len(),
                font: self.code_font.clone(),
                color: self.fence_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        // Add language in green
        if !language.is_empty() {
            display_text.push_str(language);
            runs.push(TextRun {
                len: language.len(),
                font: self.code_font.clone(),
                color: self.fence_lang_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        (display_text, runs)
    }

    /// Build the display text and text runs with proper fonts.
    fn build_styled_content(&self) -> (String, Vec<TextRun>) {
        // Handle fence lines specially when cursor is not on them
        if self.is_fence_line() && !self.should_show_raw_markers() {
            return self.build_fence_content();
        }

        let content_range = self.content_range();

        let mut display_text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();

        // Add leading whitespace (indentation) before the substitution prefix
        let whitespace = self.leading_whitespace();
        if !whitespace.is_empty() {
            display_text.push_str(whitespace);
            runs.push(TextRun {
                len: whitespace.len(),
                font: self.text_font.clone(),
                color: self.text_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        // Add marker substitution prefix if applicable (bullet, etc.)
        // Note: checkboxes are rendered separately as a larger element
        if let Some(prefix) = self.get_non_checkbox_substitution()
            && !prefix.is_empty()
        {
            display_text.push_str(&prefix);
            runs.push(TextRun {
                len: prefix.len(),
                font: self.text_font.clone(),
                color: self.text_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        if content_range.start >= content_range.end {
            return (display_text, runs);
        }

        // Collect all boundary points from inline styles
        let mut boundaries: Vec<usize> = vec![content_range.start, content_range.end];

        // If selection is on this line, show ALL inline markers
        // Otherwise, only show markers for regions where cursor is inside
        let show_all_markers = self.selection_on_line();

        for region in &self.inline_styles {
            // Only include boundaries within our content range
            if region.full_range.end > content_range.start
                && region.full_range.start < content_range.end
            {
                // Check if cursor is inside this specific region
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

                if show_all_markers || cursor_inside {
                    // Show full range including markers
                    boundaries.push(region.full_range.start.max(content_range.start));
                    boundaries.push(region.full_range.end.min(content_range.end));
                } else {
                    // Hide markers, show content range boundaries
                    boundaries.push(region.full_range.start.max(content_range.start));
                    boundaries.push(region.content_range.start.max(content_range.start));
                    boundaries.push(region.content_range.end.min(content_range.end));
                    boundaries.push(region.full_range.end.min(content_range.end));
                }
            }
        }

        // Add boundaries from syntax highlighting spans
        for (span, _) in &self.code_highlights {
            if span.range.end > content_range.start && span.range.start < content_range.end {
                boundaries.push(span.range.start.max(content_range.start));
                boundaries.push(span.range.end.min(content_range.end));
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

            // Check if this range should be hidden (inline marker when cursor outside region)
            let mut is_hidden = false;
            if !show_all_markers {
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
            }

            if is_hidden {
                continue;
            }

            // Add text
            let span_text = &self.text[start..end];
            let span_len = span_text.len();
            display_text.push_str(span_text);

            // Compute merged style for this span
            let mut is_bold = false;
            let mut is_italic = false;
            let mut is_code = false;
            let mut is_strikethrough = false;
            let mut is_link = false;

            for region in &self.inline_styles {
                // Check if cursor is inside this specific region
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

                // Use full_range when showing markers (selection or cursor inside), content_range otherwise
                let style_range = if show_all_markers || cursor_inside {
                    &region.full_range
                } else {
                    &region.content_range
                };

                if style_range.start <= start && end <= style_range.end {
                    if region.style.bold {
                        is_bold = true;
                    }
                    if region.style.italic {
                        is_italic = true;
                    }
                    if region.style.code {
                        is_code = true;
                    }
                    if region.style.strikethrough {
                        is_strikethrough = true;
                    }
                    if region.link_url.is_some() {
                        is_link = true;
                    }
                }
            }

            // Build the font for this run
            // Use code font for: inline code spans, or any line inside a code block
            let base_font = if is_code || self.is_code_block_line() {
                self.code_font.clone()
            } else {
                self.text_font.clone()
            };

            let font = Font {
                weight: if is_bold || self.is_line_bold() {
                    FontWeight::BOLD
                } else {
                    base_font.weight
                },
                style: if is_italic {
                    FontStyle::Italic
                } else {
                    base_font.style
                },
                ..base_font
            };

            // Determine text color
            let color: Hsla = if is_link {
                self.link_color.into()
            } else if let Some(highlight_color) = self.get_highlight_color_for_range(start, end) {
                // Code block with syntax highlighting
                highlight_color.into()
            } else {
                self.text_color.into()
            };

            // Build underline/strikethrough
            let underline = if is_link {
                Some(gpui::UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(self.link_color.into()),
                    wavy: false,
                })
            } else {
                None
            };

            let strikethrough = if is_strikethrough {
                Some(gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(self.text_color.into()),
                })
            } else {
                None
            };

            runs.push(TextRun {
                len: span_len,
                font,
                color,
                background_color: None,
                underline,
                strikethrough,
            });
        }

        (display_text, runs)
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

        // If selection is on line, all markers are shown - direct mapping
        if self.selection_on_line() {
            let visual_pos = self.cursor_offset.saturating_sub(content_range.start);
            return Some(visual_pos.min(display_text.len()));
        }

        // Calculate how much text is hidden before the cursor
        let mut hidden_before_cursor = 0usize;

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

    /// Convert a buffer offset to a visual position, accounting for hidden markers.
    fn buffer_to_visual_pos(&self, buffer_offset: usize, display_text: &str) -> usize {
        let content_range = self.content_range();

        if content_range.start >= content_range.end {
            return 0;
        }

        // Clamp buffer_offset to content range
        let clamped_offset = buffer_offset.clamp(content_range.start, content_range.end);

        // If selection is on line, all markers are shown - direct mapping
        if self.selection_on_line() {
            let visual_pos = clamped_offset.saturating_sub(content_range.start);
            return visual_pos.min(display_text.len());
        }

        // Calculate how much text is hidden before this offset
        let mut hidden_before = 0usize;

        // Account for hidden inline markers
        for region in &self.inline_styles {
            // Opening marker hidden
            let opening_start = region.full_range.start.max(content_range.start);
            let opening_end = region.content_range.start.min(content_range.end);
            if opening_end > opening_start && clamped_offset > opening_end {
                hidden_before += opening_end - opening_start;
            }

            // Closing marker hidden
            let closing_start = region.content_range.end.max(content_range.start);
            let closing_end = region.full_range.end.min(content_range.end);
            if closing_end > closing_start && clamped_offset > closing_end {
                hidden_before += closing_end - closing_start;
            }
        }

        let buffer_pos_in_content = clamped_offset.saturating_sub(content_range.start);
        let visual_pos = buffer_pos_in_content.saturating_sub(hidden_before);

        visual_pos.min(display_text.len())
    }

    /// Compute the visual selection range for this line.
    /// Returns None if selection doesn't intersect this line.
    fn compute_visual_selection_range(&self, display_text: &str) -> Option<Range<usize>> {
        let selection = self.selection_range.as_ref()?;
        let line_range = &self.line.range;

        // Check if selection intersects this line
        if selection.end <= line_range.start || selection.start >= line_range.end {
            return None;
        }

        // Clamp selection to line bounds
        let sel_start = selection.start.max(line_range.start);
        let sel_end = selection.end.min(line_range.end);

        // Convert to visual positions
        let visual_start = self.buffer_to_visual_pos(sel_start, display_text);
        let visual_end = self.buffer_to_visual_pos(sel_end, display_text);

        if visual_start < visual_end {
            Some(visual_start..visual_end)
        } else {
            None
        }
    }

    /// Render the selection overlay.
    fn render_selection(
        &self,
        visual_range: Range<usize>,
        text_layout: gpui::TextLayout,
    ) -> impl IntoElement {
        let selection_color = self.selection_color;

        canvas(
            move |bounds, _window, _cx| {
                let start_pos = text_layout.position_for_index(visual_range.start);
                let end_pos = text_layout.position_for_index(visual_range.end);
                (start_pos, end_pos, bounds)
            },
            move |_bounds, data, window: &mut Window, _cx| {
                let (start_opt, end_opt, _bounds) = data;
                if let (Some(start), Some(end)) = (start_opt, end_opt) {
                    let text_style = window.text_style();
                    let font_size = text_style.font_size.to_pixels(window.rem_size());
                    let line_height = text_style
                        .line_height
                        .to_pixels(font_size.into(), window.rem_size());

                    // Check if selection spans multiple visual lines (wrapped text)
                    if start.y == end.y {
                        // Single line selection
                        let rect = gpui::Bounds {
                            origin: point(start.x, start.y),
                            size: gpui::Size {
                                width: end.x - start.x,
                                height: line_height,
                            },
                        };
                        window.paint_quad(gpui::fill(rect, selection_color));
                    } else {
                        // Multi-line selection (wrapped text)
                        // Use a large width to ensure we cover to the edge
                        // The parent div clips, so this is safe
                        let full_width = px(10000.0);

                        // First line: from start.x to end of line
                        let first_rect = gpui::Bounds {
                            origin: point(start.x, start.y),
                            size: gpui::Size {
                                width: full_width,
                                height: line_height,
                            },
                        };
                        window.paint_quad(gpui::fill(first_rect, selection_color));

                        // Middle lines: full width
                        let mut y = start.y + line_height;
                        while y < end.y {
                            let mid_rect = gpui::Bounds {
                                origin: point(px(0.0), y),
                                size: gpui::Size {
                                    width: full_width,
                                    height: line_height,
                                },
                            };
                            window.paint_quad(gpui::fill(mid_rect, selection_color));
                            y += line_height;
                        }

                        // Last line: from start to end.x
                        let last_rect = gpui::Bounds {
                            origin: point(px(0.0), end.y),
                            size: gpui::Size {
                                width: end.x,
                                height: line_height,
                            },
                        };
                        window.paint_quad(gpui::fill(last_rect, selection_color));
                    }
                }
            },
        )
        .absolute()
        .size_full()
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

/// Create a base line div with common styling (max-width, centered).
fn line_base(line_number: usize) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("line", line_number))
        .max_w(px(800.0))
        .w_full()
        .mx_auto()
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
                let (display_text, mut runs) = self.build_styled_content();
                let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

                let display_text = if display_text.is_empty() {
                    // Add a run for the placeholder space
                    runs.push(TextRun {
                        len: 1,
                        font: self.line_font(),
                        color: self.text_color.into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                    " ".to_string()
                } else {
                    display_text
                };

                let shared_text: SharedString = display_text.into();
                let styled_text = StyledText::new(shared_text).with_runs(runs);
                let text_layout = styled_text.layout().clone();

                // Show text first (with cursor), then image below
                // This ensures scrolling to this line shows the editable text
                let mut line_div = line_base(line_number)
                    .relative()
                    .child(styled_text)
                    .child(create_image(&image_source, alt_text.clone()));

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
                            on_click(
                                buffer_offset,
                                event.modifiers.shift,
                                event.click_count,
                                window,
                                cx,
                            );
                        },
                    );
                }

                return line_div;
            } else {
                // Cursor not on line: show only the image, hide text
                return line_base(line_number)
                    .overflow_hidden()
                    .child(create_image(&image_source, alt_text));
            }
        }

        // Build styled content
        let (display_text, mut runs) = self.build_styled_content();
        let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

        // For blank lines, we need some content for the cursor to attach to
        let display_text = if display_text.is_empty() && self.cursor_on_line() {
            // Add a run for the placeholder space
            runs.push(TextRun {
                len: 1,
                font: self.line_font(),
                color: self.text_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            " ".to_string() // Placeholder for cursor positioning
        } else if display_text.is_empty() {
            // Add a run for a regular space to maintain line height
            runs.push(TextRun {
                len: 1,
                font: self.line_font(),
                color: self.text_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            " ".to_string() // Space to maintain line height
        } else {
            display_text
        };

        // Compute selection range before converting display_text
        let visual_selection = self.compute_visual_selection_range(&display_text);

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_runs(runs);
        let text_layout = styled_text.layout().clone();

        // Base div with line-specific styling
        let mut line_div = match self.line.kind() {
            LineKind::Heading(level) => {
                let base = line_base(line_number)
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
                let base = line_base(line_number).relative();
                // Only show left border when cursor is away (hiding the > markers)
                if self.should_show_raw_markers() {
                    base
                } else {
                    base.pl_3().border_l_2().border_color(self.border_color)
                }
            }
            LineKind::CodeBlock { .. } => line_base(line_number).relative().text_size(rems(0.9)),
            _ => {
                let base = line_base(line_number).relative();
                // Add blockquote border for list items inside blockquotes
                if self.should_show_border() {
                    base.pl_3().border_l_2().border_color(self.border_color)
                } else {
                    base
                }
            }
        };

        // For checkbox lines, render checkbox as a styled box element
        if let Some(checked) = self.checkbox_state() {
            let on_checkbox = self.on_checkbox.clone();
            let line_num = self.line.line_number;
            let check_color = self.text_color;

            // Outer box: square matching line height
            let box_size = rems(1.0);
            let inner_size = rems(0.6);
            let mut checkbox_div = div()
                .size(box_size)
                .border_1()
                .border_color(self.text_color)
                .cursor(CursorStyle::PointingHand)
                .mr_2()
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(MouseButton::Left, move |_event, window, cx: &mut App| {
                    cx.stop_propagation();
                    if let Some(ref cb) = on_checkbox {
                        cb(line_num, window, cx);
                    }
                });

            // Add filled inner square when checked (with gap from border)
            if checked {
                let inner_box = div().size(inner_size).bg(check_color);
                checkbox_div = checkbox_div.child(inner_box);
            }

            // Make line a flex row with checkbox + content
            line_div = line_div
                .flex()
                .flex_row()
                .items_center()
                .child(checkbox_div);
        }

        // Wrap text content in a relative div for overlays
        let mut text_container = div().relative().child(styled_text);

        // Add selection overlay (positioned absolutely, so appears behind text visually)
        if let Some(sel_range) = visual_selection {
            text_container =
                text_container.child(self.render_selection(sel_range, text_layout.clone()));
        }

        // Add cursor overlay if cursor is on this line
        if let Some(cursor_pos) = visual_cursor_pos {
            text_container =
                text_container.child(self.render_cursor(cursor_pos, text_layout.clone()));
        }

        line_div = line_div.child(text_container);

        // Add click handler for text content (not checkbox - that has its own handler)
        if let Some(ref on_click) = self.on_click {
            let on_click = on_click.clone();
            let layout_for_click = text_layout.clone();
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
            // Markers are visible (not hidden) if:
            // - selection is on line (show_all_markers), OR
            // - cursor is inside that specific region
            let show_all_markers = self.selection_on_line();
            let cursor_offset = self.cursor_offset;
            let hidden_regions: Vec<(usize, usize, usize, usize)> = if show_all_markers {
                Vec::new()
            } else {
                self.inline_styles
                    .iter()
                    .filter_map(|region| {
                        // If cursor is inside this region, its markers are visible
                        let cursor_inside = cursor_offset >= region.full_range.start
                            && cursor_offset <= region.full_range.end;
                        if cursor_inside {
                            None // Not hidden
                        } else {
                            let opening_start = region.full_range.start.max(content_range.start);
                            let opening_end = region.content_range.start.min(content_range.end);
                            let closing_start = region.content_range.end.max(content_range.start);
                            let closing_end = region.full_range.end.min(content_range.end);
                            Some((opening_start, opening_end, closing_start, closing_end))
                        }
                    })
                    .collect()
            };

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

                    on_click(
                        buffer_offset,
                        event.modifiers.shift,
                        event.click_count,
                        window,
                        cx,
                    );
                },
            );
        }

        // Add drag handler for mouse move with button pressed
        if let Some(ref on_drag) = self.on_drag {
            let on_drag = on_drag.clone();
            let layout_for_drag = text_layout;
            let line_range = self.line.range.clone();

            line_div = line_div.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                // Only handle drag when left button is pressed
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }

                let visual_index = match layout_for_drag.index_for_position(event.position) {
                    Ok(idx) => idx,
                    Err(idx) => idx,
                };

                // Simple mapping: visual index to buffer offset
                // Note: This doesn't perfectly account for hidden markers, but avoids
                // the jumping issue when markers are revealed during drag. The offset
                // will be slightly off when markers are hidden, but the re-render will
                // correct it on the next frame.
                let buffer_offset = (line_range.start + visual_index).min(line_range.end);

                on_drag(buffer_offset, window, cx);
            });
        }

        // Attach scroll anchor if this is the cursor line
        line_div.anchor_scroll(self.scroll_anchor)
    }
}
