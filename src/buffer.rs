use ropey::Rope;
use std::ops::Range;
use std::str::FromStr;
use tree_sitter::{InputEdit, Point};

use crate::parser::{MarkdownParser, MarkdownTree};

/// A text buffer backed by a rope with incremental tree-sitter parsing.
///
/// The buffer contains raw markdown text. Tree-sitter parses it into
/// block structure (paragraphs, headings, lists) and inline structure
/// (bold, italic, links, etc.) which we use for rendering.
pub struct Buffer {
    /// The raw text content
    text: Rope,
    /// The markdown parser (reused for incremental parsing)
    parser: MarkdownParser,
    /// The current parse tree (block + inline trees)
    tree: Option<MarkdownTree>,
    /// Whether the buffer has unsaved changes
    dirty: bool,
}

impl Buffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        Self {
            text: Rope::new(),
            parser: MarkdownParser::default(),
            tree: None,
            dirty: false,
        }
    }

    /// Check if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the buffer as clean (after saving).
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Get the full text as a String.
    pub fn text(&self) -> String {
        self.text.to_string()
    }

    /// Get the length in bytes.
    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }

    /// Get the length in characters.
    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.text.len_bytes() == 0
    }

    /// Get a reference to the underlying rope.
    pub fn rope(&self) -> &Rope {
        &self.text
    }

    /// Get the current parse tree.
    pub fn tree(&self) -> Option<&MarkdownTree> {
        self.tree.as_ref()
    }

    /// Convert a byte offset to a tree-sitter Point (row, column).
    fn byte_to_point(&self, byte_offset: usize) -> Point {
        let line = self.byte_to_line(byte_offset);
        let line_start_byte = self.line_to_byte(line);
        let column = byte_offset - line_start_byte;
        Point::new(line, column)
    }

    /// Insert text at a byte offset.
    pub fn insert(&mut self, byte_offset: usize, text: &str) {
        // Build the edit description before modifying the rope
        let start_point = self.byte_to_point(byte_offset);
        let edit = InputEdit {
            start_byte: byte_offset,
            old_end_byte: byte_offset,
            new_end_byte: byte_offset + text.len(),
            start_position: start_point,
            old_end_position: start_point,
            new_end_position: self.compute_new_end_point(start_point, text),
        };

        // Convert byte offset to char offset for rope
        let char_offset = self.text.byte_to_char(byte_offset);
        self.text.insert(char_offset, text);
        self.dirty = true;
        self.reparse(edit);
    }

    /// Delete a range of bytes.
    pub fn delete(&mut self, byte_range: Range<usize>) {
        // Build the edit description before modifying the rope
        let start_point = self.byte_to_point(byte_range.start);
        let old_end_point = self.byte_to_point(byte_range.end);
        let edit = InputEdit {
            start_byte: byte_range.start,
            old_end_byte: byte_range.end,
            new_end_byte: byte_range.start,
            start_position: start_point,
            old_end_position: old_end_point,
            new_end_position: start_point,
        };

        // Convert byte offsets to char offsets for rope
        let char_start = self.text.byte_to_char(byte_range.start);
        let char_end = self.text.byte_to_char(byte_range.end);
        self.text.remove(char_start..char_end);
        self.dirty = true;
        self.reparse(edit);
    }

    /// Replace a range of bytes with new text.
    pub fn replace(&mut self, byte_range: Range<usize>, text: &str) {
        // Build the edit description before modifying the rope
        let start_point = self.byte_to_point(byte_range.start);
        let old_end_point = self.byte_to_point(byte_range.end);
        let edit = InputEdit {
            start_byte: byte_range.start,
            old_end_byte: byte_range.end,
            new_end_byte: byte_range.start + text.len(),
            start_position: start_point,
            old_end_position: old_end_point,
            new_end_position: self.compute_new_end_point(start_point, text),
        };

        let char_start = self.text.byte_to_char(byte_range.start);
        let char_end = self.text.byte_to_char(byte_range.end);
        self.text.remove(char_start..char_end);
        self.text.insert(char_start, text);
        self.dirty = true;
        self.reparse(edit);
    }

    /// Compute the end point after inserting text at a given start point.
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

    /// Re-parse the document after an edit using incremental parsing.
    fn reparse(&mut self, edit: InputEdit) {
        // Update the old tree with edit info so tree-sitter can reuse unchanged parts
        if let Some(ref mut tree) = self.tree {
            tree.edit(&edit);
        }
        let text = self.text.to_string();
        self.tree = self.parser.parse(text.as_bytes(), self.tree.as_ref());
    }

    /// Get the line number (0-indexed) for a byte offset.
    pub fn byte_to_line(&self, byte_offset: usize) -> usize {
        let char_offset = self.text.byte_to_char(byte_offset);
        self.text.char_to_line(char_offset)
    }

    /// Get the byte offset for the start of a line.
    pub fn line_to_byte(&self, line: usize) -> usize {
        let char_offset = self.text.line_to_char(line);
        self.text.char_to_byte(char_offset)
    }

    /// Get the number of lines.
    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    /// Get a line's text (without trailing newline).
    pub fn line(&self, line_idx: usize) -> String {
        let line = self.text.line(line_idx);
        let s = line.to_string();
        s.trim_end_matches('\n').to_string()
    }

    /// Get the byte range for a line.
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
        let tree = parser.parse(s.as_bytes(), None);

        Ok(Self {
            text,
            parser,
            tree,
            dirty: false,
        })
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
        buf.insert(5, " beautiful");
        assert_eq!(buf.text(), "hello beautiful world");
    }

    #[test]
    fn test_delete() {
        let mut buf: Buffer = "hello world".parse().unwrap();
        buf.delete(5..11);
        assert_eq!(buf.text(), "hello");
    }

    #[test]
    fn test_replace() {
        let mut buf: Buffer = "hello world".parse().unwrap();
        buf.replace(6..11, "rust");
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
        assert_eq!(buf.byte_to_line(0), 0); // 'a'
        assert_eq!(buf.byte_to_line(3), 0); // '\n' at end of line 0
        assert_eq!(buf.byte_to_line(4), 1); // 'd'
        assert_eq!(buf.byte_to_line(8), 2); // 'g'
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

        buf.insert(7, " World");
        let tree2 = buf.tree().map(|t| t.block_tree().root_node().to_sexp());

        // Trees should be different after edit
        assert_ne!(tree1, tree2);
    }
}
