use std::ops::Range;

use gpui::{FontStyle, FontWeight, HighlightStyle, px};
use strum::EnumDiscriminants;

use crate::theme::Theme;

#[derive(EnumDiscriminants, Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextStyle {
    Bold,
    Italic,
    Code,
    Strikethrough,
    Link { url: String },
    Image { url: String },
}

impl TextStyle {
    fn open_marker(&self) -> &str {
        match self {
            TextStyle::Bold => "**",
            TextStyle::Italic => "*",
            TextStyle::Code => "`",
            TextStyle::Strikethrough => "~~",
            TextStyle::Link { .. } => "[",
            TextStyle::Image { .. } => "![",
        }
    }

    fn close_marker(&self) -> String {
        match self {
            TextStyle::Bold => "**".to_string(),
            TextStyle::Italic => "*".to_string(),
            TextStyle::Code => "`".to_string(),
            TextStyle::Strikethrough => "~~".to_string(),
            TextStyle::Link { url } => format!("]({})", url),
            TextStyle::Image { url } => format!("]({})", url),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleSet {
    /// Styles in order from outermost to innermost
    pub styles: Vec<TextStyle>,
}

impl StyleSet {
    pub fn new() -> Self {
        Self { styles: Vec::new() }
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        if !self.styles.contains(&style) {
            self.styles.push(style);
        }
        self
    }

    pub fn contains(&self, style: &TextStyle) -> bool {
        self.styles.contains(style)
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct TextChunk {
    pub text: String,
    /// Styles in order from outermost to innermost
    pub styles: StyleSet,
}

impl TextChunk {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            styles: StyleSet::new(),
        }
    }

    pub fn styled(text: impl Into<String>, styles: StyleSet) -> Self {
        Self {
            text: text.into(),
            styles,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RichText {
    pub chunks: Vec<TextChunk>,
}

impl RichText {
    pub fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            chunks: vec![TextChunk::plain(text)],
        }
    }

    /// Total character count across all chunks
    pub fn len(&self) -> usize {
        self.chunks.iter().map(|c| c.text.len()).sum()
    }

    /// Get the styles at a specific character offset
    /// Returns the styles of the chunk containing that offset, or empty if at end
    pub fn styles_at(&self, offset: usize) -> StyleSet {
        let mut pos = 0;
        for chunk in &self.chunks {
            let chunk_end = pos + chunk.text.len();
            if offset < chunk_end {
                return chunk.styles.clone();
            }
            pos = chunk_end;
        }
        // At end of text - return empty styles
        StyleSet::new()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty() || self.chunks.iter().all(|c| c.text.is_empty())
    }

    /// Push text with given styles, merging with last chunk if styles match
    pub fn push(&mut self, text: &str, styles: StyleSet) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.chunks.last_mut()
            && last.styles == styles
        {
            last.text.push_str(text);
            return;
        }
        self.chunks.push(TextChunk::styled(text, styles));
    }

    /// Split at character offset, returning (before, after)
    pub fn split_at(&self, offset: usize) -> (RichText, RichText) {
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut pos = 0;

        for chunk in &self.chunks {
            let chunk_end = pos + chunk.text.len();

            if chunk_end <= offset {
                before.push(chunk.clone());
            } else if pos >= offset {
                after.push(chunk.clone());
            } else {
                let split_point = offset - pos;
                before.push(TextChunk {
                    text: chunk.text[..split_point].to_string(),
                    styles: chunk.styles.clone(),
                });
                after.push(TextChunk {
                    text: chunk.text[split_point..].to_string(),
                    styles: chunk.styles.clone(),
                });
            }

            pos = chunk_end;
        }

        (RichText { chunks: before }, RichText { chunks: after })
    }

    /// Append another RichText, merging adjacent chunks with same styles
    pub fn append(&mut self, other: RichText) {
        for chunk in other.chunks {
            if let Some(last) = self.chunks.last_mut()
                && last.styles == chunk.styles
            {
                last.text.push_str(&chunk.text);
                continue;
            }
            self.chunks.push(chunk);
        }
    }

    /// Insert text at the given offset, inheriting styles from surrounding text
    pub fn insert_at(&mut self, offset: usize, new_text: &str) {
        if new_text.is_empty() {
            return;
        }

        if self.chunks.is_empty() {
            self.chunks.push(TextChunk::plain(new_text));
            return;
        }

        let mut pos = 0;
        for chunk in &mut self.chunks {
            let chunk_end = pos + chunk.text.len();
            if offset >= pos && offset <= chunk_end {
                // Insert within this chunk
                let insert_pos = offset - pos;
                chunk.text.insert_str(insert_pos, new_text);
                return;
            }
            pos = chunk_end;
        }

        // Offset is at end - append to last chunk
        if let Some(last) = self.chunks.last_mut() {
            last.text.push_str(new_text);
        }
    }

    /// Delete characters in range [start, end)
    pub fn delete_range(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }

        let (before, _) = self.split_at(start);
        let (_, after) = self.split_at(end);

        self.chunks = before.chunks;
        self.append(after);
    }

    /// Insert text at the given offset with specific styles
    pub fn insert_styled_at(&mut self, offset: usize, new_text: &str, styles: StyleSet) {
        if new_text.is_empty() {
            return;
        }

        if self.chunks.is_empty() {
            self.chunks.push(TextChunk::styled(new_text, styles));
            return;
        }

        // Find the chunk containing this offset
        let mut pos = 0;
        for i in 0..self.chunks.len() {
            let chunk_end = pos + self.chunks[i].text.len();

            if offset >= pos && offset <= chunk_end {
                let insert_pos = offset - pos;

                // If styles match, just insert into this chunk
                if self.chunks[i].styles == styles {
                    self.chunks[i].text.insert_str(insert_pos, new_text);
                    return;
                }

                // Need to split the chunk and insert new styled chunk
                if insert_pos == 0 {
                    // Insert before this chunk
                    self.chunks.insert(i, TextChunk::styled(new_text, styles));
                    self.merge_adjacent_chunks();
                    return;
                } else if insert_pos == self.chunks[i].text.len() {
                    // Insert after this chunk
                    self.chunks
                        .insert(i + 1, TextChunk::styled(new_text, styles));
                    self.merge_adjacent_chunks();
                    return;
                } else {
                    // Split chunk: [before][new][after]
                    let after_text = self.chunks[i].text.split_off(insert_pos);
                    let after_styles = self.chunks[i].styles.clone();

                    // Insert new chunk and remainder
                    self.chunks
                        .insert(i + 1, TextChunk::styled(new_text, styles));
                    self.chunks
                        .insert(i + 2, TextChunk::styled(after_text, after_styles));
                    self.merge_adjacent_chunks();
                    return;
                }
            }
            pos = chunk_end;
        }

        // Offset at end - append new chunk
        self.chunks.push(TextChunk::styled(new_text, styles));
        self.merge_adjacent_chunks();
    }

    /// Merge adjacent chunks with identical styles
    fn merge_adjacent_chunks(&mut self) {
        let mut i = 0;
        while i + 1 < self.chunks.len() {
            if self.chunks[i].styles == self.chunks[i + 1].styles {
                let next_text = self.chunks.remove(i + 1).text;
                self.chunks[i].text.push_str(&next_text);
            } else {
                i += 1;
            }
        }
        // Remove empty chunks
        self.chunks.retain(|c| !c.text.is_empty());
    }

    /// Apply a style to a range of text [start, end)
    /// This is used for retroactively styling text (e.g., when completing a link)
    pub fn apply_style_to_range(&mut self, start: usize, end: usize, style: TextStyle) {
        if start >= end {
            return;
        }

        // We need to find all chunks that overlap with [start, end) and add the style to them
        // This may require splitting chunks at the boundaries

        let mut new_chunks = Vec::new();
        let mut pos = 0;

        for chunk in &self.chunks {
            let chunk_start = pos;
            let chunk_end = pos + chunk.text.len();
            pos = chunk_end;

            if chunk_end <= start || chunk_start >= end {
                // Chunk is entirely outside the range - keep as is
                new_chunks.push(chunk.clone());
            } else if chunk_start >= start && chunk_end <= end {
                // Chunk is entirely inside the range - add style
                let mut new_styles = chunk.styles.clone();
                new_styles.styles.push(style.clone());
                new_chunks.push(TextChunk {
                    text: chunk.text.clone(),
                    styles: new_styles,
                });
            } else {
                // Chunk partially overlaps - need to split
                let text = &chunk.text;

                // Part before the range
                if chunk_start < start {
                    let before_len = start - chunk_start;
                    new_chunks.push(TextChunk {
                        text: text[..before_len].to_string(),
                        styles: chunk.styles.clone(),
                    });
                }

                // Part inside the range
                let inside_start = start.saturating_sub(chunk_start);
                let inside_end = (end - chunk_start).min(text.len());
                if inside_start < inside_end {
                    let mut new_styles = chunk.styles.clone();
                    new_styles.styles.push(style.clone());
                    new_chunks.push(TextChunk {
                        text: text[inside_start..inside_end].to_string(),
                        styles: new_styles,
                    });
                }

                // Part after the range
                if chunk_end > end {
                    let after_start = end - chunk_start;
                    new_chunks.push(TextChunk {
                        text: text[after_start..].to_string(),
                        styles: chunk.styles.clone(),
                    });
                }
            }
        }

        self.chunks = new_chunks;
        self.merge_adjacent_chunks();
    }

    /// Debug representation showing styled chunks
    /// Format: "<i>hello</i><b>world</b>"
    pub fn to_debug_string(&self) -> String {
        let mut result = String::new();
        for chunk in &self.chunks {
            if chunk.styles.is_empty() {
                result.push_str(&chunk.text);
            } else {
                // Open tags
                for style in &chunk.styles.styles {
                    result.push_str(match style {
                        TextStyle::Bold => "<b>",
                        TextStyle::Italic => "<i>",
                        TextStyle::Code => "<code>",
                        TextStyle::Strikethrough => "<s>",
                        TextStyle::Link { .. } => "<a>",
                        TextStyle::Image { .. } => "<img>",
                    });
                }
                result.push_str(&chunk.text);
                // Close tags in reverse order
                for style in chunk.styles.styles.iter().rev() {
                    result.push_str(match style {
                        TextStyle::Bold => "</b>",
                        TextStyle::Italic => "</i>",
                        TextStyle::Code => "</code>",
                        TextStyle::Strikethrough => "</s>",
                        TextStyle::Link { .. } => "</a>",
                        TextStyle::Image { .. } => "</img>",
                    });
                }
            }
        }
        result
    }

    pub fn to_markdown(&self) -> String {
        let mut result = String::new();
        let mut open_styles: Vec<TextStyle> = Vec::new();

        for chunk in &self.chunks {
            let chunk_styles = &chunk.styles.styles;

            // Find which styles need to be closed (in reverse order of opening)
            // A style needs closing if it's open but not in the current chunk
            let mut styles_to_close = Vec::new();
            for style in open_styles.iter().rev() {
                if !chunk_styles.contains(style) {
                    styles_to_close.push(style.clone());
                }
            }

            // Close styles (they're already in reverse order)
            for style in &styles_to_close {
                result.push_str(&style.close_marker());
                open_styles.retain(|s| s != style);
            }

            // Open new styles (in the order they appear in chunk_styles)
            for style in chunk_styles {
                if !open_styles.contains(style) {
                    result.push_str(style.open_marker());
                    open_styles.push(style.clone());
                }
            }

            // Write the text
            result.push_str(&chunk.text);
        }

        // Close any remaining open styles (in reverse order)
        for style in open_styles.iter().rev() {
            result.push_str(&style.close_marker());
        }

        result
    }

    pub fn to_highlights(&self, theme: &Theme) -> Vec<(Range<usize>, HighlightStyle)> {
        let mut highlights = Vec::new();
        let mut byte_offset = 0;

        for chunk in &self.chunks {
            let chunk_len = chunk.text.len();
            let range = byte_offset..(byte_offset + chunk_len);

            if !chunk.styles.is_empty() {
                let mut style = HighlightStyle::default();

                for text_style in &chunk.styles.styles {
                    match text_style {
                        TextStyle::Bold => {
                            style.font_weight = Some(FontWeight::BOLD);
                        }
                        TextStyle::Italic => {
                            style.font_style = Some(FontStyle::Italic);
                        }
                        TextStyle::Code => {
                            // TODO: Use monospace font
                            style.background_color = Some(theme.selection.into());
                        }
                        TextStyle::Strikethrough => {
                            style.strikethrough = Some(gpui::StrikethroughStyle {
                                thickness: px(1.0),
                                color: Some(theme.foreground.into()),
                            });
                        }
                        TextStyle::Link { .. } => {
                            style.color = Some(theme.cyan.into());
                            // Underline is added dynamically on hover in editor/block.rs
                        }
                        TextStyle::Image { .. } => {
                            // Style images similar to links
                            style.color = Some(theme.cyan.into());
                        }
                    }
                }

                highlights.push((range, style));
            }

            byte_offset += chunk_len;
        }

        highlights
    }

    /// Remove link/image styles from chunk containing the given offset.
    /// Returns true if a link/image style was removed.
    pub fn remove_link_style_at(&mut self, offset: usize) -> bool {
        let mut pos = 0;
        for chunk in &mut self.chunks {
            let chunk_end = pos + chunk.text.len();
            if offset < chunk_end {
                // Found the chunk - remove any link/image styles
                let had_link = chunk
                    .styles
                    .styles
                    .iter()
                    .any(|s| matches!(s, TextStyle::Link { .. } | TextStyle::Image { .. }));
                if had_link {
                    chunk
                        .styles
                        .styles
                        .retain(|s| !matches!(s, TextStyle::Link { .. } | TextStyle::Image { .. }));
                    return true;
                }
                return false;
            }
            pos = chunk_end;
        }
        false
    }

    /// Get clickable link ranges with their URLs.
    /// Returns (byte_range, url) for each link/image in the text.
    pub fn clickable_links(&self) -> Vec<(Range<usize>, String)> {
        let mut links = Vec::new();
        let mut byte_offset = 0;

        for chunk in &self.chunks {
            let chunk_len = chunk.text.len();
            let range = byte_offset..(byte_offset + chunk_len);

            for style in &chunk.styles.styles {
                match style {
                    TextStyle::Link { url } | TextStyle::Image { url } => {
                        links.push((range.clone(), url.clone()));
                        break; // Only one link per chunk
                    }
                    _ => {}
                }
            }

            byte_offset += chunk_len;
        }

        links
    }
}
