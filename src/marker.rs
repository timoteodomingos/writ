//! Line markers for markdown block-level elements.
//!
//! This module provides types for representing markers (blockquotes, lists,
//! headings, etc.) and functions for extracting them from the parse tree.

use std::ops::Range;
use tree_sitter::Node;

/// The type of marker on a line.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkerKind {
    BlockQuote,
    ListItem { ordered: bool },
    Checkbox { checked: bool },
    Heading(u8),
    CodeBlockFence { language: Option<String> },
    CodeBlockContent,
    ThematicBreak,
    Indent,
}

/// A marker on a line with its byte range.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub kind: MarkerKind,
    pub range: Range<usize>,
}

/// A line with its markers and metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct LineMarkers {
    pub range: Range<usize>,
    pub line_number: usize,
    pub markers: Vec<Marker>,
}

impl LineMarkers {
    /// Returns the combined byte range of all markers, or None if no markers.
    pub fn marker_range(&self) -> Option<Range<usize>> {
        if self.markers.is_empty() {
            return None;
        }
        // Markers are ordered innermost to outermost.
        // Outermost (last) has earliest start, innermost (first) has latest end.
        let start = self.markers.last()?.range.start;
        let end = self.markers.first()?.range.end;
        Some(start..end)
    }

    /// Returns the width of the marker (including trailing space) relative to line start.
    /// This is the number of spaces needed to nest under this line.
    /// E.g., "- " = 2, "1. " = 3, "10. " = 4
    pub fn marker_width(&self) -> usize {
        if let Some(range) = self.marker_range() {
            range.end - self.range.start
        } else {
            0
        }
    }

    /// Returns the visual substitution text for all markers.
    /// E.g., "• " for unordered list, "[ ] " for unchecked task.
    /// Computes leading whitespace from line start to the first non-whitespace
    /// character, to respect user's manual indentation.
    pub fn substitution(&self, text: &str) -> String {
        // No markers = no substitution (e.g., code block content lines)
        if self.markers.is_empty() {
            return String::new();
        }

        // If the only marker is Indent, return empty - padding is handled by rendering
        if self.markers.len() == 1 && matches!(self.markers[0].kind, MarkerKind::Indent) {
            return String::new();
        }

        let line_text = &text[self.range.clone()];

        // Find leading whitespace in the line
        let leading_ws_len = line_text.len() - line_text.trim_start().len();
        let leading_ws = &line_text[..leading_ws_len];

        // Build substitution: leading whitespace + non-Indent marker substitutions
        let mut result = leading_ws.to_string();
        for m in self.markers.iter().rev() {
            if !matches!(m.kind, MarkerKind::Indent) {
                result.push_str(m.kind.substitution());
            }
        }
        result
    }

    /// Returns the continuation text to insert on Enter.
    /// E.g., "> " for blockquote, "- " for list.
    /// For Indent and ListItem markers, extracts the actual text from the range
    /// (which includes leading whitespace for nested lists).
    /// Markers are stored innermost to outermost, but continuation should be
    /// in text order (outermost to innermost), so we reverse.
    pub fn continuation(&self, text: &str) -> String {
        self.markers
            .iter()
            .rev()
            .map(|m| match &m.kind {
                MarkerKind::Indent | MarkerKind::ListItem { .. } => {
                    text[m.range.clone()].to_string()
                }
                _ => m.kind.continuation().to_string(),
            })
            .collect()
    }

    /// Returns true if any marker has a left border (blockquotes).
    pub fn has_border(&self) -> bool {
        self.markers.iter().any(|m| m.kind.has_border())
    }

    /// Returns the checkbox state if this line has a task list marker.
    pub fn checkbox(&self) -> Option<bool> {
        for m in &self.markers {
            if let MarkerKind::Checkbox { checked } = m.kind {
                return Some(checked);
            }
        }
        None
    }

    /// Returns the leading whitespace before the first marker.
    pub fn leading_whitespace(&self, text: &str) -> String {
        if let Some(first) = self.markers.first()
            && first.range.start > self.range.start
        {
            return text[self.range.start..first.range.start].to_string();
        }
        String::new()
    }

    /// Returns true if this line is code block content (not a fence line).
    pub fn is_code_block_content(&self) -> bool {
        self.markers
            .iter()
            .any(|m| matches!(m.kind, MarkerKind::CodeBlockContent))
    }

    /// Returns true if this line is a code block fence (opening or closing).
    pub fn is_fence(&self) -> bool {
        self.markers
            .iter()
            .any(|m| matches!(m.kind, MarkerKind::CodeBlockFence { .. }))
    }

    /// Returns the heading level if this line is a heading.
    pub fn heading_level(&self) -> Option<u8> {
        for m in &self.markers {
            if let MarkerKind::Heading(level) = m.kind {
                return Some(level);
            }
        }
        None
    }
}

impl MarkerKind {
    /// Visual substitution text for this marker kind.
    pub fn substitution(&self) -> &'static str {
        match self {
            MarkerKind::BlockQuote => "  ", // Replace "> " with spaces, border shows visually
            MarkerKind::ListItem { ordered: false } => "• ",
            MarkerKind::ListItem { ordered: true } => "",
            MarkerKind::Checkbox { checked: false } => "[ ] ",
            MarkerKind::Checkbox { checked: true } => "[x] ",
            MarkerKind::Heading(_) => "",
            MarkerKind::CodeBlockFence { .. } => "",
            MarkerKind::CodeBlockContent => "",
            MarkerKind::ThematicBreak => "",
            MarkerKind::Indent => "  ",
        }
    }

    /// Continuation text to insert on Enter.
    pub fn continuation(&self) -> &'static str {
        match self {
            MarkerKind::BlockQuote => "> ",
            MarkerKind::ListItem { ordered: false } => "- ",
            MarkerKind::ListItem { ordered: true } => "1. ",
            MarkerKind::Checkbox { .. } => "[ ] ",
            MarkerKind::Heading(_) => "",
            MarkerKind::CodeBlockFence { .. } => "",
            MarkerKind::CodeBlockContent => "",
            MarkerKind::ThematicBreak => "",
            MarkerKind::Indent => "",
        }
    }

    /// Whether this marker has a left border.
    pub fn has_border(&self) -> bool {
        matches!(self, MarkerKind::BlockQuote)
    }
}

/// Find the index of the first node with start_byte >= target.
/// Uses binary search since nodes are in document order (sorted by start_byte).
fn find_node_index(nodes: &[Node], target_byte: usize) -> usize {
    nodes
        .binary_search_by_key(&target_byte, |n| n.start_byte())
        .unwrap_or_else(|idx| idx)
}

/// Find the nearest container to the left of cursor position.
/// Returns the marker width needed to nest into that container.
/// Uses cached LineMarkers for O(log n) lookup instead of traversing nodes.
pub fn find_container_indent_from_lines(lines: &[LineMarkers], cursor_pos: usize) -> Option<usize> {
    // Binary search to find the line containing cursor_pos
    let line_idx = lines
        .binary_search_by(|line| {
            if cursor_pos < line.range.start {
                std::cmp::Ordering::Greater
            } else if cursor_pos >= line.range.end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .unwrap_or_else(|idx| idx.saturating_sub(1));

    // Walk backwards from cursor line looking for a container marker
    for line in lines[..=line_idx.min(lines.len().saturating_sub(1))]
        .iter()
        .rev()
    {
        // Check if this line has a list or blockquote marker
        for marker in &line.markers {
            match &marker.kind {
                MarkerKind::ListItem { .. } | MarkerKind::BlockQuote => {
                    return Some(marker.range.end - marker.range.start);
                }
                _ => {}
            }
        }
    }

    None
}

/// Collect all nodes in document order (preorder traversal).
pub fn collect_nodes<'a>(root: &Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = root.walk();
    let mut nodes = Vec::new();

    loop {
        nodes.push(cursor.node());

        if cursor.goto_first_child() {
            continue;
        }
        if cursor.goto_next_sibling() {
            continue;
        }
        loop {
            if !cursor.goto_parent() {
                return nodes;
            }
            if cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Find all markers for a line by scanning nodes that start within the line.
/// Returns markers innermost to outermost (reverse document order).
/// Takes a pre-computed nodes vec from `collect_nodes()` for efficiency.
pub fn markers_at(nodes: &[Node], text: &str, line_start: usize, line_end: usize) -> Vec<Marker> {
    let mut markers = Vec::new();

    // Binary search to find first node past line_end - we iterate backwards from here
    let end_idx = find_node_index(nodes, line_end + 1);

    // Iterate in reverse to get innermost markers first
    for node in nodes[..end_idx].iter().rev() {
        let start = node.start_byte();
        // Stop once we're before the line
        if start < line_start {
            break;
        }
        let end = node.end_byte();
        let kind = node.kind();

        match kind {
            "block_quote_marker" | "block_continuation" => {
                // Skip block_continuation inside indented_code_block (it's code indent, not a marker)
                if kind == "block_continuation"
                    && let Some(parent) = node.parent()
                    && parent.kind() == "indented_code_block"
                {
                    continue;
                }
                let content = &text[start..end];
                if content.contains('>') {
                    markers.push(Marker {
                        kind: MarkerKind::BlockQuote,
                        range: start..end,
                    });
                } else if !content.is_empty() && content.chars().all(|c| c.is_whitespace()) {
                    markers.push(Marker {
                        kind: MarkerKind::Indent,
                        range: start..end,
                    });
                }
            }
            "list_marker_minus" | "list_marker_plus" | "list_marker_star" => {
                markers.push(Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: start..end,
                });
            }
            "list_marker_dot" | "list_marker_parenthesis" => {
                markers.push(Marker {
                    kind: MarkerKind::ListItem { ordered: true },
                    range: start..end,
                });
            }
            "task_list_marker_unchecked" => {
                // Include trailing space after ] if present
                let range_end = if text.as_bytes().get(end) == Some(&b' ') {
                    end + 1
                } else {
                    end
                };
                markers.push(Marker {
                    kind: MarkerKind::Checkbox { checked: false },
                    range: start..range_end,
                });
            }
            "task_list_marker_checked" => {
                // Include trailing space after ] if present
                let range_end = if text.as_bytes().get(end) == Some(&b' ') {
                    end + 1
                } else {
                    end
                };
                markers.push(Marker {
                    kind: MarkerKind::Checkbox { checked: true },
                    range: start..range_end,
                });
            }
            "atx_h1_marker" | "atx_h2_marker" | "atx_h3_marker" | "atx_h4_marker"
            | "atx_h5_marker" | "atx_h6_marker" => {
                let level = match kind {
                    "atx_h1_marker" => 1,
                    "atx_h2_marker" => 2,
                    "atx_h3_marker" => 3,
                    "atx_h4_marker" => 4,
                    "atx_h5_marker" => 5,
                    _ => 6,
                };
                // Include trailing space after # if present
                let range_end = if text.as_bytes().get(end) == Some(&b' ') {
                    end + 1
                } else {
                    end
                };
                markers.push(Marker {
                    kind: MarkerKind::Heading(level),
                    range: start..range_end,
                });
            }
            "thematic_break" => {
                markers.push(Marker {
                    kind: MarkerKind::ThematicBreak,
                    range: start..end,
                });
            }
            "fenced_code_block_delimiter" => {
                // Check if we already recorded a language from info_string
                let language = markers.iter().find_map(|m| {
                    if let MarkerKind::CodeBlockFence { language } = &m.kind {
                        language.clone()
                    } else {
                        None
                    }
                });
                // Remove any placeholder fence marker we added from info_string
                markers.retain(|m| !matches!(m.kind, MarkerKind::CodeBlockFence { .. }));
                markers.push(Marker {
                    kind: MarkerKind::CodeBlockFence { language },
                    range: start..end,
                });
            }
            "info_string" => {
                let lang = text[start..end].trim();
                let language = if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_string())
                };
                // Store the language temporarily - will be picked up by delimiter
                markers.push(Marker {
                    kind: MarkerKind::CodeBlockFence { language },
                    range: start..end,
                });
            }

            _ => {}
        }
    }

    markers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::lines::extract_lines;

    fn kinds(markers: &[Marker]) -> Vec<&MarkerKind> {
        markers.iter().map(|m| &m.kind).collect()
    }

    fn print_tree(node: &tree_sitter::Node, text: &str, indent: usize) {
        let spacing = "  ".repeat(indent);
        let preview: String = text[node.byte_range()]
            .chars()
            .take(20)
            .flat_map(|c| if c == '\n' { vec!['\\', 'n'] } else { vec![c] })
            .collect();
        println!(
            "{}{} [{}-{}] {:?}",
            spacing,
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            preview,
        );
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                print_tree(&child, text, indent + 1);
            }
        }
    }

    fn print_nodes_by_position(root: &tree_sitter::Node, text: &str) {
        let mut cursor = root.walk();
        let mut nodes = Vec::new();

        loop {
            nodes.push((
                cursor.node().start_byte(),
                cursor.node().end_byte(),
                cursor.node().kind().to_string(),
            ));

            if cursor.goto_first_child() {
                continue;
            }
            if cursor.goto_next_sibling() {
                continue;
            }
            loop {
                if !cursor.goto_parent() {
                    // Print sorted by start position
                    nodes.sort_by_key(|(start, _, _)| *start);
                    println!("\nNodes by position:");
                    for (start, end, kind) in &nodes {
                        let preview: String = text[*start..*end]
                            .chars()
                            .take(15)
                            .flat_map(|c| if c == '\n' { vec!['\\', 'n'] } else { vec![c] })
                            .collect();
                        println!("  [{}-{}] {} {:?}", start, end, kind, preview);
                    }
                    return;
                }
                if cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    #[test]
    fn test_block_continuation_structure() {
        // Understand where block_continuation nodes appear
        let buf: Buffer = "> Line 1\n> Line 2\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("\n=== Multiline blockquote ===");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 2 is bytes 9-17 ("> Line 2")
        // Probe at end of line (16) - where do we land?
        let probe = 16;
        let node = root.descendant_for_byte_range(probe, probe);
        println!(
            "\nProbe at {}: {:?}",
            probe,
            node.map(|n| (n.kind(), n.byte_range()))
        );

        // What is the first child of inline?
        if let Some(inline) = node {
            println!(
                "inline first child: {:?}",
                inline.child(0).map(|c| (c.kind(), c.byte_range()))
            );
        }

        // What about probing at 10 (inside block_continuation)?
        let probe = 10;
        let node = root.descendant_for_byte_range(probe, probe);
        println!(
            "Probe at {}: {:?}",
            probe,
            node.map(|n| (n.kind(), n.byte_range()))
        );
    }

    #[test]
    fn test_simple_list() {
        let buf: Buffer = "- Item\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
    }

    #[test]
    fn test_multiline_blockquote() {
        let buf: Buffer = "> Line 1\n> Line 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        println!("Line 0 markers: {:?}", lines[0].markers);
        println!("Line 1 markers: {:?}", lines[1].markers);

        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::BlockQuote]);
        assert_eq!(kinds(&lines[1].markers), vec![&MarkerKind::BlockQuote]);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("\n=== Nested blockquote ===");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);
        print_nodes_by_position(&root, &text);

        let lines = extract_lines(&buf);
        println!("Line 0 markers: {:?}", lines[0].markers);
        println!("Line 1 markers: {:?}", lines[1].markers);

        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::BlockQuote]);
        assert_eq!(
            kinds(&lines[1].markers),
            vec![&MarkerKind::BlockQuote, &MarkerKind::BlockQuote]
        );
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![
                &MarkerKind::ListItem { ordered: false },
                &MarkerKind::BlockQuote
            ]
        );
    }

    #[test]
    fn test_ordered_list() {
        let buf: Buffer = "1. First\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: true }]
        );
    }

    #[test]
    fn test_task_list() {
        let buf: Buffer = "- [ ] Todo\n- [x] Done\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            kinds(&lines[0].markers),
            vec![
                &MarkerKind::Checkbox { checked: false },
                &MarkerKind::ListItem { ordered: false },
            ]
        );
        assert_eq!(
            kinds(&lines[1].markers),
            vec![
                &MarkerKind::Checkbox { checked: true },
                &MarkerKind::ListItem { ordered: false },
            ]
        );
    }

    #[test]
    fn test_heading() {
        let buf: Buffer = "## Heading\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::Heading(2)]);
    }

    #[test]
    fn test_fenced_code_block() {
        let buf: Buffer = "```rust\nlet x = 1;\n```\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Opening fence with language
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::CodeBlockFence {
                language: Some("rust".to_string())
            }]
        );
        // Content lines have no markers (code block detection handled separately)
        assert_eq!(kinds(&lines[1].markers), vec![] as Vec<&MarkerKind>);
        // Closing fence
        assert_eq!(
            kinds(&lines[2].markers),
            vec![&MarkerKind::CodeBlockFence { language: None }]
        );
    }

    #[test]
    fn test_fenced_code_block_with_indentation() {
        let buf: Buffer = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n"
            .parse()
            .unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::CodeBlockFence {
                language: Some("rust".to_string())
            }]
        );
        // Content lines have no markers
        assert_eq!(kinds(&lines[1].markers), vec![] as Vec<&MarkerKind>);
        assert_eq!(kinds(&lines[2].markers), vec![] as Vec<&MarkerKind>);
        assert_eq!(kinds(&lines[3].markers), vec![] as Vec<&MarkerKind>);
        // Closing fence
        assert_eq!(
            kinds(&lines[4].markers),
            vec![&MarkerKind::CodeBlockFence { language: None }]
        );
    }

    #[test]
    fn test_indented_code_block() {
        // Indented code blocks have no markers - detection handled separately
        let buf: Buffer = "    let x = 1;\n    let y = 2;\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_nodes_by_position(&root, &text);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);
        println!("Line 1: {:?}", lines[1].markers);

        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
        assert_eq!(kinds(&lines[1].markers), vec![] as Vec<&MarkerKind>);
    }

    #[test]
    fn test_indented_code_block_in_blockquote() {
        // Blockquote containing an indented code block - should still have blockquote marker
        let buf: Buffer = ">     code\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_nodes_by_position(&root, &text);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);

        // Should have the blockquote marker even though content is indented code
        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::BlockQuote]);
    }

    #[test]
    fn test_thematic_break() {
        let buf: Buffer = "---\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::ThematicBreak]);
    }

    #[test]
    fn test_soft_wrapped_list_item() {
        let buf: Buffer = "- First line\n  continuation\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("\n=== Soft wrapped list item ===");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Line 0 markers: {:?}", lines[0].markers);
        println!("Line 1 markers: {:?}", lines[1].markers);

        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
        // Line 2: continuation has Indent marker for the "  " prefix
        assert_eq!(kinds(&lines[1].markers), vec![&MarkerKind::Indent]);
    }

    #[test]
    fn test_multi_paragraph_list_item() {
        let buf: Buffer = "- First line\n\n  Second paragraph\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
        // Line 2: empty line - no markers
        assert_eq!(kinds(&lines[1].markers), vec![] as Vec<&MarkerKind>);
        // Line 3: second paragraph with indent
        assert_eq!(kinds(&lines[2].markers), vec![&MarkerKind::Indent]);
    }

    // ========================================================================
    // Tests for Line struct methods
    // ========================================================================

    fn make_line(range: Range<usize>, markers: Vec<Marker>) -> LineMarkers {
        LineMarkers {
            range,
            line_number: 0,
            markers,
        }
    }

    #[test]
    fn test_line_marker_range_empty() {
        let line = make_line(0..10, vec![]);
        assert_eq!(line.marker_range(), None);
    }

    #[test]
    fn test_line_marker_range_single() {
        let line = make_line(
            0..10,
            vec![Marker {
                kind: MarkerKind::ListItem { ordered: false },
                range: 0..2,
            }],
        );
        assert_eq!(line.marker_range(), Some(0..2));
    }

    #[test]
    fn test_line_marker_range_multiple() {
        // Markers are innermost to outermost (ListItem inside BlockQuote)
        let line = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 2..4,
                },
                Marker {
                    kind: MarkerKind::BlockQuote,
                    range: 0..2,
                },
            ],
        );
        assert_eq!(line.marker_range(), Some(0..4));
    }

    #[test]
    fn test_line_substitution() {
        // Markers are innermost to outermost (ListItem inside BlockQuote)
        let line = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 2..4,
                },
                Marker {
                    kind: MarkerKind::BlockQuote,
                    range: 0..2,
                },
            ],
        );
        let text = "> - Item text here";
        // Blockquote substitutes "> " with "  " (spaces), plus bullet "• "
        assert_eq!(line.substitution(text), "  • ");
    }

    #[test]
    fn test_line_substitution_task_list() {
        let text = "- [ ] Task item";
        // Markers are innermost to outermost: Checkbox is inside ListItem
        let line = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::Checkbox { checked: false },
                    range: 2..6,
                },
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 0..2,
                },
            ],
        );
        // Substitution reverses to outermost first: bullet then checkbox
        assert_eq!(line.substitution(text), "• [ ] ");
    }

    #[test]
    fn test_line_continuation() {
        let text = "> - Item text here";
        // Markers are innermost to outermost (ListItem inside BlockQuote)
        let line = make_line(
            0..18,
            vec![
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 2..4,
                },
                Marker {
                    kind: MarkerKind::BlockQuote,
                    range: 0..2,
                },
            ],
        );
        assert_eq!(line.continuation(text), "> - ");
    }

    #[test]
    fn test_line_continuation_with_indent() {
        let text = "  Second paragraph";
        let line = make_line(
            0..18,
            vec![Marker {
                kind: MarkerKind::Indent,
                range: 0..2,
            }],
        );
        // Indent marker extracts actual whitespace from text
        assert_eq!(line.continuation(text), "  ");
    }

    #[test]
    fn test_line_continuation_nested_list() {
        // Nested list: "    - Nested" where marker includes leading whitespace
        let text = "    - Nested";
        let line = make_line(
            0..12,
            vec![Marker {
                kind: MarkerKind::ListItem { ordered: false },
                range: 0..6, // "    - " includes indent
            }],
        );
        // ListItem marker extracts actual text including indent
        assert_eq!(line.continuation(text), "    - ");
    }

    #[test]
    fn test_line_has_border() {
        let line_with_quote = make_line(
            0..10,
            vec![Marker {
                kind: MarkerKind::BlockQuote,
                range: 0..2,
            }],
        );
        assert!(line_with_quote.has_border());

        let line_with_list = make_line(
            0..10,
            vec![Marker {
                kind: MarkerKind::ListItem { ordered: false },
                range: 0..2,
            }],
        );
        assert!(!line_with_list.has_border());
    }

    #[test]
    fn test_line_checkbox() {
        let line_unchecked = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 0..2,
                },
                Marker {
                    kind: MarkerKind::Checkbox { checked: false },
                    range: 2..6,
                },
            ],
        );
        assert_eq!(line_unchecked.checkbox(), Some(false));

        let line_checked = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 0..2,
                },
                Marker {
                    kind: MarkerKind::Checkbox { checked: true },
                    range: 2..6,
                },
            ],
        );
        assert_eq!(line_checked.checkbox(), Some(true));

        let line_no_checkbox = make_line(
            0..10,
            vec![Marker {
                kind: MarkerKind::ListItem { ordered: false },
                range: 0..2,
            }],
        );
        assert_eq!(line_no_checkbox.checkbox(), None);
    }

    #[test]
    fn test_line_leading_whitespace() {
        let text = "  - Item\n";
        let line = make_line(
            0..8,
            vec![Marker {
                kind: MarkerKind::ListItem { ordered: false },
                range: 2..4,
            }],
        );
        assert_eq!(line.leading_whitespace(text), "  ");
    }

    #[test]
    fn test_line_leading_whitespace_none() {
        let text = "- Item\n";
        let line = make_line(
            0..6,
            vec![Marker {
                kind: MarkerKind::ListItem { ordered: false },
                range: 0..2,
            }],
        );
        assert_eq!(line.leading_whitespace(text), "");
    }

    #[test]
    fn test_nested_list() {
        let buf: Buffer = "- First\n    - Nested\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        print_nodes_by_position(&root, &text);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);
        println!("Line 1: {:?}", lines[1].markers);

        // Line 1: "    - Nested" - nested list item
        // block_continuation [8-10] has "  " and list_marker_minus [10-14] has "  - "
        // Together they form "    - "
        assert_eq!(
            kinds(&lines[1].markers),
            vec![
                &MarkerKind::ListItem { ordered: false },
                &MarkerKind::Indent
            ]
        );
        assert_eq!(&text[lines[1].markers[0].range.clone()], "  - ");
        assert_eq!(&text[lines[1].markers[1].range.clone()], "  ");
    }

    #[test]
    fn test_two_nested_items_same_level() {
        // Both nested items have 4-space indent, should render at same level
        let buf: Buffer = "- test\n    - hey\n    - hey\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        print_tree(&root, &text, 0);
        print_nodes_by_position(&root, &text);

        let lines = extract_lines(&buf);
        for (i, line) in lines.iter().enumerate() {
            let line_text = &text[line.range.clone()];
            let leading = line.leading_whitespace(&text);
            let sub = line.substitution(&text);
            println!(
                "Line {}: {:?}\n  markers={:?}\n  leading_whitespace={:?} substitution={:?}",
                i, line_text, line.markers, leading, sub
            );
        }

        // Both line 1 and line 2 should have the same substitution
        // (indentation is now included in substitution via Indent markers)
        assert_eq!(lines[1].substitution(&text), lines[2].substitution(&text));
    }

    #[test]
    fn test_marker_width_unordered() {
        let buf: Buffer = "- item\n".parse().unwrap();
        let lines = extract_lines(&buf);
        // "- " is 2 chars
        assert_eq!(lines[0].marker_width(), 2);
    }

    #[test]
    fn test_marker_width_ordered_single_digit() {
        let buf: Buffer = "1. item\n".parse().unwrap();
        let lines = extract_lines(&buf);
        // "1. " is 3 chars
        assert_eq!(lines[0].marker_width(), 3);
    }

    #[test]
    fn test_marker_width_ordered_double_digit() {
        // Need 10 items to get a double-digit marker (Buffer normalizes "10. " to "1. ")
        let buf: Buffer = "1. a\n2. b\n3. c\n4. d\n5. e\n6. f\n7. g\n8. h\n9. i\n10. j\n"
            .parse()
            .unwrap();
        let lines = extract_lines(&buf);
        // Line 9 (0-indexed) is "10. j" - marker is "10. " = 4 chars
        assert_eq!(lines[9].marker_width(), 4);
    }

    #[test]
    fn test_marker_width_no_marker() {
        let buf: Buffer = "just text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(lines[0].marker_width(), 0);
    }

    #[test]
    fn test_nesting_threshold_unordered() {
        // "- " is 2 chars, so 2 spaces should nest
        let buf: Buffer = "- top\n  - nested\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Line 1 should have an Indent marker (it's nested)
        assert!(
            lines[1]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::Indent))
        );
    }

    #[test]
    fn test_nesting_threshold_unordered_insufficient() {
        // "- " is 2 chars, so 1 space should NOT nest (becomes sibling)
        let buf: Buffer = "- top\n - not nested\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);
        println!("Line 1: {:?}", lines[1].markers);

        // Line 1 should NOT have an Indent marker (it's a sibling, not nested)
        assert!(
            !lines[1]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::Indent))
        );
    }

    #[test]
    fn test_nesting_threshold_ordered_single_digit() {
        // "1. " is 3 chars, so 3 spaces should nest
        let buf: Buffer = "1. top\n   - nested\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);
        println!("Line 1: {:?}", lines[1].markers);

        // Line 1 should have an Indent marker (it's nested)
        assert!(
            lines[1]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::Indent))
        );
    }

    #[test]
    fn test_nesting_threshold_ordered_double_digit() {
        // "10. " is 4 chars, so 4 spaces should nest
        let buf: Buffer = "10. top\n    - nested\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);
        println!("Line 1: {:?}", lines[1].markers);

        // Line 1 should have an Indent marker (it's nested)
        assert!(
            lines[1]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::Indent))
        );
    }

    #[test]
    fn test_nesting_threshold_ordered_triple_digit() {
        // "100. " is 5 chars, so 5 spaces should nest
        let buf: Buffer = "100. top\n     - nested\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Line 0: {:?}", lines[0].markers);
        println!("Line 1: {:?}", lines[1].markers);

        // Line 1 should have an Indent marker (it's nested)
        assert!(
            lines[1]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::Indent))
        );
    }

    // ========================================================================
    // Tests for marker detection with/without trailing whitespace
    // ========================================================================

    #[test]
    fn test_blockquote_with_space() {
        let buf: Buffer = "> text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::BlockQuote]);
    }

    #[test]
    fn test_blockquote_without_space() {
        // Does ">text" (no space after >) get recognized as blockquote?
        let buf: Buffer = ">text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '>text': {:?}", lines[0].markers);
        // Result: YES, blockquote is recognized without space
        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::BlockQuote]);
    }

    #[test]
    fn test_unordered_list_minus_with_space() {
        let buf: Buffer = "- text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
    }

    #[test]
    fn test_unordered_list_minus_without_space() {
        // Does "-text" (no space after -) get recognized as list?
        let buf: Buffer = "-text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '-text': {:?}", lines[0].markers);
        // Result: NO, list is NOT recognized without space
        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
    }

    #[test]
    fn test_unordered_list_star_with_space() {
        let buf: Buffer = "* text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
    }

    #[test]
    fn test_unordered_list_star_without_space() {
        // Does "*text" (no space after *) get recognized as list?
        let buf: Buffer = "*text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '*text': {:?}", lines[0].markers);
        // Result: NO, list is NOT recognized without space (parsed as emphasis)
        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
    }

    #[test]
    fn test_unordered_list_plus_with_space() {
        let buf: Buffer = "+ text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
    }

    #[test]
    fn test_unordered_list_plus_without_space() {
        // Does "+text" (no space after +) get recognized as list?
        let buf: Buffer = "+text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '+text': {:?}", lines[0].markers);
        // Result: NO, list is NOT recognized without space
        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
    }

    #[test]
    fn test_ordered_list_with_space() {
        let buf: Buffer = "1. text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: true }]
        );
    }

    #[test]
    fn test_ordered_list_without_space() {
        // Does "1.text" (no space after .) get recognized as list?
        let buf: Buffer = "1.text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '1.text': {:?}", lines[0].markers);
        // Result: NO, list is NOT recognized without space
        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
    }

    #[test]
    fn test_heading_with_space() {
        let buf: Buffer = "# text\n".parse().unwrap();
        let lines = extract_lines(&buf);
        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::Heading(1)]);
    }

    #[test]
    fn test_heading_without_space() {
        // Does "#text" (no space after #) get recognized as heading?
        let buf: Buffer = "#text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '#text': {:?}", lines[0].markers);
        // Result: NO, heading is NOT recognized without space
        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
    }

    #[test]
    fn test_heading_h2_without_space() {
        // Does "##text" (no space after ##) get recognized as heading?
        let buf: Buffer = "##text\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();
        print_tree(&root, &text, 0);

        let lines = extract_lines(&buf);
        println!("Markers for '##text': {:?}", lines[0].markers);
        // Result: NO, heading is NOT recognized without space
        assert_eq!(kinds(&lines[0].markers), vec![] as Vec<&MarkerKind>);
    }
}
