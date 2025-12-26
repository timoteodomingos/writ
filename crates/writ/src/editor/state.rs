use slotmap::DefaultKey;

use crate::document::{Block, BlockKind, Document, RichText, StyleSet, TextStyle};

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

/// Represents a pending style marker that hasn't been committed yet
#[derive(Debug, Clone, PartialEq)]
pub enum PendingMarker {
    /// Single `*` - could become italic or upgrade to bold/bold-italic
    SingleAsterisk,
    /// Double `**` - bold mode, or upgrade to bold-italic
    DoubleAsterisk,
    /// Triple `***` - bold+italic mode
    TripleAsterisk,
    /// Single backtick - code mode
    Backtick,
    /// Single `~` - could upgrade to strikethrough
    SingleTilde,
    /// Double `~~` - strikethrough mode
    DoubleTilde,
}

impl PendingMarker {
    /// Get the display text for this pending marker
    pub fn as_str(&self) -> &'static str {
        match self {
            PendingMarker::SingleAsterisk => "*",
            PendingMarker::DoubleAsterisk => "**",
            PendingMarker::TripleAsterisk => "***",
            PendingMarker::Backtick => "`",
            PendingMarker::SingleTilde => "~",
            PendingMarker::DoubleTilde => "~~",
        }
    }

    /// Convert a marker character to a pending marker
    fn from_char(c: char) -> Option<PendingMarker> {
        match c {
            '*' => Some(PendingMarker::SingleAsterisk),
            '`' => Some(PendingMarker::Backtick),
            '~' => Some(PendingMarker::SingleTilde),
            _ => None,
        }
    }

    /// Try to upgrade this marker (e.g., * -> ** -> ***)
    fn try_upgrade(&self, c: char) -> Option<PendingMarker> {
        match (self, c) {
            (PendingMarker::SingleAsterisk, '*') => Some(PendingMarker::DoubleAsterisk),
            (PendingMarker::DoubleAsterisk, '*') => Some(PendingMarker::TripleAsterisk),
            (PendingMarker::SingleTilde, '~') => Some(PendingMarker::DoubleTilde),
            _ => None,
        }
    }

    /// Downgrade marker by one character (for backspace)
    fn downgrade(&self) -> Option<PendingMarker> {
        match self {
            PendingMarker::TripleAsterisk => Some(PendingMarker::DoubleAsterisk),
            PendingMarker::DoubleAsterisk => Some(PendingMarker::SingleAsterisk),
            PendingMarker::DoubleTilde => Some(PendingMarker::SingleTilde),
            _ => None,
        }
    }

    /// Convert to open styles (if this is a valid style marker)
    /// Returns None for invalid markers, or Some with a list of styles to open
    fn to_open_styles(&self) -> Option<Vec<OpenStyle>> {
        match self {
            PendingMarker::SingleAsterisk => Some(vec![OpenStyle {
                style: TextStyle::Italic,
                marker: "*".to_string(),
            }]),
            PendingMarker::DoubleAsterisk => Some(vec![OpenStyle {
                style: TextStyle::Bold,
                marker: "**".to_string(),
            }]),
            PendingMarker::TripleAsterisk => Some(vec![
                OpenStyle {
                    style: TextStyle::Bold,
                    marker: "**".to_string(),
                },
                OpenStyle {
                    style: TextStyle::Italic,
                    marker: "*".to_string(),
                },
            ]),
            PendingMarker::Backtick => Some(vec![OpenStyle {
                style: TextStyle::Code,
                marker: "`".to_string(),
            }]),
            PendingMarker::DoubleTilde => Some(vec![OpenStyle {
                style: TextStyle::Strikethrough,
                marker: "~~".to_string(),
            }]),
            PendingMarker::SingleTilde => None, // Single ~ is not a valid style
        }
    }
}

/// Tracks an open style with its marker for matching on close
#[derive(Debug, Clone, PartialEq)]
pub struct OpenStyle {
    pub style: TextStyle,
    pub marker: String,
}

/// Inline styling state for the editor
#[derive(Debug, Clone, Default)]
pub struct InlineStyleState {
    /// Pending marker characters that haven't been committed to a chunk yet
    pub pending_marker: Option<PendingMarker>,
    /// Stack of currently open styles (outermost first)
    pub open_styles: Vec<OpenStyle>,
}

/// Represents a pending block marker (e.g., `#` for headings)
#[derive(Debug, Clone, PartialEq)]
pub enum PendingBlockMarker {
    /// Heading marker with level (1-6) and whether space has been typed
    Heading { level: usize, has_space: bool },
}

impl PendingBlockMarker {
    /// Get the display text for this pending block marker
    pub fn as_str(&self) -> String {
        match self {
            PendingBlockMarker::Heading { level, has_space } => {
                let hashes = "#".repeat(*level);
                if *has_space {
                    format!("{} ", hashes)
                } else {
                    hashes
                }
            }
        }
    }

    /// Try to upgrade this marker (e.g., # -> ## -> ###)
    fn try_upgrade(&self, c: char) -> Option<PendingBlockMarker> {
        match (self, c) {
            (PendingBlockMarker::Heading { level, has_space }, '#')
                if !*has_space && *level < 6 =>
            {
                Some(PendingBlockMarker::Heading {
                    level: level + 1,
                    has_space: false,
                })
            }
            (PendingBlockMarker::Heading { level, has_space }, ' ') if !*has_space => {
                Some(PendingBlockMarker::Heading {
                    level: *level,
                    has_space: true,
                })
            }
            _ => None,
        }
    }

    /// Downgrade marker by one step (for backspace)
    fn downgrade(&self) -> Option<PendingBlockMarker> {
        match self {
            PendingBlockMarker::Heading { level, has_space } => {
                if *has_space {
                    // Remove space first
                    Some(PendingBlockMarker::Heading {
                        level: *level,
                        has_space: false,
                    })
                } else if *level > 1 {
                    // Downgrade level
                    Some(PendingBlockMarker::Heading {
                        level: level - 1,
                        has_space: false,
                    })
                } else {
                    // Level 1 with no space - remove entirely
                    None
                }
            }
        }
    }
}

/// Block styling state for the editor
#[derive(Debug, Clone, Default)]
pub struct BlockStyleState {
    /// Pending block marker that hasn't been committed yet
    pub pending_marker: Option<PendingBlockMarker>,
}

/// All editing operations as commands
#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    InsertText(String),
    Backspace,
    Delete,
    Enter,
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
    /// Inline styling state (pending markers, open styles)
    pub inline_style: InlineStyleState,
    /// Block styling state (pending block markers like #)
    pub block_style: BlockStyleState,
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
            inline_style: InlineStyleState::default(),
            block_style: BlockStyleState::default(),
        }
    }

    /// Get the pending inline marker text for display
    pub fn pending_marker_text(&self) -> &str {
        self.inline_style
            .pending_marker
            .as_ref()
            .map(|m| m.as_str())
            .unwrap_or("")
    }

    /// Get the pending block marker text for display
    pub fn pending_block_marker_text(&self) -> Option<String> {
        self.block_style.pending_marker.as_ref().map(|m| m.as_str())
    }

    /// Get current active styles from the open_styles stack
    fn current_styles(&self) -> StyleSet {
        StyleSet {
            styles: self
                .inline_style
                .open_styles
                .iter()
                .map(|os| os.style.clone())
                .collect(),
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
            EditorAction::Enter => self.enter(),
            EditorAction::SetCursor { block_key, offset } => self.set_cursor(block_key, offset),
        }
    }

    fn set_cursor(&mut self, block_key: DefaultKey, offset: usize) {
        // Clear style state on cursor set (click)
        self.inline_style = InlineStyleState::default();
        self.block_style = BlockStyleState::default();

        if let Some(block) = self.document.blocks.get(block_key) {
            let max_offset = block.text.len();
            self.cursor.block_key = block_key;
            self.cursor.offset = offset.min(max_offset);
        }
    }

    /// Debug representation showing cursor position and pending markers
    /// Format: "Hello *[|]" for cursor with pending italic marker
    pub fn to_debug_string(&self) -> String {
        let mut result = String::new();

        for (idx, block_key) in self.document.block_order.values().enumerate() {
            if idx > 0 {
                result.push('\n');
            }

            let block = &self.document.blocks[*block_key];
            let text = self.block_plain_text(block);

            if *block_key == self.cursor.block_key {
                // Insert cursor marker at offset, with pending marker before cursor
                let offset = self.cursor.offset.min(text.len());
                let (before, after) = text.split_at(offset);
                result.push_str(before);
                result.push_str(self.pending_marker_text());
                result.push_str("[|]");
                result.push_str(after);
            } else {
                result.push_str(&text);
            }
        }

        result
    }

    /// Debug representation showing styled chunks
    /// Format: "<i>hello</i><b>world</b>"
    pub fn to_styled_debug_string(&self) -> String {
        let block = &self.document.blocks[self.cursor.block_key];
        block.text.to_debug_string()
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
        // Clear style state on cursor movement
        self.inline_style = InlineStyleState::default();
        self.block_style = BlockStyleState::default();

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
        for c in text.chars() {
            self.insert_char(c);
        }
    }

    /// Check if a character is a potential inline style marker
    fn is_marker_char(c: char) -> bool {
        matches!(c, '*' | '`' | '~')
    }

    /// Check if we're at a position where block markers are valid
    /// (cursor at offset 0 with no text in the block, or we already have a pending block marker)
    fn can_use_block_marker(&self) -> bool {
        self.block_style.pending_marker.is_some()
            || (self.cursor.offset == 0 && self.current_block_len() == 0)
    }

    fn insert_char(&mut self, c: char) {
        // Check for block marker handling first
        if self.can_use_block_marker() {
            if let Some(ref pending) = self.block_style.pending_marker {
                // Try to upgrade existing block marker
                if let Some(upgraded) = pending.try_upgrade(c) {
                    self.block_style.pending_marker = Some(upgraded);
                    return;
                }
                // Block marker with space and text input - convert to heading
                let PendingBlockMarker::Heading { level, has_space } = pending;
                if *has_space {
                    // Convert block to heading and insert the character
                    let block = &mut self.document.blocks[self.cursor.block_key];
                    block.kind = BlockKind::Heading {
                        level: *level,
                        id: None,
                    };
                    self.block_style.pending_marker = None;
                    // Now insert the character as regular text
                    self.insert_char_as_text(c);
                    return;
                }
            } else if c == '#' {
                // Start a new heading marker
                self.block_style.pending_marker = Some(PendingBlockMarker::Heading {
                    level: 1,
                    has_space: false,
                });
                return;
            }
        }

        // Regular character handling (inline styles or plain text)
        if Self::is_marker_char(c) {
            self.handle_marker_char(c);
        } else {
            self.handle_regular_char(c);
        }
    }

    /// Insert a character as plain text with current styles
    fn insert_char_as_text(&mut self, c: char) {
        let styles = self.current_styles();
        let block = &mut self.document.blocks[self.cursor.block_key];
        block
            .text
            .insert_styled_at(self.cursor.offset, &c.to_string(), styles);
        self.cursor.offset += c.len_utf8();
    }

    fn handle_marker_char(&mut self, c: char) {
        // Check if we can upgrade the pending marker
        if let Some(ref pending) = self.inline_style.pending_marker {
            if let Some(upgraded) = pending.try_upgrade(c) {
                self.inline_style.pending_marker = Some(upgraded);
                return;
            }

            // Can't upgrade - the pending marker is complete, need to resolve it
            // before starting a new pending marker
            self.resolve_pending_marker();
        }

        // Start a new pending marker
        self.inline_style.pending_marker = PendingMarker::from_char(c);
    }

    /// Resolve a pending marker - either close matching open styles or open new styles
    fn resolve_pending_marker(&mut self) {
        if let Some(pending) = self.inline_style.pending_marker.take() {
            // For TripleAsterisk, we need to close both ** and * (or open both)
            // For others, just close/open the single marker
            if matches!(pending, PendingMarker::TripleAsterisk) {
                // Try to close both ** and *
                let closed_bold = self.try_close_style("**");
                let closed_italic = self.try_close_style("*");

                if closed_bold || closed_italic {
                    // We closed at least one - if we didn't close the other, open it
                    if !closed_bold {
                        self.inline_style.open_styles.push(OpenStyle {
                            style: TextStyle::Bold,
                            marker: "**".to_string(),
                        });
                    }
                    if !closed_italic {
                        self.inline_style.open_styles.push(OpenStyle {
                            style: TextStyle::Italic,
                            marker: "*".to_string(),
                        });
                    }
                    return;
                }

                // Nothing to close - open both styles
                if let Some(open_styles) = pending.to_open_styles() {
                    for style in open_styles {
                        self.inline_style.open_styles.push(style);
                    }
                }
                return;
            }

            let marker = pending.as_str();

            // Check if this closes an open style
            if self.try_close_style(marker) {
                return;
            }

            // Doesn't close anything - open new styles (or insert literal for invalid markers)
            if let Some(open_styles) = pending.to_open_styles() {
                for style in open_styles {
                    self.inline_style.open_styles.push(style);
                }
            } else {
                // Invalid pending marker (e.g., single ~) - insert as literal text
                let block = &mut self.document.blocks[self.cursor.block_key];
                block.text.insert_at(self.cursor.offset, marker);
                self.cursor.offset += marker.len();
            }
        }
    }

    /// Try to close an open style with the given marker
    fn try_close_style(&mut self, marker: &str) -> bool {
        // Find the most recently opened style with matching marker
        for i in (0..self.inline_style.open_styles.len()).rev() {
            if self.inline_style.open_styles[i].marker == marker {
                self.inline_style.open_styles.remove(i);
                return true;
            }
        }
        false
    }

    fn handle_regular_char(&mut self, c: char) {
        // Resolve pending marker (close or open style)
        self.resolve_pending_marker();

        // Insert character with current styles
        let styles = self.current_styles();
        let block = &mut self.document.blocks[self.cursor.block_key];
        block
            .text
            .insert_styled_at(self.cursor.offset, &c.to_string(), styles);
        self.cursor.offset += c.len_utf8();
    }

    fn backspace(&mut self) {
        // First, consume pending block marker character by character
        if let Some(pending) = &self.block_style.pending_marker {
            if let Some(downgraded) = pending.downgrade() {
                self.block_style.pending_marker = Some(downgraded);
            } else {
                self.block_style.pending_marker = None;
            }
            return;
        }

        // Next, consume pending inline marker character by character
        if let Some(pending) = &self.inline_style.pending_marker {
            if let Some(downgraded) = pending.downgrade() {
                self.inline_style.pending_marker = Some(downgraded);
            } else {
                self.inline_style.pending_marker = None;
            }
            return;
        }

        // If no pending marker but there are open styles, pop the most recent one
        if !self.inline_style.open_styles.is_empty() && self.cursor.offset == 0 {
            self.inline_style.open_styles.pop();
            return;
        }

        // At offset 0 with heading block, convert to paragraph
        if self.cursor.offset == 0 {
            let block = &mut self.document.blocks[self.cursor.block_key];
            if let BlockKind::Heading { .. } = block.kind {
                block.kind = BlockKind::Paragraph { parent: None };
                return;
            }
        }

        // Then delete actual text
        if self.cursor.offset > 0 {
            let block = &mut self.document.blocks[self.cursor.block_key];
            block
                .text
                .delete_range(self.cursor.offset - 1, self.cursor.offset);
            self.cursor.offset -= 1;
        } else {
            // At start of block - merge with previous block
            if let Some(prev_key) = self.previous_block_key() {
                // Get the length of the previous block (cursor will go here)
                let prev_len = self.document.blocks[prev_key].text.len();

                // Get the current block's text
                let current_text = self.document.blocks[self.cursor.block_key].text.clone();

                // Append current block's text to previous block
                self.document.blocks[prev_key].text.append(current_text);

                // Remove current block
                let current_key = self.cursor.block_key;
                self.document.remove_block(current_key);

                // Move cursor to end of previous block's original text
                self.cursor.block_key = prev_key;
                self.cursor.offset = prev_len;
            }
        }
    }

    fn delete(&mut self) {
        let block_len = self.current_block_len();
        if self.cursor.offset < block_len {
            // Delete character after cursor
            let block = &mut self.document.blocks[self.cursor.block_key];
            block
                .text
                .delete_range(self.cursor.offset, self.cursor.offset + 1);
        } else {
            // At end of block - merge with next block
            if let Some(next_key) = self.next_block_key() {
                // Get the next block's text
                let next_text = self.document.blocks[next_key].text.clone();

                // Append next block's text to current block
                self.document.blocks[self.cursor.block_key]
                    .text
                    .append(next_text);

                // Remove next block
                self.document.remove_block(next_key);

                // Cursor stays in same position
            }
        }
    }

    fn enter(&mut self) {
        // Clear any pending markers and open styles
        self.inline_style = InlineStyleState::default();
        self.block_style = BlockStyleState::default();

        let current_key = self.cursor.block_key;
        let offset = self.cursor.offset;
        let block_len = self.current_block_len();

        if offset == block_len {
            // At end: create new empty paragraph after
            let new_block = Block {
                kind: BlockKind::Paragraph { parent: None },
                text: RichText::new(),
            };
            let new_key = self.document.insert_block_after(current_key, new_block);
            self.cursor.block_key = new_key;
            self.cursor.offset = 0;
        } else {
            // In middle: split block
            let block = &mut self.document.blocks[current_key];
            let (before, after) = block.text.split_at(offset);

            // Keep first part in current block
            block.text = before;

            // Create new paragraph with second part
            let new_block = Block {
                kind: BlockKind::Paragraph { parent: None },
                text: after,
            };
            let new_key = self.document.insert_block_after(current_key, new_block);
            self.cursor.block_key = new_key;
            self.cursor.offset = 0;
        }
    }
}
