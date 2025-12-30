/// An editor action that can be executed programmatically.
#[derive(Clone, Debug)]
pub enum EditorAction {
    /// Insert a character at the cursor.
    Type(char),
    /// Insert a newline.
    Enter,
    /// Smart enter - continues list items, blockquotes, etc.
    ShiftEnter,
    /// Delete the character before the cursor.
    Backspace,
    /// Move cursor in a direction.
    Move(Direction),
}

/// Cursor movement direction.
#[derive(Clone, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
