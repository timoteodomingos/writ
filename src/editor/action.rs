#[derive(Clone, Debug)]
pub enum EditorAction {
    Type(char),
    Enter,
    ShiftEnter,
    Backspace,
    Move(Direction),
}

#[derive(Clone, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
