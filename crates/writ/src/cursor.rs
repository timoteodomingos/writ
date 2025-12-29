use std::ops::Range;

use crate::buffer::Buffer;

/// A cursor position in the buffer, represented as a byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    /// Byte offset in the buffer
    pub offset: usize,
}

impl Cursor {
    /// Create a new cursor at the given byte offset.
    pub fn new(offset: usize) -> Self {
        Self { offset }
    }

    /// Create a cursor at the start of the buffer.
    pub fn start() -> Self {
        Self { offset: 0 }
    }

    /// Create a cursor at the end of the buffer.
    pub fn end(buffer: &Buffer) -> Self {
        Self {
            offset: buffer.len_bytes(),
        }
    }

    /// Clamp the cursor to valid buffer bounds.
    pub fn clamp(&self, buffer: &Buffer) -> Self {
        Self {
            offset: self.offset.min(buffer.len_bytes()),
        }
    }

    /// Move cursor left by one character.
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

    /// Move cursor right by one character.
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

    /// Move cursor up one line, trying to maintain column position.
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

    /// Move cursor down one line, trying to maintain column position.
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

    /// Move cursor to start of current line.
    pub fn move_to_line_start(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        Self {
            offset: buffer.line_to_byte(current_line),
        }
    }

    /// Move cursor to end of current line.
    pub fn move_to_line_end(&self, buffer: &Buffer) -> Self {
        let current_line = buffer.byte_to_line(self.offset);
        let line_start = buffer.line_to_byte(current_line);
        let line_text = buffer.line(current_line);
        Self {
            offset: line_start + line_text.len(),
        }
    }

    /// Move cursor to start of buffer.
    pub fn move_to_start(&self) -> Self {
        Self::start()
    }

    /// Move cursor to end of buffer.
    pub fn move_to_end(&self, buffer: &Buffer) -> Self {
        Self::end(buffer)
    }
}

/// A selection in the buffer, represented as anchor and head positions.
///
/// The anchor is where the selection started, the head is where it ends
/// (i.e., where the cursor visually is). They can be in either order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started (byte offset)
    pub anchor: usize,
    /// Where the selection ends / cursor is (byte offset)
    pub head: usize,
}

impl Selection {
    /// Create a new selection.
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// Create a selection from a cursor (collapsed, no actual selection).
    pub fn from_cursor(cursor: Cursor) -> Self {
        Self {
            anchor: cursor.offset,
            head: cursor.offset,
        }
    }

    /// Check if this is a collapsed selection (cursor with no range).
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    /// Get the cursor position (the head of the selection).
    pub fn cursor(&self) -> Cursor {
        Cursor::new(self.head)
    }

    /// Get the selected range as (start, end) where start <= end.
    pub fn range(&self) -> Range<usize> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    /// Extend the selection by moving the head.
    pub fn extend_to(&self, new_head: usize) -> Self {
        Self {
            anchor: self.anchor,
            head: new_head,
        }
    }

    /// Collapse the selection to the head position.
    pub fn collapse(&self) -> Self {
        Self {
            anchor: self.head,
            head: self.head,
        }
    }

    /// Collapse the selection to the start of the range.
    pub fn collapse_to_start(&self) -> Self {
        let start = self.range().start;
        Self {
            anchor: start,
            head: start,
        }
    }

    /// Collapse the selection to the end of the range.
    pub fn collapse_to_end(&self) -> Self {
        let end = self.range().end;
        Self {
            anchor: end,
            head: end,
        }
    }

    /// Clamp the selection to valid buffer bounds.
    pub fn clamp(&self, buffer: &Buffer) -> Self {
        let len = buffer.len_bytes();
        Self {
            anchor: self.anchor.min(len),
            head: self.head.min(len),
        }
    }

    /// Select all text in the buffer.
    pub fn select_all(buffer: &Buffer) -> Self {
        Self {
            anchor: 0,
            head: buffer.len_bytes(),
        }
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
}
