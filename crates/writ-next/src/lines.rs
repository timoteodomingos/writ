//! Line-based rendering model.
//!
//! This module provides line-by-line rendering where each line in the buffer
//! gets rendered as a separate element, with styling determined by tree-sitter.

use crate::buffer::Buffer;
use crate::parser::MarkdownTree;
use crate::render::{StyledRegion, TextStyle};
use std::ops::Range;
use tree_sitter::Node;

/// Information about a single line for rendering purposes.
#[derive(Debug, Clone, PartialEq)]
pub struct LineInfo {
    /// Byte range of this line in the buffer (excluding the newline)
    pub range: Range<usize>,
    /// The line number (0-indexed)
    pub line_number: usize,
    /// What kind of line this is
    pub kind: LineKind,
    /// Nesting depth (for lists, blockquotes)
    pub nesting_depth: usize,
    /// Range of the marker to hide when cursor is not on this line (e.g., "# ", "- ", "> ")
    pub marker_range: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    /// Empty line
    Blank,
    /// Regular paragraph text
    Paragraph,
    /// Heading with level 1-6
    Heading(u8),
    /// List item (ordered or unordered)
    ListItem {
        ordered: bool,
        checked: Option<bool>,
    },
    /// Blockquote line
    BlockQuote,
    /// Code block line (inside fenced code block)
    CodeBlock {
        language: Option<String>,
        is_fence: bool,
    },
}

/// Extract line information from a buffer using tree-sitter.
pub fn extract_lines(buffer: &Buffer) -> Vec<LineInfo> {
    let text = buffer.text();
    let tree = buffer.tree();

    // First, split the buffer into lines
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut line_number = 0;

    for (i, c) in text.char_indices() {
        if c == '\n' {
            lines.push((line_number, line_start..i));
            line_start = i + 1;
            line_number += 1;
        }
    }
    // Handle last line if it doesn't end with newline
    if line_start < text.len() {
        lines.push((line_number, line_start..text.len()));
    }
    // Handle empty buffer or buffer ending with newline (adds empty final line)
    if text.is_empty() || text.ends_with('\n') {
        lines.push((line_number, line_start..line_start));
    }

    // Now determine the kind of each line using tree-sitter
    lines
        .into_iter()
        .map(|(line_num, range)| {
            let kind = if range.start == range.end {
                LineKind::Blank
            } else if let Some(tree) = &tree {
                determine_line_kind(tree, &text, &range)
            } else {
                LineKind::Paragraph
            };

            let (nesting_depth, marker_range) = if let Some(tree) = &tree {
                determine_line_context(tree, &text, &range, &kind)
            } else {
                (0, None)
            };

            LineInfo {
                range,
                line_number: line_num,
                kind,
                nesting_depth,
                marker_range,
            }
        })
        .collect()
}

/// Extract inline styles (bold, italic, code, etc.) for a specific line.
pub fn extract_inline_styles(buffer: &Buffer, line: &LineInfo) -> Vec<StyledRegion> {
    let Some(tree) = buffer.tree() else {
        return Vec::new();
    };

    let text = buffer.text();
    let mut styles = Vec::new();

    // Find the inline node that covers this line's content
    let root = tree.block_tree().root_node();
    collect_inline_styles_in_range(&root, tree, &text, &line.range, &mut styles);

    styles
}

/// Recursively find inline nodes and collect their styles if they overlap with the given range.
fn collect_inline_styles_in_range(
    node: &Node,
    tree: &MarkdownTree,
    text: &str,
    range: &Range<usize>,
    styles: &mut Vec<StyledRegion>,
) {
    // Skip nodes that don't overlap with our range
    if node.end_byte() <= range.start || node.start_byte() >= range.end {
        return;
    }

    // If this is an inline node, get its inline tree and collect styles
    if node.kind() == "inline" {
        if let Some(inline_tree) = tree.inline_tree(node) {
            collect_inline_styles_recursive(inline_tree.root_node(), text, styles);
        }
        return;
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_inline_styles_in_range(&child, tree, text, range, styles);
        }
    }
}

/// Recursively collect inline styles from an inline tree.
fn collect_inline_styles_recursive(node: Node, text: &str, styles: &mut Vec<StyledRegion>) {
    match node.kind() {
        "emphasis" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::italic()) {
                styles.push(region);
            }
        }
        "strong_emphasis" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::bold()) {
                styles.push(region);
            }
        }
        "code_span" => {
            if let Some(region) = extract_code_span_region(&node) {
                styles.push(region);
            }
        }
        "strikethrough" => {
            if let Some(region) = extract_emphasis_region(&node, TextStyle::strikethrough()) {
                styles.push(region);
            }
        }
        _ => {}
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_inline_styles_recursive(child, text, styles);
        }
    }
}

/// Extract a styled region from an emphasis-like node.
fn extract_emphasis_region(node: &Node, style: TextStyle) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    // Find delimiter boundaries
    let mut delimiters: Vec<(usize, usize)> = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "emphasis_delimiter" || kind.ends_with("_delimiter") {
                delimiters.push((child.start_byte(), child.end_byte()));
            }
        }
    }

    // Opening delimiters from start
    for &(start, end) in &delimiters {
        if start == content_start {
            content_start = end;
        }
    }

    // Closing delimiters from end
    for &(start, end) in delimiters.iter().rev() {
        if end == content_end {
            content_end = start;
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style,
    })
}

/// Extract a styled region from a code span.
fn extract_code_span_region(node: &Node) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "code_span_delimiter" {
                if child.start_byte() == full_start {
                    content_start = child.end_byte();
                } else if child.end_byte() == full_end {
                    content_end = child.start_byte();
                }
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::code(),
    })
}

fn determine_line_kind(tree: &MarkdownTree, text: &str, range: &Range<usize>) -> LineKind {
    // Walk tree-sitter to find what node contains this line's start position
    let root = tree.block_tree().root_node();

    // Find node and its ancestor chain at this position
    if let Some((node, ancestors)) = find_node_with_ancestors(&root, range.start) {
        // Check ancestors for container/block types
        // The innermost relevant ancestor determines the line type
        for ancestor in ancestors.iter().rev() {
            match ancestor.kind() {
                "atx_heading" => {
                    let line_text = &text[range.clone()];
                    let level = line_text.chars().take_while(|&c| c == '#').count() as u8;
                    return LineKind::Heading(level.min(6).max(1));
                }
                "block_quote" => return LineKind::BlockQuote,
                "list_item" => {
                    let line_text = &text[range.clone()];
                    let checked = if line_text.contains("[ ]") {
                        Some(false)
                    } else if line_text.contains("[x]") || line_text.contains("[X]") {
                        Some(true)
                    } else {
                        None
                    };
                    let trimmed = line_text.trim_start();
                    let ordered = trimmed
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false);
                    return LineKind::ListItem { ordered, checked };
                }
                "fenced_code_block" => {
                    let line_text = &text[range.clone()];
                    let is_fence = line_text.trim_start().starts_with("```");
                    return LineKind::CodeBlock {
                        language: None,
                        is_fence,
                    };
                }
                _ => {}
            }
        }

        // No container ancestor, check the node itself
        match node.kind() {
            "atx_heading" => {
                let line_text = &text[range.clone()];
                let level = line_text.chars().take_while(|&c| c == '#').count() as u8;
                LineKind::Heading(level.min(6).max(1))
            }
            "list_item" => {
                let line_text = &text[range.clone()];
                let checked = if line_text.contains("[ ]") {
                    Some(false)
                } else if line_text.contains("[x]") || line_text.contains("[X]") {
                    Some(true)
                } else {
                    None
                };
                let trimmed = line_text.trim_start();
                let ordered = trimmed
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false);
                LineKind::ListItem { ordered, checked }
            }
            "block_quote" => LineKind::BlockQuote,
            "fenced_code_block" => {
                let line_text = &text[range.clone()];
                let is_fence = line_text.trim_start().starts_with("```");
                LineKind::CodeBlock {
                    language: None,
                    is_fence,
                }
            }
            "paragraph" | "inline" => LineKind::Paragraph,
            _ => LineKind::Paragraph,
        }
    } else {
        // No node found - treat as blank or paragraph
        if range.start == range.end {
            LineKind::Blank
        } else {
            LineKind::Paragraph
        }
    }
}

/// Find the deepest node at a position, along with all its ancestors.
fn find_node_with_ancestors<'a>(
    root: &tree_sitter::Node<'a>,
    pos: usize,
) -> Option<(tree_sitter::Node<'a>, Vec<tree_sitter::Node<'a>>)> {
    fn find_recursive<'a>(
        node: &tree_sitter::Node<'a>,
        pos: usize,
        ancestors: &mut Vec<tree_sitter::Node<'a>>,
    ) -> Option<tree_sitter::Node<'a>> {
        if pos < node.start_byte() || pos >= node.end_byte() {
            return None;
        }

        // Try to find in children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                ancestors.push(*node);
                if let Some(found) = find_recursive(&child, pos, ancestors) {
                    return Some(found);
                }
                ancestors.pop();
            }
        }

        // No child contains it, return this node
        Some(*node)
    }

    let mut ancestors = Vec::new();
    find_recursive(root, pos, &mut ancestors).map(|node| (node, ancestors))
}

fn determine_line_context(
    _tree: &MarkdownTree,
    text: &str,
    range: &Range<usize>,
    kind: &LineKind,
) -> (usize, Option<Range<usize>>) {
    // TODO: Calculate nesting depth and marker range
    let marker_range = match kind {
        LineKind::Heading(_) => {
            // Find the "# " prefix
            let line_text = &text[range.clone()];
            let hashes = line_text.chars().take_while(|&c| c == '#').count();
            let space = if line_text.chars().nth(hashes) == Some(' ') {
                1
            } else {
                0
            };
            if hashes > 0 {
                Some(range.start..range.start + hashes + space)
            } else {
                None
            }
        }
        LineKind::ListItem { .. } => {
            // Find the "- " or "1. " prefix
            let line_text = &text[range.clone()];
            let trimmed_start = line_text.len() - line_text.trim_start().len();
            let marker_len = if line_text.trim_start().starts_with("- ") {
                2
            } else if line_text.trim_start().starts_with("* ") {
                2
            } else {
                // Ordered list: find "N. "
                let rest = line_text.trim_start();
                let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
                if rest.chars().nth(digits) == Some('.') {
                    digits + 2 // "N. "
                } else {
                    0
                }
            };
            if marker_len > 0 {
                Some(range.start..range.start + trimmed_start + marker_len)
            } else {
                None
            }
        }
        LineKind::BlockQuote => {
            // Find the "> " prefix
            let line_text = &text[range.clone()];
            if line_text.starts_with("> ") {
                Some(range.start..range.start + 2)
            } else if line_text.starts_with(">") {
                Some(range.start..range.start + 1)
            } else {
                None
            }
        }
        _ => None,
    };

    (0, marker_range) // TODO: nesting depth
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== BLANK LINE TESTS ==========

    #[test]
    fn test_empty_buffer() {
        let buf: Buffer = "".parse().unwrap();
        let lines = extract_lines(&buf);

        // Empty buffer should have one blank line (the cursor needs somewhere to be)
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, LineKind::Blank);
        assert_eq!(lines[0].range, 0..0);
    }

    #[test]
    fn test_single_newline() {
        let buf: Buffer = "\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // "\n" = one blank line, then cursor position after
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, LineKind::Blank);
        assert_eq!(lines[0].range, 0..0);
        assert_eq!(lines[1].kind, LineKind::Blank);
        assert_eq!(lines[1].range, 1..1);
    }

    #[test]
    fn test_blank_line_between_paragraphs() {
        let buf: Buffer = "Hello\n\nWorld\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // "Hello\n\nWorld\n" = 4 lines
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].kind, LineKind::Paragraph);
        assert_eq!(lines[0].range, 0..5); // "Hello"
        assert_eq!(lines[1].kind, LineKind::Blank);
        assert_eq!(lines[1].range, 6..6); // empty
        assert_eq!(lines[2].kind, LineKind::Paragraph);
        assert_eq!(lines[2].range, 7..12); // "World"
        assert_eq!(lines[3].kind, LineKind::Blank);
        assert_eq!(lines[3].range, 13..13); // after final \n
    }

    #[test]
    fn test_enter_at_start_of_line() {
        // Simulating: had "Hello\n", pressed Enter at position 0
        // Buffer becomes "\nHello\n"
        let buf: Buffer = "\nHello\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].kind, LineKind::Blank);
        assert_eq!(lines[0].range, 0..0);
        assert_eq!(lines[1].kind, LineKind::Paragraph);
        assert_eq!(lines[1].range, 1..6); // "Hello"
        assert_eq!(lines[2].kind, LineKind::Blank);
    }

    // ========== HEADING TESTS ==========

    #[test]
    fn test_heading_tree_structure() {
        let buf: Buffer = "# Hello\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        fn print_tree(node: &tree_sitter::Node, indent: usize) {
            eprintln!(
                "{:indent$}{} [{}-{}]",
                "",
                node.kind(),
                node.start_byte(),
                node.end_byte(),
                indent = indent
            );
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    print_tree(&child, indent + 2);
                }
            }
        }

        eprintln!("=== Heading tree structure ===");
        print_tree(&root, 0);
    }

    #[test]
    fn test_heading_line() {
        let buf: Buffer = "# Hello\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, LineKind::Heading(1));
        assert_eq!(lines[0].marker_range, Some(0..2)); // "# "
    }

    #[test]
    fn test_heading_levels() {
        let buf: Buffer = "# H1\n## H2\n### H3\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::Heading(1));
        assert_eq!(lines[1].kind, LineKind::Heading(2));
        assert_eq!(lines[2].kind, LineKind::Heading(3));
    }

    // ========== LIST TESTS ==========

    #[test]
    fn test_unordered_list() {
        let buf: Buffer = "- Item 1\n- Item 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            lines[0].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(lines[0].marker_range, Some(0..2)); // "- "
        assert_eq!(
            lines[1].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
    }

    #[test]
    fn test_ordered_list() {
        let buf: Buffer = "1. First\n2. Second\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            lines[0].kind,
            LineKind::ListItem {
                ordered: true,
                checked: None
            }
        );
        assert_eq!(lines[0].marker_range, Some(0..3)); // "1. "
    }

    #[test]
    fn test_task_list() {
        let buf: Buffer = "- [ ] Unchecked\n- [x] Checked\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            lines[0].kind,
            LineKind::ListItem {
                ordered: false,
                checked: Some(false)
            }
        );
        assert_eq!(
            lines[1].kind,
            LineKind::ListItem {
                ordered: false,
                checked: Some(true)
            }
        );
    }

    #[test]
    fn test_nested_list() {
        let buf: Buffer = "- Item 1\n  - Nested\n- Item 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // All should be list items, but nested one should have nesting_depth > 0
        assert_eq!(
            lines[0].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(
            lines[1].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        // TODO: Check nesting depth once implemented
        // assert_eq!(lines[1].nesting_depth, 1);
        assert_eq!(
            lines[2].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
    }

    // ========== BLOCKQUOTE TESTS ==========

    #[test]
    fn test_blockquote_tree_structure() {
        // Debug test to see what tree-sitter produces for blockquotes
        let buf: Buffer = "> Quote\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        fn print_tree(node: &tree_sitter::Node, indent: usize) {
            eprintln!(
                "{:indent$}{} [{}-{}]",
                "",
                node.kind(),
                node.start_byte(),
                node.end_byte(),
                indent = indent
            );
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    print_tree(&child, indent + 2);
                }
            }
        }

        eprintln!("=== Blockquote tree structure ===");
        print_tree(&root, 0);
    }

    #[test]
    fn test_blockquote() {
        let buf: Buffer = "> Quote line 1\n> Quote line 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::BlockQuote);
        assert_eq!(lines[0].marker_range, Some(0..2)); // "> "
        assert_eq!(lines[1].kind, LineKind::BlockQuote);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::BlockQuote);
        assert_eq!(lines[1].kind, LineKind::BlockQuote);
        // TODO: nesting depth should differ
    }

    // ========== CODE BLOCK TESTS ==========

    #[test]
    fn test_code_block() {
        let buf: Buffer = "```rust\nlet x = 1;\n```\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(matches!(
            lines[0].kind,
            LineKind::CodeBlock { is_fence: true, .. }
        ));
        assert!(matches!(
            lines[1].kind,
            LineKind::CodeBlock {
                is_fence: false,
                ..
            }
        ));
        assert!(matches!(
            lines[2].kind,
            LineKind::CodeBlock { is_fence: true, .. }
        ));
    }

    // ========== MIXED CONTENT TESTS ==========

    #[test]
    fn test_mixed_content() {
        let buf: Buffer = "# Title\n\nParagraph\n\n- List item\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::Heading(1));
        assert_eq!(lines[1].kind, LineKind::Blank);
        assert_eq!(lines[2].kind, LineKind::Paragraph);
        assert_eq!(lines[3].kind, LineKind::Blank);
        assert_eq!(
            lines[4].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
    }

    // ========== CURSOR POSITIONING TESTS ==========
    // These test that we can map cursor byte offset to (line, column)

    #[test]
    fn test_cursor_to_line_mapping() {
        let buf: Buffer = "Hello\n\nWorld\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Cursor at byte 0 = line 0, col 0
        assert!(lines[0].range.contains(&0) || lines[0].range.start == 0);

        // Cursor at byte 6 (the blank line) = line 1
        // The blank line range is 6..6, so cursor at 6 should map to line 1
        assert_eq!(lines[1].range, 6..6);

        // Cursor at byte 7 = line 2, col 0 (start of "World")
        assert!(lines[2].range.start == 7);
    }
}
