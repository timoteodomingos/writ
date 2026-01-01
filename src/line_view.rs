use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Font, FontStyle, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Rgba, ScrollAnchor, SharedString, StyledText, TextRun, Window, canvas, div,
    img, point, prelude::*, px, rems,
};

use crate::highlight::HighlightSpan;
use crate::lines::StyledRegion;
use crate::tree_walk::{Line, MarkerKind};

pub type ClickCallback = Rc<dyn Fn(usize, bool, usize, &mut Window, &mut App)>;
pub type DragCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;
pub type CheckboxCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;
/// Callback for hover state changes: (hovering_checkbox, hovering_link_region)
pub type HoverCallback = Rc<dyn Fn(bool, bool, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct LineViewTheme {
    pub text_color: Rgba,
    pub cursor_color: Rgba,
    pub link_color: Rgba,
    pub selection_color: Rgba,
    pub border_color: Rgba,
    pub code_color: Rgba,
    pub fence_color: Rgba,
    pub fence_lang_color: Rgba,
    pub text_font: Font,
    pub code_font: Font,
}

pub struct LineView<'a> {
    line: &'a Line,
    text: &'a str,
    cursor_offset: usize,
    inline_styles: Vec<StyledRegion>,
    theme: LineViewTheme,
    selection_range: Option<Range<usize>>,
    code_highlights: Vec<(HighlightSpan, Rgba)>,
    base_path: Option<PathBuf>,
    on_click: Option<ClickCallback>,
    on_drag: Option<DragCallback>,
    on_checkbox: Option<CheckboxCallback>,
    on_hover: Option<HoverCallback>,
    scroll_anchor: Option<ScrollAnchor>,
}

impl<'a> LineView<'a> {
    pub fn new(
        line: &'a Line,
        text: &'a str,
        cursor_offset: usize,
        inline_styles: Vec<StyledRegion>,
        theme: LineViewTheme,
        selection_range: Option<Range<usize>>,
        code_highlights: Vec<(HighlightSpan, Rgba)>,
        base_path: Option<PathBuf>,
    ) -> Self {
        Self {
            line,
            text,
            cursor_offset,
            inline_styles,
            theme,
            selection_range,
            code_highlights,
            base_path,
            on_click: None,
            on_drag: None,
            on_checkbox: None,
            on_hover: None,
            scroll_anchor: None,
        }
    }

    pub fn with_scroll_anchor(mut self, anchor: Option<ScrollAnchor>) -> Self {
        self.scroll_anchor = anchor;
        self
    }

    pub fn on_click(mut self, callback: ClickCallback) -> Self {
        self.on_click = Some(callback);
        self
    }

    pub fn on_drag(mut self, callback: DragCallback) -> Self {
        self.on_drag = Some(callback);
        self
    }

    pub fn on_checkbox(mut self, callback: CheckboxCallback) -> Self {
        self.on_checkbox = Some(callback);
        self
    }

    pub fn on_hover(mut self, callback: HoverCallback) -> Self {
        self.on_hover = Some(callback);
        self
    }

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

    fn selection_on_line(&self) -> bool {
        if let Some(ref sel) = self.selection_range {
            let line_range = &self.line.range;
            // Selection intersects if it overlaps with line range
            sel.start < line_range.end && sel.end > line_range.start
        } else {
            false
        }
    }

    /// Returns true if this line is inside a code block (has syntax highlighting).
    fn is_code_block_line(&self) -> bool {
        !self.code_highlights.is_empty() || self.line.is_fence()
    }

    /// Returns the image URL if this line contains only a standalone image
    /// (no other content, no block markers, cursor not on line).
    fn standalone_image_url(&self) -> Option<&str> {
        // Don't render image if cursor or selection is on this line
        if self.cursor_on_line() || self.selection_on_line() {
            return None;
        }

        // Must have no block markers (not in list, blockquote, etc.)
        if !self.line.markers.is_empty() {
            return None;
        }

        // Must have exactly one inline style and it must be an image
        if self.inline_styles.len() != 1 {
            return None;
        }

        let style = &self.inline_styles[0];
        if !style.is_image {
            return None;
        }

        // Image must span the entire line (allowing for trailing newline)
        let line_content = self.text[self.line.range.clone()].trim_end();
        let image_text = &self.text[style.full_range.clone()];
        if line_content != image_text {
            return None;
        }

        style.link_url.as_deref()
    }

    fn content_range(&self) -> Range<usize> {
        let range = &self.line.range;

        // Always hide block markers (they're replaced by substitution)
        // Exception: ordered lists have no substitution, so show the marker
        // Exception: fence lines - show from fence start (hide only preceding markers like blockquote)
        if let Some(marker_range) = self.line.marker_range() {
            if self
                .line
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { ordered: true }))
            {
                range.clone()
            } else if let Some(fence_marker) = self
                .line
                .markers
                .iter()
                .find(|m| matches!(m.kind, MarkerKind::CodeBlockFence { .. }))
            {
                // For fence lines, show from fence start (keeps ```lang visible)
                fence_marker.range.start..range.end
            } else {
                // Hide the block marker
                marker_range.end..range.end
            }
        } else {
            range.clone()
        }
    }

    fn get_substitution(&self) -> Option<String> {
        // Always substitute block markers (not conditional on cursor position)
        let substitution = self.line.substitution(self.text);
        if substitution.is_empty() {
            None
        } else {
            Some(substitution)
        }
    }

    fn text_run(&self, len: usize, font: Font, color: Rgba) -> TextRun {
        TextRun {
            len,
            font,
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    fn line_font(&self) -> Font {
        // Headings are bold
        if self.line.heading_level().is_some() {
            Font {
                weight: FontWeight::BOLD,
                ..self.theme.text_font.clone()
            }
        } else {
            self.theme.text_font.clone()
        }
    }

    fn get_highlight_color_for_range(&self, start: usize, end: usize) -> Option<Rgba> {
        for (span, color) in &self.code_highlights {
            // Check if this highlight span contains our range
            if span.range.start <= start && end <= span.range.end {
                return Some(*color);
            }
        }
        None
    }

    fn build_styled_content(&self) -> (String, Vec<TextRun>) {
        let content_range = self.content_range();

        let mut display_text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();

        // Add marker substitution prefix if applicable (bullet, indent, checkbox, etc.)
        // Use monospace font so indentation aligns correctly
        // Checkboxes are colored differently to indicate they're interactive
        if let Some(prefix) = self.get_substitution()
            && !prefix.is_empty()
        {
            // Check if this line has a checkbox and find its position in the prefix
            if self.line.checkbox().is_some() {
                // Find the checkbox pattern in the prefix ([ ] or [x])
                if let Some(checkbox_start) = prefix.find('[') {
                    let checkbox_end = prefix.find(']').map(|i| i + 2).unwrap_or(prefix.len()); // include "] "

                    // Part before checkbox (indent + bullet)
                    if checkbox_start > 0 {
                        let before = &prefix[..checkbox_start];
                        display_text.push_str(before);
                        runs.push(self.text_run(
                            before.len(),
                            self.theme.code_font.clone(),
                            self.theme.text_color,
                        ));
                    }

                    // Checkbox portion in accent color
                    let checkbox = &prefix[checkbox_start..checkbox_end.min(prefix.len())];
                    display_text.push_str(checkbox);
                    runs.push(self.text_run(
                        checkbox.len(),
                        self.theme.code_font.clone(),
                        self.theme.link_color,
                    ));

                    // Part after checkbox (if any)
                    if checkbox_end < prefix.len() {
                        let after = &prefix[checkbox_end..];
                        display_text.push_str(after);
                        runs.push(self.text_run(
                            after.len(),
                            self.theme.code_font.clone(),
                            self.theme.text_color,
                        ));
                    }
                } else {
                    // Fallback: no bracket found, render normally
                    display_text.push_str(&prefix);
                    runs.push(self.text_run(
                        prefix.len(),
                        self.theme.code_font.clone(),
                        self.theme.text_color,
                    ));
                }
            } else {
                // No checkbox, render prefix normally
                display_text.push_str(&prefix);
                runs.push(self.text_run(
                    prefix.len(),
                    self.theme.code_font.clone(),
                    self.theme.text_color,
                ));
            }
        }

        if content_range.start >= content_range.end {
            return (display_text, runs);
        }

        // Handle fence lines specially - color backticks and language
        if self.line.is_fence() {
            let fence_text = &self.text[content_range.clone()];
            let backticks: String = fence_text.chars().take_while(|&c| c == '`').collect();
            let language = fence_text[backticks.len()..].trim_end();

            // Add backticks in fence color (comment)
            if !backticks.is_empty() {
                display_text.push_str(&backticks);
                runs.push(self.text_run(
                    backticks.len(),
                    self.theme.code_font.clone(),
                    self.theme.fence_color,
                ));
            }

            // Add language in green
            if !language.is_empty() {
                display_text.push_str(language);
                runs.push(self.text_run(
                    language.len(),
                    self.theme.code_font.clone(),
                    self.theme.fence_lang_color,
                ));
            }

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
                self.theme.code_font.clone()
            } else {
                self.theme.text_font.clone()
            };

            let font = Font {
                weight: if is_bold || self.line.heading_level().is_some() {
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
                self.theme.link_color.into()
            } else if let Some(highlight_color) = self.get_highlight_color_for_range(start, end) {
                // Code block with syntax highlighting
                highlight_color.into()
            } else if is_code && !self.is_code_block_line() {
                // Inline code gets a distinct color
                self.theme.code_color.into()
            } else {
                self.theme.text_color.into()
            };

            // Build underline/strikethrough
            let underline = if is_link {
                Some(gpui::UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(self.theme.link_color.into()),
                    wavy: false,
                })
            } else {
                None
            };

            let strikethrough = if is_strikethrough {
                Some(gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(self.theme.text_color.into()),
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

    /// Resolve an image URL to an ImageSource.
    /// - HTTP/HTTPS URLs use string source
    /// - Local paths use PathBuf source
    fn resolve_image_source(&self, url: &str) -> gpui::ImageSource {
        // HTTP URLs pass through as strings
        if url.starts_with("http://") || url.starts_with("https://") {
            return gpui::ImageSource::from(url.to_string());
        }

        // Local file path - resolve and use PathBuf
        let path = std::path::Path::new(url);
        let resolved_path = if path.is_absolute() {
            PathBuf::from(url)
        } else if let Some(base) = &self.base_path {
            base.join(url)
        } else {
            PathBuf::from(url)
        };

        gpui::ImageSource::from(resolved_path)
    }

    fn hidden_bytes_before(&self, offset: usize, content_range: &Range<usize>) -> usize {
        let mut hidden = 0usize;
        for region in &self.inline_styles {
            // If cursor is inside this region, its markers are visible (not hidden)
            let cursor_inside = self.cursor_offset >= region.full_range.start
                && self.cursor_offset <= region.full_range.end;
            if cursor_inside {
                continue;
            }

            // Opening marker
            let opening_start = region.full_range.start.max(content_range.start);
            let opening_end = region.content_range.start.min(content_range.end);
            if opening_end > opening_start && offset > opening_end {
                hidden += opening_end - opening_start;
            }
            // Closing marker
            let closing_start = region.content_range.end.max(content_range.start);
            let closing_end = region.full_range.end.min(content_range.end);
            if closing_end > closing_start && offset > closing_end {
                hidden += closing_end - closing_start;
            }
        }
        hidden
    }

    fn compute_visual_cursor_pos(&self, display_text: &str) -> Option<usize> {
        if !self.cursor_on_line() {
            return None;
        }
        Some(self.buffer_to_visual_pos(self.cursor_offset, display_text))
    }

    fn buffer_to_visual_pos(&self, buffer_offset: usize, display_text: &str) -> usize {
        let content_range = self.content_range();

        // Block markers are always substituted now
        let prefix_len = self.get_substitution().map(|s| s.len()).unwrap_or(0);

        if content_range.start >= content_range.end {
            return prefix_len;
        }

        let clamped_offset = buffer_offset.clamp(content_range.start, content_range.end);

        let hidden = self.hidden_bytes_before(clamped_offset, &content_range);
        let buffer_pos_in_content = clamped_offset.saturating_sub(content_range.start);
        let visual_pos = prefix_len + buffer_pos_in_content.saturating_sub(hidden);

        visual_pos.min(display_text.len())
    }

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

    fn render_selection(
        &self,
        visual_range: Range<usize>,
        text_layout: gpui::TextLayout,
    ) -> impl IntoElement {
        let selection_color = self.theme.selection_color;

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

    fn render_cursor(&self, cursor_pos: usize, text_layout: gpui::TextLayout) -> impl IntoElement {
        let cursor_color = self.theme.cursor_color;

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

        // If line is a standalone image (only content, cursor not on line), render just the image
        if let Some(url) = self.standalone_image_url() {
            let source = self.resolve_image_source(url);
            return line_base(line_number).child(img(source).max_w_full());
        }

        // Build styled content
        let (display_text, mut runs) = self.build_styled_content();
        let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

        // For blank lines, we need some content for the cursor to attach to
        let display_text = if display_text.is_empty() {
            // Add a run for the placeholder space (for cursor or to maintain line height)
            runs.push(self.text_run(1, self.line_font(), self.theme.text_color));
            " ".to_string()
        } else {
            display_text
        };

        // Compute selection range before converting display_text
        let visual_selection = self.compute_visual_selection_range(&display_text);

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_runs(runs);
        let text_layout = styled_text.layout().clone();

        // Base div with marker-driven styling
        let mut line_div = line_base(line_number).relative();

        for marker in &self.line.markers {
            match &marker.kind {
                MarkerKind::Heading(level) => {
                    line_div = line_div.font_weight(FontWeight::BOLD);
                    line_div = match level {
                        1 => line_div.text_size(rems(2.0)),
                        2 => line_div.text_size(rems(1.75)),
                        3 => line_div.text_size(rems(1.5)),
                        4 => line_div.text_size(rems(1.25)),
                        5 => line_div.text_size(rems(1.1)),
                        _ => line_div,
                    };
                }
                MarkerKind::BlockQuote => {
                    // Always show left border for blockquotes
                    line_div = line_div
                        .pl_3()
                        .border_l_2()
                        .border_color(self.theme.border_color);
                }
                MarkerKind::CodeBlockFence { .. } | MarkerKind::CodeBlockContent => {
                    line_div = line_div.text_size(rems(0.9));
                }
                MarkerKind::ThematicBreak => {
                    // When cursor is on line, show raw markers (---, ***, ___)
                    // When cursor is away, render invisible text with HR line behind it
                    if !self.cursor_on_line() && !self.selection_on_line() {
                        // Build invisible text runs (same text, transparent color)
                        let line_text = &self.text[self.line.range.clone()];
                        let invisible_run = TextRun {
                            len: line_text.len(),
                            font: self.theme.text_font.clone(),
                            color: gpui::transparent_black(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shared_text: SharedString = line_text.to_string().into();
                        let styled_text =
                            StyledText::new(shared_text).with_runs(vec![invisible_run]);
                        let text_layout = styled_text.layout().clone();

                        // Horizontal line overlay (doesn't affect text layout)
                        let hr_line = div()
                            .absolute()
                            .top_1_2() // Center vertically
                            .left_0()
                            .right_0()
                            .h(px(1.0))
                            .bg(self.theme.border_color);

                        let mut hr_div = line_base(line_number)
                            .relative()
                            .child(styled_text)
                            .child(hr_line);

                        // Add click handler using the text layout for positioning
                        if let Some(ref on_click) = self.on_click {
                            let on_click = on_click.clone();
                            hr_div = hr_div.on_mouse_down(
                                MouseButton::Left,
                                move |event: &MouseDownEvent, window, cx| {
                                    let visual_index =
                                        match text_layout.index_for_position(event.position) {
                                            Ok(idx) => idx,
                                            Err(idx) => idx,
                                        };
                                    let buffer_offset = line_range.start + visual_index;
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

                        return hr_div;
                    }
                }
                MarkerKind::ListItem { .. } | MarkerKind::Checkbox { .. } | MarkerKind::Indent => {
                    // These are handled via substitution text
                }
            }
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

        // TODO: Ctrl+hover link cursor not yet implemented
        // The canvas-based approach doesn't work because position_for_index
        // returns None in the prepaint callback (text not yet laid out).
        // Need to implement at Editor level with proper state tracking.

        line_div = line_div.child(text_container);

        // Add click handler for text content
        if let Some(ref on_click) = self.on_click {
            let on_click = on_click.clone();
            let on_checkbox = self.on_checkbox.clone();
            let layout_for_click = text_layout.clone();
            let content_range = self.content_range();
            let line_number = self.line.line_number;

            // Calculate checkbox click range within the prefix (if this line has a checkbox)
            let checkbox_click_range: Option<std::ops::Range<usize>> =
                if self.line.checkbox().is_some() {
                    self.get_substitution().and_then(|prefix| {
                        let start = prefix.find('[')?;
                        let end = prefix.find(']').map(|i| i + 1)?; // include ']'
                        Some(start..end)
                    })
                } else {
                    None
                };

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

            // Calculate prefix length for visual-to-buffer conversion
            let prefix_len = self.get_substitution().map(|s| s.len()).unwrap_or(0);

            line_div = line_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // Check if click is on checkbox in the prefix
                    if let Some(ref range) = checkbox_click_range
                        && visual_index >= range.start
                        && visual_index < range.end
                        && let Some(ref on_checkbox) = on_checkbox
                    {
                        on_checkbox(line_number, window, cx);
                        return;
                    }

                    // Adjust for substitution prefix (clicks in prefix map to start of content)
                    let content_visual_index = visual_index.saturating_sub(prefix_len);

                    // Convert visual index to buffer offset, accounting for hidden markers
                    let buffer_offset = {
                        if content_range.start >= content_range.end {
                            content_range.start
                        } else {
                            let mut buffer_pos = content_range.start;
                            let mut visible_count = 0usize;

                            while buffer_pos < content_range.end
                                && visible_count < content_visual_index
                            {
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

        // Add mouse move handler for drag and link/checkbox hover detection
        {
            let on_drag = self.on_drag.clone();
            let on_hover = self.on_hover.clone();
            let layout_for_move = text_layout;
            let line_range_for_move = self.line.range.clone();
            let content_range = self.content_range();

            // Calculate prefix length for visual-to-buffer conversion
            let prefix_len = self.get_substitution().map(|s| s.len()).unwrap_or(0);

            // Calculate checkbox hover range within the prefix (if this line has a checkbox)
            let checkbox_hover_range: Option<Range<usize>> = if self.line.checkbox().is_some() {
                self.get_substitution().and_then(|prefix| {
                    let start = prefix.find('[')?;
                    let end = prefix.find(']').map(|i| i + 1)?;
                    Some(start..end)
                })
            } else {
                None
            };

            // Extract link content ranges for hover detection
            let link_content_ranges: Vec<Range<usize>> = self
                .inline_styles
                .iter()
                .filter(|region| region.link_url.is_some())
                .map(|region| region.content_range.clone())
                .collect();

            line_div = line_div.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                // Handle drag when left button is pressed
                if event.pressed_button == Some(MouseButton::Left)
                    && let Some(ref on_drag) = on_drag
                {
                    let visual_index = match layout_for_move.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // Adjust for substitution prefix
                    let content_visual_index = visual_index.saturating_sub(prefix_len);
                    let buffer_offset =
                        (content_range.start + content_visual_index).min(line_range_for_move.end);
                    on_drag(buffer_offset, window, cx);
                }

                // Handle link and checkbox hover detection
                if let Some(ref on_hover) = on_hover {
                    let visual_index = match layout_for_move.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    // Check if hovering over checkbox in prefix
                    let hovering_checkbox = checkbox_hover_range.as_ref().is_some_and(|range| {
                        visual_index >= range.start && visual_index < range.end
                    });

                    // Check if hovering over a link region
                    let content_visual_index = visual_index.saturating_sub(prefix_len);
                    let buffer_offset =
                        (content_range.start + content_visual_index).min(line_range_for_move.end);
                    let hovering_link_region = link_content_ranges
                        .iter()
                        .any(|range| buffer_offset >= range.start && buffer_offset < range.end);

                    on_hover(hovering_checkbox, hovering_link_region, window, cx);
                }
            });
        }

        // Attach scroll anchor if this is the cursor line
        line_div.anchor_scroll(self.scroll_anchor)
    }
}
