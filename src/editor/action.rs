/// An action that can be executed on the editor programmatically.
///
/// Use with [`Editor::execute`](super::Editor::execute) to control the editor
/// from code, such as in scripted demos.
#[derive(Clone, Debug)]
pub enum EditorAction {
    /// Insert a character at the cursor.
    Type(char),
    /// Insert a newline.
    Enter,
    /// Smart enter: continues lists, blockquotes, etc.
    ShiftEnter,
    /// Delete the character before the cursor.
    Backspace,
    /// Move the cursor in a direction.
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
