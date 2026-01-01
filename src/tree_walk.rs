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
        // Markers are ordered innermost to outermost.
        // Outermost (last) has earliest start, innermost (first) has latest end.
        let start = self.markers.last()?.range.start;
        let end = self.markers.first()?.range.end;
        Some(start..end)
    }

    /// Returns the visual substitution text for all markers.
    /// E.g., "• " for unordered list, "☐ " for unchecked task.
    pub fn substitution(&self) -> String {
        self.markers.iter().map(|m| m.kind.substitution()).collect()
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

/// Find all markers for a line by probing at `probe_pos` and walking up ancestors.
/// Returns markers from outermost to innermost.
pub fn markers_at(root: &Node, text: &str, line_start: usize, probe_pos: usize) -> Vec<Marker> {
    let Some(leaf) = root.descendant_for_byte_range(probe_pos, probe_pos) else {
        return Vec::new();
    };

    let mut markers = Vec::new();
    let mut current = Some(leaf);

    while let Some(node) = current {
        // Check if current node's prev sibling is a block_continuation with only whitespace
        if let Some(sib) = node.prev_sibling()
            && sib.kind() == "block_continuation"
            && sib.start_byte() >= line_start
            && sib.start_byte() < sib.end_byte()
        {
            let content = &text[sib.byte_range()];
            if content.chars().all(|c| c.is_whitespace()) {
                markers.push(Marker {
                    kind: MarkerKind::Indent,
                    range: sib.byte_range(),
                });
            }
        }

        match node.kind() {
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
    use crate::lines::extract_lines;

    fn kinds(markers: &[Marker]) -> Vec<&MarkerKind> {
        markers.iter().map(|m| &m.kind).collect()
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

        assert_eq!(kinds(&lines[0].markers), vec![&MarkerKind::BlockQuote]);
        assert_eq!(kinds(&lines[1].markers), vec![&MarkerKind::BlockQuote]);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

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

        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::CodeBlockFence {
                language: Some("rust".to_string())
            }]
        );
        assert_eq!(
            kinds(&lines[1].markers),
            vec![&MarkerKind::CodeBlockContent]
        );
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
        // Lines 1-3 are content
        assert_eq!(
            kinds(&lines[1].markers),
            vec![&MarkerKind::CodeBlockContent]
        );
        assert_eq!(
            kinds(&lines[2].markers),
            vec![&MarkerKind::CodeBlockContent]
        );
        assert_eq!(
            kinds(&lines[3].markers),
            vec![&MarkerKind::CodeBlockContent]
        );
        // Line 4 is closing fence
        assert_eq!(
            kinds(&lines[4].markers),
            vec![&MarkerKind::CodeBlockFence { language: None }]
        );
    }

    #[test]
    fn test_indented_code_block() {
        let buf: Buffer = "    let x = 1;\n    let y = 2;\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::CodeBlockContent]
        );
        assert_eq!(
            kinds(&lines[1].markers),
            vec![&MarkerKind::CodeBlockContent]
        );
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
        let lines = extract_lines(&buf);

        assert_eq!(
            kinds(&lines[0].markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
        // Line 2: continuation within same paragraph - no markers
        assert_eq!(kinds(&lines[1].markers), vec![] as Vec<&MarkerKind>);
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

        // Line 2: "    - Nested" - nested list item (bytes 8-20)
        // Tree structure:
        //   block_continuation [8-10] "  " (belongs to parent paragraph, not this line)
        //   list_marker_minus [10-14] "  - " (2 spaces indent + "- ")
        // The nested list marker includes the indent - no separate Indent marker needed
        let markers = markers_at(&root, &text, 8, 19);
        assert_eq!(
            kinds(&markers),
            vec![&MarkerKind::ListItem { ordered: false }]
        );
        // Marker range is "  - " which includes the indent
        assert_eq!(&text[markers[0].range.clone()], "  - ");
    }
}
