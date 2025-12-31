//! Upward tree traversal for finding markers on a line.
//!
//! The core idea: probe at end of line, walk up ancestors, and the ancestor
//! node types (block_quote, list_item, etc.) tell us what markers apply.

use tree_sitter::Node;

/// A marker on a line, derived from ancestor node types.
#[derive(Debug, Clone, PartialEq)]
pub enum Marker {
    BlockQuote,
    UnorderedList,
    OrderedList,
    TaskList { checked: bool },
    Heading(u8),
}

/// Find all markers for a line by probing at `probe_pos` and walking up ancestors.
/// Returns markers from outermost to innermost.
pub fn markers_at(root: &Node, probe_pos: usize) -> Vec<Marker> {
    let Some(leaf) = root.descendant_for_byte_range(probe_pos, probe_pos) else {
        return Vec::new();
    };

    let mut markers = Vec::new();
    let mut current = Some(leaf);

    while let Some(node) = current {
        match node.kind() {
            "block_quote" => markers.push(Marker::BlockQuote),
            "list_item" => {
                markers.push(marker_for_list_item(&node));
            }
            "atx_heading" => {
                // Get heading level from the marker child (atx_h1_marker, atx_h2_marker, etc.)
                if let Some(level) = get_heading_level(&node) {
                    markers.push(Marker::Heading(level));
                }
            }
            _ => {}
        }
        current = node.parent();
    }

    markers.reverse(); // outermost first
    markers
}

fn marker_for_list_item(list_item: &Node) -> Marker {
    // First child is the list marker (e.g. list_marker_minus)
    // Second child might be a task marker (task_list_marker_checked/unchecked)
    if let Some(second) = list_item.child(1) {
        match second.kind() {
            "task_list_marker_unchecked" => return Marker::TaskList { checked: false },
            "task_list_marker_checked" => return Marker::TaskList { checked: true },
            _ => {}
        }
    }

    if let Some(first) = list_item.child(0) {
        match first.kind() {
            "list_marker_dot" | "list_marker_parenthesis" => return Marker::OrderedList,
            _ => {}
        }
    }

    Marker::UnorderedList
}

fn get_heading_level(heading: &Node) -> Option<u8> {
    // First child is the marker (atx_h1_marker, atx_h2_marker, etc.)
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

    #[test]
    fn test_simple_list() {
        let buf: Buffer = "- Item\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Probe at end of line (byte 5)
        let markers = markers_at(&root, 5);
        assert_eq!(markers, vec![Marker::UnorderedList]);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 2: probe at byte 20
        let markers = markers_at(&root, 20);
        assert_eq!(markers, vec![Marker::BlockQuote, Marker::BlockQuote]);
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Probe at end of line (byte 7)
        let markers = markers_at(&root, 7);
        assert_eq!(markers, vec![Marker::BlockQuote, Marker::UnorderedList]);
    }

    #[test]
    fn test_ordered_list() {
        let buf: Buffer = "1. First\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, 7);
        assert_eq!(markers, vec![Marker::OrderedList]);
    }

    #[test]
    fn test_task_list() {
        let buf: Buffer = "- [ ] Todo\n- [x] Done\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 1: unchecked
        let markers = markers_at(&root, 9);
        assert_eq!(markers, vec![Marker::TaskList { checked: false }]);

        // Line 2: checked
        let markers = markers_at(&root, 20);
        assert_eq!(markers, vec![Marker::TaskList { checked: true }]);
    }

    #[test]
    fn test_heading() {
        let buf: Buffer = "## Heading\n".parse().unwrap();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, 9);
        assert_eq!(markers, vec![Marker::Heading(2)]);
    }
}
