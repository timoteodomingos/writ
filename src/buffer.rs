use ropey::Rope;
use std::ops::Range;
use std::str::FromStr;
use tree_sitter::{InputEdit, Point};
use undo::Record;

use crate::parser::{MarkdownParser, MarkdownTree};

/// A text edit operation that can be undone/redone.
#[derive(Clone, Debug)]
pub struct TextEdit {
    /// Byte offset where the edit occurs
    offset: usize,
    /// Text that was deleted (empty for pure insertions)
    deleted: String,
    /// Text that was inserted (empty for pure deletions)
    inserted: String,
    /// Cursor position before the edit
    cursor_before: usize,
    /// Cursor position after the edit
    cursor_after: usize,
}

impl TextEdit {
    /// Create a new text edit.
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
        // Apply: delete `deleted`, insert `inserted`
        target.apply_edit(self.offset, &self.deleted, &self.inserted);
        self.cursor_after
    }

    fn undo(&mut self, target: &mut BufferContent) -> Self::Output {
        // Reverse: delete `inserted`, insert `deleted`
        target.apply_edit(self.offset, &self.inserted, &self.deleted);
        self.cursor_before
    }
}

/// The actual buffer content that TextEdit operates on.
pub struct BufferContent {
    /// The raw text content
    text: Rope,
    /// The markdown parser (reused for incremental parsing)
    parser: MarkdownParser,
    /// The current parse tree (block + inline trees)
    tree: Option<MarkdownTree>,
}

impl BufferContent {
    /// Create a new empty buffer content.
    pub fn new() -> Self {
        Self {
            text: Rope::new(),
            parser: MarkdownParser::default(),
            tree: None,
        }
    }

    /// Apply an edit: delete `to_delete` worth of content at offset, then insert `to_insert`.
    /// This handles both rope modification AND tree-sitter incremental parsing.
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

        // Compute new end position after the edit
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
        let text = self.text.to_string();
        self.tree = self.parser.parse(text.as_bytes(), self.tree.as_ref());
    }

    /// Convert a byte offset to a tree-sitter Point (row, column).
    fn byte_to_point(&self, byte_offset: usize) -> Point {
        let byte_offset = byte_offset.min(self.text.len_bytes());
        let char_offset = self.text.byte_to_char(byte_offset);
        let line = self.text.char_to_line(char_offset);
        let line_start_char = self.text.line_to_char(line);
        let line_start_byte = self.text.char_to_byte(line_start_char);
        let column = byte_offset - line_start_byte;
        Point::new(line, column)
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

    /// Get the text in a byte range.
    fn slice(&self, range: Range<usize>) -> String {
        let char_start = self.text.byte_to_char(range.start);
        let char_end = self.text.byte_to_char(range.end);
        self.text.slice(char_start..char_end).to_string()
    }
}

impl Default for BufferContent {
    fn default() -> Self {
        Self::new()
    }
}

/// A text buffer backed by a rope with incremental tree-sitter parsing and undo/redo.
///
/// The buffer contains raw markdown text. Tree-sitter parses it into
/// block structure (paragraphs, headings, lists) and inline structure
/// (bold, italic, links, etc.) which we use for rendering.
pub struct Buffer {
    /// The actual buffer content
    content: BufferContent,
    /// The undo/redo history record
    history: Record<TextEdit>,
}

impl Buffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        Self {
            content: BufferContent::new(),
            history: Record::new(),
        }
    }

    /// Check if the buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        !self.history.is_saved()
    }

    /// Mark the buffer as clean (after saving).
    pub fn mark_clean(&mut self) {
        self.history.set_saved();
    }

    /// Get the full text as a String.
    pub fn text(&self) -> String {
        self.content.text()
    }

    /// Get the length in bytes.
    pub fn len_bytes(&self) -> usize {
        self.content.len_bytes()
    }

    /// Get the length in characters.
    pub fn len_chars(&self) -> usize {
        self.content.len_chars()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Get a reference to the underlying rope.
    pub fn rope(&self) -> &Rope {
        self.content.rope()
    }

    /// Get the current parse tree.
    pub fn tree(&self) -> Option<&MarkdownTree> {
        self.content.tree()
    }

    /// Get the line number (0-indexed) for a byte offset.
    pub fn byte_to_line(&self, byte_offset: usize) -> usize {
        self.content.byte_to_line(byte_offset)
    }

    /// Get the byte offset for the start of a line.
    pub fn line_to_byte(&self, line: usize) -> usize {
        self.content.line_to_byte(line)
    }

    /// Get the number of lines.
    pub fn line_count(&self) -> usize {
        self.content.line_count()
    }

    /// Get a line's text (without trailing newline).
    pub fn line(&self, line_idx: usize) -> String {
        self.content.line(line_idx)
    }

    /// Get the byte range for a line.
    pub fn line_byte_range(&self, line_idx: usize) -> Range<usize> {
        self.content.line_byte_range(line_idx)
    }

    /// Insert text at a byte offset.
    /// Returns the new cursor position.
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

    /// Delete a range of bytes.
    /// Returns the new cursor position.
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

    /// Replace a range of bytes with new text.
    /// Returns the new cursor position.
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

    /// Undo the last edit.
    /// Returns the cursor position to restore, or None if nothing to undo.
    pub fn undo(&mut self) -> Option<usize> {
        self.history.undo(&mut self.content)
    }

    /// Redo the last undone edit.
    /// Returns the cursor position to restore, or None if nothing to redo.
    pub fn redo(&mut self) -> Option<usize> {
        self.history.redo(&mut self.content)
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
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

        let content = BufferContent { text, parser, tree };
        let mut history = Record::new();
        // Mark initial state as saved
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

        buf.insert(7, " World", 7);
        let tree2 = buf.tree().map(|t| t.block_tree().root_node().to_sexp());

        // Trees should be different after edit
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
        assert_eq!(buf.text(), "hello");

        let cursor = buf.redo();
        assert_eq!(cursor, Some(11)); // cursor_after from insert
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
        buf.mark_clean(); // Saved at "hello world"
        assert!(!buf.is_dirty());

        buf.insert(11, "!", 11);
        assert!(buf.is_dirty());

        // Undo back to save point
        buf.undo();
        assert!(!buf.is_dirty()); // Should be clean again
    }

    #[test]
    fn test_dirty_save_point_unreachable() {
        let mut buf: Buffer = "hello".parse().unwrap();
        buf.insert(5, " world", 5);
        buf.mark_clean(); // Saved at "hello world"

        // Undo past save point
        buf.undo();
        assert!(buf.is_dirty());

        // New edit makes save point unreachable
        buf.insert(5, " rust", 5);
        assert!(buf.is_dirty());

        // Undo doesn't get back to "hello world" save point
        buf.undo();
        assert!(buf.is_dirty()); // Still dirty because save point is gone
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
}
