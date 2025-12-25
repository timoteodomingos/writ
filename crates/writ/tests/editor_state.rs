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

// ============================================================================
// Inline Style Marker Tests
// ============================================================================

#[test]
fn test_italic_marker_pending() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete); // Clear the placeholder
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "*[|]");
}

#[test]
fn test_bold_marker_upgrade() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "*[|]");
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "**[|]");
}

#[test]
fn test_italic_text_insertion() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("hello".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));

    assert_eq!(state.to_styled_debug_string(), "<i>hello</i>");

    // Verify highlights are generated
    let block = &state.document.blocks[state.cursor.block_key];
    let theme = writ::theme::dracula();
    let highlights = block.text.to_highlights(&theme);
    assert!(!highlights.is_empty(), "Highlights should not be empty");
}

#[test]
fn test_bold_text_insertion() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("**".to_string()));
    state.apply(EditorAction::InsertText("bold".to_string()));
    state.apply(EditorAction::InsertText("**".to_string()));
    assert_eq!(state.to_styled_debug_string(), "<b>bold</b>");
}

#[test]
fn test_code_style() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("`".to_string()));
    state.apply(EditorAction::InsertText("code".to_string()));
    state.apply(EditorAction::InsertText("`".to_string()));
    assert_eq!(state.to_styled_debug_string(), "<code>code</code>");
}

#[test]
fn test_strikethrough_style() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("~~".to_string()));
    state.apply(EditorAction::InsertText("deleted".to_string()));
    state.apply(EditorAction::InsertText("~~".to_string()));
    assert_eq!(state.to_styled_debug_string(), "<s>deleted</s>");
}

#[test]
fn test_nested_styles() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // *italic **bold-italic** italic*
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("italic ".to_string()));
    state.apply(EditorAction::InsertText("**".to_string()));
    state.apply(EditorAction::InsertText("bold-italic".to_string()));
    state.apply(EditorAction::InsertText("**".to_string()));
    state.apply(EditorAction::InsertText(" italic".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));

    assert_eq!(
        state.to_styled_debug_string(),
        "<i>italic </i><i><b>bold-italic</b></i><i> italic</i>"
    );
}

#[test]
fn test_triple_asterisk_bold_italic() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // ***bold-italic***
    state.apply(EditorAction::InsertText("***".to_string()));
    assert_eq!(state.to_debug_string(), "***[|]"); // All 3 asterisks visible as pending
    state.apply(EditorAction::InsertText("text".to_string()));
    state.apply(EditorAction::InsertText("***".to_string()));
    // Should be both bold and italic
    assert_eq!(state.to_styled_debug_string(), "<b><i>text</i></b>");
}

#[test]
fn test_triple_asterisk_marker_upgrade() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "*[|]");
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "**[|]");
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "***[|]");
}

#[test]
fn test_backspace_removes_pending_marker() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "Hello*[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "Hello[|]");
}

#[test]
fn test_backspace_double_marker() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("**".to_string()));
    assert_eq!(state.to_debug_string(), "**[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "*[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]");
}

#[test]
fn test_cursor_movement_clears_pending() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "*[|]Hello");
    state.apply(EditorAction::MoveCursor(Direction::Right));
    // Moving should clear pending marker
    assert_eq!(state.to_debug_string(), "H[|]ello");
    assert!(state.inline_style.pending_marker.is_none());
}

#[test]
fn test_cursor_movement_clears_open_styles() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("test".to_string()));
    // Now we have open italic style
    assert!(!state.inline_style.open_styles.is_empty());
    state.apply(EditorAction::MoveCursor(Direction::Right));
    // Moving should clear open styles
    assert!(state.inline_style.open_styles.is_empty());
}

#[test]
fn test_single_tilde_is_literal() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("~".to_string()));
    state.apply(EditorAction::InsertText("hello".to_string()));
    // Single ~ is not a valid style marker, should be inserted as literal
    assert_eq!(state.to_styled_debug_string(), "~hello");
}

#[test]
fn test_mixed_plain_and_styled() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("plain ".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("italic".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText(" plain".to_string()));
    assert_eq!(state.to_styled_debug_string(), "plain <i>italic</i> plain");
}

#[test]
fn test_backspace_clears_open_style_at_start() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Open italic style
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("a".to_string()));
    assert!(!state.inline_style.open_styles.is_empty());
    // Delete the 'a'
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]");
    // Now at start of line with open style - backspace should clear it
    assert!(!state.inline_style.open_styles.is_empty());
    state.apply(EditorAction::Backspace);
    assert!(state.inline_style.open_styles.is_empty());
}

#[test]
fn test_backspace_clears_nested_open_styles_one_at_a_time() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Open bold then italic: **bold *italic
    state.apply(EditorAction::InsertText("**".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("a".to_string()));
    assert_eq!(state.inline_style.open_styles.len(), 2);
    // Delete the 'a'
    state.apply(EditorAction::Backspace);
    // Backspace should pop styles one at a time (most recent first)
    state.apply(EditorAction::Backspace);
    assert_eq!(state.inline_style.open_styles.len(), 1);
    state.apply(EditorAction::Backspace);
    assert_eq!(state.inline_style.open_styles.len(), 0);
}

#[test]
fn test_backspace_triple_asterisk_marker() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("***".to_string()));
    assert_eq!(state.to_debug_string(), "***[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "**[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "*[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]");
}

#[test]
fn test_double_tilde_strikethrough_marker() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("~".to_string()));
    assert_eq!(state.to_debug_string(), "~[|]");
    state.apply(EditorAction::InsertText("~".to_string()));
    assert_eq!(state.to_debug_string(), "~~[|]");
}

#[test]
fn test_backspace_double_tilde_marker() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("~~".to_string()));
    assert_eq!(state.to_debug_string(), "~~[|]");
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "~[|]");
    // Single tilde is invalid, next backspace clears it
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]");
}

#[test]
fn test_marker_resolved_on_different_marker_type() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Type * then ` - the * should resolve (open italic) and ` becomes pending
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("`".to_string()));
    assert_eq!(state.to_debug_string(), "`[|]");
    assert_eq!(state.inline_style.open_styles.len(), 1);
    assert_eq!(state.inline_style.open_styles[0].marker, "*");
}
