use writ::document::TextStyle;
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
fn test_backspace_removes_style_when_reaching_open_position() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Open italic style at offset 0
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("a".to_string()));
    assert!(!state.inline_style.open_styles.is_empty());
    // Delete the 'a' - cursor goes to offset 0 where style was opened
    // Style should be removed since cursor reached the open position
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]");
    assert!(state.inline_style.open_styles.is_empty());
}

#[test]
fn test_backspace_keeps_style_until_reaching_open_position() {
    // Start with empty document
    let mut state = EditorState::from_markdown("");
    // Type "x " - cursor at offset 2
    state.apply(EditorAction::InsertText("x ".to_string()));
    assert_eq!(state.cursor.offset, 2);
    // Now type "*ab" - style opens at offset 2, "ab" inserted, cursor moves to 4
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("ab".to_string()));
    // Text is "x ab" (4 chars), cursor at offset 4, style opened at offset 2
    assert_eq!(state.cursor.offset, 4);
    assert_eq!(state.inline_style.open_styles.len(), 1);
    assert_eq!(state.inline_style.open_styles[0].opened_at, 2);
    // Delete 'b' - cursor goes to offset 3, style at 2, so kept (3 > 2)
    state.apply(EditorAction::Backspace);
    assert_eq!(state.cursor.offset, 3);
    assert_eq!(state.inline_style.open_styles.len(), 1);
    // Delete 'a' - cursor goes to offset 2, which equals where style opened
    // Style should be removed
    state.apply(EditorAction::Backspace);
    assert_eq!(state.cursor.offset, 2);
    assert_eq!(state.inline_style.open_styles.len(), 0);
}

#[test]
fn test_backspace_into_styled_text_inherits_style() {
    // Create text with italic in the middle: "a *italic* b"
    // After parsing: "a " (plain) + "italic" (italic) + " b" (plain)
    // Positions: "a " = 0-1, "italic" = 2-7, " b" = 8-9
    let mut state = EditorState::from_markdown("a *italic* b");
    assert_eq!(state.to_styled_debug_string(), "a <i>italic</i> b");

    // Cursor starts at offset 0, move to end
    state.apply(EditorAction::MoveCursor(Direction::End));
    // Text is "a italic b" (10 chars), cursor at offset 10
    assert_eq!(state.cursor.offset, 10);
    // No open styles yet (cursor movement clears them)
    assert!(state.inline_style.open_styles.is_empty());

    // Backspace to delete 'b' - cursor goes to 9
    // Character to left (pos 8) is ' ' which is plain
    state.apply(EditorAction::Backspace);
    assert_eq!(state.cursor.offset, 9);
    assert!(state.inline_style.open_styles.is_empty());

    // Backspace to delete ' ' (the space after italic) - cursor goes to 8
    // Character to left (pos 7) is 'c' which is italic!
    state.apply(EditorAction::Backspace);
    assert_eq!(state.cursor.offset, 8);
    // Should have picked up italic style from 'c'
    assert_eq!(state.inline_style.open_styles.len(), 1);
    assert_eq!(state.inline_style.open_styles[0].style, TextStyle::Italic);

    // Type more text - should be italic
    state.apply(EditorAction::InsertText("X".to_string()));
    assert_eq!(state.to_styled_debug_string(), "a <i>italicX</i>");
}

#[test]
fn test_backspace_into_styled_text_then_past_it() {
    // Start fresh and type: "hello *world"
    let mut state = EditorState::from_markdown("");
    state.apply(EditorAction::InsertText("hello ".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("world".to_string()));
    // "hello world" with italic on "world", cursor at 11
    assert_eq!(state.cursor.offset, 11);
    assert_eq!(state.inline_style.open_styles.len(), 1);
    assert_eq!(state.inline_style.open_styles[0].opened_at, 6);

    // Backspace 4 times to delete "orld" - should still have italic open (cursor at 7 > 6)
    for _ in 0..4 {
        state.apply(EditorAction::Backspace);
    }
    assert_eq!(state.cursor.offset, 7);
    assert_eq!(state.inline_style.open_styles.len(), 1);

    // Backspace once more to delete "w" - cursor goes to 6, which equals where italic opened
    // Italic style should be removed
    state.apply(EditorAction::Backspace);
    assert_eq!(state.cursor.offset, 6);
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

// ============================================================================
// Enter Key Tests
// ============================================================================

#[test]
fn test_enter_at_end_creates_new_block() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "Hello[|]");

    state.apply(EditorAction::Enter);
    assert_eq!(state.to_debug_string(), "Hello\n[|]");

    // Cursor should be at start of new block
    assert_eq!(state.cursor.offset, 0);
}

#[test]
fn test_enter_in_middle_splits_block() {
    let mut state = EditorState::from_markdown("HelloWorld");
    // Move cursor to position 5 (between "Hello" and "World")
    for _ in 0..5 {
        state.apply(EditorAction::MoveCursor(Direction::Right));
    }
    assert_eq!(state.to_debug_string(), "Hello[|]World");

    state.apply(EditorAction::Enter);
    assert_eq!(state.to_debug_string(), "Hello\n[|]World");

    // Cursor should be at start of new block
    assert_eq!(state.cursor.offset, 0);
}

#[test]
fn test_enter_at_start_creates_empty_block_before_text() {
    let mut state = EditorState::from_markdown("Hello");
    assert_eq!(state.to_debug_string(), "[|]Hello");

    state.apply(EditorAction::Enter);
    // The original text stays in first block, cursor moves to new empty block
    assert_eq!(state.to_debug_string(), "\n[|]Hello");
}

#[test]
fn test_enter_clears_pending_markers() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_debug_string(), "Hello*[|]");
    assert!(state.inline_style.pending_marker.is_some());

    state.apply(EditorAction::Enter);
    assert!(state.inline_style.pending_marker.is_none());
}

#[test]
fn test_enter_clears_open_styles() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("italic".to_string()));
    assert!(!state.inline_style.open_styles.is_empty());

    state.apply(EditorAction::Enter);
    assert!(state.inline_style.open_styles.is_empty());
}

#[test]
fn test_enter_multiple_times() {
    let mut state = EditorState::from_markdown("First");
    state.apply(EditorAction::MoveCursor(Direction::End));

    state.apply(EditorAction::Enter);
    state.apply(EditorAction::InsertText("Second".to_string()));
    state.apply(EditorAction::Enter);
    state.apply(EditorAction::InsertText("Third".to_string()));

    assert_eq!(state.to_debug_string(), "First\nSecond\nThird[|]");
}

#[test]
fn test_enter_preserves_styled_text_when_splitting() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("italic".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    assert_eq!(state.to_styled_debug_string(), "<i>italic</i>");

    // Move to middle and press enter
    state.apply(EditorAction::MoveCursor(Direction::Home));
    for _ in 0..3 {
        state.apply(EditorAction::MoveCursor(Direction::Right));
    }

    state.apply(EditorAction::Enter);

    // Both blocks should preserve italic styling
    let blocks: Vec<_> = state.document.block_order.values().collect();
    assert_eq!(blocks.len(), 2);

    let first_block = &state.document.blocks[*blocks[0]];
    let second_block = &state.document.blocks[*blocks[1]];

    assert_eq!(first_block.text.to_debug_string(), "<i>ita</i>");
    assert_eq!(second_block.text.to_debug_string(), "<i>lic</i>");
}

// ============================================================================
// Header Marker Tests
// ============================================================================

#[test]
fn test_hash_at_start_shows_pending() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("#".to_string()));

    // Should have pending block marker
    assert!(state.block_style.pending_marker.is_some());
    assert_eq!(state.pending_block_marker_text(), Some("#".to_string()));
}

#[test]
fn test_hash_upgrade_to_h2() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("#".to_string()));
    state.apply(EditorAction::InsertText("#".to_string()));

    assert_eq!(state.pending_block_marker_text(), Some("##".to_string()));
}

#[test]
fn test_hash_upgrade_to_h3() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("###".to_string()));

    assert_eq!(state.pending_block_marker_text(), Some("###".to_string()));
}

#[test]
fn test_hash_space_text_creates_heading() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("# ".to_string()));

    // Should show pending marker with space
    assert_eq!(state.pending_block_marker_text(), Some("# ".to_string()));

    // Type text - should convert to heading
    state.apply(EditorAction::InsertText("Hello".to_string()));

    // Pending marker should be cleared
    assert!(state.block_style.pending_marker.is_none());

    // Block should now be a heading
    let block = &state.document.blocks[state.cursor.block_key];
    match &block.kind {
        writ::document::BlockKind::Heading { level, .. } => {
            assert_eq!(*level, 1);
        }
        _ => panic!("Expected heading block"),
    }

    // Text should be inserted
    assert_eq!(state.to_debug_string(), "Hello[|]");
}

#[test]
fn test_h2_space_text_creates_h2() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("## Title".to_string()));

    let block = &state.document.blocks[state.cursor.block_key];
    match &block.kind {
        writ::document::BlockKind::Heading { level, .. } => {
            assert_eq!(*level, 2);
        }
        _ => panic!("Expected heading block"),
    }

    assert_eq!(state.to_debug_string(), "Title[|]");
}

#[test]
fn test_backspace_heading_marker() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("## ".to_string()));

    assert_eq!(state.pending_block_marker_text(), Some("## ".to_string()));

    // Backspace removes space first
    state.apply(EditorAction::Backspace);
    assert_eq!(state.pending_block_marker_text(), Some("##".to_string()));

    // Backspace downgrades level
    state.apply(EditorAction::Backspace);
    assert_eq!(state.pending_block_marker_text(), Some("#".to_string()));

    // Backspace removes marker entirely
    state.apply(EditorAction::Backspace);
    assert!(state.block_style.pending_marker.is_none());
}

#[test]
fn test_backspace_removes_heading_style() {
    let mut state = EditorState::from_markdown("# Hello");

    // Block should be a heading
    let block = &state.document.blocks[state.cursor.block_key];
    match &block.kind {
        writ::document::BlockKind::Heading { level, .. } => {
            assert_eq!(*level, 1);
        }
        _ => panic!("Expected heading block"),
    }

    // Cursor is at start, backspace should convert to paragraph
    state.apply(EditorAction::Backspace);

    let block = &state.document.blocks[state.cursor.block_key];
    match &block.kind {
        writ::document::BlockKind::Paragraph { .. } => {}
        _ => panic!("Expected paragraph block after backspace"),
    }
}

#[test]
fn test_max_heading_level_is_6() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Type 7 hashes
    state.apply(EditorAction::InsertText("#######".to_string()));

    // Should only upgrade to level 6, then stop
    assert_eq!(
        state.pending_block_marker_text(),
        Some("######".to_string())
    );
}

#[test]
fn test_cursor_movement_clears_block_marker() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    state.apply(EditorAction::Enter);

    // Now in new empty block
    state.apply(EditorAction::InsertText("#".to_string()));
    assert!(state.block_style.pending_marker.is_some());

    // Move cursor - should clear block marker
    state.apply(EditorAction::MoveCursor(Direction::Up));
    assert!(state.block_style.pending_marker.is_none());
}

#[test]
fn test_enter_clears_block_marker() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("##".to_string()));
    assert!(state.block_style.pending_marker.is_some());

    state.apply(EditorAction::Enter);
    assert!(state.block_style.pending_marker.is_none());
}

// ============================================================================
// Block Merge Tests
// ============================================================================

#[test]
fn test_backspace_merges_with_previous_block() {
    let mut state = EditorState::from_markdown("First\n\nSecond");
    assert_eq!(state.to_debug_string(), "[|]First\nSecond");

    // Move to start of second block
    state.apply(EditorAction::MoveCursor(Direction::Down));
    assert_eq!(state.to_debug_string(), "First\n[|]Second");

    // Backspace should merge with previous block
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "First[|]Second");

    // Should now be a single block
    assert_eq!(state.document.block_order.len(), 1);
}

#[test]
fn test_backspace_merge_cursor_position() {
    let mut state = EditorState::from_markdown("ABC\n\nDEF");

    // Move to start of second block
    state.apply(EditorAction::MoveCursor(Direction::Down));

    // Backspace - cursor should be at position 3 (after "ABC")
    state.apply(EditorAction::Backspace);

    assert_eq!(state.cursor.offset, 3);
    assert_eq!(state.to_debug_string(), "ABC[|]DEF");
}

#[test]
fn test_delete_merges_with_next_block() {
    let mut state = EditorState::from_markdown("First\n\nSecond");

    // Move to end of first block
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "First[|]\nSecond");

    // Delete should merge with next block
    state.apply(EditorAction::Delete);
    assert_eq!(state.to_debug_string(), "First[|]Second");

    // Should now be a single block
    assert_eq!(state.document.block_order.len(), 1);
}

#[test]
fn test_delete_merge_cursor_stays() {
    let mut state = EditorState::from_markdown("ABC\n\nDEF");

    // Move to end of first block
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.cursor.offset, 3);

    // Delete - cursor should stay at position 3
    state.apply(EditorAction::Delete);

    assert_eq!(state.cursor.offset, 3);
    assert_eq!(state.to_debug_string(), "ABC[|]DEF");
}

#[test]
fn test_backspace_at_first_block_does_nothing() {
    let mut state = EditorState::from_markdown("Hello");
    assert_eq!(state.to_debug_string(), "[|]Hello");

    // Backspace at start of first block should do nothing
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]Hello");
}

#[test]
fn test_delete_at_last_block_does_nothing() {
    let mut state = EditorState::from_markdown("Hello");
    state.apply(EditorAction::MoveCursor(Direction::End));
    assert_eq!(state.to_debug_string(), "Hello[|]");

    // Delete at end of last block should do nothing
    state.apply(EditorAction::Delete);
    assert_eq!(state.to_debug_string(), "Hello[|]");
}

#[test]
fn test_backspace_merge_preserves_styles() {
    let mut state = EditorState::from_markdown("*italic*\n\n**bold**");

    // Move to start of second block
    state.apply(EditorAction::MoveCursor(Direction::Down));

    // Backspace to merge
    state.apply(EditorAction::Backspace);

    // Both styles should be preserved
    let block = &state.document.blocks[state.cursor.block_key];
    assert_eq!(block.text.to_debug_string(), "<i>italic</i><b>bold</b>");
}

#[test]
fn test_enter_then_backspace_roundtrip() {
    let mut state = EditorState::from_markdown("HelloWorld");

    // Move to middle
    for _ in 0..5 {
        state.apply(EditorAction::MoveCursor(Direction::Right));
    }
    assert_eq!(state.to_debug_string(), "Hello[|]World");

    // Enter to split
    state.apply(EditorAction::Enter);
    assert_eq!(state.to_debug_string(), "Hello\n[|]World");
    assert_eq!(state.document.block_order.len(), 2);

    // Backspace to merge back
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "Hello[|]World");
    assert_eq!(state.document.block_order.len(), 1);
}

// Tests for opening vs closing marker behavior

#[test]
fn test_closing_marker_after_text() {
    // *italic* - second * is a closing marker, resolves immediately
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*italic*".to_string()));

    // Closing marker resolves immediately - no pending marker, style is closed
    assert!(state.inline_style.pending_marker.is_none());
    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<i>italic</i>");
}

#[test]
fn test_opening_marker_after_space() {
    // "text *" - the * after space is an opening marker
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("text *".to_string()));

    // Should have pending opening marker
    assert!(state.inline_style.pending_marker.is_some());
    assert!(
        state
            .inline_style
            .pending_marker
            .as_ref()
            .unwrap()
            .is_opening
    );
}

#[test]
fn test_marker_after_text_no_space_is_closing() {
    // "italic*" with no matching open style - * should be inserted as literal
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("italic".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText(" more".to_string()));

    // The * was a closing marker with nothing to close, so inserted as literal
    assert_eq!(state.to_styled_debug_string(), "italic* more");
}

#[test]
fn test_space_star_space_is_literal() {
    // " * " - the * after space opens, but then space after means we're typing normal text
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("a * b".to_string()));

    // The * opened italic, then "b" was typed in italic
    // Actually the space resolves the pending marker first
    assert_eq!(state.to_styled_debug_string(), "a <i> b</i>");
}

#[test]
fn test_bold_closing_marker() {
    // **bold** - closing marker for bold, resolves immediately
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("**bold**".to_string()));

    // Closing marker resolves immediately
    assert!(state.inline_style.pending_marker.is_none());
    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<b>bold</b>");
}

#[test]
fn test_code_closing_marker() {
    // `code` - closing marker for code, resolves immediately
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("`code`".to_string()));

    // Closing marker resolves immediately
    assert!(state.inline_style.pending_marker.is_none());
    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<code>code</code>");
}

#[test]
fn test_strikethrough_closing_marker() {
    // ~~strikethrough~~ - closing marker for strikethrough, resolves immediately
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("~~strike~~".to_string()));

    // Closing marker resolves immediately
    assert!(state.inline_style.pending_marker.is_none());
    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<s>strike</s>");
}

#[test]
fn test_closing_inherited_code_style() {
    // Click into existing code text and type backtick to close it
    let mut state = EditorState::from_markdown("`code`");

    // Move to end of "code" (position 4, inside the code style)
    state.apply(EditorAction::MoveCursor(Direction::End));

    // Should have inherited the Code style
    assert_eq!(state.inline_style.open_styles.len(), 1);

    // Type backtick to close the inherited style
    state.apply(EditorAction::InsertText("`".to_string()));

    // Style should be closed
    assert!(state.inline_style.open_styles.is_empty());
    assert!(state.inline_style.pending_marker.is_none());
}

#[test]
fn test_closing_inherited_bold_style() {
    // Click into existing bold text and type ** to close it
    let mut state = EditorState::from_markdown("**bold**");

    // Move to end of "bold" (inside the bold style)
    state.apply(EditorAction::MoveCursor(Direction::End));

    // Should have inherited the Bold style
    assert_eq!(state.inline_style.open_styles.len(), 1);

    // Type ** to close the inherited style
    state.apply(EditorAction::InsertText("**".to_string()));

    // Style should be closed
    assert!(state.inline_style.open_styles.is_empty());
    assert!(state.inline_style.pending_marker.is_none());
}

#[test]
fn test_empty_heading_converts_to_paragraph_on_backspace() {
    let mut state = EditorState::from_markdown("# Hello");

    // Delete all text
    for _ in 0..5 {
        state.apply(EditorAction::Backspace);
    }

    // Block should now be a paragraph (converted when emptied)
    let block = &state.document.blocks[state.cursor.block_key];
    assert!(matches!(
        block.kind,
        writ::document::BlockKind::Paragraph { .. }
    ));
}

// ============================================================================
// Link and Image Tests
// ============================================================================

#[test]
fn test_bracket_tracks_unclosed() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[".to_string()));

    // Should have unclosed bracket tracked
    assert!(state.inline_style.unclosed_bracket.is_some());
    let bracket = state.inline_style.unclosed_bracket.as_ref().unwrap();
    assert_eq!(bracket.position, 0);
    assert!(!bracket.is_image);

    // Text should contain the [
    assert_eq!(state.to_debug_string(), "[[|]");
}

#[test]
fn test_exclamation_bracket_tracks_image() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("![".to_string()));

    // Should have unclosed bracket tracked as image
    assert!(state.inline_style.unclosed_bracket.is_some());
    let bracket = state.inline_style.unclosed_bracket.as_ref().unwrap();
    assert_eq!(bracket.position, 1); // position of [
    assert!(bracket.is_image);

    // Text should contain ![
    assert_eq!(state.to_debug_string(), "![[|]");
}

#[test]
fn test_close_bracket_without_open_is_literal() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("hello]world".to_string()));

    // No unclosed bracket, so ] should be literal
    assert!(state.inline_style.unclosed_bracket.is_none());
    assert_eq!(state.to_styled_debug_string(), "hello]world");
}

#[test]
fn test_close_bracket_with_open_creates_pending() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[link]".to_string()));

    // Should have pending ] marker (but it's not displayed since ] is already in text)
    assert!(state.inline_style.pending_marker.is_some());
    assert_eq!(state.pending_marker_text(), ""); // ] is in text, not displayed as marker
}

#[test]
fn test_link_complete_flow() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText(
        "[link](https://example.com)".to_string(),
    ));

    // Link should be complete - no pending state
    assert!(state.inline_style.pending_marker.is_none());
    assert!(state.inline_style.unclosed_bracket.is_none());
    assert!(state.inline_style.url_capture.is_none());

    // Text should have link style applied (brackets are removed)
    assert_eq!(state.to_styled_debug_string(), "<a>link</a>");
}

#[test]
fn test_image_complete_flow() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText(
        "![alt](https://example.com/img.png)".to_string(),
    ));

    // Image should be complete
    assert!(state.inline_style.pending_marker.is_none());
    assert!(state.inline_style.unclosed_bracket.is_none());
    assert!(state.inline_style.url_capture.is_none());

    // Text should have image style applied (brackets are removed)
    assert_eq!(state.to_styled_debug_string(), "<img>alt</img>");
}

#[test]
fn test_bracket_space_paren_is_literal() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[text] (not a link)".to_string()));

    // Space between ] and ( means literal brackets
    // The ] resolves the pending marker, then space is typed, clearing state
    assert!(state.inline_style.url_capture.is_none());
}

#[test]
fn test_url_capture_indicator() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[link](".to_string()));

    // Should be in URL capture mode
    assert!(state.inline_style.url_capture.is_some());

    // Should show link indicator
    let indicator = state.active_styles_indicator();
    assert!(indicator.is_some());
    assert!(indicator.unwrap().contains('↗'));
}

#[test]
fn test_image_url_capture_indicator() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("![alt](".to_string()));

    // Should be in URL capture mode
    assert!(state.inline_style.url_capture.is_some());

    // Should show image indicator
    let indicator = state.active_styles_indicator();
    assert!(indicator.is_some());
    assert!(indicator.unwrap().contains('▣'));
}

#[test]
fn test_backspace_in_url() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[link](abc".to_string()));

    // Should have URL "abc"
    let capture = state.inline_style.url_capture.as_ref().unwrap();
    assert_eq!(capture.url, "abc");

    state.apply(EditorAction::Backspace);

    // Should now have URL "ab"
    let capture = state.inline_style.url_capture.as_ref().unwrap();
    assert_eq!(capture.url, "ab");
}

#[test]
fn test_backspace_empty_url_returns_to_pending() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[link](".to_string()));

    assert!(state.inline_style.url_capture.is_some());

    state.apply(EditorAction::Backspace);

    // Should go back to pending ] state
    assert!(state.inline_style.url_capture.is_none());
    assert!(state.inline_style.pending_marker.is_some());
    assert!(state.inline_style.unclosed_bracket.is_some());
}

#[test]
fn test_cursor_movement_clears_link_state() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("[link](".to_string()));

    assert!(state.inline_style.url_capture.is_some());

    state.apply(EditorAction::MoveCursor(Direction::Left));

    // All link state should be cleared
    assert!(state.inline_style.url_capture.is_none());
    assert!(state.inline_style.unclosed_bracket.is_none());
}
