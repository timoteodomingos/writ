use std::ops::Range;

use crate::buffer::Buffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub offset: usize,
}

impl Cursor {
    pub fn new(offset: usize) -> Self {
        Self { offset }
    }

    pub fn start() -> Self {
        Self { offset: 0 }
    }

    pub fn end(buffer: &Buffer) -> Self {
        Self {
            offset: buffer.len_bytes(),
        }
    }

    pub fn clamp(&self, buffer: &Buffer) -> Self {
        Self {
            offset: self.offset.min(buffer.len_bytes()),
        }
    }

    pub fn move_left(&self, buffer: &Buffer) -> Self {
        if self.offset == 0 {
            return *self;
        }

        // Find the start of the previous character (handle UTF-8)
        let text = buffer.text();
        let mut new_offset = self.offset - 1;
        while new_offset > 0 && !text.is_char_boundary(new_offset) {
            new_offset -= 1;
        }

        Self { offset: new_offset }
    }

    pub fn move_right(&self, buffer: &Buffer) -> Self {
        let len = buffer.len_bytes();
        if self.offset >= len {
            return *self;
        }

        // Find the start of the next character (handle UTF-8)
        let text = buffer.text();
        let mut new_offset = self.offset + 1;
        while new_offset < len && !text.is_char_boundary(new_offset) {
            new_offset += 1;
        }

        Self { offset: new_offset }
    }

    pub fn move_up(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        if current_line == 0 {
            // Already on first line, go to start
            return Self::start();
        }

        // Get column offset within current line
        let line_start = buffer.line_to_byte(current_line);
        let column = self.offset - line_start;

        // Move to previous line, same column (or end of line if shorter)
        let prev_line = current_line - 1;
        let prev_line_start = buffer.line_to_byte(prev_line);
        let prev_line_text = buffer.line(prev_line);
        let prev_line_len = prev_line_text.len();

        let new_column = column.min(prev_line_len);
        Self {
            offset: prev_line_start + new_column,
        }
    }

    pub fn move_down(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        let line_count = buffer.line_count();

        if current_line >= line_count - 1 {
            // Already on last line, go to end
            return Self::end(buffer);
        }

        // Get column offset within current line
        let line_start = buffer.line_to_byte(current_line);
        let column = self.offset - line_start;

        // Move to next line, same column (or end of line if shorter)
        let next_line = current_line + 1;
        let next_line_start = buffer.line_to_byte(next_line);
        let next_line_text = buffer.line(next_line);
        let next_line_len = next_line_text.len();

        let new_column = column.min(next_line_len);
        Self {
            offset: next_line_start + new_column,
        }
    }

    pub fn move_to_line_start(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        Self {
            offset: buffer.line_to_byte(current_line),
        }
    }

    pub fn move_to_line_end(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        let line_start = buffer.line_to_byte(current_line);
        let line_text = buffer.line(current_line);
        Self {
            offset: line_start + line_text.len(),
        }
    }

    pub fn move_to_start(&self) -> Self {
        Self::start()
    }

    pub fn move_to_end(&self, buffer: &Buffer) -> Self {
        Self::end(buffer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn from_cursor(cursor: Cursor) -> Self {
        Self {
            anchor: cursor.offset,
            head: cursor.offset,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    pub fn cursor(&self) -> Cursor {
        Cursor::new(self.head)
    }

    pub fn range(&self) -> Range<usize> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    pub fn extend_to(&self, new_head: usize) -> Self {
        Self {
            anchor: self.anchor,
            head: new_head,
        }
    }

    pub fn collapse(&self) -> Self {
        Self {
            anchor: self.head,
            head: self.head,
        }
    }

    pub fn collapse_to_start(&self) -> Self {
        let start = self.range().start;
        Self {
            anchor: start,
            head: start,
        }
    }

    pub fn collapse_to_end(&self) -> Self {
        let end = self.range().end;
        Self {
            anchor: end,
            head: end,
        }
    }

    pub fn clamp(&self, buffer: &Buffer) -> Self {
        let len = buffer.len_bytes();
        Self {
            anchor: self.anchor.min(len),
            head: self.head.min(len),
        }
    }

    pub fn select_all(buffer: &Buffer) -> Self {
        Self {
            anchor: 0,
            head: buffer.len_bytes(),
        }
    }

    pub fn select_word_at(offset: usize, buffer: &Buffer) -> Self {
        let text = buffer.text();
        let len = text.len();

        if len == 0 || offset >= len {
            return Self::new(offset.min(len), offset.min(len));
        }

        // Helper to check if a character is part of a word
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        // Find the character at offset
        let char_at_offset = text[offset..].chars().next();

        // If we're on a non-word character, just select that character
        if let Some(c) = char_at_offset
            && !is_word_char(c)
        {
            // Find end of this character
            let char_end = offset + c.len_utf8();
            return Self::new(offset, char_end.min(len));
        }

        // Find word start (scan backward)
        let mut start = offset;
        for (i, c) in text[..offset].char_indices().rev() {
            if is_word_char(c) {
                start = i;
            } else {
                break;
            }
        }

        // Find word end (scan forward)
        let mut end = offset;
        for (i, c) in text[offset..].char_indices() {
            if is_word_char(c) {
                end = offset + i + c.len_utf8();
            } else {
                break;
            }
        }

        Self::new(start, end)
    }

    pub fn select_line_at(offset: usize, buffer: &Buffer) -> Self {
        let line = buffer.byte_to_line(offset);
        let line_start = buffer.line_to_byte(line);

        // Find line end (including newline if present)
        let line_count = buffer.line_count();
        let line_end = if line + 1 < line_count {
            buffer.line_to_byte(line + 1)
        } else {
            buffer.len_bytes()
        };

        Self::new(line_start, line_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_move_left_right() {
        let buf: Buffer = "hello".parse().unwrap();
        let cursor = Cursor::new(2);

        let left = cursor.move_left(&buf);
        assert_eq!(left.offset, 1);

        let right = cursor.move_right(&buf);
        assert_eq!(right.offset, 3);
    }

    #[test]
    fn test_cursor_move_left_at_start() {
        let buf: Buffer = "hello".parse().unwrap();
        let cursor = Cursor::start();

        let left = cursor.move_left(&buf);
        assert_eq!(left.offset, 0); // stays at 0
    }

    #[test]
    fn test_cursor_move_right_at_end() {
        let buf: Buffer = "hello".parse().unwrap();
        let cursor = Cursor::end(&buf);

        let right = cursor.move_right(&buf);
        assert_eq!(right.offset, 5); // stays at end
    }

    #[test]
    fn test_cursor_move_up_down() {
        let buf: Buffer = "line one\nline two\nline three".parse().unwrap();
        // Start at "two" (offset 14, column 5 on line 1)
        let cursor = Cursor::new(14);

        let up = cursor.move_up(&buf);
        // Should be at column 5 on line 0 = offset 5
        assert_eq!(up.offset, 5);

        let down = cursor.move_down(&buf);
        // Should be at column 5 on line 2 = offset 9 + 9 + 5 = 23
        assert_eq!(down.offset, 23);
    }

    #[test]
    fn test_cursor_move_up_from_first_line() {
        let buf: Buffer = "hello\nworld".parse().unwrap();
        let cursor = Cursor::new(3); // middle of "hello"

        let up = cursor.move_up(&buf);
        assert_eq!(up.offset, 0); // goes to start
    }

    #[test]
    fn test_cursor_move_down_from_last_line() {
        let buf: Buffer = "hello\nworld".parse().unwrap();
        let cursor = Cursor::new(8); // middle of "world"

        let down = cursor.move_down(&buf);
        assert_eq!(down.offset, 11); // goes to end
    }

    #[test]
    fn test_cursor_line_start_end() {
        let buf: Buffer = "hello\nworld".parse().unwrap();
        let cursor = Cursor::new(8); // middle of "world"

        let start = cursor.move_to_line_start(&buf);
        assert_eq!(start.offset, 6); // start of "world"

        let end = cursor.move_to_line_end(&buf);
        assert_eq!(end.offset, 11); // end of "world"
    }

    #[test]
    fn test_selection_range() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.range(), 5..10);

        // Reversed selection
        let sel_rev = Selection::new(10, 5);
        assert_eq!(sel_rev.range(), 5..10);
    }

    #[test]
    fn test_selection_is_collapsed() {
        let sel = Selection::new(5, 5);
        assert!(sel.is_collapsed());

        let sel2 = Selection::new(5, 10);
        assert!(!sel2.is_collapsed());
    }

    #[test]
    fn test_selection_extend() {
        let sel = Selection::new(5, 10);
        let extended = sel.extend_to(15);
        assert_eq!(extended.anchor, 5);
        assert_eq!(extended.head, 15);
    }

    #[test]
    fn test_selection_collapse() {
        let sel = Selection::new(5, 10);

        let collapsed = sel.collapse();
        assert_eq!(collapsed.anchor, 10);
        assert_eq!(collapsed.head, 10);

        let to_start = sel.collapse_to_start();
        assert_eq!(to_start.anchor, 5);
        assert_eq!(to_start.head, 5);

        let to_end = sel.collapse_to_end();
        assert_eq!(to_end.anchor, 10);
        assert_eq!(to_end.head, 10);
    }

    #[test]
    fn test_selection_select_all() {
        let buf: Buffer = "hello world".parse().unwrap();
        let sel = Selection::select_all(&buf);
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, 11);
    }

    #[test]
    fn test_selection_select_word_at() {
        let buf: Buffer = "hello world test".parse().unwrap();

        // Click in middle of "hello"
        let sel = Selection::select_word_at(2, &buf);
        assert_eq!(sel.range(), 0..5); // "hello"

        // Click in middle of "world"
        let sel = Selection::select_word_at(8, &buf);
        assert_eq!(sel.range(), 6..11); // "world"

        // Click on space (non-word char)
        let sel = Selection::select_word_at(5, &buf);
        assert_eq!(sel.range(), 5..6); // just the space
    }

    #[test]
    fn test_selection_select_line_at() {
        let buf: Buffer = "line one\nline two\nline three".parse().unwrap();

        // Click on first line
        let sel = Selection::select_line_at(3, &buf);
        assert_eq!(sel.range(), 0..9); // "line one\n"

        // Click on second line
        let sel = Selection::select_line_at(12, &buf);
        assert_eq!(sel.range(), 9..18); // "line two\n"

        // Click on last line (no trailing newline)
        let sel = Selection::select_line_at(22, &buf);
        assert_eq!(sel.range(), 18..28); // "line three"
    }
}
