use slotmap::DefaultKey;
use strum::IntoDiscriminant;

use crate::document::{
    Block, BlockKind, BlockKindDiscriminants, Document, RichText, StyleSet, TextStyle,
};

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
pub struct PendingMarker {
    pub kind: PendingMarkerKind,
    /// True if this marker was preceded by whitespace (opening marker)
    /// False if preceded by non-whitespace (closing marker)
    pub is_opening: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingMarkerKind {
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

impl PendingMarkerKind {
    /// Get the display text for this pending marker kind
    pub fn as_str(&self) -> &'static str {
        match self {
            PendingMarkerKind::SingleAsterisk => "*",
            PendingMarkerKind::DoubleAsterisk => "**",
            PendingMarkerKind::TripleAsterisk => "***",
            PendingMarkerKind::Backtick => "`",
            PendingMarkerKind::SingleTilde => "~",
            PendingMarkerKind::DoubleTilde => "~~",
        }
    }

    /// Convert a marker character to a pending marker kind
    fn from_char(c: char) -> Option<PendingMarkerKind> {
        match c {
            '*' => Some(PendingMarkerKind::SingleAsterisk),
            '`' => Some(PendingMarkerKind::Backtick),
            '~' => Some(PendingMarkerKind::SingleTilde),
            _ => None,
        }
    }

    /// Try to upgrade this marker (e.g., * -> ** -> ***)
    fn try_upgrade(&self, c: char) -> Option<PendingMarkerKind> {
        match (self, c) {
            (PendingMarkerKind::SingleAsterisk, '*') => Some(PendingMarkerKind::DoubleAsterisk),
            (PendingMarkerKind::DoubleAsterisk, '*') => Some(PendingMarkerKind::TripleAsterisk),
            (PendingMarkerKind::SingleTilde, '~') => Some(PendingMarkerKind::DoubleTilde),
            _ => None,
        }
    }

    /// Downgrade marker by one character (for backspace)
    fn downgrade(&self) -> Option<PendingMarkerKind> {
        match self {
            PendingMarkerKind::TripleAsterisk => Some(PendingMarkerKind::DoubleAsterisk),
            PendingMarkerKind::DoubleAsterisk => Some(PendingMarkerKind::SingleAsterisk),
            PendingMarkerKind::DoubleTilde => Some(PendingMarkerKind::SingleTilde),
            _ => None,
        }
    }

    /// Convert to styles to open (if this is a valid style marker)
    /// Returns None for invalid markers, or Some with a list of (style, marker) pairs
    fn to_styles(&self) -> Option<Vec<(TextStyle, &'static str)>> {
        match self {
            PendingMarkerKind::SingleAsterisk => Some(vec![(TextStyle::Italic, "*")]),
            PendingMarkerKind::DoubleAsterisk => Some(vec![(TextStyle::Bold, "**")]),
            PendingMarkerKind::TripleAsterisk => {
                Some(vec![(TextStyle::Bold, "**"), (TextStyle::Italic, "*")])
            }
            PendingMarkerKind::Backtick => Some(vec![(TextStyle::Code, "`")]),
            PendingMarkerKind::DoubleTilde => Some(vec![(TextStyle::Strikethrough, "~~")]),
            PendingMarkerKind::SingleTilde => None, // Single ~ is not a valid style
        }
    }
}

impl PendingMarker {
    /// Get the display text for this pending marker
    pub fn as_str(&self) -> &'static str {
        self.kind.as_str()
    }

    /// Create a new pending marker
    fn new(kind: PendingMarkerKind, is_opening: bool) -> Self {
        Self { kind, is_opening }
    }

    /// Convert a marker character to a pending marker
    fn from_char(c: char, is_opening: bool) -> Option<PendingMarker> {
        PendingMarkerKind::from_char(c).map(|kind| PendingMarker::new(kind, is_opening))
    }

    /// Try to upgrade this marker (e.g., * -> ** -> ***)
    fn try_upgrade(&self, c: char) -> Option<PendingMarker> {
        self.kind
            .try_upgrade(c)
            .map(|kind| PendingMarker::new(kind, self.is_opening))
    }

    /// Downgrade marker by one character (for backspace)
    fn downgrade(&self) -> Option<PendingMarker> {
        self.kind
            .downgrade()
            .map(|kind| PendingMarker::new(kind, self.is_opening))
    }
}

/// Tracks an open style with its marker for matching on close
#[derive(Debug, Clone, PartialEq)]
pub struct OpenStyle {
    pub style: TextStyle,
    /// The marker used to open this style (empty if inherited from existing text)
    pub marker: String,
    /// The cursor offset where this style was opened
    pub opened_at: usize,
}

impl OpenStyle {
    /// Create an explicitly opened style (via marker like `*`, `**`, etc.)
    fn explicit(style: TextStyle, marker: impl Into<String>, opened_at: usize) -> Self {
        Self {
            style,
            marker: marker.into(),
            opened_at,
        }
    }

    /// Create an inherited style (from existing styled text)
    fn inherited(style: TextStyle, opened_at: usize) -> Self {
        Self {
            style,
            marker: String::new(),
            opened_at,
        }
    }

    /// Whether this style was explicitly opened (vs inherited)
    fn is_explicit(&self) -> bool {
        !self.marker.is_empty()
    }
}

/// A stack of open styles with operations for push, close, and pruning
#[derive(Debug, Clone, Default)]
pub struct StyleStack {
    styles: Vec<OpenStyle>,
}

impl StyleStack {
    /// Create an empty style stack
    pub fn new() -> Self {
        Self { styles: Vec::new() }
    }

    /// Push an explicitly opened style onto the stack
    pub fn push_explicit(&mut self, style: TextStyle, marker: impl Into<String>, opened_at: usize) {
        self.styles
            .push(OpenStyle::explicit(style, marker, opened_at));
    }

    /// Push an inherited style onto the stack (if not already present)
    pub fn push_inherited(&mut self, style: TextStyle, opened_at: usize) {
        if !self.styles.iter().any(|os| os.style == style) {
            self.styles.push(OpenStyle::inherited(style, opened_at));
        }
    }

    /// Try to close a style with the given marker (searches from most recent)
    /// Returns true if a matching style was found and closed
    pub fn close_matching(&mut self, marker: &str) -> bool {
        for i in (0..self.styles.len()).rev() {
            if self.styles[i].marker == marker {
                self.styles.remove(i);
                return true;
            }
        }
        false
    }

    /// Try to close a style by its type (for inherited styles without markers)
    /// Returns true if a matching style was found and closed
    pub fn close_by_style(&mut self, style: &TextStyle) -> bool {
        for i in (0..self.styles.len()).rev() {
            if &self.styles[i].style == style {
                self.styles.remove(i);
                return true;
            }
        }
        false
    }

    /// Check if there's an open style with the given marker
    pub fn has_matching(&self, marker: &str) -> bool {
        self.styles.iter().any(|os| os.marker == marker)
    }

    /// Check if there's an open style of the given type (explicit or inherited)
    pub fn has_style(&self, style: &TextStyle) -> bool {
        self.styles.iter().any(|os| &os.style == style)
    }

    /// Remove styles when cursor reaches their opened_at position
    /// Call this before deleting text, with the new cursor offset
    pub fn prune_at_offset(&mut self, new_offset: usize) {
        self.styles.retain(|style| new_offset > style.opened_at);
    }

    /// Sync with text styles at cursor position:
    /// - Inherit any styles from text that aren't already open
    /// - Remove inherited styles that are no longer backed by text
    pub fn sync_with_text(&mut self, text_styles: &StyleSet, cursor_offset: usize) {
        // Add inherited styles from text
        for style in &text_styles.styles {
            self.push_inherited(style.clone(), cursor_offset);
        }

        // Remove inherited styles not backed by text
        self.styles
            .retain(|os| os.is_explicit() || text_styles.styles.contains(&os.style));
    }

    /// Clear only inherited styles (keep explicitly opened ones)
    pub fn clear_inherited(&mut self) {
        self.styles.retain(|os| os.is_explicit());
    }

    /// Clear all styles
    pub fn clear(&mut self) {
        self.styles.clear();
    }

    /// Check if the stack is empty
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Get the number of open styles
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Get the current styles as a StyleSet
    pub fn to_style_set(&self) -> StyleSet {
        StyleSet {
            styles: self.styles.iter().map(|os| os.style.clone()).collect(),
        }
    }

    /// Iterate over open styles
    pub fn iter(&self) -> impl Iterator<Item = &OpenStyle> {
        self.styles.iter()
    }

    /// Get an open style by index
    pub fn get(&self, index: usize) -> Option<&OpenStyle> {
        self.styles.get(index)
    }
}

impl std::ops::Index<usize> for StyleStack {
    type Output = OpenStyle;

    fn index(&self, index: usize) -> &Self::Output {
        &self.styles[index]
    }
}

/// Inline styling state for the editor
#[derive(Debug, Clone, Default)]
pub struct InlineStyleState {
    /// Pending marker characters that haven't been committed to a chunk yet
    pub pending_marker: Option<PendingMarker>,
    /// Stack of currently open styles
    pub open_styles: StyleStack,
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
            .first_block_key()
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

    /// Create editor state from a Document, placing cursor at end of last block
    /// This is useful when opening an existing file for editing
    pub fn new_at_end(document: Document) -> Self {
        let last_block_key = document
            .block_order
            .values()
            .last()
            .copied()
            .expect("Document must have at least one block");
        let last_block_len = document.blocks[last_block_key].text.len();

        Self {
            document,
            cursor: Cursor {
                block_key: last_block_key,
                offset: last_block_len,
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

    /// Get an indicator string for active styles (e.g. "BI" for bold+italic)
    pub fn active_styles_indicator(&self) -> Option<String> {
        if self.inline_style.open_styles.is_empty() {
            return None;
        }

        let mut indicator = String::new();
        for open_style in self.inline_style.open_styles.iter() {
            let ch = match open_style.style {
                TextStyle::Bold => 'B',
                TextStyle::Italic => 'I',
                TextStyle::Code => 'C',
                TextStyle::Strikethrough => 'S',
                TextStyle::Link { .. } => 'L',
            };
            indicator.push(ch);
        }
        Some(indicator)
    }

    /// Get current active styles from the open_styles stack
    fn current_styles(&self) -> StyleSet {
        self.inline_style.open_styles.to_style_set()
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
        // Clear pending markers and block style state
        self.inline_style.pending_marker = None;
        self.block_style = BlockStyleState::default();

        if let Some(block) = self.document.blocks.get(block_key) {
            let max_offset = block.text.len();
            self.cursor.block_key = block_key;
            self.cursor.offset = offset.min(max_offset);

            // Sync styles with text at the new position
            self.sync_styles_with_text();
        }
    }

    /// Sync open styles with the styled text at the current cursor position.
    /// This inherits styles from the character to the left of cursor.
    fn sync_styles_with_text(&mut self) {
        // Clear existing styles first
        self.inline_style.open_styles.clear();

        // If cursor is not at the start, inherit styles from character to the left
        if self.cursor.offset > 0 {
            let block = &self.document.blocks[self.cursor.block_key];
            let styles_at_cursor = block.text.styles_at(self.cursor.offset - 1);
            for style in styles_at_cursor.styles {
                self.inline_style
                    .open_styles
                    .push_inherited(style, self.cursor.offset);
            }
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
            let text = block.plain_text();

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

    fn current_block(&self) -> &Block {
        &self.document.blocks[self.cursor.block_key]
    }

    fn current_block_len(&self) -> usize {
        self.current_block().text.len()
    }

    fn move_cursor(&mut self, direction: Direction) {
        // Clear pending markers and block style state
        self.inline_style.pending_marker = None;
        self.block_style = BlockStyleState::default();

        match direction {
            Direction::Left => self.move_left(),
            Direction::Right => self.move_right(),
            Direction::Home => self.move_home(),
            Direction::End => self.move_end(),
            Direction::Up => self.move_up(),
            Direction::Down => self.move_down(),
        }

        // Sync styles with text at the new position
        self.sync_styles_with_text();
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

    fn previous_block_key(&self) -> Option<DefaultKey> {
        self.document.previous_block_key(self.cursor.block_key)
    }

    fn next_block_key(&self) -> Option<DefaultKey> {
        self.document.next_block_key(self.cursor.block_key)
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

    /// Check if the character immediately before cursor is whitespace (or cursor is at start)
    fn char_before_cursor_is_whitespace(&self) -> bool {
        if self.cursor.offset == 0 {
            return true;
        }
        let block = self.current_block();
        let text = block.plain_text();
        text.chars()
            .nth(self.cursor.offset - 1)
            .map(|c| c.is_whitespace())
            .unwrap_or(true)
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
        let is_opening = self.char_before_cursor_is_whitespace();

        // Check if we can upgrade the pending marker
        if let Some(ref pending) = self.inline_style.pending_marker {
            if let Some(upgraded) = pending.try_upgrade(c) {
                let is_closing = !upgraded.is_opening;
                let matches = self.has_matching_open_style(upgraded.as_str());

                // Keep as pending
                self.inline_style.pending_marker = Some(upgraded);

                // For closing markers, resolve immediately if it matches an open style
                if is_closing && matches {
                    self.resolve_pending_marker();
                }
                return;
            }

            // Can't upgrade - the pending marker is complete, need to resolve it
            // before starting a new pending marker
            self.resolve_pending_marker();
        }

        if is_opening {
            // Opening marker - show as pending until next character
            self.inline_style.pending_marker = PendingMarker::from_char(c, true);
        } else {
            // Closing marker - check if we should resolve immediately or wait for potential upgrade
            if let Some(marker) = PendingMarker::from_char(c, false) {
                let matches_current = self.has_matching_open_style(marker.as_str());
                let could_match_upgraded = marker.kind.try_upgrade(c).is_some()
                    && self.could_match_upgraded_marker(&marker.kind, c);

                if matches_current && !could_match_upgraded {
                    // Matches current and can't upgrade to something better - resolve now
                    self.inline_style.pending_marker = Some(marker);
                    self.resolve_pending_marker();
                } else {
                    // Either no match yet, or could upgrade to match something else
                    // Keep pending to allow upgrading (* -> ** -> ***)
                    self.inline_style.pending_marker = Some(marker);
                }
            }
        }
    }

    /// Check if upgrading the marker could match an open style
    fn could_match_upgraded_marker(&self, kind: &PendingMarkerKind, c: char) -> bool {
        if let Some(upgraded) = kind.try_upgrade(c) {
            if self.has_matching_open_style(upgraded.as_str()) {
                return true;
            }
            // Check one more level of upgrade
            if let Some(double_upgraded) = upgraded.try_upgrade(c)
                && self.has_matching_open_style(double_upgraded.as_str())
            {
                return true;
            }
        }
        false
    }

    /// Check if there's an open style that matches the given marker
    /// This checks both explicit markers AND inherited styles of the same type
    fn has_matching_open_style(&self, marker: &str) -> bool {
        // Check for explicit marker match
        if self.inline_style.open_styles.has_matching(marker) {
            return true;
        }

        // Check for inherited style of the same type
        // Map marker to style type
        let style = match marker {
            "*" => Some(TextStyle::Italic),
            "**" => Some(TextStyle::Bold),
            "`" => Some(TextStyle::Code),
            "~~" => Some(TextStyle::Strikethrough),
            _ => None,
        };

        if let Some(style) = style {
            return self.inline_style.open_styles.has_style(&style);
        }

        false
    }

    /// Resolve a pending marker - either close matching open styles or open new styles
    fn resolve_pending_marker(&mut self) {
        if let Some(pending) = self.inline_style.pending_marker.take() {
            let marker = pending.as_str();

            if pending.is_opening {
                // Opening marker - always open new styles (never close)
                if let Some(styles) = pending.kind.to_styles() {
                    for (style, style_marker) in styles {
                        self.inline_style.open_styles.push_explicit(
                            style,
                            style_marker,
                            self.cursor.offset,
                        );
                    }
                } else {
                    // Invalid pending marker (e.g., single ~) - insert as literal text
                    let block = &mut self.document.blocks[self.cursor.block_key];
                    block.text.insert_at(self.cursor.offset, marker);
                    self.cursor.offset += marker.len();
                }
            } else {
                // Closing marker - try to close matching open style
                // For TripleAsterisk, we need to close both ** and *
                if matches!(pending.kind, PendingMarkerKind::TripleAsterisk) {
                    // Try explicit markers first
                    let mut closed_bold = self.inline_style.open_styles.close_matching("**");
                    let mut closed_italic = self.inline_style.open_styles.close_matching("*");

                    // Fall back to closing by style type (for inherited styles)
                    if !closed_bold {
                        closed_bold = self
                            .inline_style
                            .open_styles
                            .close_by_style(&TextStyle::Bold);
                    }
                    if !closed_italic {
                        closed_italic = self
                            .inline_style
                            .open_styles
                            .close_by_style(&TextStyle::Italic);
                    }

                    if !closed_bold && !closed_italic {
                        // Nothing to close - insert as literal text
                        let block = &mut self.document.blocks[self.cursor.block_key];
                        block.text.insert_at(self.cursor.offset, marker);
                        self.cursor.offset += marker.len();
                    }
                    return;
                }

                // Try to close matching open style by marker
                if self.inline_style.open_styles.close_matching(marker) {
                    return;
                }

                // Fall back to closing by style type (for inherited styles)
                if let Some(styles) = pending.kind.to_styles() {
                    for (style, _) in &styles {
                        if self.inline_style.open_styles.close_by_style(style) {
                            return;
                        }
                    }
                }

                // No matching open style - insert as literal text
                let block = &mut self.document.blocks[self.cursor.block_key];
                block.text.insert_at(self.cursor.offset, marker);
                self.cursor.offset += marker.len();
            }
        }
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

        // Remove styles when cursor reaches their opened_at position
        if self.cursor.offset > 0 {
            self.inline_style
                .open_styles
                .prune_at_offset(self.cursor.offset - 1);
        } else {
            // At offset 0, all open styles should be removed
            self.inline_style.open_styles.clear();
        }

        // At offset 0 with heading block, convert to paragraph
        if self.cursor.offset == 0 {
            let block = &mut self.document.blocks[self.cursor.block_key];
            if block.kind.discriminant() != BlockKindDiscriminants::Paragraph {
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

            // Sync styles with the text we're now in
            if self.cursor.offset > 0 {
                let styles_at_cursor = block.text.styles_at(self.cursor.offset - 1);
                self.inline_style
                    .open_styles
                    .sync_with_text(&styles_at_cursor, self.cursor.offset);
            } else {
                // At offset 0, remove inherited styles but keep explicitly opened ones
                self.inline_style.open_styles.clear_inherited();
            }

            // If we've deleted all text in the block, convert heading to paragraph
            if block.text.is_empty()
                && block.kind.discriminant() != BlockKindDiscriminants::Paragraph
            {
                block.kind = BlockKind::Paragraph { parent: None };
            }
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
