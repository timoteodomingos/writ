use slotmap::DefaultKey;

use crate::document::{Block, Document};

/// Cursor position within the document
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    /// The block the cursor is in
    pub block_key: DefaultKey,
    /// Character offset within the block's text
    pub offset: usize,
}

/// Direction for cursor movement
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// All editing operations as commands
#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    InsertText(String),
    Backspace,
    Delete,
    MoveCursor(Direction),
    /// Set cursor to specific block and offset (e.g., from a click)
    SetCursor {
        block_key: DefaultKey,
        offset: usize,
    },
}

/// Core editor state - independent of GPUI
pub struct EditorState {
    pub document: Document,
    pub cursor: Cursor,
}

impl EditorState {
    /// Create editor state from a Document, placing cursor at start of first block
    pub fn new(document: Document) -> Self {
        let first_block_key = document
            .block_order
            .values()
            .next()
            .copied()
            .expect("Document must have at least one block");

        Self {
            document,
            cursor: Cursor {
                block_key: first_block_key,
                offset: 0,
            },
        }
    }

    /// Create editor state from markdown string
    pub fn from_markdown(markdown: &str) -> Self {
        let document = Document::from_markdown(markdown);
        Self::new(document)
    }

    /// Apply an editing action to the state
    pub fn apply(&mut self, action: EditorAction) {
        match action {
            EditorAction::MoveCursor(direction) => self.move_cursor(direction),
            EditorAction::InsertText(text) => self.insert_text(&text),
            EditorAction::Backspace => self.backspace(),
            EditorAction::Delete => self.delete(),
            EditorAction::SetCursor { block_key, offset } => self.set_cursor(block_key, offset),
        }
    }

    fn set_cursor(&mut self, block_key: DefaultKey, offset: usize) {
        if let Some(block) = self.document.blocks.get(block_key) {
            let max_offset = block.text.len();
            self.cursor.block_key = block_key;
            self.cursor.offset = offset.min(max_offset);
        }
    }

    /// Debug representation showing cursor position
    /// Format: "Hello [|]world" for cursor between "Hello " and "world"
    pub fn to_debug_string(&self) -> String {
        let mut result = String::new();

        for (idx, block_key) in self.document.block_order.values().enumerate() {
            if idx > 0 {
                result.push('\n');
            }

            let block = &self.document.blocks[*block_key];
            let text = self.block_plain_text(block);

            if *block_key == self.cursor.block_key {
                // Insert cursor marker at offset
                let offset = self.cursor.offset.min(text.len());
                let (before, after) = text.split_at(offset);
                result.push_str(before);
                result.push_str("[|]");
                result.push_str(after);
            } else {
                result.push_str(&text);
            }
        }

        result
    }

    /// Extract plain text from a block (ignoring styles)
    fn block_plain_text(&self, block: &Block) -> String {
        block.text.chunks.iter().map(|c| c.text.as_str()).collect()
    }

    fn current_block(&self) -> &Block {
        &self.document.blocks[self.cursor.block_key]
    }

    fn current_block_len(&self) -> usize {
        self.current_block().text.len()
    }

    fn move_cursor(&mut self, direction: Direction) {
        match direction {
            Direction::Left => self.move_left(),
            Direction::Right => self.move_right(),
            Direction::Home => self.move_home(),
            Direction::End => self.move_end(),
            Direction::Up => self.move_up(),
            Direction::Down => self.move_down(),
        }
    }

    fn move_left(&mut self) {
        if self.cursor.offset > 0 {
            self.cursor.offset -= 1;
        } else {
            // Move to end of previous block
            if let Some(prev_key) = self.previous_block_key() {
                self.cursor.block_key = prev_key;
                self.cursor.offset = self.current_block_len();
            }
        }
    }

    fn move_right(&mut self) {
        let block_len = self.current_block_len();
        if self.cursor.offset < block_len {
            self.cursor.offset += 1;
        } else {
            // Move to start of next block
            if let Some(next_key) = self.next_block_key() {
                self.cursor.block_key = next_key;
                self.cursor.offset = 0;
            }
        }
    }

    fn move_home(&mut self) {
        self.cursor.offset = 0;
    }

    fn move_end(&mut self) {
        self.cursor.offset = self.current_block_len();
    }

    fn move_up(&mut self) {
        if let Some(prev_key) = self.previous_block_key() {
            let prev_len = self.document.blocks[prev_key].text.len();
            self.cursor.block_key = prev_key;
            // Try to maintain column, but clamp to block length
            self.cursor.offset = self.cursor.offset.min(prev_len);
        }
    }

    fn move_down(&mut self) {
        if let Some(next_key) = self.next_block_key() {
            let next_len = self.document.blocks[next_key].text.len();
            self.cursor.block_key = next_key;
            self.cursor.offset = self.cursor.offset.min(next_len);
        }
    }

    /// Get the previous block key in document order
    fn previous_block_key(&self) -> Option<DefaultKey> {
        let mut prev = None;
        for key in self.document.block_order.values() {
            if *key == self.cursor.block_key {
                return prev;
            }
            prev = Some(*key);
        }
        None
    }

    /// Get the next block key in document order
    fn next_block_key(&self) -> Option<DefaultKey> {
        let mut found = false;
        for key in self.document.block_order.values() {
            if found {
                return Some(*key);
            }
            if *key == self.cursor.block_key {
                found = true;
            }
        }
        None
    }

    fn insert_text(&mut self, text: &str) {
        let block = &mut self.document.blocks[self.cursor.block_key];
        block.text.insert_at(self.cursor.offset, text);
        self.cursor.offset += text.len();
    }

    fn backspace(&mut self) {
        if self.cursor.offset > 0 {
            // Delete character before cursor
            let block = &mut self.document.blocks[self.cursor.block_key];
            block
                .text
                .delete_range(self.cursor.offset - 1, self.cursor.offset);
            self.cursor.offset -= 1;
        }
        // TODO: At start of block - merge with previous block
    }

    fn delete(&mut self) {
        let block_len = self.current_block_len();
        if self.cursor.offset < block_len {
            // Delete character after cursor
            let block = &mut self.document.blocks[self.cursor.block_key];
            block
                .text
                .delete_range(self.cursor.offset, self.cursor.offset + 1);
        }
        // TODO: At end of block - merge with next block
    }
}
