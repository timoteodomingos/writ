use std::borrow::Cow;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Font, FontStyle, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, Rgba, ScrollAnchor, SharedString, StyledText, TextRun, Window, canvas, div,
    img, point, prelude::*, px, rems,
};
use ropey::Rope;

use crate::highlight::HighlightSpan;
use crate::marker::{LineMarkers, MarkerKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub heading_level: u8,
}

impl TextStyle {
    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Default::default()
        }
    }

    pub fn italic() -> Self {
        Self {
            italic: true,
            ..Default::default()
        }
    }

    pub fn code() -> Self {
        Self {
            code: true,
            ..Default::default()
        }
    }

    pub fn strikethrough() -> Self {
        Self {
            strikethrough: true,
            ..Default::default()
        }
    }

    pub fn heading(level: u8) -> Self {
        Self {
            heading_level: level,
            bold: true,
            ..Default::default()
        }
    }

    pub fn merge(&self, other: &TextStyle) -> Self {
        Self {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            code: self.code || other.code,
            strikethrough: self.strikethrough || other.strikethrough,
            heading_level: self.heading_level.max(other.heading_level),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyledRegion {
    pub full_range: Range<usize>,
    pub content_range: Range<usize>,
    pub style: TextStyle,
    pub link_url: Option<String>,
    pub is_image: bool,
}

use crate::buffer::Buffer;
use crate::parser::MarkdownTree;
use tree_sitter::Node;

pub fn extract_inline_styles(buffer: &Buffer, line: &LineMarkers) -> Vec<StyledRegion> {
    extract_inline_styles_from_parts(buffer.rope(), buffer.tree(), line)
}

fn extract_inline_styles_from_parts(
    rope: &Rope,
    tree: Option<&MarkdownTree>,
    line: &LineMarkers,
) -> Vec<StyledRegion> {
    let Some(tree) = tree else {
        return Vec::new();
    };

    let mut styles = Vec::new();
    let root = tree.block_tree().root_node();
    collect_inline_styles_in_range(&root, tree, rope, &line.range, &mut styles);

    styles
}

fn collect_inline_styles_in_range(
    node: &Node,
    tree: &MarkdownTree,
    rope: &Rope,
    range: &Range<usize>,
    styles: &mut Vec<StyledRegion>,
) {
    // Skip nodes that don't overlap with our range
    if node.end_byte() <= range.start || node.start_byte() >= range.end {
        return;
    }

    // If this is an inline node, get its inline tree and collect styles
    if node.kind() == "inline" {
        if let Some(inline_tree) = tree.inline_tree(node) {
            collect_inline_styles_recursive(inline_tree.root_node(), rope, styles);
        }
        return;
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_inline_styles_in_range(&child, tree, rope, range, styles);
        }
    }
}

fn collect_inline_styles_recursive(node: Node, rope: &Rope, styles: &mut Vec<StyledRegion>) {
    match node.kind() {
        "emphasis" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::italic()) {
                styles.push(region);
            }
        }
        "strong_emphasis" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::bold()) {
                styles.push(region);
            }
        }
        "code_span" => {
            if let Some(region) = extract_code_span_region(&node) {
                styles.push(region);
            }
        }
        "strikethrough" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::strikethrough()) {
                styles.push(region);
            }
        }
        "inline_link" | "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
            if let Some(region) = extract_link_region(&node, rope) {
                styles.push(region);
            }
        }
        "image" => {
            if let Some(region) = extract_image_region(&node, rope) {
                styles.push(region);
            }
        }
        _ => {}
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_inline_styles_recursive(child, rope, styles);
        }
    }
}

fn extract_emphasis_region(node: &Node, style: TextStyle) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    // Find delimiter boundaries
    let mut delimiters: Vec<(usize, usize)> = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "emphasis_delimiter" || kind.ends_with("_delimiter") {
                delimiters.push((child.start_byte(), child.end_byte()));
            }
        }
    }

    // Opening delimiters from start
    for &(start, end) in &delimiters {
        if start == content_start {
            content_start = end;
        }
    }

    // Closing delimiters from end
    for &(start, end) in delimiters.iter().rev() {
        if end == content_end {
            content_end = start;
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style,
        link_url: None,
        is_image: false,
    })
}

fn extract_code_span_region(node: &Node) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "code_span_delimiter"
        {
            if child.start_byte() == full_start {
                content_start = child.end_byte();
            } else if child.end_byte() == full_end {
                content_end = child.start_byte();
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::code(),
        link_url: None,
        is_image: false,
    })
}

fn extract_link_region(node: &Node, rope: &Rope) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;
    let mut url: Option<String> = None;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "link_text" => {
                    content_start = child.start_byte();
                    content_end = child.end_byte();
                }
                "link_destination" => {
                    let start = rope.byte_to_char(child.start_byte());
                    let end = rope.byte_to_char(child.end_byte());
                    url = Some(rope.slice(start..end).to_string());
                }
                _ => {}
            }
        }
    }

    if url.is_none() {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "[" {
                    content_start = child.end_byte();
                } else if child.kind() == "]" {
                    content_end = child.start_byte();
                }
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::default(),
        link_url: url,
        is_image: false,
    })
}

fn extract_image_region(node: &Node, rope: &Rope) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut alt_start = full_start;
    let mut alt_end = full_end;
    let mut url: Option<String> = None;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "image_description" => {
                    alt_start = child.start_byte();
                    alt_end = child.end_byte();
                }
                "link_destination" => {
                    let start = rope.byte_to_char(child.start_byte());
                    let end = rope.byte_to_char(child.end_byte());
                    url = Some(rope.slice(start..end).to_string());
                }
                _ => {}
            }
        }
    }

    let url = url?;

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: alt_start..alt_end,
        style: TextStyle::default(),
        link_url: Some(url),
        is_image: true,
    })
}

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

pub struct Line<'a> {
    line: &'a LineMarkers,
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
}

impl<'a> Line<'a> {
    pub fn new(
        line: &'a LineMarkers,
        rope: Rope,
        cursor_offset: usize,
        inline_styles: Vec<StyledRegion>,
        theme: LineTheme,
        selection_range: Option<Range<usize>>,
        code_highlights: Vec<(HighlightSpan, Rgba)>,
        base_path: Option<PathBuf>,
    ) -> Self {
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

    fn is_code_block_line(&self) -> bool {
        !self.code_highlights.is_empty() || self.line.is_fence()
    }

    fn standalone_image_url(&self) -> Option<&str> {
        if self.cursor_on_line() || self.selection_on_line() {
            return None;
        }

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
                fence_marker.range.start..range.end
            } else {
                marker_range.end..range.end
            }
        } else {
            range.clone()
        }
    }

    fn get_substitution(&self) -> Option<String> {
        let substitution = self.line.substitution_rope(&self.rope);
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

            // Check if this run overlaps with the selection
            if run_end <= selection_range.start || run_start >= selection_range.end {
                // No overlap, keep run as-is
                result.push(run);
            } else {
                // There's overlap - we may need to split the run
                let sel_start_in_run = selection_range.start.saturating_sub(run_start);
                let sel_end_in_run = (selection_range.end - run_start).min(run.len);

                // Part before selection (if any)
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

                // Selected part
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

                // Part after selection (if any)
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
        let content_range = self.content_range();

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
                    display_text.push_str(&prefix);
                    runs.push(self.text_run(
                        prefix.len(),
                        self.theme.code_font.clone(),
                        self.theme.text_color,
                    ));
                }
            } else {
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

        if self.line.is_fence() {
            let fence_text = self.slice(content_range.clone());
            let backticks: String = fence_text.chars().take_while(|&c| c == '`').collect();
            let language = fence_text[backticks.len()..].trim_end();

            if !backticks.is_empty() {
                display_text.push_str(&backticks);
                runs.push(self.text_run(
                    backticks.len(),
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

            return (display_text, runs);
        }

        let mut boundaries: Vec<usize> = vec![content_range.start, content_range.end];
        let show_all_markers = self.selection_on_line();

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

        for window in boundaries.windows(2) {
            let start = window[0];
            let end = window[1];

            if start >= end {
                continue;
            }

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

            let span_text = self.slice(start..end);
            let span_len = span_text.len();
            display_text.push_str(&span_text);

            let mut is_bold = false;
            let mut is_italic = false;
            let mut is_code = false;
            let mut is_strikethrough = false;
            let mut is_link = false;

            for region in &self.inline_styles {
                let cursor_inside = self.cursor_offset >= region.full_range.start
                    && self.cursor_offset <= region.full_range.end;

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

            let color: Hsla = if is_link {
                self.theme.link_color.into()
            } else if let Some(highlight_color) = self.get_highlight_color_for_range(start, end) {
                highlight_color.into()
            } else if is_code && !self.is_code_block_line() {
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

    fn compute_visual_cursor_pos(&self, display_text: &str) -> Option<usize> {
        if !self.cursor_on_line() {
            return None;
        }
        Some(self.buffer_to_visual_pos(self.cursor_offset, display_text))
    }

    fn buffer_to_visual_pos(&self, buffer_offset: usize, display_text: &str) -> usize {
        let content_range = self.content_range();
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

impl IntoElement for Line<'_> {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        let line_number = self.line.line_number;
        let line_range = self.line.range.clone();

        if let Some(url) = self.standalone_image_url() {
            let source = self.resolve_image_source(url);
            return line_base(line_number).child(img(source).max_w_full());
        }

        let (display_text, mut runs) = self.build_styled_content();
        let visual_cursor_pos = self.compute_visual_cursor_pos(&display_text);

        let display_text = if display_text.is_empty() {
            runs.push(self.text_run(1, self.line_font(), self.theme.text_color));
            " ".to_string()
        } else {
            display_text
        };

        let visual_selection = self.compute_visual_selection_range(&display_text);

        // Apply selection background color to runs
        let runs = if let Some(ref sel_range) = visual_selection {
            self.apply_selection_to_runs(runs, sel_range.clone())
        } else {
            runs
        };

        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text).with_runs(runs);
        let text_layout = styled_text.layout().clone();

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
                    line_div = line_div
                        .pl_3()
                        .border_l_2()
                        .border_color(self.theme.border_color);
                }
                MarkerKind::CodeBlockFence { .. } | MarkerKind::CodeBlockContent => {
                    line_div = line_div.text_size(rems(0.9));
                }
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
                            .top_1_2() // Center vertically
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
                MarkerKind::ListItem { .. } | MarkerKind::TaskList { .. } | MarkerKind::Indent => {}
            }
        }

        // If the only marker is Indent (nested block like paragraph under list item),
        // add left padding so wrapped lines align properly.
        let needs_indent_padding =
            self.line.markers.len() == 1 && matches!(self.line.markers[0].kind, MarkerKind::Indent);

        let mut text_container = div().relative().child(styled_text);

        if let Some(cursor_pos) = visual_cursor_pos {
            text_container =
                text_container.child(self.render_cursor(cursor_pos, text_layout.clone()));
        }

        if needs_indent_padding {
            // Use the actual indent marker length (varies: "- " = 2, "1. " = 3, "10. " = 4, etc.)
            let indent_chars = self.line.markers[0].range.len();
            let indent_width = self.theme.monospace_char_width * indent_chars as f32;
            line_div = line_div.pl(indent_width).child(text_container);
        } else {
            line_div = line_div.child(text_container);
        }

        if let Some(ref on_click) = self.on_click {
            let on_click = on_click.clone();
            let on_checkbox = self.on_checkbox.clone();
            let layout_for_click = text_layout.clone();
            let content_range = self.content_range();
            let line_number = self.line.line_number;

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

            let show_all_markers = self.selection_on_line();
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
                            let opening_start = region.full_range.start.max(content_range.start);
                            let opening_end = region.content_range.start.min(content_range.end);
                            let closing_start = region.content_range.end.max(content_range.start);
                            let closing_end = region.full_range.end.min(content_range.end);
                            Some((opening_start, opening_end, closing_start, closing_end))
                        }
                    })
                    .collect()
            };

            let prefix_len = self.get_substitution().map(|s| s.len()).unwrap_or(0);

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

                    let buffer_offset = {
                        if content_range.start >= content_range.end {
                            content_range.start
                        } else {
                            let mut buffer_pos = content_range.start;
                            let mut visible_count = 0usize;

                            while buffer_pos < content_range.end
                                && visible_count < content_visual_index
                            {
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

                    if event.modifiers.control || event.modifiers.platform {
                        for (range, url) in &link_regions {
                            if buffer_offset >= range.start && buffer_offset <= range.end {
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

        {
            let on_drag = self.on_drag.clone();
            let on_hover = self.on_hover.clone();
            let layout_for_move = text_layout;
            let line_range_for_move = self.line.range.clone();
            let content_range = self.content_range();
            let prefix_len = self.get_substitution().map(|s| s.len()).unwrap_or(0);

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
                    let buffer_offset =
                        (content_range.start + content_visual_index).min(line_range_for_move.end);
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
                    let buffer_offset =
                        (content_range.start + content_visual_index).min(line_range_for_move.end);
                    let hovering_link_region = link_content_ranges
                        .iter()
                        .any(|range| buffer_offset >= range.start && buffer_offset < range.end);

                    on_hover(hovering_checkbox, hovering_link_region, window, cx);
                }
            });
        }

        line_div.anchor_scroll(self.scroll_anchor)
    }
}
