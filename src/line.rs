use std::borrow::Cow;
use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    App, Font, FontStyle, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, RenderOnce, Rgba, SharedString, StyledText, TextRun, Window, black, canvas,
    div, img, point, prelude::*, px, rems,
};
use ropey::Rope;

use crate::editor::{DispatchEditorAction, EditorAction};
use crate::highlight::HighlightSpan;
use crate::inline::StyledRegion;
use crate::marker::{LineMarkers, MarkerKind};

/// (opening_start, opening_end, closing_start, closing_end) for collapsed markdown syntax.
pub type HiddenRegion = (usize, usize, usize, usize);

/// A display_text region that is currently shown in collapsed form.
/// Used for click position mapping when the user clicks on shortened text.
#[derive(Clone)]
pub struct CollapsedDisplayText {
    /// Visual range in the display string (byte offsets)
    pub visual_range: Range<usize>,
    /// Buffer range of the full content
    pub buffer_range: Range<usize>,
    /// The shortened display text being shown
    pub display_text: String,
    /// The full buffer text (e.g., the full URL)
    pub buffer_text: String,
}

impl CollapsedDisplayText {
    /// Map a pixel x-offset within this collapsed region to a buffer offset.
    /// Uses text measurement to find the proportional position in the full text.
    pub fn map_x_to_buffer_offset(
        &self,
        x_offset: gpui::Pixels,
        font: &Font,
        font_size: gpui::Pixels,
        window: &Window,
    ) -> usize {
        let full_text: SharedString = self.buffer_text.clone().into();
        let run = TextRun {
            len: self.buffer_text.len(),
            font: font.clone(),
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(full_text, font_size, &[run], None);
        let index = shaped.index_for_x(x_offset.max(px(0.0))).unwrap_or(0);
        self.buffer_range.start + index
    }
}

/// Convert a visual position (index into display text, after prefix) to a buffer offset.
pub fn visual_to_buffer_pos(
    visual_index: usize,
    content_range: &Range<usize>,
    heading_marker_len: usize,
    hidden_regions: &[HiddenRegion],
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
        let is_hidden = hidden_regions.iter().any(
            |&(opening_start, opening_end, closing_start, closing_end)| {
                (buffer_pos >= opening_start && buffer_pos < opening_end)
                    || (buffer_pos >= closing_start && buffer_pos < closing_end)
            },
        );

        if !is_hidden {
            visible_count += 1;
        }
        buffer_pos += 1;
    }

    buffer_pos.min(line_end)
}

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
    pub checkbox_unchecked_color: Rgba,
    pub checkbox_checked_color: Rgba,
    pub text_font: Font,
    pub code_font: Font,
    /// Width of a single monospace character in the code font.
    /// Used for precise indentation of nested blocks.
    pub monospace_char_width: gpui::Pixels,
    /// Line height for text lines.
    pub line_height: gpui::Rems,
}

#[derive(IntoElement)]
pub struct Line {
    line: LineMarkers,
    rope: Rope,
    cursor_offset: usize,
    inline_styles: Vec<StyledRegion>,
    theme: LineTheme,
    selection_range: Option<Range<usize>>,
    code_highlights: Vec<(HighlightSpan, Rgba)>,
    base_path: Option<PathBuf>,
    substitution: Option<String>,
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
            substitution,
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

        // Use marker_range() which excludes Checkbox markers.
        // The checkbox text "[ ] " will be part of the content and rendered
        // inline (not as a spacer), which gives correct wrap indent.
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

    fn build_styled_content(&self) -> (String, Vec<TextRun>, Vec<CollapsedDisplayText>) {
        let content_range = if self.line.heading_level().is_some() {
            self.line.range.clone()
        } else {
            self.content_range()
        };

        let mut display_text = String::new();
        let mut runs: Vec<TextRun> = Vec::new();
        let mut collapsed_regions: Vec<CollapsedDisplayText> = Vec::new();

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
            // Use content after prefix markers (Indent, BlockQuote) but include the fence itself
            let fence_start = self
                .line
                .prefix_marker_range()
                .map(|r| r.end)
                .unwrap_or(self.line.range.start);
            let fence_text = self.slice(fence_start..self.line.range.end);
            // Fence can be ``` or ~~~
            let fence_char = fence_text.chars().next().unwrap_or('`');
            let fence_markers: String = fence_text
                .chars()
                .take_while(|&c| c == fence_char && (c == '`' || c == '~'))
                .collect();
            let language = &fence_text[fence_markers.len()..];

            if !fence_markers.is_empty() {
                display_text.push_str(&fence_markers);
                runs.push(self.text_run(
                    fence_markers.len(),
                    self.theme.code_font.clone(),
                    self.theme.fence_color,
                ));
            }

            if !language.is_empty() {
                display_text.push_str(language);
                runs.push(self.text_run(
                    language.len(),
                    self.theme.code_font.clone(),
                    self.theme.fence_lang_color,
                ));
            }

            return (display_text, runs, collapsed_regions);
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

            return (display_text, runs, collapsed_regions);
        }

        if content_range.start >= content_range.end {
            return (display_text, runs, collapsed_regions);
        }

        let mut boundaries: Vec<usize> = vec![content_range.start, content_range.end];
        let show_all_markers = false;

        if self.line.heading_level().is_some()
            && let Some(marker_range) = self.line.marker_range()
        {
            boundaries.push(marker_range.start);
            boundaries.push(marker_range.end);
        }

        for region in &self.inline_styles {
            // Add boundaries for checkbox regions (from synthetic StyledRegion)
            if region.checkbox.is_some() {
                boundaries.push(region.full_range.start.max(content_range.start));
                boundaries.push(region.full_range.end.min(content_range.end));
            }
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

            // Check if this span exactly matches a display_text region
            // (naked URLs create a single boundary window matching their full_range)
            // Only show display_text when cursor is NOT inside the region;
            // when cursor is inside, expand to show the full buffer content.
            let display_text_region = self.inline_styles.iter().find(|region| {
                region.display_text.is_some()
                    && region.full_range.start == start
                    && region.full_range.end == end
                    && !(self.cursor_offset >= region.full_range.start
                        && self.cursor_offset <= region.full_range.end)
            });

            if let Some(region) = display_text_region {
                // Emit the display_text instead of the buffer content
                let substitution = region.display_text.as_ref().unwrap();
                let visual_start = display_text.len();
                display_text.push_str(substitution);
                let visual_end = display_text.len();

                // Track this collapsed region for click position mapping
                let buffer_text = self.slice(region.full_range.start..region.full_range.end);
                collapsed_regions.push(CollapsedDisplayText {
                    visual_range: visual_start..visual_end,
                    buffer_range: region.full_range.clone(),
                    display_text: substitution.clone(),
                    buffer_text: buffer_text.to_string(),
                });

                // Style it as a link
                runs.push(TextRun {
                    len: substitution.len(),
                    font: self.theme.text_font.clone(),
                    color: self.theme.link_color.into(),
                    background_color: None,
                    underline: Some(gpui::UnderlineStyle {
                        thickness: px(1.0),
                        color: Some(self.theme.link_color.into()),
                        wavy: false,
                    }),
                    strikethrough: None,
                });
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
            let mut checkbox_state: Option<bool> = None; // None = not checkbox, Some(false) = unchecked, Some(true) = checked

            for (style_range, region) in &style_ranges {
                if style_range.start <= start && end <= style_range.end {
                    is_bold = is_bold || region.style.bold;
                    is_italic = is_italic || region.style.italic;
                    is_code = is_code || region.style.code;
                    is_strikethrough = is_strikethrough || region.style.strikethrough;
                    is_link = is_link || region.link_url.is_some();
                    if checkbox_state.is_none() && region.checkbox.is_some() {
                        checkbox_state = region.checkbox;
                    }

                    if is_bold
                        && is_italic
                        && is_code
                        && is_strikethrough
                        && is_link
                        && checkbox_state.is_some()
                    {
                        break;
                    }
                }
            }
            let is_checkbox = checkbox_state.is_some();

            let in_ordered_marker = ordered_marker_range
                .as_ref()
                .is_some_and(|r| start < r.end && end > r.start);

            let base_font = if is_code || is_code_block || in_ordered_marker || is_checkbox {
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
            } else if let Some(checked) = checkbox_state {
                if checked {
                    self.theme.checkbox_checked_color.into()
                } else {
                    self.theme.checkbox_unchecked_color.into()
                }
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

        (display_text, runs, collapsed_regions)
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
        let content_range = if self.line.is_fence() {
            // Fence content starts after prefix markers (Indent, BlockQuote)
            let start = self
                .line
                .prefix_marker_range()
                .map(|r| r.end)
                .unwrap_or(self.line.range.start);
            start..self.line.range.end
        } else if self.line.is_thematic_break()
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
            move |_bounds, _window, _cx| {
                // For cursor positioning, we want the position at the START of the character
                // at cursor_pos. At wrap boundaries, position_for_index(n) returns end-of-row
                // position. Check if next char exists and is on a different row - if so, use
                // the y from next char's position with x=0 (start of that row).
                if cursor_pos < text_layout.len()
                    && let Some(next_pos) = text_layout.position_for_index(cursor_pos + 1)
                    && let Some(curr_pos) = text_layout.position_for_index(cursor_pos)
                {
                    let line_height = text_layout.line_height();
                    let curr_row = (curr_pos.y / line_height).floor();
                    let next_row = (next_pos.y / line_height).floor();
                    // At wrap boundary: curr is at end of row, next is at start of new row
                    if next_row > curr_row {
                        // The cursor should appear at the start of the wrapped row.
                        // Use x from first char (index 0) to get the row start position.
                        let row_start_x = text_layout
                            .position_for_index(0)
                            .map(|p| p.x)
                            .unwrap_or(px(0.0));
                        return Some(point(row_start_x, next_pos.y));
                    }
                }
                text_layout.position_for_index(cursor_pos)
            },
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
        .h(self.theme.line_height)
    }
}

fn line_base(line_number: usize) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("line", line_number))
        .max_w(px(800.0))
        .w_full()
        .mx_auto()
}

impl RenderOnce for Line {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let line_number = self.line.line_number;
        let line_range = self.line.range.clone();

        let standalone_image = self.standalone_image_url().map(|url| {
            let source = self.resolve_image_source(url);
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
            (source, line_end, open_url)
        });

        if let Some((source, line_end, open_url)) = standalone_image.clone()
            && !self.cursor_on_line()
            && !self.selection_on_line()
        {
            return line_base(line_number).child(img(source).max_w_full().on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    if event.modifiers.control || event.modifiers.platform {
                        window.dispatch_action(
                            Box::new(DispatchEditorAction(EditorAction::OpenLink {
                                url: open_url.clone(),
                            })),
                            cx,
                        );
                        return;
                    }
                    window.dispatch_action(
                        Box::new(DispatchEditorAction(EditorAction::Click {
                            offset: line_end,
                            shift: event.modifiers.shift,
                            click_count: event.click_count,
                        })),
                        cx,
                    );
                },
            ));
        }

        let (display_text, mut runs, collapsed_regions) = self.build_styled_content();

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
                    let marker_start = marker.range.start;
                    spacer = spacer.on_mouse_down(
                        MouseButton::Left,
                        move |event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.dispatch_action(
                                Box::new(DispatchEditorAction(EditorAction::Click {
                                    offset: marker_start,
                                    shift: event.modifiers.shift,
                                    click_count: event.click_count,
                                })),
                                cx,
                            );
                        },
                    );
                    let marker_start = marker.range.start;
                    spacer = spacer.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            cx.stop_propagation();
                            window.dispatch_action(
                                Box::new(DispatchEditorAction(EditorAction::Drag {
                                    offset: marker_start,
                                })),
                                cx,
                            );
                        }
                    });
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
                                window.dispatch_action(
                                    Box::new(DispatchEditorAction(EditorAction::Click {
                                        offset: buffer_offset,
                                        shift: event.modifiers.shift,
                                        click_count: event.click_count,
                                    })),
                                    cx,
                                );
                            },
                        );

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
                    let marker_start = marker.range.start;
                    spacer = spacer.on_mouse_down(
                        MouseButton::Left,
                        move |event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.dispatch_action(
                                Box::new(DispatchEditorAction(EditorAction::Click {
                                    offset: marker_start,
                                    shift: event.modifiers.shift,
                                    click_count: event.click_count,
                                })),
                                cx,
                            );
                        },
                    );
                    let marker_start = marker.range.start;
                    spacer = spacer.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            cx.stop_propagation();
                            window.dispatch_action(
                                Box::new(DispatchEditorAction(EditorAction::Drag {
                                    offset: marker_start,
                                })),
                                cx,
                            );
                        }
                    });
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

                    let marker_start = marker.range.start;
                    marker_label = marker_label.on_mouse_down(
                        MouseButton::Left,
                        move |event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.dispatch_action(
                                Box::new(DispatchEditorAction(EditorAction::Click {
                                    offset: marker_start,
                                    shift: event.modifiers.shift,
                                    click_count: event.click_count,
                                })),
                                cx,
                            );
                        },
                    );
                    let marker_start = marker.range.start;
                    marker_label =
                        marker_label.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                cx.stop_propagation();
                                window.dispatch_action(
                                    Box::new(DispatchEditorAction(EditorAction::Drag {
                                        offset: marker_start,
                                    })),
                                    cx,
                                );
                            }
                        });

                    spacers.push(marker_label);
                }
                MarkerKind::Checkbox { .. } => {
                    // Checkbox is rendered inline with the text, not as a spacer.
                    // The "[ ] " text is part of content_range and will be rendered
                    // in the styled text section. Click handling is done there.
                    // This gives correct wrap indent (only list marker width, not checkbox).
                }
            }
        }

        if self.is_code_block_line() {
            line_div = line_div.text_size(rems(0.9));
        }

        let mut text_container = div().relative().flex_1().min_w_0().child(styled_text);

        if let Some(cursor_pos) = visual_cursor_pos {
            text_container =
                text_container.child(self.render_cursor(cursor_pos, text_layout.clone()));
        }

        let content_range_for_handlers = if self.line.is_fence() {
            // Fence content starts after prefix markers (Indent, BlockQuote)
            let start = self
                .line
                .prefix_marker_range()
                .map(|r| r.end)
                .unwrap_or(self.line.range.start);
            start..self.line.range.end
        } else if self.line.is_thematic_break() || self.line.heading_level().is_some() {
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

        let show_all_markers = false;
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

        {
            let layout_for_click = text_layout.clone();
            let content_range = content_range_for_handlers.clone();
            let line_number = self.line.line_number;
            let hidden_regions = hidden_regions.clone();
            let collapsed_regions_for_click = collapsed_regions.clone();
            let text_font_for_click = self.theme.text_font.clone();

            // Find the checkbox click range in the content.
            // The checkbox "[ ]" or "[x]" is now rendered as part of content, not substitution.
            let checkbox_click_range: Option<std::ops::Range<usize>> =
                if self.line.checkbox().is_some() {
                    // The checkbox is at the start of the content (after list marker spacer)
                    // It's 4 chars: "[ ] " or "[x] "
                    Some(prefix_len..prefix_len + 4)
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

            text_container = text_container.on_mouse_down(
                MouseButton::Left,
                move |event: &MouseDownEvent, window, cx| {
                    let visual_index = match layout_for_click.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    if let Some(ref range) = checkbox_click_range
                        && visual_index >= range.start
                        && visual_index < range.end
                    {
                        window.dispatch_action(
                            Box::new(DispatchEditorAction(EditorAction::ToggleCheckbox {
                                line_number,
                            })),
                            cx,
                        );
                        return;
                    }

                    let content_visual_index = visual_index.saturating_sub(prefix_len);

                    // Check if click is within a collapsed display_text region
                    // If so, use text measurement to map the click position proportionally
                    let collapsed_region = collapsed_regions_for_click.iter().find(|r| {
                        content_visual_index >= r.visual_range.start
                            && content_visual_index < r.visual_range.end
                    });

                    let buffer_offset = if let Some(region) = collapsed_region {
                        // Get pixel position relative to start of the collapsed text
                        if let Some(visual_start_pos) = layout_for_click
                            .position_for_index(prefix_len + region.visual_range.start)
                        {
                            let x_offset = event.position.x - visual_start_pos.x;
                            let font_size = window.rem_size(); // Use rem as base font size
                            region.map_x_to_buffer_offset(
                                x_offset,
                                &text_font_for_click,
                                font_size,
                                window,
                            )
                        } else {
                            region.buffer_range.start
                        }
                    } else {
                        visual_to_buffer_pos(
                            content_visual_index,
                            &content_range,
                            heading_marker_len,
                            &hidden_regions,
                            line_range.end,
                        )
                    };

                    if event.modifiers.control || event.modifiers.platform {
                        for (range, url) in &link_regions {
                            if buffer_offset >= range.start && buffer_offset <= range.end {
                                window.dispatch_action(
                                    Box::new(DispatchEditorAction(EditorAction::OpenLink {
                                        url: url.clone(),
                                    })),
                                    cx,
                                );
                                return;
                            }
                        }
                    }

                    window.prevent_default();
                    window.dispatch_action(
                        Box::new(DispatchEditorAction(EditorAction::Click {
                            offset: buffer_offset,
                            shift: event.modifiers.shift,
                            click_count: event.click_count,
                        })),
                        cx,
                    );
                },
            );
        }

        {
            let layout_for_move = text_layout;
            let line_range_for_move = self.line.range.clone();
            let content_range = content_range_for_handlers;
            let collapsed_regions_for_move = collapsed_regions;
            let text_font_for_move = self.theme.text_font.clone();

            // Checkbox hover range - checkbox is now at start of content (after spacer)
            let checkbox_hover_range: Option<Range<usize>> = if self.line.checkbox().is_some() {
                // The checkbox is 4 chars: "[ ] " or "[x] "
                Some(prefix_len..prefix_len + 4)
            } else {
                None
            };

            let link_content_ranges: Vec<Range<usize>> = self
                .inline_styles
                .iter()
                .filter(|region| region.link_url.is_some())
                .map(|region| region.content_range.clone())
                .collect();

            text_container =
                text_container.on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        let visual_index = match layout_for_move.index_for_position(event.position)
                        {
                            Ok(idx) => idx,
                            Err(idx) => idx,
                        };

                        let content_visual_index = visual_index.saturating_sub(prefix_len);

                        // Check if drag is within a collapsed display_text region
                        let collapsed_region = collapsed_regions_for_move.iter().find(|r| {
                            content_visual_index >= r.visual_range.start
                                && content_visual_index < r.visual_range.end
                        });

                        let buffer_offset = if let Some(region) = collapsed_region {
                            if let Some(visual_start_pos) = layout_for_move
                                .position_for_index(prefix_len + region.visual_range.start)
                            {
                                let x_offset = event.position.x - visual_start_pos.x;
                                let font_size = window.rem_size();
                                region.map_x_to_buffer_offset(
                                    x_offset,
                                    &text_font_for_move,
                                    font_size,
                                    window,
                                )
                            } else {
                                region.buffer_range.start
                            }
                        } else {
                            visual_to_buffer_pos(
                                content_visual_index,
                                &content_range,
                                heading_marker_len,
                                &hidden_regions,
                                line_range_for_move.end,
                            )
                        };

                        window.dispatch_action(
                            Box::new(DispatchEditorAction(EditorAction::Drag {
                                offset: buffer_offset,
                            })),
                            cx,
                        );
                    }

                    let visual_index = match layout_for_move.index_for_position(event.position) {
                        Ok(idx) => idx,
                        Err(idx) => idx,
                    };

                    let hovering_checkbox = checkbox_hover_range.as_ref().is_some_and(|range| {
                        visual_index >= range.start && visual_index < range.end
                    });

                    let content_visual_index = visual_index.saturating_sub(prefix_len);
                    let buffer_offset = visual_to_buffer_pos(
                        content_visual_index,
                        &content_range,
                        heading_marker_len,
                        &hidden_regions,
                        line_range_for_move.end,
                    );
                    let hovering_link_region = link_content_ranges
                        .iter()
                        .any(|range| buffer_offset >= range.start && buffer_offset < range.end);

                    window.dispatch_action(
                        Box::new(DispatchEditorAction(EditorAction::UpdateHover {
                            over_checkbox: hovering_checkbox,
                            over_link: hovering_link_region,
                        })),
                        cx,
                    );
                });
        }

        // Add spacers and text container to line_div
        for spacer in spacers {
            line_div = line_div.child(spacer);
        }
        line_div = line_div.child(text_container);

        if let Some((source, _, open_url)) = standalone_image {
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
                ));
        }

        line_div
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapsed_display_text_ranges() {
        // Simulate a GitHub URL at buffer positions 10..54 (44 chars)
        // displayed as "rust-lang/rust#123" (18 chars) at visual positions 5..23
        let collapsed = CollapsedDisplayText {
            visual_range: 5..23,
            buffer_range: 10..54,
            display_text: "rust-lang/rust#123".to_string(),
            buffer_text: "https://github.com/rust-lang/rust/issues/123".to_string(),
        };

        // Verify the display text is shorter than buffer text
        assert!(collapsed.display_text.len() < collapsed.buffer_text.len());

        // Verify ranges are consistent
        assert_eq!(
            collapsed.visual_range.len(),
            collapsed.display_text.len(),
            "visual_range length should match display_text length"
        );
        assert_eq!(
            collapsed.buffer_range.len(),
            collapsed.buffer_text.len(),
            "buffer_range length should match buffer_text length"
        );
    }

    #[test]
    fn test_collapsed_region_detection_logic() {
        // Test the logic used to find collapsed regions during click handling
        let collapsed_regions = [
            CollapsedDisplayText {
                visual_range: 0..18,
                buffer_range: 0..44,
                display_text: "rust-lang/rust#123".to_string(),
                buffer_text: "https://github.com/rust-lang/rust/issues/123".to_string(),
            },
            CollapsedDisplayText {
                visual_range: 25..43,
                buffer_range: 51..95,
                display_text: "rust-lang/rust#456".to_string(),
                buffer_text: "https://github.com/rust-lang/rust/issues/456".to_string(),
            },
        ];

        // Click at visual index 10 should find first region
        let content_visual_index = 10;
        let found = collapsed_regions.iter().find(|r| {
            content_visual_index >= r.visual_range.start
                && content_visual_index < r.visual_range.end
        });
        assert!(found.is_some());
        assert_eq!(found.unwrap().buffer_range, 0..44);

        // Click at visual index 30 should find second region
        let content_visual_index = 30;
        let found = collapsed_regions.iter().find(|r| {
            content_visual_index >= r.visual_range.start
                && content_visual_index < r.visual_range.end
        });
        assert!(found.is_some());
        assert_eq!(found.unwrap().buffer_range, 51..95);

        // Click at visual index 20 (between regions) should find nothing
        let content_visual_index = 20;
        let found = collapsed_regions.iter().find(|r| {
            content_visual_index >= r.visual_range.start
                && content_visual_index < r.visual_range.end
        });
        assert!(found.is_none());
    }
}
