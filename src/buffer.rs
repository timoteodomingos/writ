use ropey::Rope;
use std::ops::Range;
use std::str::FromStr;
use tree_sitter::{InputEdit, Point};
use undo::Record;

use crate::highlight::{HighlightSpan, Highlighter};
use crate::lines::LineKind;
use crate::parser::{MarkdownParser, MarkdownTree};

#[derive(Clone, Debug, Default)]
struct CodeHighlightCache {
    highlights: Vec<(Range<usize>, Vec<HighlightSpan>)>,
    valid: bool,
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
}

impl BufferContent {
    pub fn new() -> Self {
        Self {
            text: Rope::new(),
            parser: MarkdownParser::default(),
            highlighter: Highlighter::new(),
            tree: None,
            code_highlight_cache: CodeHighlightCache::default(),
        }
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
        let text = self.text.to_string();
        self.tree = self.parser.parse(text.as_bytes(), self.tree.as_ref());

        // Normalize ordered list numbering
        self.normalize_ordered_lists();

        // Invalidate code highlight cache
        self.code_highlight_cache.valid = false;
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

    pub fn rope(&self) -> &Rope {
        &self.text
    }

    pub fn tree(&self) -> Option<&MarkdownTree> {
        self.tree.as_ref()
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

    pub fn line(&self, line_idx: usize) -> String {
        let line = self.text.line(line_idx);
        let s = line.to_string();
        s.trim_end_matches('\n').to_string()
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

    pub fn code_highlights_for_range(&mut self, range: Range<usize>) -> Vec<HighlightSpan> {
        // Rebuild cache if invalid
        if !self.code_highlight_cache.valid {
            self.rebuild_code_highlight_cache();
        }

        // Find highlights that overlap with the range
        let mut result = Vec::new();
        for (block_range, highlights) in &self.code_highlight_cache.highlights {
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
        self.code_highlight_cache.highlights.clear();

        let text = self.text.to_string();
        let lines = crate::lines::extract_lines_from_parts(&text, self.tree.as_ref());

        let mut i = 0;
        while i < lines.len() {
            if let LineKind::CodeBlock {
                language: Some(lang),
                is_fence: true,
            } = &lines[i].kind
            {
                let lang = lang.clone();
                let block_start = lines[i].range.start;
                i += 1;

                // Collect code content
                let mut code_content = String::new();
                let mut content_start_offset: Option<usize> = None;
                let mut block_end = block_start;

                while i < lines.len() {
                    match &lines[i].kind {
                        LineKind::CodeBlock { is_fence: true, .. } => {
                            block_end = lines[i].range.end;
                            i += 1;
                            break;
                        }
                        LineKind::CodeBlock {
                            is_fence: false, ..
                        } => {
                            if content_start_offset.is_none() {
                                content_start_offset = Some(lines[i].range.start);
                            }
                            code_content.push_str(&text[lines[i].range.clone()]);
                            code_content.push('\n');
                            block_end = lines[i].range.end;
                            i += 1;
                        }
                        _ => break,
                    }
                }

                // Highlight and store
                if let Some(start_offset) = content_start_offset {
                    let mut spans = self.highlighter.highlight(&code_content, &lang);
                    for span in &mut spans {
                        span.range.start += start_offset;
                        span.range.end += start_offset;
                    }
                    self.code_highlight_cache
                        .highlights
                        .push((block_start..block_end, spans));
                }
            } else {
                i += 1;
            }
        }

        self.code_highlight_cache.valid = true;
    }

    pub fn normalize_ordered_lists(&mut self) {
        let Some(tree) = &self.tree else { return };

        let corrections = self.find_ordered_list_corrections(tree.block_tree().root_node());

        if corrections.is_empty() {
            return;
        }

        for (marker_range, correct_number) in corrections.into_iter().rev() {
            let new_marker = format!("{}. ", correct_number);

            let char_start = self.text.byte_to_char(marker_range.start);
            let char_end = self.text.byte_to_char(marker_range.end);
            self.text.remove(char_start..char_end);

            let char_offset = self.text.byte_to_char(marker_range.start);
            self.text.insert(char_offset, &new_marker);
        }

        let text = self.text.to_string();
        self.tree = self.parser.parse(text.as_bytes(), None);
    }

    fn find_ordered_list_corrections(&self, root: tree_sitter::Node) -> Vec<(Range<usize>, usize)> {
        let mut corrections = Vec::new();
        self.collect_list_corrections(&root, &mut corrections);
        corrections
    }

    fn collect_list_corrections(
        &self,
        node: &tree_sitter::Node,
        corrections: &mut Vec<(Range<usize>, usize)>,
    ) {
        if node.kind() == "list" {
            let is_ordered = self.is_ordered_list(node);

            if is_ordered {
                let mut item_number = 1;
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i as u32)
                        && child.kind() == "list_item"
                        && let Some((marker_range, current_number)) =
                            self.extract_ordered_marker(&child)
                    {
                        if current_number != item_number {
                            corrections.push((marker_range, item_number));
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
                            || marker.kind() == "list_marker_dot";
                    }
                }
            }
        }
        false
    }

    fn extract_ordered_marker(
        &self,
        list_item: &tree_sitter::Node,
    ) -> Option<(Range<usize>, usize)> {
        for i in 0..list_item.child_count() {
            if let Some(marker) = list_item.child(i as u32)
                && (marker.kind().starts_with("list_marker_decimal")
                    || marker.kind() == "list_marker_dot")
            {
                let start = marker.start_byte();
                let end = marker.end_byte();
                let text = self.text();
                let marker_text = &text[start..end];

                let number: usize = marker_text
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(1);

                return Some((start..end, number));
            }
        }
        None
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

    pub fn text(&self) -> String {
        self.content.text()
    }

    pub fn len_bytes(&self) -> usize {
        self.content.len_bytes()
    }

    pub fn len_chars(&self) -> usize {
        self.content.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn rope(&self) -> &Rope {
        self.content.rope()
    }

    pub fn tree(&self) -> Option<&MarkdownTree> {
        self.content.tree()
    }

    pub fn byte_to_line(&self, byte_offset: usize) -> usize {
        self.content.byte_to_line(byte_offset)
    }

    pub fn line_to_byte(&self, line: usize) -> usize {
        self.content.line_to_byte(line)
    }

    pub fn line_count(&self) -> usize {
        self.content.line_count()
    }

    pub fn line(&self, line_idx: usize) -> String {
        self.content.line(line_idx)
    }

    pub fn line_byte_range(&self, line_idx: usize) -> Range<usize> {
        self.content.line_byte_range(line_idx)
    }

    pub fn code_highlights_for_range(&mut self, range: Range<usize>) -> Vec<HighlightSpan> {
        self.content.code_highlights_for_range(range)
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

        let mut content = BufferContent {
            text,
            parser,
            highlighter: Highlighter::new(),
            tree,
            code_highlight_cache: CodeHighlightCache::default(),
        };

        content.normalize_ordered_lists();

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
    fn test_ordered_list_normalization_on_load() {
        let buf: Buffer = "1. First\n5. Second\n9. Third\n".parse().unwrap();
        assert_eq!(buf.text(), "1. First\n2. Second\n3. Third\n");
    }

    #[test]
    fn test_ordered_list_correct_numbers_unchanged() {
        let buf: Buffer = "1. First\n2. Second\n3. Third\n".parse().unwrap();
        assert_eq!(buf.text(), "1. First\n2. Second\n3. Third\n");
    }

    #[test]
    fn test_unordered_list_unchanged() {
        let buf: Buffer = "- First\n- Second\n- Third\n".parse().unwrap();
        assert_eq!(buf.text(), "- First\n- Second\n- Third\n");
    }
}
