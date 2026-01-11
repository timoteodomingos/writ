use std::borrow::Cow;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, CursorStyle, Font, FontStyle, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Rgba, ScrollAnchor, SharedString, StyledText, TextRun, Window, canvas, div,
    img, point, prelude::*, px, rems,
};
use ropey::Rope;

use crate::highlight::HighlightSpan;
use crate::inline::StyledRegion;
use crate::marker::{LineMarkers, MarkerKind};

pub type ClickCallback = Rc<dyn Fn(usize, bool, usize, &mut Window, &mut App)>;
pub type DragCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;
pub type CheckboxCallback = Rc<dyn Fn(usize, &mut Window, &mut App)>;
/// Callback for hover state changes: (hovering_checkbox, hovering_link_region)
pub type HoverCallback = Rc<dyn Fn(bool, bool, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct LineTheme {
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
    /// Width of a single monospace character in the code font.
    /// Used for precise indentation of nested blocks.
    pub monospace_char_width: gpui::Pixels,
}

pub struct Line {
    line: LineMarkers,
    /// Owned Rope - clone is O(1) due to internal Arc sharing.
    rope: Rope,
    cursor_offset: usize,
    inline_styles: Vec<StyledRegion>,
    theme: LineTheme,
    selection_range: Option<Range<usize>>,
    code_highlights: Vec<(HighlightSpan, Rgba)>,
    base_path: Option<PathBuf>,
    on_click: Option<ClickCallback>,
    on_drag: Option<DragCallback>,
    on_checkbox: Option<CheckboxCallback>,
    on_hover: Option<HoverCallback>,
    scroll_anchor: Option<ScrollAnchor>,
    /// Cached substitution string (computed once in new())
    substitution: Option<String>,
    /// If this line is a fence, whether it should be visible (cursor in code block).
    fence_visible: bool,
    /// True while actively dragging a selection. Used to keep markers expanded.
    is_selecting: bool,
}

impl Line {
    pub fn new(
        line: LineMarkers,
        rope: Rope,
        cursor_offset: usize,
        inline_styles: Vec<StyledRegion>,
        theme: LineTheme,
        selection_range: Option<Range<usize>>,
        code_highlights: Vec<(HighlightSpan, Rgba)>,
        base_path: Option<PathBuf>,
        fence_visible: bool,
        is_selecting: bool,
    ) -> Self {
        let substitution = {
            let s = line.substitution_rope(&rope);
            if s.is_empty() { None } else { Some(s) }
        };
        Self {
            line,
            rope,
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
            substitution,
            fence_visible,
            is_selecting,
        }
    }

    /// Get a slice from the rope as a Cow<str>.
    fn slice(&self, range: Range<usize>) -> Cow<'_, str> {
        let start = self.rope.byte_to_char(range.start);
        let end = self.rope.byte_to_char(range.end);
        let slice = self.rope.slice(start..end);
        match slice.as_str() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(slice.to_string()),
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
        if range.start == range.end {
            self.cursor_offset == range.start
        } else {
            self.cursor_offset >= range.start && self.cursor_offset <= range.end
        }
    }

    fn selection_on_line(&self) -> bool {
        if let Some(ref sel) = self.selection_range {
            let line_range = &self.line.range;
            sel.start < line_range.end && sel.end > line_range.start
        } else {
            false
        }
    }

    fn marker_in_selection(&self, marker_range: &Range<usize>) -> bool {
        if let Some(ref sel) = self.selection_range {
            sel.start < marker_range.end && sel.end > marker_range.start
        } else {
            false
        }
    }

    fn is_code_block_line(&self) -> bool {
        self.line.in_code_block || self.line.is_fence()
    }

    fn standalone_image_url(&self) -> Option<&str> {
        // Always show image, even when cursor is on line
        if !self.line.markers.is_empty() {
            return None;
        }

        if self.inline_styles.len() != 1 {
            return None;
        }

        let style = &self.inline_styles[0];
        if !style.is_image {
            return None;
        }

        let line_content = self.slice(self.line.range.clone());
        let line_content = line_content.trim_end();
        let image_text = self.slice(style.full_range.clone());
        if line_content != image_text.as_ref() {
            return None;
        }

        style.link_url.as_deref()
    }

    fn content_range(&self) -> Range<usize> {
        let range = &self.line.range;

        if let Some(marker_range) = self.line.marker_range() {
            marker_range.end..range.end
        } else {
            range.clone()
        }
    }

    fn get_substitution(&self) -> Option<&str> {
        self.substitution.as_deref()
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
            if span.range.start <= start && end <= span.range.end {
                return Some(*color);
            }
        }
        None
    }

    /// Apply selection background color to text runs that overlap with the selection range.
    /// This modifies runs in-place, splitting them if needed to correctly highlight
    /// only the selected portion.
    fn apply_selection_to_runs(
        &self,
        runs: Vec<TextRun>,
        selection_range: Range<usize>,
    ) -> Vec<TextRun> {
        let selection_color: Hsla = self.theme.selection_color.into();
        let mut result = Vec::new();
        let mut pos = 0;

        for run in runs {
            let run_start = pos;
            let run_end = pos + run.len;

            if run_end <= selection_range.start || run_start >= selection_range.end {
                result.push(run);
            } else {
                let sel_start_in_run = selection_range.start.saturating_sub(run_start);
                let sel_end_in_run = (selection_range.end - run_start).min(run.len);

                if sel_start_in_run > 0 {
                    result.push(TextRun {
                        len: sel_start_in_run,
                        font: run.font.clone(),
                        color: run.color,
                        background_color: run.background_color,
                        underline: run.underline,
                        strikethrough: run.strikethrough,
                    });
                }

                let selected_len = sel_end_in_run - sel_start_in_run;
                if selected_len > 0 {
                    result.push(TextRun {
                        len: selected_len,
                        font: run.font.clone(),
                        color: run.color,
                        background_color: Some(selection_color),
                        underline: run.underline,
                        strikethrough: run.strikethrough,
                    });
                }

                if sel_end_in_run < run.len {
                    result.push(TextRun {
                        len: run.len - sel_end_in_run,
                        font: run.font.clone(),
                        color: run.color,
                        background_color: run.background_color,
                        underline: run.underline,
                        strikethrough: run.strikethrough,
                    });
                }
            }

            pos = run_end;
        }

        result
    }

    fn build_styled_content(&self) -> (String, Vec<TextRun>) {
        let content_range = if self.line.heading_level().is_some() {
            self.line.range.clone()
        } else {
            self.content_range()
        };

        let mut display_text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();

        if let Some(prefix) = self.get_substitution()
            && !prefix.is_empty()
        {
            if self.line.checkbox().is_some() {
                if let Some(checkbox_start) = prefix.find('[') {
                    let checkbox_end = prefix.find(']').map(|i| i + 2).unwrap_or(prefix.len());

                    if checkbox_start > 0 {
                        let before = &prefix[..checkbox_start];
                        display_text.push_str(before);
                        runs.push(self.text_run(
                            before.len(),
                            self.theme.code_font.clone(),
                            self.theme.text_color,
                        ));
                    }

                    let checkbox = &prefix[checkbox_start..checkbox_end.min(prefix.len())];
                    display_text.push_str(checkbox);
                    runs.push(self.text_run(
                        checkbox.len(),
                        self.theme.code_font.clone(),
                        self.theme.link_color,
                    ));

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
                    display_text.push_str(prefix);
                    runs.push(self.text_run(
                        prefix.len(),
                        self.theme.code_font.clone(),
                        self.theme.text_color,
                    ));
                }
            } else {
                display_text.push_str(prefix);
                runs.push(self.text_run(
                    prefix.len(),
                    self.theme.code_font.clone(),
                    self.theme.text_color,
                ));
            }
        }

        if self.line.is_fence() {
            let is_visible =
                self.cursor_on_line() || self.selection_on_line() || self.fence_visible;
            let fence_text = self.slice(self.line.range.clone());
            let backticks: String = fence_text.chars().take_while(|&c| c == '`').collect();
            let language = fence_text[backticks.len()..].trim_end();

            let transparent = Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };

            if !backticks.is_empty() {
                display_text.push_str(&backticks);
                let color = if is_visible {
                    self.theme.fence_color
                } else {
                    transparent
                };
                runs.push(self.text_run(backticks.len(), self.theme.code_font.clone(), color));
            }

            if !language.is_empty() {
                display_text.push_str(language);
                let color = if is_visible {
                    self.theme.fence_lang_color
                } else {
                    transparent
                };
                runs.push(self.text_run(language.len(), self.theme.code_font.clone(), color));
            }

            return (display_text, runs);
        }

        if self.line.is_thematic_break() {
            let is_visible = self.cursor_on_line() || self.selection_on_line();
            let break_text = self.slice(self.line.range.clone());

            let transparent = Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };
            let color = if is_visible {
                self.theme.text_color
            } else {
                transparent
            };

            display_text.push_str(&break_text);
            runs.push(self.text_run(break_text.len(), self.theme.text_font.clone(), color));

            return (display_text, runs);
        }

        if content_range.start >= content_range.end {
            return (display_text, runs);
        }

        let mut boundaries: Vec<usize> = vec![content_range.start, content_range.end];
        let show_all_markers =
            self.selection_on_line() || (self.is_selecting && self.cursor_on_line());

        if self.line.heading_level().is_some()
            && let Some(marker_range) = self.line.marker_range()
        {
            boundaries.push(marker_range.start);
            boundaries.push(marker_range.end);
        }

        for region in &self.inline_styles {
            if region.full_range.end > content_range.start
                && region.full_range.start < content_range.end
            {
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

                if show_all_markers || cursor_inside {
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

        for (span, _) in &self.code_highlights {
            if span.range.end > content_range.start && span.range.start < content_range.end {
                boundaries.push(span.range.start.max(content_range.start));
                boundaries.push(span.range.end.min(content_range.end));
            }
        }

        boundaries.retain(|&b| b >= content_range.start && b <= content_range.end);
        boundaries.sort();
        boundaries.dedup();

        let mut hidden_ranges: Vec<(usize, usize)> = Vec::new();
        let mut style_ranges: Vec<(Range<usize>, &StyledRegion)> = Vec::new();

        if self.line.heading_level().is_some()
            && !self.cursor_on_line()
            && !self.selection_on_line()
            && let Some(marker_range) = self.line.marker_range()
        {
            hidden_ranges.push((marker_range.start, marker_range.end));
        }

        let is_code_block = self.is_code_block_line();
        let base_code_font = &self.theme.code_font;
        let base_text_font = &self.theme.text_font;

        let ordered_marker_range = self
            .line
            .markers
            .iter()
            .find(|m| matches!(m.kind, MarkerKind::ListItem { ordered: true, .. }))
            .map(|m| m.range.clone());

        for region in &self.inline_styles {
            let cursor_inside = self.cursor_offset >= region.full_range.start
                && self.cursor_offset <= region.full_range.end;

            if !show_all_markers && !cursor_inside {
                let opening_start = region.full_range.start.max(content_range.start);
                let opening_end = region.content_range.start.min(content_range.end);
                if opening_end > opening_start {
                    hidden_ranges.push((opening_start, opening_end));
                }

                let closing_start = region.content_range.end.max(content_range.start);
                let closing_end = region.full_range.end.min(content_range.end);
                if closing_end > closing_start {
                    hidden_ranges.push((closing_start, closing_end));
                }
            }

            let style_range = if show_all_markers || cursor_inside {
                region.full_range.clone()
            } else {
                region.content_range.clone()
            };
            style_ranges.push((style_range, region));
        }

        for window in boundaries.windows(2) {
            let start = window[0];
            let end = window[1];

            if start >= end {
                continue;
            }

            let is_hidden = hidden_ranges
                .iter()
                .any(|&(h_start, h_end)| start >= h_start && end <= h_end);

            if is_hidden {
                continue;
            }

            let span_text = self.slice(start..end);
            let span_len = span_text.len();
            display_text.push_str(&span_text);

            let mut is_bold = false;
            let mut is_italic = false;
            let mut is_code = false;
            let mut is_strikethrough = false;
            let mut is_link = false;

            for (style_range, region) in &style_ranges {
                if style_range.start <= start && end <= style_range.end {
                    is_bold = is_bold || region.style.bold;
                    is_italic = is_italic || region.style.italic;
                    is_code = is_code || region.style.code;
                    is_strikethrough = is_strikethrough || region.style.strikethrough;
                    is_link = is_link || region.link_url.is_some();

                    if is_bold && is_italic && is_code && is_strikethrough && is_link {
                        break;
                    }
                }
            }

            let in_ordered_marker = ordered_marker_range
                .as_ref()
                .is_some_and(|r| start < r.end && end > r.start);

            let base_font = if is_code || is_code_block || in_ordered_marker {
                base_code_font
            } else {
                base_text_font
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
                ..base_font.clone()
            };

            let color: Hsla = if is_strikethrough {
                self.theme.border_color.into()
            } else if is_link {
                self.theme.link_color.into()
            } else if let Some(highlight_color) = self.get_highlight_color_for_range(start, end) {
                highlight_color.into()
            } else if is_code && !is_code_block {
                self.theme.code_color.into()
            } else {
                self.theme.text_color.into()
            };

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
                    color: Some(self.theme.border_color.into()),
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

    fn resolve_image_source(&self, url: &str) -> gpui::ImageSource {
        if url.starts_with("http://") || url.starts_with("https://") {
            return gpui::ImageSource::from(url.to_string());
        }

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
        // When selection is on line or actively selecting on this line, all markers are revealed
        if self.selection_on_line() || (self.is_selecting && self.cursor_on_line()) {
            return 0;
        }

        let mut hidden = 0usize;

        // For headings, the marker is hidden when cursor is not on line
        if self.line.heading_level().is_some()
            && !self.cursor_on_line()
            && let Some(marker_range) = self.line.marker_range()
            && offset > marker_range.end
        {
            hidden += marker_range.end - marker_range.start;
        }

        for region in &self.inline_styles {
            let cursor_inside = self.cursor_offset >= region.full_range.start
                && self.cursor_offset <= region.full_range.end;
            if cursor_inside {
                continue;
            }

            let opening_start = region.full_range.start.max(content_range.start);
            let opening_end = region.content_range.start.min(content_range.end);
            if opening_end > opening_start && offset > opening_end {
                hidden += opening_end - opening_start;
            }

            let closing_start = region.content_range.end.max(content_range.start);
            let closing_end = region.full_range.end.min(content_range.end);
            if closing_end > closing_start && offset > closing_end {
                hidden += closing_end - closing_start;
            }
        }
        hidden
    }

    /// Returns true if the cursor is in the marker area (before content starts)
    fn cursor_in_marker_area(&self) -> bool {
        if !self.cursor_on_line() {
            return false;
        }
        if self.line.is_fence()
            || self.line.is_thematic_break()
            || self.line.heading_level().is_some()
        {
            return false;
        }
        let content_range = self.content_range();
        self.cursor_offset < content_range.start
    }

    fn compute_visual_cursor_pos(&self, display_text: &str) -> Option<usize> {
        if !self.cursor_on_line() {
            return None;
        }
        if self.cursor_in_marker_area() {
            return None;
        }
        Some(self.buffer_to_visual_pos(self.cursor_offset, display_text))
    }

    fn buffer_to_visual_pos(&self, buffer_offset: usize, display_text: &str) -> usize {
        let content_range = if self.line.is_fence()
            || self.line.is_thematic_break()
            || (self.line.heading_level().is_some()
                && (self.cursor_on_line() || self.selection_on_line()))
        {
            self.line.range.clone()
        } else {
            self.content_range()
        };

        if content_range.start >= content_range.end {
            return 0;
        }

        let clamped_offset = buffer_offset.min(content_range.end);

        let hidden = self.hidden_bytes_before(clamped_offset, &content_range);
        let buffer_pos_in_content = clamped_offset.saturating_sub(content_range.start);
        let visual_pos = buffer_pos_in_content.saturating_sub(hidden);

        visual_pos.min(display_text.len())
    }

    /// Convert a visual position (index into display text, after prefix) to a buffer offset.
    /// This is the inverse of `buffer_to_visual_pos` and accounts for hidden regions.
    fn visual_to_buffer_pos(
        visual_index: usize,
        content_range: &Range<usize>,
        heading_marker_len: usize,
        hidden_regions: &[(usize, usize, usize, usize)],
        line_end: usize,
    ) -> usize {
        if content_range.start >= content_range.end {
            return content_range.start;
        }

        if hidden_regions.is_empty() {
            return (content_range.start + heading_marker_len + visual_index).min(line_end);
        }

        let mut buffer_pos = content_range.start + heading_marker_len;
        let mut visible_count = 0usize;

        while buffer_pos < content_range.end && visible_count < visual_index {
            let mut is_hidden = false;

            for &(opening_start, opening_end, closing_start, closing_end) in hidden_regions {
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

        buffer_pos.min(line_end)
    }

    fn compute_visual_selection_range(&self, display_text: &str) -> Option<Range<usize>> {
        let selection = self.selection_range.as_ref()?;
        let line_range = &self.line.range;

        if selection.end <= line_range.start || selection.start >= line_range.end {
            return None;
        }

        let sel_start = selection.start.max(line_range.start);
        let sel_end = selection.end.min(line_range.end);
        let visual_start = self.buffer_to_visual_pos(sel_start, display_text);
        let visual_end = self.buffer_to_visual_pos(sel_end, display_text);

        if visual_start < visual_end {
            Some(visual_start..visual_end)
        } else {
            None
        }
    }

    fn render_cursor(&self, cursor_pos: usize, text_layout: gpui::TextLayout) -> impl IntoElement {
        let cursor_color = self.theme.cursor_color;

        canvas(
            move |_bounds, _window, _cx| text_layout.position_for_index(cursor_pos),
            move |bounds, cursor_pos_result, window: &mut Window, cx| {
                let pos =
                    cursor_pos_result.unwrap_or_else(|| point(bounds.origin.x, bounds.origin.y));

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
            },
        )
        .absolute()
        .size_full()
    }

    fn render_spacer_cursor(&self, char_offset: usize) -> impl IntoElement {
        let cursor_color = self.theme.cursor_color;
        let cursor_font = self.theme.text_font.clone();
        let char_width = self.theme.monospace_char_width;
        let x_pos = char_width * char_offset as f32;

        canvas(
            move |_bounds, _window, _cx| (),
            move |bounds, _, window: &mut Window, cx| {
                let text_style = window.text_style();
                let font_size = text_style.font_size.to_pixels(window.rem_size());
                let line_height = text_style
                    .line_height
                    .to_pixels(font_size.into(), window.rem_size());

                let cursor_char: SharedString = "\u{258F}".into();
                let cursor_font_size = font_size * 1.4;
                let cursor_run = TextRun {
                    len: cursor_char.len(),
                    font: cursor_font.clone(),
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
                let cursor_pos = point(bounds.origin.x + x_pos, bounds.origin.y + y_offset);
                let _ = shaped_cursor.paint(cursor_pos, cursor_height, window, cx);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .w(char_width * (char_offset as f32 + 2.0))
        .h(rems(1.6))
    }
}

fn line_base(line_number: usize) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("line", line_number))
        .max_w(px(800.0))
        .w_full()
        .mx_auto()
}

impl IntoElement for Line {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        let line_number = self.line.line_number;
        let line_range = self.line.range.clone();

        let standalone_image = self.standalone_image_url().map(|url| {
            let source = self.resolve_image_source(url);
            let on_click = self.on_click.clone();
            let line_end = line_range.end;
            let open_url = if url.starts_with("http://") || url.starts_with("https://") {
                url.to_string()
            } else {
                let path = std::path::Path::new(url);
                if path.is_absolute() {
                    url.to_string()
                } else if let Some(base) = &self.base_path {
                    base.join(url).to_string_lossy().to_string()
                } else {
                    url.to_string()
                }
            };
            (source, on_click, line_end, open_url)
        });

        if let Some((source, on_click, line_end, open_url)) = standalone_image.clone()
            && !self.cursor_on_line()
            && !self.selection_on_line()
        {
            return line_base(line_number).child(img(source).max_w_full().on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    if event.modifiers.control || event.modifiers.platform {
                        let _ = open::that(&open_url);
                        return;
                    }
                    if let Some(ref on_click) = on_click {
                        on_click(
                            line_end,
                            event.modifiers.shift,
                            event.click_count,
                            window,
                            cx,
                        );
                    }
                },
            ));
        }

        let (display_text, mut runs) = self.build_styled_content();

        let display_text = if display_text.is_empty() {
            runs.push(self.text_run(1, self.line_font(), self.theme.text_color));
            " ".to_string()
        } else {
            display_text
        };

        let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

        let visual_selection = self.compute_visual_selection_range(&display_text);

        let runs = if let Some(ref sel_range) = visual_selection {
            self.apply_selection_to_runs(runs, sel_range.clone())
        } else {
            runs
        };

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_runs(runs);
        let text_layout = styled_text.layout().clone();

        let mut line_div = line_base(line_number).relative().flex().flex_row();

        let mut spacers: Vec<gpui::Div> = Vec::new();
        let cursor_in_markers = self.cursor_in_marker_area();

        for marker in self.line.markers.iter().rev() {
            let cursor_in_this_marker = cursor_in_markers
                && self.cursor_offset >= marker.range.start
                && self.cursor_offset < marker.range.end;
            let cursor_char_offset = if cursor_in_this_marker {
                self.cursor_offset - marker.range.start
            } else {
                0
            };

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
                    let marker_chars = marker.range.len();
                    let spacer_width = self.theme.monospace_char_width * marker_chars as f32;
                    let border_color = if self.line.in_checked_task {
                        self.theme.selection_color
                    } else {
                        self.theme.border_color
                    };
                    let border_element = div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(2.0))
                        .bg(border_color);
                    let mut spacer = div()
                        .relative()
                        .w(spacer_width)
                        .min_h_full()
                        .child(border_element);
                    if self.marker_in_selection(&marker.range) {
                        spacer = spacer.bg(self.theme.selection_color);
                    }
                    if cursor_in_this_marker {
                        spacer = spacer.child(self.render_spacer_cursor(cursor_char_offset));
                    }
                    if let Some(ref on_click) = self.on_click {
                        let on_click = on_click.clone();
                        let marker_start = marker.range.start;
                        spacer = spacer.on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                on_click(
                                    marker_start,
                                    event.modifiers.shift,
                                    event.click_count,
                                    window,
                                    cx,
                                );
                            },
                        );
                    }
                    if let Some(ref on_drag) = self.on_drag {
                        let on_drag = on_drag.clone();
                        let marker_start = marker.range.start;
                        spacer = spacer.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                cx.stop_propagation();
                                on_drag(marker_start, window, cx);
                            }
                        });
                    }
                    spacers.push(spacer);
                }
                MarkerKind::CodeBlockFence { .. } => {}
                MarkerKind::ThematicBreak => {
                    if !self.cursor_on_line() && !self.selection_on_line() {
                        let line_text = self.slice(self.line.range.clone());
                        let invisible_run = TextRun {
                            len: line_text.len(),
                            font: self.theme.text_font.clone(),
                            color: gpui::transparent_black(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shared_text: SharedString = line_text.into_owned().into();
                        let styled_text =
                            StyledText::new(shared_text).with_runs(vec![invisible_run]);
                        let text_layout = styled_text.layout().clone();

                        let hr_line = div()
                            .absolute()
                            .top_1_2()
                            .left_0()
                            .right_0()
                            .h(px(1.0))
                            .bg(self.theme.border_color);

                        let mut hr_div = line_base(line_number)
                            .relative()
                            .child(styled_text)
                            .child(hr_line);

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
                MarkerKind::Indent => {
                    let indent_chars = marker.range.len();
                    let spacer_width = self.theme.monospace_char_width * indent_chars as f32;
                    let mut spacer = div().relative().w(spacer_width).min_h_full();
                    if self.marker_in_selection(&marker.range) {
                        spacer = spacer.bg(self.theme.selection_color);
                    }
                    if cursor_in_this_marker {
                        spacer = spacer.child(self.render_spacer_cursor(cursor_char_offset));
                    }
                    if let Some(ref on_click) = self.on_click {
                        let on_click = on_click.clone();
                        let marker_start = marker.range.start;
                        spacer = spacer.on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                on_click(
                                    marker_start,
                                    event.modifiers.shift,
                                    event.click_count,
                                    window,
                                    cx,
                                );
                            },
                        );
                    }
                    if let Some(ref on_drag) = self.on_drag {
                        let on_drag = on_drag.clone();
                        let marker_start = marker.range.start;
                        spacer = spacer.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                cx.stop_propagation();
                                on_drag(marker_start, window, cx);
                            }
                        });
                    }
                    spacers.push(spacer);
                }
                MarkerKind::ListItem {
                    ordered,
                    unordered_marker,
                    ..
                } => {
                    let marker_chars = marker.range.len();
                    let spacer_width = self.theme.monospace_char_width * marker_chars as f32;

                    let marker_text = if *ordered {
                        self.slice(marker.range.clone()).into_owned()
                    } else {
                        unordered_marker.map_or("• ", |m| m.bullet()).to_string()
                    };

                    let marker_color = if self.line.in_checked_task {
                        self.theme.selection_color
                    } else {
                        self.theme.text_color
                    };

                    let mut marker_label = div()
                        .relative()
                        .w(spacer_width)
                        .min_h_full()
                        .font_family(self.theme.code_font.family.clone())
                        .text_color(marker_color)
                        .child(marker_text);

                    if self.marker_in_selection(&marker.range) {
                        marker_label = marker_label.bg(self.theme.selection_color);
                    }
                    if cursor_in_this_marker {
                        marker_label =
                            marker_label.child(self.render_spacer_cursor(cursor_char_offset));
                    }

                    if let Some(ref on_click) = self.on_click {
                        let on_click = on_click.clone();
                        let marker_start = marker.range.start;
                        marker_label = marker_label.on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                on_click(
                                    marker_start,
                                    event.modifiers.shift,
                                    event.click_count,
                                    window,
                                    cx,
                                );
                            },
                        );
                    }
                    if let Some(ref on_drag) = self.on_drag {
                        let on_drag = on_drag.clone();
                        let marker_start = marker.range.start;
                        marker_label = marker_label.on_mouse_move(
                            move |event: &MouseMoveEvent, window, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    cx.stop_propagation();
                                    on_drag(marker_start, window, cx);
                                }
                            },
                        );
                    }

                    spacers.push(marker_label);
                }
                MarkerKind::TaskList {
                    checked,
                    unordered_marker,
                } => {
                    let marker_chars = marker.range.len();
                    let spacer_width = self.theme.monospace_char_width * marker_chars as f32;

                    let checkbox_str = if *checked { "[x] " } else { "[ ] " };
                    let bullet = unordered_marker.map_or("• ", |m| m.bullet());

                    let mut bullet_div = div()
                        .font_family(self.theme.code_font.family.clone())
                        .text_color(self.theme.text_color)
                        .child(bullet.to_string());

                    if let Some(ref on_click) = self.on_click {
                        let on_click = on_click.clone();
                        let marker_start = marker.range.start;
                        bullet_div = bullet_div.on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                on_click(
                                    marker_start,
                                    event.modifiers.shift,
                                    event.click_count,
                                    window,
                                    cx,
                                );
                            },
                        );
                    }

                    let mut checkbox_div = div()
                        .font_family(self.theme.code_font.family.clone())
                        .text_color(self.theme.link_color)
                        .cursor(CursorStyle::PointingHand)
                        .child(checkbox_str.to_string());

                    if let Some(ref on_checkbox) = self.on_checkbox {
                        let on_checkbox = on_checkbox.clone();
                        let line_number = self.line.line_number;
                        checkbox_div = checkbox_div.on_mouse_down(
                            MouseButton::Left,
                            move |_event, window, cx| {
                                cx.stop_propagation();
                                on_checkbox(line_number, window, cx);
                            },
                        );
                    }

                    let mut marker_label = div()
                        .relative()
                        .w(spacer_width)
                        .min_h_full()
                        .flex()
                        .flex_row()
                        .child(bullet_div)
                        .child(checkbox_div);

                    if self.marker_in_selection(&marker.range) {
                        marker_label = marker_label.bg(self.theme.selection_color);
                    }
                    if cursor_in_this_marker {
                        marker_label =
                            marker_label.child(self.render_spacer_cursor(cursor_char_offset));
                    }
                    if let Some(ref on_drag) = self.on_drag {
                        let on_drag = on_drag.clone();
                        let marker_start = marker.range.start;
                        marker_label = marker_label.on_mouse_move(
                            move |event: &MouseMoveEvent, window, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    cx.stop_propagation();
                                    on_drag(marker_start, window, cx);
                                }
                            },
                        );
                    }

                    spacers.push(marker_label);
                }
            }
        }

        if self.line.in_code_block {
            line_div = line_div.text_size(rems(0.9));
        }

        let mut text_container = div().relative().flex_1().min_w_0().child(styled_text);

        if let Some(cursor_pos) = visual_cursor_pos {
            text_container =
                text_container.child(self.render_cursor(cursor_pos, text_layout.clone()));
        }

        for spacer in spacers {
            line_div = line_div.child(spacer);
        }
        line_div = line_div.child(text_container);

        let content_range_for_handlers = if self.line.is_fence()
            || self.line.is_thematic_break()
            || self.line.heading_level().is_some()
        {
            self.line.range.clone()
        } else {
            self.content_range()
        };

        let heading_marker_len = if self.line.heading_level().is_some()
            && !self.cursor_on_line()
            && !self.selection_on_line()
        {
            self.line.marker_range().map(|r| r.len()).unwrap_or(0)
        } else {
            0
        };

        let show_all_markers =
            self.selection_on_line() || (self.is_selecting && self.cursor_on_line());
        let cursor_offset = self.cursor_offset;
        let hidden_regions: Vec<(usize, usize, usize, usize)> = if show_all_markers {
            Vec::new()
        } else {
            self.inline_styles
                .iter()
                .filter_map(|region| {
                    let cursor_inside = cursor_offset >= region.full_range.start
                        && cursor_offset <= region.full_range.end;
                    if cursor_inside {
                        None
                    } else {
                        let opening_start = region
                            .full_range
                            .start
                            .max(content_range_for_handlers.start);
                        let opening_end = region
                            .content_range
                            .start
                            .min(content_range_for_handlers.end);
                        let closing_start = region
                            .content_range
                            .end
                            .max(content_range_for_handlers.start);
                        let closing_end = region.full_range.end.min(content_range_for_handlers.end);
                        Some((opening_start, opening_end, closing_start, closing_end))
                    }
                })
                .collect()
        };

        let prefix_len = self.get_substitution().map(|s| s.len()).unwrap_or(0);

        if let Some(ref on_click) = self.on_click {
            let on_click = on_click.clone();
            let on_checkbox = self.on_checkbox.clone();
            let layout_for_click = text_layout.clone();
            let content_range = content_range_for_handlers.clone();
            let line_number = self.line.line_number;
            let hidden_regions = hidden_regions.clone();

            let checkbox_click_range: Option<std::ops::Range<usize>> =
                if self.line.checkbox().is_some() {
                    self.get_substitution().and_then(|prefix| {
                        let start = prefix.find('[')?;
                        let end = prefix.find(']').map(|i| i + 1)?;
                        Some(start..end)
                    })
                } else {
                    None
                };

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

            line_div = line_div.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    if let Some(ref range) = checkbox_click_range
                        && visual_index >= range.start
                        && visual_index < range.end
                        && let Some(ref on_checkbox) = on_checkbox
                    {
                        on_checkbox(line_number, window, cx);
                        return;
                    }

                    let content_visual_index = visual_index.saturating_sub(prefix_len);

                    let buffer_offset = Line::visual_to_buffer_pos(
                        content_visual_index,
                        &content_range,
                        heading_marker_len,
                        &hidden_regions,
                        line_range.end,
                    );

                    if event.modifiers.control || event.modifiers.platform {
                        for (range, url) in &link_regions {
                            if buffer_offset >= range.start && buffer_offset <= range.end {
                                let _ = open::that(url);
                                return;
                            }
                        }
                    }

                    window.prevent_default();
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

        {
            let on_drag = self.on_drag.clone();
            let on_hover = self.on_hover.clone();
            let layout_for_move = text_layout;
            let line_range_for_move = self.line.range.clone();
            let content_range = content_range_for_handlers;

            let checkbox_hover_range: Option<Range<usize>> = if self.line.checkbox().is_some() {
                self.get_substitution().and_then(|prefix| {
                    let start = prefix.find('[')?;
                    let end = prefix.find(']').map(|i| i + 1)?;
                    Some(start..end)
                })
            } else {
                None
            };

            let link_content_ranges: Vec<Range<usize>> = self
                .inline_styles
                .iter()
                .filter(|region| region.link_url.is_some())
                .map(|region| region.content_range.clone())
                .collect();

            line_div = line_div.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                if event.pressed_button == Some(MouseButton::Left)
                    && let Some(ref on_drag) = on_drag
                {
                    let visual_index = match layout_for_move.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    let content_visual_index = visual_index.saturating_sub(prefix_len);
                    let buffer_offset = Line::visual_to_buffer_pos(
                        content_visual_index,
                        &content_range,
                        heading_marker_len,
                        &hidden_regions,
                        line_range_for_move.end,
                    );
                    on_drag(buffer_offset, window, cx);
                }

                if let Some(ref on_hover) = on_hover {
                    let visual_index = match layout_for_move.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    let hovering_checkbox = checkbox_hover_range.as_ref().is_some_and(|range| {
                        visual_index >= range.start && visual_index < range.end
                    });

                    let content_visual_index = visual_index.saturating_sub(prefix_len);
                    let buffer_offset = Line::visual_to_buffer_pos(
                        content_visual_index,
                        &content_range,
                        heading_marker_len,
                        &hidden_regions,
                        line_range_for_move.end,
                    );
                    let hovering_link_region = link_content_ranges
                        .iter()
                        .any(|range| buffer_offset >= range.start && buffer_offset < range.end);

                    on_hover(hovering_checkbox, hovering_link_region, window, cx);
                }
            });
        }

        if let Some((source, _, _, open_url)) = standalone_image {
            return div()
                .id(line_number)
                .max_w(px(800.0))
                .w_full()
                .mx_auto()
                .flex()
                .flex_col()
                .child(line_div)
                .child(img(source).max_w_full().on_mouse_down(
                    MouseButton::Left,
                    move |event: &MouseDownEvent, _, _| {
                        if event.modifiers.control || event.modifiers.platform {
                            let _ = open::that(&open_url);
                        }
                    },
                ))
                .anchor_scroll(self.scroll_anchor);
        }

        line_div.anchor_scroll(self.scroll_anchor)
    }
}
