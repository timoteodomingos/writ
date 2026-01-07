use ropey::Rope;
use std::ops::{Deref, DerefMut, Range};
use std::rc::Rc;
use std::str::FromStr;
use tree_sitter::{InputEdit, Point};
use undo::Record;

use crate::highlight::{HighlightSpan, Highlighter};
use crate::inline::{StyledRegion, extract_all_inline_styles, styles_in_range};
use crate::marker::{LineMarkers, MarkerKind, collect_nodes, markers_at};
use crate::parser::{MarkdownParser, MarkdownTree};

/// A snapshot of buffer data for rendering. All fields use Rc for O(1) cloning.
#[derive(Clone)]
pub struct RenderSnapshot {
    pub lines: Rc<Vec<LineMarkers>>,
    pub rope: Rope,
    pub inline_styles: Rc<Vec<StyledRegion>>,
    pub code_highlights: Rc<Vec<(Range<usize>, Vec<HighlightSpan>)>>,
}

impl RenderSnapshot {
    /// Get inline styles for a specific line. O(log n) binary search.
    pub fn inline_styles_for_line(&self, line_idx: usize) -> Vec<StyledRegion> {
        let line = &self.lines[line_idx];
        styles_in_range(&self.inline_styles, &line.range)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get code highlights for a specific line. O(code_blocks) scan.
    pub fn code_highlights_for_line(&self, line_idx: usize) -> Vec<HighlightSpan> {
        let line = &self.lines[line_idx];
        let range = &line.range;
        let mut result = Vec::new();
        for (block_range, highlights) in self.code_highlights.iter() {
            if range.start < block_range.end && range.end > block_range.start {
                for span in highlights {
                    if span.range.start < range.end && span.range.end > range.start {
                        result.push(span.clone());
                    }
                }
            }
        }
        result
    }
}

#[derive(Clone, Debug)]
struct CodeHighlightCache {
    highlights: Rc<Vec<(Range<usize>, Vec<HighlightSpan>)>>,
    valid: bool,
}

impl Default for CodeHighlightCache {
    fn default() -> Self {
        Self {
            highlights: Rc::new(Vec::new()),
            valid: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextEdit {
    offset: usize,
    deleted: String,
    inserted: String,
    cursor_before: usize,
    cursor_after: usize,
}

impl TextEdit {
    pub fn new(
        offset: usize,
        deleted: String,
        inserted: String,
        cursor_before: usize,
        cursor_after: usize,
    ) -> Self {
        Self {
            offset,
            deleted,
            inserted,
            cursor_before,
            cursor_after,
        }
    }
}

impl undo::Edit for TextEdit {
    type Target = BufferContent;
    type Output = usize; // Returns cursor position

    fn edit(&mut self, target: &mut BufferContent) -> Self::Output {
        target.apply_edit(self.offset, &self.deleted, &self.inserted);
        self.cursor_after
    }

    fn undo(&mut self, target: &mut BufferContent) -> Self::Output {
        target.apply_edit(self.offset, &self.inserted, &self.deleted);
        self.cursor_before
    }
}

pub struct BufferContent {
    text: Rope,
    parser: MarkdownParser,
    tree: Option<MarkdownTree>,
    highlighter: Highlighter,
    code_highlight_cache: CodeHighlightCache,
    /// Cached line info, updated when tree changes.
    /// Wrapped in Rc for O(1) cloning in render snapshots.
    lines: Rc<Vec<LineMarkers>>,
    /// Cached inline styles, updated when tree changes.
    /// Sorted by start position for efficient binary search lookup.
    /// Wrapped in Rc for O(1) cloning in render snapshots.
    inline_styles: Rc<Vec<StyledRegion>>,
}

impl BufferContent {
    pub fn new() -> Self {
        Self {
            text: Rope::new(),
            parser: MarkdownParser::default(),
            highlighter: Highlighter::new(),
            tree: None,
            code_highlight_cache: CodeHighlightCache::default(),
            lines: Rc::new(Vec::new()),
            inline_styles: Rc::new(Vec::new()),
        }
    }

    /// Recompute cached lines and inline styles from current tree.
    fn update_lines_cache(&mut self) {
        let nodes = self
            .tree
            .as_ref()
            .map(|t| collect_nodes(&t.block_tree().root_node()));

        let mut byte_offset = 0;
        self.lines = Rc::new(
            self.text
                .lines()
                .enumerate()
                .map(|(line_idx, line_slice)| {
                    let start_byte = byte_offset;
                    let len = line_slice.len_bytes();
                    byte_offset += len;
                    // Rope's lines() includes trailing newline; exclude it from range
                    let has_newline = line_slice.get_byte(len.saturating_sub(1)) == Some(b'\n');
                    let end_byte = if has_newline {
                        start_byte + len - 1
                    } else {
                        start_byte + len
                    };

                    let markers = if let Some(ref nodes) = nodes {
                        markers_at(nodes, &self.text, start_byte, end_byte)
                    } else {
                        Vec::new()
                    };

                    LineMarkers {
                        range: start_byte..end_byte,
                        line_number: line_idx,
                        markers,
                    }
                })
                .collect(),
        );

        // Update inline styles cache
        self.inline_styles = Rc::new(if let Some(ref tree) = self.tree {
            extract_all_inline_styles(tree, &self.text)
        } else {
            Vec::new()
        });
    }

    fn apply_edit(&mut self, offset: usize, to_delete: &str, to_insert: &str) {
        let delete_len = to_delete.len();
        let insert_len = to_insert.len();

        // Build the edit description for tree-sitter before modifying the rope
        let start_point = self.byte_to_point(offset);
        let old_end_point = if delete_len > 0 {
            self.byte_to_point(offset + delete_len)
        } else {
            start_point
        };

        let new_end_position = if insert_len > 0 {
            self.compute_new_end_point(start_point, to_insert)
        } else {
            start_point
        };

        let edit = InputEdit {
            start_byte: offset,
            old_end_byte: offset + delete_len,
            new_end_byte: offset + insert_len,
            start_position: start_point,
            old_end_position: old_end_point,
            new_end_position,
        };

        // Modify rope
        if delete_len > 0 {
            let char_start = self.text.byte_to_char(offset);
            let char_end = self.text.byte_to_char(offset + delete_len);
            self.text.remove(char_start..char_end);
        }
        if insert_len > 0 {
            let char_offset = self.text.byte_to_char(offset);
            self.text.insert(char_offset, to_insert);
        }

        // Incremental reparse
        if let Some(ref mut tree) = self.tree {
            tree.edit(&edit);
        }
        self.tree = self.parser.parse_rope(&self.text, self.tree.as_ref());

        // Normalize ordered list numbering (may re-parse)
        self.normalize_ordered_lists();

        // Update cached lines
        self.update_lines_cache();

        // Invalidate code highlight cache
        self.code_highlight_cache.valid = false;
    }

    /// Normalize ordered list numbering - ensure sequential numbers (1, 2, 3...).
    /// Modifies the rope directly and re-parses if changes were made.
    fn normalize_ordered_lists(&mut self) -> bool {
        let Some(tree) = &self.tree else {
            return false;
        };

        let corrections = self.find_ordered_list_corrections(tree.block_tree().root_node());

        if corrections.is_empty() {
            return false;
        }

        // Apply corrections in reverse order to preserve byte offsets
        for (marker_range, correct_number, is_parenthesis) in corrections.into_iter().rev() {
            let new_marker = if is_parenthesis {
                format!("{}) ", correct_number)
            } else {
                format!("{}. ", correct_number)
            };

            let char_start = self.text.byte_to_char(marker_range.start);
            let char_end = self.text.byte_to_char(marker_range.end);
            self.text.remove(char_start..char_end);

            let char_offset = self.text.byte_to_char(marker_range.start);
            self.text.insert(char_offset, &new_marker);
        }

        self.tree = self.parser.parse_rope(&self.text, None);
        true
    }

    /// Returns corrections as (range, correct_number, is_parenthesis_style)
    fn find_ordered_list_corrections(
        &self,
        root: tree_sitter::Node,
    ) -> Vec<(Range<usize>, usize, bool)> {
        let mut corrections = Vec::new();
        self.collect_list_corrections(&root, &mut corrections);
        corrections
    }

    fn collect_list_corrections(
        &self,
        node: &tree_sitter::Node,
        corrections: &mut Vec<(Range<usize>, usize, bool)>,
    ) {
        if node.kind() == "list" {
            let is_ordered = self.is_ordered_list(node);

            if is_ordered {
                let mut item_number = 1;
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32)
                        && child.kind() == "list_item"
                        && let Some((marker_range, current_number, is_parenthesis)) =
                            self.extract_ordered_marker(&child)
                    {
                        if current_number != item_number {
                            corrections.push((marker_range, item_number, is_parenthesis));
                        }
                        item_number += 1;
                    }
                }
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                self.collect_list_corrections(&child, corrections);
            }
        }
    }

    fn is_ordered_list(&self, list_node: &tree_sitter::Node) -> bool {
        for i in 0..list_node.child_count() {
            if let Some(child) = list_node.child(i as u32)
                && child.kind() == "list_item"
            {
                for j in 0..child.child_count() {
                    if let Some(marker) = child.child(j as u32) {
                        return marker.kind().starts_with("list_marker_decimal")
                            || marker.kind() == "list_marker_dot"
                            || marker.kind() == "list_marker_parenthesis";
                    }
                }
            }
        }
        false
    }

    /// Extract ordered list marker info: (range, current_number, is_parenthesis_style)
    fn extract_ordered_marker(
        &self,
        list_item: &tree_sitter::Node,
    ) -> Option<(Range<usize>, usize, bool)> {
        for i in 0..list_item.child_count() {
            if let Some(marker) = list_item.child(i as u32)
                && (marker.kind().starts_with("list_marker_decimal")
                    || marker.kind() == "list_marker_dot"
                    || marker.kind() == "list_marker_parenthesis")
            {
                let start = marker.start_byte();
                let end = marker.end_byte();
                let is_parenthesis = marker.kind() == "list_marker_parenthesis";

                // Extract digits from the marker using rope slice
                let char_start = self.text.byte_to_char(start);
                let char_end = self.text.byte_to_char(end);
                let slice = self.text.slice(char_start..char_end);

                let number: usize = slice
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(1);

                return Some((start..end, number, is_parenthesis));
            }
        }
        None
    }

    fn byte_to_point(&self, byte_offset: usize) -> Point {
        let byte_offset = byte_offset.min(self.text.len_bytes());
        let char_offset = self.text.byte_to_char(byte_offset);
        let line = self.text.char_to_line(char_offset);
        let line_start_char = self.text.line_to_char(line);
        let line_start_byte = self.text.char_to_byte(line_start_char);
        let column = byte_offset - line_start_byte;
        Point::new(line, column)
    }

    fn compute_new_end_point(&self, start: Point, text: &str) -> Point {
        let newlines: Vec<_> = text.match_indices('\n').collect();
        if newlines.is_empty() {
            Point::new(start.row, start.column + text.len())
        } else {
            let last_newline_pos = newlines.last().unwrap().0;
            let column = text.len() - last_newline_pos - 1;
            Point::new(start.row + newlines.len(), column)
        }
    }

    pub fn text(&self) -> String {
        self.text.to_string()
    }

    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.text.len_bytes() == 0
    }

    /// Get a single byte at the given offset, if it exists.
    pub fn byte_at(&self, offset: usize) -> Option<u8> {
        if offset >= self.text.len_bytes() {
            return None;
        }
        self.text
            .byte_slice(offset..offset + 1)
            .as_str()
            .and_then(|s| s.bytes().next())
    }

    pub fn rope(&self) -> &Rope {
        &self.text
    }

    pub fn tree(&self) -> Option<&MarkdownTree> {
        self.tree.as_ref()
    }

    pub fn lines(&self) -> &[LineMarkers] {
        &self.lines
    }

    /// Create a render snapshot for efficient virtualized rendering.
    /// Ensures code highlight cache is valid before creating the snapshot.
    /// All Rc clones are O(1).
    pub fn render_snapshot(&mut self) -> RenderSnapshot {
        // Ensure code highlight cache is valid
        if !self.code_highlight_cache.valid {
            self.rebuild_code_highlight_cache();
        }

        RenderSnapshot {
            lines: Rc::clone(&self.lines),
            rope: self.text.clone(),
            inline_styles: Rc::clone(&self.inline_styles),
            code_highlights: Rc::clone(&self.code_highlight_cache.highlights),
        }
    }

    /// Get inline styles that overlap with a byte range.
    /// Uses binary search for efficient O(log n) lookup.
    pub fn inline_styles_for_range(&self, range: &Range<usize>) -> Vec<StyledRegion> {
        styles_in_range(&self.inline_styles, range)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn byte_to_line(&self, byte_offset: usize) -> usize {
        let char_offset = self.text.byte_to_char(byte_offset);
        self.text.char_to_line(char_offset)
    }

    pub fn line_to_byte(&self, line: usize) -> usize {
        let char_offset = self.text.line_to_char(line);
        self.text.char_to_byte(char_offset)
    }

    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    /// Returns true if the line has no content (only markers or whitespace).
    /// Code fences are always considered content.
    pub fn is_line_empty(&self, line_idx: usize) -> bool {
        if line_idx >= self.lines.len() {
            return true;
        }
        let line = &self.lines[line_idx];

        // Code fences are always content
        if line.is_fence() {
            return false;
        }

        self.slice_cow(line.content_start()..line.range.end)
            .trim()
            .is_empty()
    }

    #[cfg(test)]
    pub fn line(&self, line_idx: usize) -> String {
        let line = self.text.line(line_idx);
        // Avoid double allocation - trim in place
        let mut s = line.to_string();
        if s.ends_with('\n') {
            s.pop();
        }
        s
    }

    pub fn line_byte_range(&self, line_idx: usize) -> Range<usize> {
        let start_char = self.text.line_to_char(line_idx);
        let end_char = if line_idx + 1 < self.text.len_lines() {
            self.text.line_to_char(line_idx + 1)
        } else {
            self.text.len_chars()
        };
        let start_byte = self.text.char_to_byte(start_char);
        let end_byte = self.text.char_to_byte(end_char);
        start_byte..end_byte
    }

    fn slice(&self, range: Range<usize>) -> String {
        let char_start = self.text.byte_to_char(range.start);
        let char_end = self.text.byte_to_char(range.end);
        self.text.slice(char_start..char_end).to_string()
    }

    /// Get a byte slice from the rope, borrowing if possible.
    /// Returns a Cow that borrows if the slice fits in one chunk, allocates otherwise.
    pub fn slice_cow(&self, range: Range<usize>) -> std::borrow::Cow<'_, str> {
        let len = self.text.len_bytes();
        // Clamp range to valid bounds
        let start = range.start.min(len);
        let end = range.end.min(len);
        if start >= end {
            return std::borrow::Cow::Borrowed("");
        }
        let char_start = self.text.byte_to_char(start);
        let char_end = self.text.byte_to_char(end);
        let slice = self.text.slice(char_start..char_end);
        match slice.as_str() {
            Some(s) => std::borrow::Cow::Borrowed(s),
            None => std::borrow::Cow::Owned(slice.to_string()),
        }
    }

    pub fn code_highlights_for_range(&mut self, range: Range<usize>) -> Vec<HighlightSpan> {
        // Rebuild cache if invalid
        if !self.code_highlight_cache.valid {
            self.rebuild_code_highlight_cache();
        }

        // Find highlights that overlap with the range
        let mut result = Vec::new();
        for (block_range, highlights) in self.code_highlight_cache.highlights.iter() {
            if range.start < block_range.end && range.end > block_range.start {
                for span in highlights {
                    if span.range.start < range.end && span.range.end > range.start {
                        result.push(span.clone());
                    }
                }
            }
        }
        result
    }

    fn rebuild_code_highlight_cache(&mut self) {
        let highlights = Rc::make_mut(&mut self.code_highlight_cache.highlights);
        highlights.clear();

        let lines = &self.lines;

        let mut i = 0;
        while i < lines.len() {
            // Check if this is a fence line with a language
            let fence_lang = lines[i].markers.iter().find_map(|m| {
                if let MarkerKind::CodeBlockFence { language, .. } = &m.kind {
                    language.clone()
                } else {
                    None
                }
            });

            if let Some(lang) = fence_lang {
                let block_start = lines[i].range.start;
                i += 1;

                // Collect code content
                let mut code_content = String::new();
                let mut content_start_offset: Option<usize> = None;
                let mut block_end = block_start;

                while i < lines.len() {
                    if lines[i].is_fence() {
                        // Closing fence
                        block_end = lines[i].range.end;
                        i += 1;
                        break;
                    } else {
                        // Code content line (any non-fence line between opening and closing fence)
                        if content_start_offset.is_none() {
                            content_start_offset = Some(lines[i].range.start);
                        }
                        // Use rope slice instead of full buffer string
                        let range = &lines[i].range;
                        let char_start = self.text.byte_to_char(range.start);
                        let char_end = self.text.byte_to_char(range.end);
                        let slice = self.text.slice(char_start..char_end);
                        code_content.push_str(&slice.to_string());
                        code_content.push('\n');
                        block_end = lines[i].range.end;
                        i += 1;
                    }
                }

                // Highlight and store
                if let Some(start_offset) = content_start_offset {
                    let mut spans = self.highlighter.highlight(&code_content, &lang);
                    for span in &mut spans {
                        span.range.start += start_offset;
                        span.range.end += start_offset;
                    }
                    highlights.push((block_start..block_end, spans));
                }
            } else {
                i += 1;
            }
        }

        self.code_highlight_cache.valid = true;
    }

    /// Run all normalization passes on the document.
    /// This ensures the document conforms to the editor's canonical markdown format.
    /// Returns true if any changes were made.
    /// Normalize the document. Currently does nothing - we preserve the file as-is.
    pub fn normalize_document(&mut self) -> bool {
        // Normalization has been removed. We now support both soft-wrap style
        // (single newlines) and hard-wrap style (blank lines between paragraphs).
        // The file is loaded exactly as-is.
        false
    }
}

impl Default for BufferContent {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Buffer {
    content: BufferContent,
    history: Record<TextEdit>,
}

impl Deref for Buffer {
    type Target = BufferContent;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.content
    }
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            content: BufferContent::new(),
            history: Record::new(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.history.is_saved()
    }

    pub fn mark_clean(&mut self) {
        self.history.set_saved();
    }

    pub fn insert(&mut self, byte_offset: usize, text: &str, cursor_before: usize) -> usize {
        let cursor_after = byte_offset + text.len();
        let edit = TextEdit::new(
            byte_offset,
            String::new(),
            text.to_string(),
            cursor_before,
            cursor_after,
        );
        self.history.edit(&mut self.content, edit)
    }

    pub fn delete(&mut self, byte_range: Range<usize>, cursor_before: usize) -> usize {
        let deleted = self.content.slice(byte_range.clone());
        let cursor_after = byte_range.start;
        let edit = TextEdit::new(
            byte_range.start,
            deleted,
            String::new(),
            cursor_before,
            cursor_after,
        );
        self.history.edit(&mut self.content, edit)
    }

    pub fn replace(&mut self, byte_range: Range<usize>, text: &str, cursor_before: usize) -> usize {
        let deleted = self.content.slice(byte_range.clone());
        let cursor_after = byte_range.start + text.len();
        let edit = TextEdit::new(
            byte_range.start,
            deleted,
            text.to_string(),
            cursor_before,
            cursor_after,
        );
        self.history.edit(&mut self.content, edit)
    }

    pub fn undo(&mut self) -> Option<usize> {
        self.history.undo(&mut self.content)
    }

    pub fn redo(&mut self) -> Option<usize> {
        self.history.redo(&mut self.content)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Load a file, normalize its content, and save it back.
    /// Returns the buffer and whether any normalization changes were made.
    /// If the file doesn't exist or can't be read, returns a buffer with empty content.
    /// Load a buffer from a file. Returns the buffer and whether the file was modified.
    /// Currently always returns false for modified since we don't normalize on load.
    pub fn from_file(path: &std::path::Path) -> std::io::Result<(Self, bool)> {
        let content = std::fs::read_to_string(path).unwrap_or_default();

        // Parse the file exactly as-is - no normalization
        let mut buffer: Buffer = content.parse().expect("Buffer parsing is infallible");

        // Mark as clean since we just loaded
        buffer.history.set_saved();

        // Second return value is false - we never modify the file on load
        Ok((buffer, false))
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for Buffer {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let text = Rope::from_str(s);
        let mut parser = MarkdownParser::default();
        let tree = parser.parse_rope(&text, None);

        let mut content = BufferContent {
            text,
            parser,
            highlighter: Highlighter::new(),
            tree,
            code_highlight_cache: CodeHighlightCache::default(),
            lines: Rc::new(Vec::new()),
            inline_styles: Rc::new(Vec::new()),
        };

        content.update_lines_cache();

        let mut history = Record::new();
        history.set_saved();

        Ok(Self { content, history })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buf = Buffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len_bytes(), 0);
    }

    #[test]
    fn test_from_str() {
        let buf: Buffer = "hello world".parse().unwrap();
        assert_eq!(buf.text(), "hello world");
        assert_eq!(buf.len_bytes(), 11);
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_insert() {
        let mut buf: Buffer = "hello world".parse().unwrap();
        buf.insert(5, " beautiful", 5);
        assert_eq!(buf.text(), "hello beautiful world");
    }

    #[test]
    fn test_delete() {
        let mut buf: Buffer = "hello world".parse().unwrap();
        buf.delete(5..11, 11);
        assert_eq!(buf.text(), "hello");
    }

    #[test]
    fn test_replace() {
        let mut buf: Buffer = "hello world".parse().unwrap();
        buf.replace(6..11, "rust", 11);
        assert_eq!(buf.text(), "hello rust");
    }

    #[test]
    fn test_line_operations() {
        let buf: Buffer = "line one\nline two\nline three".parse().unwrap();
        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.line(0), "line one");
        assert_eq!(buf.line(1), "line two");
        assert_eq!(buf.line(2), "line three");
    }

    #[test]
    fn test_byte_to_line() {
        let buf: Buffer = "abc\ndef\nghi".parse().unwrap();
        assert_eq!(buf.byte_to_line(0), 0);
        assert_eq!(buf.byte_to_line(3), 0);
        assert_eq!(buf.byte_to_line(4), 1);
        assert_eq!(buf.byte_to_line(8), 2);
    }

    #[test]
    fn test_tree_exists_after_parse() {
        let buf: Buffer = "# Hello\n\nSome **bold** text.".parse().unwrap();
        assert!(buf.tree().is_some());
    }

    #[test]
    fn test_tree_updated_after_edit() {
        let mut buf: Buffer = "# Hello".parse().unwrap();
        let tree1 = buf.tree().map(|t| t.block_tree().root_node().to_sexp());

        buf.insert(7, " World", 7);
        let tree2 = buf.tree().map(|t| t.block_tree().root_node().to_sexp());

        assert_ne!(tree1, tree2);
    }

    #[test]
    fn test_undo_insert() {
        let mut buf: Buffer = "hello".parse().unwrap();
        buf.insert(5, " world", 5);
        assert_eq!(buf.text(), "hello world");

        let cursor = buf.undo();
        assert_eq!(cursor, Some(5));
        assert_eq!(buf.text(), "hello");
    }

    #[test]
    fn test_redo_insert() {
        let mut buf: Buffer = "hello".parse().unwrap();
        buf.insert(5, " world", 5);
        buf.undo();

        let cursor = buf.redo();
        assert_eq!(cursor, Some(11));
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn test_undo_delete() {
        let mut buf: Buffer = "hello world".parse().unwrap();
        buf.delete(5..11, 11);
        assert_eq!(buf.text(), "hello");

        let cursor = buf.undo();
        assert_eq!(cursor, Some(11));
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn test_dirty_state() {
        let mut buf: Buffer = "hello".parse().unwrap();
        assert!(!buf.is_dirty());

        buf.insert(5, " world", 5);
        assert!(buf.is_dirty());

        buf.mark_clean();
        assert!(!buf.is_dirty());

        buf.insert(11, "!", 11);
        assert!(buf.is_dirty());
    }

    #[test]
    fn test_dirty_after_undo_to_save_point() {
        let mut buf: Buffer = "hello".parse().unwrap();
        buf.insert(5, " world", 5);
        buf.mark_clean();
        assert!(!buf.is_dirty());

        buf.insert(11, "!", 11);
        assert!(buf.is_dirty());

        buf.undo();
        assert!(!buf.is_dirty());
    }

    #[test]
    fn test_dirty_save_point_unreachable() {
        let mut buf: Buffer = "hello".parse().unwrap();
        buf.insert(5, " world", 5);
        buf.mark_clean();

        buf.undo();
        assert!(buf.is_dirty());

        buf.insert(5, " rust", 5);
        assert!(buf.is_dirty());

        buf.undo();
        assert!(buf.is_dirty());
    }

    #[test]
    fn test_multiple_undo_redo() {
        let mut buf: Buffer = "a".parse().unwrap();
        buf.insert(1, "b", 1);
        buf.insert(2, "c", 2);
        buf.insert(3, "d", 3);
        assert_eq!(buf.text(), "abcd");

        buf.undo();
        assert_eq!(buf.text(), "abc");
        buf.undo();
        assert_eq!(buf.text(), "ab");
        buf.undo();
        assert_eq!(buf.text(), "a");

        buf.redo();
        assert_eq!(buf.text(), "ab");
        buf.redo();
        assert_eq!(buf.text(), "abc");
        buf.redo();
        assert_eq!(buf.text(), "abcd");
    }

    #[test]
    fn test_ordered_list_no_normalization() {
        // Numbers are preserved as-is (no renumbering)
        let buf: Buffer = "1. First\n5. Second\n9. Third\n".parse().unwrap();
        assert_eq!(buf.text(), "1. First\n5. Second\n9. Third\n");
    }

    #[test]
    fn test_ordered_list_parenthesis_no_normalization() {
        // Parenthesis style numbers are preserved as-is
        let buf: Buffer = "1) First\n5) Second\n9) Third\n".parse().unwrap();
        assert_eq!(buf.text(), "1) First\n5) Second\n9) Third\n");
    }

    #[test]
    fn test_unordered_list_unchanged() {
        let buf: Buffer = "- First\n- Second\n- Third\n".parse().unwrap();
        assert_eq!(buf.text(), "- First\n- Second\n- Third\n");
    }

    // Line extraction tests (moved from lines.rs)

    use crate::marker::MarkerKind;

    #[test]
    fn test_lines_empty_buffer() {
        let buf: Buffer = "".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].range, 0..0);
        assert!(lines[0].markers.is_empty());
    }

    #[test]
    fn test_lines_single_newline() {
        let buf: Buffer = "\n".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].range, 0..0);
        assert_eq!(lines[1].range, 1..1);
    }

    #[test]
    fn test_lines_paragraph() {
        let buf: Buffer = "Hello\n\nWorld\n".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].range, 0..5);
        assert_eq!(lines[1].range, 6..6); // blank line
        assert_eq!(lines[2].range, 7..12);
    }

    #[test]
    fn test_heading_markers() {
        let buf: Buffer = "# Hello\n".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].heading_level(), Some(1));
        assert_eq!(lines[0].marker_range(), Some(0..2));
    }

    #[test]
    fn test_heading_levels() {
        let buf: Buffer = "# H1\n## H2\n### H3\n".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(lines[0].heading_level(), Some(1));
        assert_eq!(lines[1].heading_level(), Some(2));
        assert_eq!(lines[2].heading_level(), Some(3));
    }

    #[test]
    fn test_unordered_list_markers() {
        let buf: Buffer = "- Item 1\n- Item 2\n".parse().unwrap();
        let lines = buf.lines();

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { ordered: false, .. }))
        );
        assert_eq!(lines[0].marker_range(), Some(0..2));
    }

    #[test]
    fn test_ordered_list_markers() {
        let buf: Buffer = "1. First\n2. Second\n".parse().unwrap();
        let lines = buf.lines();

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { ordered: true, .. }))
        );
    }

    #[test]
    fn test_checkbox_markers() {
        let buf: Buffer = "- [ ] Unchecked\n- [x] Checked\n".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(lines[0].checkbox(), Some(false));
        assert_eq!(lines[1].checkbox(), Some(true));
    }

    #[test]
    fn test_blockquote_markers() {
        let buf: Buffer = "> Quote\n".parse().unwrap();
        let lines = buf.lines();

        assert!(lines[0].has_border());
        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::BlockQuote))
        );
    }

    #[test]
    fn test_is_line_empty_blockquote_only() {
        // A line with just "> " (blockquote marker, no content) should be empty
        let buf: Buffer = "> hey\n> \n> > hey".parse().unwrap();
        assert!(!buf.is_line_empty(0)); // "> hey" has content
        assert!(buf.is_line_empty(1)); // "> " is empty (marker only)
        assert!(!buf.is_line_empty(2)); // "> > hey" has content
    }

    #[test]
    fn test_marker_range_includes_trailing_space() {
        // "> hey" - marker should be "> " (bytes 0..2)
        let buf: Buffer = "> hey".parse().unwrap();
        let lines = buf.lines();
        assert_eq!(lines[0].marker_range(), Some(0..2)); // "> " includes the space

        // "- item" - marker should be "- " (bytes 0..2)
        let buf: Buffer = "- item".parse().unwrap();
        let lines = buf.lines();
        assert_eq!(lines[0].marker_range(), Some(0..2)); // "- " includes the space

        // "> > hey" - marker should be "> > " (bytes 0..4)
        let buf: Buffer = "> > hey".parse().unwrap();
        let lines = buf.lines();
        assert_eq!(lines[0].marker_range(), Some(0..4)); // "> > " includes both spaces
    }

    #[test]
    fn test_nested_blockquote_lines() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let lines = buf.lines();

        assert_eq!(
            lines[0]
                .markers
                .iter()
                .filter(|m| matches!(m.kind, MarkerKind::BlockQuote))
                .count(),
            1
        );
        assert_eq!(
            lines[1]
                .markers
                .iter()
                .filter(|m| matches!(m.kind, MarkerKind::BlockQuote))
                .count(),
            2
        );
    }

    #[test]
    fn test_list_in_blockquote_lines() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let lines = buf.lines();

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::BlockQuote))
        );
        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { .. }))
        );
    }

    #[test]
    fn test_code_block_fence_lines() {
        let buf: Buffer = "```rust\nlet x = 1;\n```\n".parse().unwrap();
        let lines = buf.lines();

        assert!(lines[0].is_fence());
        assert!(!lines[1].is_fence());
        assert!(lines[2].is_fence());
    }

    #[test]
    fn test_thematic_break_lines() {
        let buf: Buffer = "---\n".parse().unwrap();
        let lines = buf.lines();

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ThematicBreak))
        );
    }

    #[test]
    fn test_nested_list_continuation() {
        let buf: Buffer = "- Item 1\n  - Nested\n".parse().unwrap();
        let lines = buf.lines();

        let continuation = lines[1].continuation_rope(buf.rope());
        assert!(continuation.contains("- "));
    }

    #[test]
    fn test_substitution() {
        let buf: Buffer = "- Item\n".parse().unwrap();
        let lines = buf.lines();

        // List markers are now rendered as spacers, not substitution
        let sub = lines[0].substitution_rope(buf.rope());
        assert_eq!(sub, "");
    }

    #[test]
    fn test_list_in_blockquote_continuation() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let lines = buf.lines();

        let continuation = lines[0].continuation_rope(buf.rope());
        assert_eq!(continuation, "> - ");
    }

    #[test]
    fn test_multiline_blockquote_with_list_continuation() {
        let buf: Buffer = "> hey\n>\n> - foo\n".parse().unwrap();
        let lines = buf.lines();

        let continuation = lines[2].continuation_rope(buf.rope());
        assert_eq!(continuation, "> - ");
    }

    #[test]
    fn test_code_block_in_blockquote() {
        let buf: Buffer = "> ```rust\n> fn main() {}\n> ```\n".parse().unwrap();
        let lines = buf.lines();

        assert!(lines[0].is_fence());
        assert!(lines[0].has_border());
    }
}
