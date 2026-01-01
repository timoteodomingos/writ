//! Upward tree traversal for finding markers on a line.
//!
//! The core idea: probe at end of line, walk up ancestors, and the ancestor
//! node types (block_quote, list_item, etc.) tell us what markers apply.

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
pub struct Line {
    pub range: Range<usize>,
    pub line_number: usize,
    pub markers: Vec<Marker>,
}

impl Line {
    /// Returns the combined byte range of all markers, or None if no markers.
    pub fn marker_range(&self) -> Option<Range<usize>> {
        if self.markers.is_empty() {
            return None;
        }
        let start = self.markers.first()?.range.start;
        let end = self.markers.last()?.range.end;
        Some(start..end)
    }

    /// Returns the visual substitution text for all markers.
    /// E.g., "• " for unordered list, "☐ " for unchecked task.
    pub fn substitution(&self) -> String {
        self.markers.iter().map(|m| m.kind.substitution()).collect()
    }

    /// Returns the continuation text to insert on Enter.
    /// E.g., "> " for blockquote, "- " for list.
    /// For Indent markers, extracts the actual whitespace from the text.
    pub fn continuation(&self, text: &str) -> String {
        self.markers
            .iter()
            .map(|m| match &m.kind {
                MarkerKind::Indent => text[m.range.clone()].to_string(),
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
            MarkerKind::BlockQuote => "",
            MarkerKind::ListItem { ordered: false } => "• ",
            MarkerKind::ListItem { ordered: true } => "",
            MarkerKind::Checkbox { checked: false } => "☐ ",
            MarkerKind::Checkbox { checked: true } => "☑ ",
            MarkerKind::Heading(_) => "",
            MarkerKind::CodeBlockFence { .. } => "",
            MarkerKind::CodeBlockContent => "",
            MarkerKind::ThematicBreak => "",
            MarkerKind::Indent => "",
        }
    }

    /// Continuation text to insert on Enter.
    pub fn continuation(&self) -> &'static str {
        match self {
            MarkerKind::BlockQuote => "> ",
            MarkerKind::ListItem { ordered: false } => "- ",
            MarkerKind::ListItem { ordered: true } => "1. ",
            MarkerKind::Checkbox { .. } => "- [ ] ",
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
            markers.push(Marker {
                kind: MarkerKind::Indent,
                range: sib.byte_range(),
            });
        }

        match node.kind() {
            "block_continuation" => {
                // Non-empty block_continuation on this line is an Indent marker
                if node.start_byte() >= line_start && node.start_byte() < node.end_byte() {
                    markers.push(Marker {
                        kind: MarkerKind::Indent,
                        range: node.byte_range(),
                    });
                }
            }
            "block_quote" => {
                if let Some(marker_node) = node.child(0)
                    && marker_node.kind() == "block_quote_marker"
                {
                    markers.push(Marker {
                        kind: MarkerKind::BlockQuote,
                        range: marker_node.byte_range(),
                    });
                }
            }
            "list_item" => {
                // Only push if the list marker is on this line
                if let Some(marker_node) = node.child(0)
                    && marker_node.start_byte() >= line_start
                {
                    let ordered = is_ordered_list(&node);

                    // Push checkbox first (so it comes before ListItem in stack)
                    if let Some((cb_kind, cb_range)) = get_checkbox(&node) {
                        markers.push(Marker {
                            kind: cb_kind,
                            range: cb_range,
                        });
                    }

                    markers.push(Marker {
                        kind: MarkerKind::ListItem { ordered },
                        range: marker_node.byte_range(),
                    });
                }
            }
            "atx_heading" => {
                if let Some(marker_node) = node.child(0)
                    && let Some(level) = get_heading_level(&node)
                {
                    markers.push(Marker {
                        kind: MarkerKind::Heading(level),
                        range: marker_node.byte_range(),
                    });
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
                markers.push(Marker {
                    kind: MarkerKind::CodeBlockFence { language },
                    range: node.byte_range(),
                });
            }
            "info_string" => {
                let s = text[node.start_byte()..node.end_byte()].trim();
                let language = if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                };
                // Range covers the delimiter, not the info_string
                if let Some(parent) = node.parent()
                    && let Some(delim) = parent.child(0)
                {
                    markers.push(Marker {
                        kind: MarkerKind::CodeBlockFence { language },
                        range: delim.byte_range(),
                    });
                }
            }
            "thematic_break" => {
                markers.push(Marker {
                    kind: MarkerKind::ThematicBreak,
                    range: node.byte_range(),
                });
            }
            "fenced_code_block" => {
                // If we're inside a fenced_code_block but didn't hit a delimiter,
                // this is a content line
                let has_fence_marker = markers
                    .iter()
                    .any(|m| matches!(m.kind, MarkerKind::CodeBlockFence { .. }));
                if !has_fence_marker {
                    markers.push(Marker {
                        kind: MarkerKind::CodeBlockContent,
                        range: node.byte_range(),
                    });
                }
            }
            "indented_code_block" => {
                // All lines in an indented code block are content
                markers.push(Marker {
                    kind: MarkerKind::CodeBlockContent,
                    range: node.byte_range(),
                });
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

fn get_checkbox(list_item: &Node) -> Option<(MarkerKind, Range<usize>)> {
    if let Some(second) = list_item.child(1) {
        match second.kind() {
            "task_list_marker_unchecked" => {
                return Some((MarkerKind::Checkbox { checked: false }, second.byte_range()));
            }
            "task_list_marker_checked" => {
                return Some((MarkerKind::Checkbox { checked: true }, second.byte_range()));
            }
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

    fn kinds(markers: &[Marker]) -> Vec<&MarkerKind> {
        markers.iter().map(|m| &m.kind).collect()
    }

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
        assert_eq!(
            kinds(&markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
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
        assert_eq!(kinds(&markers), vec![&MarkerKind::BlockQuote]);

        // Line 2: continuation - should also just be BlockQuote
        let markers = markers_at(&root, &text, 9, 16);
        println!("Line 2 markers: {:?}", markers);
        assert_eq!(kinds(&markers), vec![&MarkerKind::BlockQuote]);
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
        assert_eq!(
            kinds(&markers),
            vec![&MarkerKind::BlockQuote, &MarkerKind::BlockQuote]
        );
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 7);
        assert_eq!(
            kinds(&markers),
            vec![
                &MarkerKind::ListItem { ordered: false },
                &MarkerKind::BlockQuote
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
            kinds(&markers),
            vec![&MarkerKind::ListItem { ordered: true }]
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
            kinds(&markers),
            vec![
                &MarkerKind::Checkbox { checked: false },
                &MarkerKind::ListItem { ordered: false },
            ]
        );

        // Line 2: checked
        let markers = markers_at(&root, &text, 11, 20);
        assert_eq!(
            kinds(&markers),
            vec![
                &MarkerKind::Checkbox { checked: true },
                &MarkerKind::ListItem { ordered: false },
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
        assert_eq!(kinds(&markers), vec![&MarkerKind::Heading(2)]);
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
            kinds(&markers),
            vec![&MarkerKind::CodeBlockFence {
                language: Some("rust".to_string())
            }]
        );

        // Line 2 (content) - CodeBlockContent marker
        let markers = markers_at(&root, &text, 8, 12);
        assert_eq!(kinds(&markers), vec![&MarkerKind::CodeBlockContent]);

        // Line 3 (closing fence) - no language
        let markers = markers_at(&root, &text, 19, 21);
        assert_eq!(
            kinds(&markers),
            vec![&MarkerKind::CodeBlockFence { language: None }]
        );
    }

    #[test]
    fn test_fenced_code_block_with_indentation() {
        // Real code with indentation - should not produce Indent markers
        let code = r#"```rust
fn main() {
    println!("hello");
}
```
"#;
        let buf: Buffer = code.parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        println!("Fenced code block with indentation:");
        println!("Text: {:?}", text);
        print_tree(&root, &text, 0);

        // Line 3: "    println!..." - indented line should NOT produce Indent marker
        // Find byte offsets for line 3
        let lines: Vec<&str> = code.lines().collect();
        println!("Lines: {:?}", lines);

        let mut offset = 0;
        for (i, line) in lines.iter().enumerate() {
            let line_start = offset;
            let line_end = offset + line.len();
            println!(
                "Line {}: bytes {}-{} {:?}",
                i + 1,
                line_start,
                line_end,
                line
            );

            let probe_pos = if line_end > line_start {
                line_end - 1
            } else {
                line_start
            };
            let markers = markers_at(&root, &text, line_start, probe_pos);
            println!("  markers: {:?}", markers);

            match i {
                0 => assert_eq!(
                    kinds(&markers),
                    vec![&MarkerKind::CodeBlockFence {
                        language: Some("rust".to_string())
                    }]
                ),
                1..=3 => assert_eq!(kinds(&markers), vec![&MarkerKind::CodeBlockContent]),
                4 => assert_eq!(
                    kinds(&markers),
                    vec![&MarkerKind::CodeBlockFence { language: None }]
                ),
                _ => {}
            }

            offset = line_end + 1; // +1 for newline
        }
    }

    #[test]
    fn test_indented_code_block() {
        let buf: Buffer = "    let x = 1;\n    let y = 2;\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Indented code blocks get CodeBlockContent marker
        let markers = markers_at(&root, &text, 0, 8);
        assert_eq!(kinds(&markers), vec![&MarkerKind::CodeBlockContent]);

        let markers = markers_at(&root, &text, 15, 22);
        assert_eq!(kinds(&markers), vec![&MarkerKind::CodeBlockContent]);
    }

    #[test]
    fn test_thematic_break() {
        let buf: Buffer = "---\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = markers_at(&root, &text, 0, 1);
        assert_eq!(kinds(&markers), vec![&MarkerKind::ThematicBreak]);
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
        assert_eq!(
            kinds(&markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );

        // Line 2: continuation within same paragraph
        // The "  " is just text the user typed, not a structural marker
        // So there are no markers on this line
        println!("\nProbing at line_start=13, probe_pos=26");
        let markers = markers_at(&root, &text, 13, 26);
        println!("markers: {:?}", markers);
        assert_eq!(kinds(&markers), vec![] as Vec<&MarkerKind>);
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
        assert_eq!(
            kinds(&markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );

        // Line 3: "  Second paragraph" - second paragraph in list item
        // block_continuation [14-16] is a sibling of paragraph [16-33]
        println!("\nProbing at line_start=14, probe_pos=31");
        let markers = markers_at(&root, &text, 14, 31);
        println!("markers: {:?}", markers);
        assert_eq!(kinds(&markers), vec![&MarkerKind::Indent]);
    }

    // ========================================================================
    // Tests for Line struct methods
    // ========================================================================

    fn make_line(range: Range<usize>, markers: Vec<Marker>) -> Line {
        Line {
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
        let line = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::BlockQuote,
                    range: 0..2,
                },
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 2..4,
                },
            ],
        );
        assert_eq!(line.marker_range(), Some(0..4));
    }

    #[test]
    fn test_line_substitution() {
        let line = make_line(
            0..15,
            vec![
                Marker {
                    kind: MarkerKind::BlockQuote,
                    range: 0..2,
                },
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 2..4,
                },
            ],
        );
        assert_eq!(line.substitution(), "• ");
    }

    #[test]
    fn test_line_substitution_task_list() {
        let line = make_line(
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
        assert_eq!(line.substitution(), "• ☐ ");
    }

    #[test]
    fn test_line_continuation() {
        let text = "> - Item text here";
        let line = make_line(
            0..18,
            vec![
                Marker {
                    kind: MarkerKind::BlockQuote,
                    range: 0..2,
                },
                Marker {
                    kind: MarkerKind::ListItem { ordered: false },
                    range: 2..4,
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
}
