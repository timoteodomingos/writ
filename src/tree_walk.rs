//! Upward tree traversal for finding markers on a line.
//!
//! The core idea: probe at end of line, walk up ancestors, and the ancestor
//! node types (block_quote, list_item, etc.) tell us what markers apply.

use tree_sitter::Node;

/// A marker on a line, derived from ancestor node types.
#[derive(Debug, Clone, PartialEq)]
pub enum Marker {
    BlockQuote,
    ListItem { ordered: bool, is_first: bool },
    Checkbox { checked: bool },
    Heading(u8),
    CodeBlockFence { language: Option<String> },
    CodeBlockContent,
    ThematicBreak,
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
        match node.kind() {
            "block_quote" => markers.push(Marker::BlockQuote),
            "list_item" => {
                let is_first = node.start_byte() >= line_start;
                let ordered = is_ordered_list(&node);

                // Push checkbox first (so after reverse, it comes after ListItem)
                if let Some(cb) = get_checkbox(&node) {
                    markers.push(cb);
                }

                markers.push(Marker::ListItem { ordered, is_first });
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
            "{}{} [{}-{}] {:?}",
            spacing,
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            preview
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
        assert_eq!(
            markers,
            vec![Marker::ListItem {
                ordered: false,
                is_first: true
            }]
        );
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

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
            vec![
                Marker::ListItem {
                    ordered: false,
                    is_first: true
                },
                Marker::BlockQuote,
            ]
        );
    }

    #[test]
    fn test_ordered_list() {
        let buf: Buffer = "1. First\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 7);
        assert_eq!(
            markers,
            vec![Marker::ListItem {
                ordered: true,
                is_first: true
            }]
        );
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
                Marker::ListItem {
                    ordered: false,
                    is_first: true
                },
            ]
        );

        // Line 2: checked
        let markers = markers_at(&root, &text, 11, 20);
        assert_eq!(
            markers,
            vec![
                Marker::Checkbox { checked: true },
                Marker::ListItem {
                    ordered: false,
                    is_first: true
                },
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
}
