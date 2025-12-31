//! Upward tree traversal for finding markers on a line.
//!
//! The core idea: probe at end of line, walk up ancestors, and the ancestor
//! node types (block_quote, list_item, etc.) tell us what markers apply.

use tree_sitter::Node;

/// A marker on a line, derived from ancestor node types.
#[derive(Debug, Clone, PartialEq)]
pub enum Marker {
    BlockQuote,
    ListItem { ordered: bool },
    Checkbox { checked: bool },
    Heading(u8),
    CodeBlockFence { language: Option<String> },
    CodeBlockContent,
    ThematicBreak,
    Indent,
}

/// Find all markers for a line by probing at `probe_pos` and walking up ancestors.
/// Returns markers from outermost to innermost.
pub fn markers_at(root: &Node, text: &str, line_start: usize, probe_pos: usize) -> Vec<Marker> {
    let Some(leaf) = root.descendant_for_byte_range(probe_pos, probe_pos) else {
        return Vec::new();
    };

    let mut markers = Vec::new();
    let mut current = Some(leaf);

    while let Some(node) = current {
        // Check if current node's prev sibling is a block_continuation
        if let Some(sib) = node.prev_sibling()
            && sib.kind() == "block_continuation"
            && sib.start_byte() >= line_start
            && sib.start_byte() < sib.end_byte()
        {
            markers.push(Marker::Indent);
        }

        match node.kind() {
            "block_continuation" => {
                // Non-empty block_continuation on this line is an Indent marker
                if node.start_byte() >= line_start && node.start_byte() < node.end_byte() {
                    markers.push(Marker::Indent);
                }
            }
            "block_quote" => {
                // Only push if the block_quote_marker is on this line
                if let Some(marker_node) = node.child(0)
                    && marker_node.kind() == "block_quote_marker"
                {
                    markers.push(Marker::BlockQuote);
                }
            }
            "list_item" => {
                // Only push if the list marker is on this line
                if let Some(marker_node) = node.child(0)
                    && marker_node.start_byte() >= line_start
                {
                    let ordered = is_ordered_list(&node);

                    // Push checkbox first (so it comes before ListItem in stack)
                    if let Some(cb) = get_checkbox(&node) {
                        markers.push(cb);
                    }

                    markers.push(Marker::ListItem { ordered });
                }
            }
            "atx_heading" => {
                if let Some(level) = get_heading_level(&node) {
                    markers.push(Marker::Heading(level));
                }
            }
            "fenced_code_block_delimiter" => {
                let language = if let Some(sibling) = node.next_sibling() {
                    if sibling.kind() == "info_string" {
                        let s = text[sibling.start_byte()..sibling.end_byte()].trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                markers.push(Marker::CodeBlockFence { language });
            }
            "info_string" => {
                let s = text[node.start_byte()..node.end_byte()].trim();
                let language = if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                };
                markers.push(Marker::CodeBlockFence { language });
            }
            "fenced_code_block" => {
                // If we didn't already push a fence marker, this is content
                if !matches!(markers.last(), Some(Marker::CodeBlockFence { .. })) {
                    markers.push(Marker::CodeBlockContent);
                }
            }
            "indented_code_block" => {
                markers.push(Marker::CodeBlockContent);
            }
            "thematic_break" => {
                markers.push(Marker::ThematicBreak);
            }
            _ => {}
        }
        current = node.parent();
    }

    markers
}

fn is_ordered_list(list_item: &Node) -> bool {
    if let Some(first) = list_item.child(0) {
        match first.kind() {
            "list_marker_dot" | "list_marker_parenthesis" => return true,
            _ => {}
        }
    }
    false
}

fn get_checkbox(list_item: &Node) -> Option<Marker> {
    if let Some(second) = list_item.child(1) {
        match second.kind() {
            "task_list_marker_unchecked" => return Some(Marker::Checkbox { checked: false }),
            "task_list_marker_checked" => return Some(Marker::Checkbox { checked: true }),
            _ => {}
        }
    }
    None
}

fn get_heading_level(heading: &Node) -> Option<u8> {
    let first = heading.child(0)?;
    let kind = first.kind();
    if kind.starts_with("atx_h") && kind.ends_with("_marker") {
        kind.chars()
            .nth(5)
            .and_then(|c| c.to_digit(10))
            .map(|d| d as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;

    #[allow(dead_code)]
    fn print_tree(node: &tree_sitter::Node, text: &str, indent: usize) {
        let spacing = "  ".repeat(indent);
        let preview: String = text[node.start_byte()..node.end_byte()]
            .chars()
            .take(20)
            .flat_map(|c| if c == '\n' { vec!['\\', 'n'] } else { vec![c] })
            .collect();
        println!(
            "{}{} [{}-{}] {:?} (named: {}, child_count: {})",
            spacing,
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            preview,
            node.is_named(),
            node.child_count()
        );
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                print_tree(&child, text, indent + 1);
            }
        }
    }

    #[test]
    fn test_simple_list() {
        let buf: Buffer = "- Item\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 5);
        assert_eq!(markers, vec![Marker::ListItem { ordered: false }]);
    }

    #[test]
    fn test_multiline_blockquote() {
        // Same blockquote spanning two lines
        let buf: Buffer = "> Line 1\n> Line 2\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("Multiline blockquote:");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 1: has the block_quote_marker
        let markers = markers_at(&root, &text, 0, 7);
        println!("Line 1 markers: {:?}", markers);
        assert_eq!(markers, vec![Marker::BlockQuote]);

        // Line 2: continuation - should also just be BlockQuote
        let markers = markers_at(&root, &text, 9, 16);
        println!("Line 2 markers: {:?}", markers);
        assert_eq!(markers, vec![Marker::BlockQuote]);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("Nested blockquote:");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 2: probe at byte 20, line starts at byte 10
        let markers = markers_at(&root, &text, 10, 20);
        assert_eq!(markers, vec![Marker::BlockQuote, Marker::BlockQuote]);
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 7);
        assert_eq!(
            markers,
            vec![Marker::ListItem { ordered: false }, Marker::BlockQuote,]
        );
    }

    #[test]
    fn test_ordered_list() {
        let buf: Buffer = "1. First\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 7);
        assert_eq!(markers, vec![Marker::ListItem { ordered: true }]);
    }

    #[test]
    fn test_task_list() {
        let buf: Buffer = "- [ ] Todo\n- [x] Done\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 1: unchecked (stack order: innermost first)
        let markers = markers_at(&root, &text, 0, 9);
        assert_eq!(
            markers,
            vec![
                Marker::Checkbox { checked: false },
                Marker::ListItem { ordered: false },
            ]
        );

        // Line 2: checked
        let markers = markers_at(&root, &text, 11, 20);
        assert_eq!(
            markers,
            vec![
                Marker::Checkbox { checked: true },
                Marker::ListItem { ordered: false },
            ]
        );
    }

    #[test]
    fn test_heading() {
        let buf: Buffer = "## Heading\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 9);
        assert_eq!(markers, vec![Marker::Heading(2)]);
    }

    #[test]
    fn test_fenced_code_block() {
        let buf: Buffer = "```rust\nlet x = 1;\n```\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("Fenced code block:");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 1 (opening fence)
        let markers = markers_at(&root, &text, 0, 5);
        assert_eq!(
            markers,
            vec![Marker::CodeBlockFence {
                language: Some("rust".to_string())
            }]
        );

        // Line 2 (content)
        let markers = markers_at(&root, &text, 8, 12);
        assert_eq!(markers, vec![Marker::CodeBlockContent]);

        // Line 3 (closing fence) - no language
        let markers = markers_at(&root, &text, 19, 21);
        assert_eq!(markers, vec![Marker::CodeBlockFence { language: None }]);
    }

    #[test]
    fn test_indented_code_block() {
        let buf: Buffer = "    let x = 1;\n    let y = 2;\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 8);
        assert_eq!(markers, vec![Marker::CodeBlockContent]);

        let markers = markers_at(&root, &text, 15, 22);
        assert_eq!(markers, vec![Marker::CodeBlockContent]);
    }

    #[test]
    fn test_thematic_break() {
        let buf: Buffer = "---\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 1);
        assert_eq!(markers, vec![Marker::ThematicBreak]);
    }

    #[test]
    fn test_soft_wrapped_list_item() {
        // Soft wrap within same paragraph - no blank line
        let buf: Buffer = "- First line\n  continuation\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("Soft wrapped list item:");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 1: has the marker (bytes 0-12)
        let markers = markers_at(&root, &text, 0, 11);
        assert_eq!(markers, vec![Marker::ListItem { ordered: false }]);

        // Line 2: continuation within same paragraph
        // The "  " is just text the user typed, not a structural marker
        // So there are no markers on this line
        println!("\nProbing at line_start=13, probe_pos=26");
        let markers = markers_at(&root, &text, 13, 26);
        println!("markers: {:?}", markers);
        assert_eq!(markers, vec![]);
    }

    #[test]
    fn test_multi_paragraph_list_item() {
        // Two paragraphs in same list item - blank line between them
        let buf: Buffer = "- First line\n\n  Second paragraph\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("Multi-paragraph list item:");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 1: has the marker
        let markers = markers_at(&root, &text, 0, 11);
        assert_eq!(markers, vec![Marker::ListItem { ordered: false }]);

        // Line 3: "  Second paragraph" - second paragraph in list item
        // block_continuation [14-16] is a sibling of paragraph [16-33]
        println!("\nProbing at line_start=14, probe_pos=31");
        let markers = markers_at(&root, &text, 14, 31);
        println!("markers: {:?}", markers);
        assert_eq!(markers, vec![Marker::Indent]);
    }
}
