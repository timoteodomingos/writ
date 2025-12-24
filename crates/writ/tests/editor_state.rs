use writ::editor::{Direction, EditorAction, EditorState};

#[test]
fn test_cursor_at_start() {
    let state = EditorState::from_markdown("Hello world");
    assert_eq!(state.to_debug_string(), "[|]Hello world");
}

#[test]
fn test_move_right() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::Right));
    assert_eq!(state.to_debug_string(), "H[|]ello");

    state.apply(EditorAction::MoveCursor(Direction::Right));
    assert_eq!(state.to_debug_string(), "He[|]llo");
}

#[test]
fn test_move_left() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "Hello[|]");

    state.apply(EditorAction::MoveCursor(Direction::Left));
    assert_eq!(state.to_debug_string(), "Hell[|]o");
}

#[test]
fn test_move_left_at_start_does_nothing() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::Left));
    assert_eq!(state.to_debug_string(), "[|]Hello");
}

#[test]
fn test_move_right_at_end_does_nothing() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    state.apply(EditorAction::MoveCursor(Direction::Right));
    assert_eq!(state.to_debug_string(), "Hello[|]");
}

#[test]
fn test_home_and_end() {
    let mut state = EditorState::from_markdown("Hello world");
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "Hello world[|]");

    state.apply(EditorAction::MoveCursor(Direction::Home));
    assert_eq!(state.to_debug_string(), "[|]Hello world");
}

#[test]
fn test_insert_text() {
    let mut state = EditorState::from_markdown("Hllo");
    state.apply(EditorAction::MoveCursor(Direction::Right));
    state.apply(EditorAction::InsertText("e".to_string()));
    assert_eq!(state.to_debug_string(), "He[|]llo");
}

#[test]
fn test_insert_text_at_start() {
    let mut state = EditorState::from_markdown("ello");
    state.apply(EditorAction::InsertText("H".to_string()));
    assert_eq!(state.to_debug_string(), "H[|]ello");
}

#[test]
fn test_insert_text_at_end() {
    let mut state = EditorState::from_markdown("Hell");
    state.apply(EditorAction::MoveCursor(Direction::End));
    state.apply(EditorAction::InsertText("o".to_string()));
    assert_eq!(state.to_debug_string(), "Hello[|]");
}

#[test]
fn test_backspace() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::Right));
    state.apply(EditorAction::MoveCursor(Direction::Right));
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "H[|]llo");
}

#[test]
fn test_backspace_at_start_does_nothing() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]Hello");
}

#[test]
fn test_delete() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::Right));
    state.apply(EditorAction::Delete);
    assert_eq!(state.to_debug_string(), "H[|]llo");
}

#[test]
fn test_delete_at_end_does_nothing() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    state.apply(EditorAction::Delete);
    assert_eq!(state.to_debug_string(), "Hello[|]");
}

#[test]
fn test_multiple_blocks() {
    let mut state = EditorState::from_markdown("First\n\nSecond");
    assert_eq!(state.to_debug_string(), "[|]First\nSecond");

    state.apply(EditorAction::MoveCursor(Direction::Down));
    assert_eq!(state.to_debug_string(), "First\n[|]Second");

    state.apply(EditorAction::MoveCursor(Direction::Up));
    assert_eq!(state.to_debug_string(), "[|]First\nSecond");
}

#[test]
fn test_vertical_movement_preserves_column() {
    let mut state = EditorState::from_markdown("Hello\n\nWorld");
    // Move to position 3 in first block
    state.apply(EditorAction::MoveCursor(Direction::Right));
    state.apply(EditorAction::MoveCursor(Direction::Right));
    state.apply(EditorAction::MoveCursor(Direction::Right));
    assert_eq!(state.to_debug_string(), "Hel[|]lo\nWorld");

    // Move down - should maintain column 3
    state.apply(EditorAction::MoveCursor(Direction::Down));
    assert_eq!(state.to_debug_string(), "Hello\nWor[|]ld");

    // Move back up - should still be at column 3
    state.apply(EditorAction::MoveCursor(Direction::Up));
    assert_eq!(state.to_debug_string(), "Hel[|]lo\nWorld");
}

#[test]
fn test_vertical_movement_clamps_to_shorter_line() {
    let mut state = EditorState::from_markdown("Hello World\n\nHi");
    // Move to end of first block
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "Hello World[|]\nHi");

    // Move down - should clamp to end of shorter second block
    state.apply(EditorAction::MoveCursor(Direction::Down));
    assert_eq!(state.to_debug_string(), "Hello World\nHi[|]");
}

#[test]
fn test_move_right_across_blocks() {
    let mut state = EditorState::from_markdown("Hi\n\nBye");
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "Hi[|]\nBye");

    state.apply(EditorAction::MoveCursor(Direction::Right));
    assert_eq!(state.to_debug_string(), "Hi\n[|]Bye");
}

#[test]
fn test_move_left_across_blocks() {
    let mut state = EditorState::from_markdown("Hi\n\nBye");
    state.apply(EditorAction::MoveCursor(Direction::Down));
    assert_eq!(state.to_debug_string(), "Hi\n[|]Bye");

    state.apply(EditorAction::MoveCursor(Direction::Left));
    assert_eq!(state.to_debug_string(), "Hi[|]\nBye");
}
