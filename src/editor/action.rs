/// An action that can be executed on the editor programmatically.
///
/// Use with [`Editor::execute`](super::Editor::execute) to control the editor
/// from code, such as in scripted demos.
#[derive(Clone, Debug)]
pub enum EditorAction {
    /// Insert a character at the cursor.
    Type(char),
    /// Insert a raw newline.
    Enter,
    /// Continue container: adds markers from current line.
    ShiftEnter,
    /// Indented continuation: creates nested paragraph.
    ShiftAltEnter,
    /// Tab: cycles forward through nesting states based on context.
    Tab,
    /// Shift-Tab: cycles backward through nesting states.
    ShiftTab,
    /// Delete the character before the cursor (markers are atomic).
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
