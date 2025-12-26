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
fn test_backspace_clears_open_styles_when_block_empty() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Open italic style
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("a".to_string()));
    assert!(!state.inline_style.open_styles.is_empty());
    // Delete the 'a' - block becomes empty, styles should be cleared
    state.apply(EditorAction::Backspace);
    assert_eq!(state.to_debug_string(), "[|]");
    assert!(state.inline_style.open_styles.is_empty());
}

#[test]
fn test_backspace_clears_all_open_styles_when_block_empty() {
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    // Open bold then italic: ***
    state.apply(EditorAction::InsertText("**".to_string()));
    state.apply(EditorAction::InsertText("*".to_string()));
    state.apply(EditorAction::InsertText("a".to_string()));
    assert_eq!(state.inline_style.open_styles.len(), 2);
    // Delete the 'a' - block becomes empty, all styles should be cleared at once
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
    // *italic* - second * is a closing marker, shown until next char
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("*italic*".to_string()));

    // Closing marker is pending (visible feedback)
    assert!(state.inline_style.pending_marker.is_some());
    assert!(
        !state
            .inline_style
            .pending_marker
            .as_ref()
            .unwrap()
            .is_opening
    );

    // Type next character to resolve the closing marker
    state.apply(EditorAction::InsertText(" ".to_string()));

    // Now style should be closed
    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<i>italic</i> ");
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
    // **bold** - closing marker for bold, shown until next char
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("**bold**".to_string()));

    // Closing marker is pending
    assert!(state.inline_style.pending_marker.is_some());

    // Type next character to resolve
    state.apply(EditorAction::InsertText(" ".to_string()));

    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<b>bold</b> ");
}

#[test]
fn test_code_closing_marker() {
    // `code` - closing marker for code, shown until next char
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("`code`".to_string()));

    // Closing marker is pending
    assert!(state.inline_style.pending_marker.is_some());

    // Type next character to resolve
    state.apply(EditorAction::InsertText(" ".to_string()));

    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<code>code</code> ");
}

#[test]
fn test_strikethrough_closing_marker() {
    // ~~strikethrough~~ - closing marker for strikethrough, shown until next char
    let mut state = EditorState::from_markdown("x");
    state.apply(EditorAction::Delete);
    state.apply(EditorAction::InsertText("~~strike~~".to_string()));

    // Closing marker is pending
    assert!(state.inline_style.pending_marker.is_some());

    // Type next character to resolve
    state.apply(EditorAction::InsertText(" ".to_string()));

    assert!(state.inline_style.open_styles.is_empty());
    assert_eq!(state.to_styled_debug_string(), "<s>strike</s> ");
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
