use crate::buffer::Buffer;
use crate::marker::{LineMarkers, collect_nodes, markers_at};
use crate::parser::MarkdownTree;
use std::ops::Range;
use tree_sitter::Node;

// Re-export for backwards compatibility
pub use crate::line::{StyledRegion, TextStyle};

pub fn extract_lines(buffer: &Buffer) -> Vec<LineMarkers> {
    extract_lines_from_parts(&buffer.text(), buffer.tree())
}

pub fn extract_lines_from_parts(text: &str, tree: Option<&MarkdownTree>) -> Vec<LineMarkers> {
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

    // Collect nodes once for the whole buffer
    let nodes = tree.map(|t| collect_nodes(&t.block_tree().root_node()));

    // Build Line for each line
    lines
        .into_iter()
        .map(|(line_num, range)| {
            // Find markers on this line using tree_walk
            let markers = if let Some(nodes) = &nodes {
                markers_at(nodes, text, range.start, range.end)
            } else {
                Vec::new()
            };

            LineMarkers {
                range,
                line_number: line_num,
                markers,
            }
        })
        .collect()
}

pub fn extract_inline_styles(buffer: &Buffer, line: &LineMarkers) -> Vec<StyledRegion> {
    extract_inline_styles_from_parts(&buffer.text(), buffer.tree(), line)
}

pub fn extract_inline_styles_from_parts(
    text: &str,
    tree: Option<&MarkdownTree>,
    line: &LineMarkers,
) -> Vec<StyledRegion> {
    let Some(tree) = tree else {
        return Vec::new();
    };

    let mut styles = Vec::new();

    // Find the inline node that covers this line's content
    let root = tree.block_tree().root_node();
    collect_inline_styles_in_range(&root, tree, text, &line.range, &mut styles);

    styles
}

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
        "inline_link" | "full_reference_link" | "collapsed_reference_link" | "shortcut_link" => {
            if let Some(region) = extract_link_region(&node, text) {
                styles.push(region);
            }
        }
        "image" => {
            if let Some(region) = extract_image_region(&node, text) {
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
        link_url: None,
        is_image: false,
    })
}

fn extract_code_span_region(node: &Node) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "code_span_delimiter"
        {
            if child.start_byte() == full_start {
                content_start = child.end_byte();
            } else if child.end_byte() == full_end {
                content_end = child.start_byte();
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::code(),
        link_url: None,
        is_image: false,
    })
}

fn extract_link_region(node: &Node, text: &str) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;
    let mut url: Option<String> = None;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "link_text" => {
                    content_start = child.start_byte();
                    content_end = child.end_byte();
                }
                "link_destination" => {
                    url = Some(text[child.start_byte()..child.end_byte()].to_string());
                }
                _ => {}
            }
        }
    }

    if url.is_none() {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "[" {
                    content_start = child.end_byte();
                } else if child.kind() == "]" {
                    content_end = child.start_byte();
                }
            }
        }
    }

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::default(),
        link_url: url,
        is_image: false,
    })
}

fn extract_image_region(node: &Node, text: &str) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut alt_start = full_start;
    let mut alt_end = full_end;
    let mut url: Option<String> = None;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "image_description" => {
                    alt_start = child.start_byte();
                    alt_end = child.end_byte();
                }
                "link_destination" => {
                    url = Some(text[child.start_byte()..child.end_byte()].to_string());
                }
                _ => {}
            }
        }
    }

    let url = url?;

    let content_start = alt_start;
    let content_end = alt_end;

    Some(StyledRegion {
        full_range: full_start..full_end,
        content_range: content_start..content_end,
        style: TextStyle::default(),
        link_url: Some(url),
        is_image: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marker::MarkerKind;

    #[test]
    fn test_extract_lines_empty_buffer() {
        let buf: Buffer = "".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].range, 0..0);
        assert!(lines[0].markers.is_empty());
    }

    #[test]
    fn test_extract_lines_single_newline() {
        let buf: Buffer = "\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].range, 0..0);
        assert_eq!(lines[1].range, 1..1);
    }

    #[test]
    fn test_extract_lines_paragraph() {
        let buf: Buffer = "Hello\n\nWorld\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].range, 0..5);
        assert_eq!(lines[1].range, 6..6); // blank line
        assert_eq!(lines[2].range, 7..12);
    }

    #[test]
    fn test_heading_markers() {
        let buf: Buffer = "# Hello\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].heading_level(), Some(1));
        // Marker range includes "# " (with trailing space)
        assert_eq!(lines[0].marker_range(), Some(0..2));
    }

    #[test]
    fn test_heading_levels() {
        let buf: Buffer = "# H1\n## H2\n### H3\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].heading_level(), Some(1));
        assert_eq!(lines[1].heading_level(), Some(2));
        assert_eq!(lines[2].heading_level(), Some(3));
    }

    #[test]
    fn test_unordered_list_markers() {
        let buf: Buffer = "- Item 1\n- Item 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { ordered: false }))
        );
        assert_eq!(lines[0].marker_range(), Some(0..2));
    }

    #[test]
    fn test_ordered_list_markers() {
        let buf: Buffer = "1. First\n2. Second\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { ordered: true }))
        );
    }

    #[test]
    fn test_checkbox_markers() {
        let buf: Buffer = "- [ ] Unchecked\n- [x] Checked\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].checkbox(), Some(false));
        assert_eq!(lines[1].checkbox(), Some(true));
    }

    #[test]
    fn test_blockquote_markers() {
        let buf: Buffer = "> Quote\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(lines[0].has_border());
        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::BlockQuote))
        );
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // First line has one blockquote marker
        assert_eq!(
            lines[0]
                .markers
                .iter()
                .filter(|m| matches!(m.kind, MarkerKind::BlockQuote))
                .count(),
            1
        );
        // Second line has two blockquote markers
        assert_eq!(
            lines[1]
                .markers
                .iter()
                .filter(|m| matches!(m.kind, MarkerKind::BlockQuote))
                .count(),
            2
        );
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Should have both blockquote and list markers
        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::BlockQuote))
        );
        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ListItem { .. }))
        );
    }

    #[test]
    fn test_code_block_fence() {
        let buf: Buffer = "```rust\nlet x = 1;\n```\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(lines[0].is_fence());
        // Content lines don't have markers (code block detection handled separately)
        assert!(!lines[1].is_fence());
        assert!(lines[2].is_fence());
    }

    #[test]
    fn test_thematic_break() {
        let buf: Buffer = "---\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(
            lines[0]
                .markers
                .iter()
                .any(|m| matches!(m.kind, MarkerKind::ThematicBreak))
        );
    }

    #[test]
    fn test_nested_list_continuation() {
        let buf: Buffer = "- Item 1\n  - Nested\n".parse().unwrap();
        let text = buf.text();
        let lines = extract_lines(&buf);

        // Line 1: "  - Nested" - continuation should include leading whitespace and marker
        let continuation = lines[1].continuation(&text);
        assert!(continuation.contains("- "));
    }

    #[test]
    fn test_substitution() {
        let buf: Buffer = "- Item\n".parse().unwrap();
        let text = buf.text();
        let lines = extract_lines(&buf);

        // Unordered list should substitute with bullet
        let sub = lines[0].substitution(&text);
        assert!(sub.contains('•') || sub.contains('-'));
    }

    #[test]
    fn test_list_in_blockquote_continuation() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let text = buf.text();
        let lines = extract_lines(&buf);

        // Continuation should be "> - " (one blockquote marker, one list marker)
        let continuation = lines[0].continuation(&text);
        assert_eq!(continuation, "> - ");
    }

    #[test]
    fn test_multiline_blockquote_with_list_continuation() {
        // Multi-line blockquote with list on line 3
        let buf: Buffer = "> hey\n>\n> - foo\n".parse().unwrap();
        let text = buf.text();
        let lines = extract_lines(&buf);

        println!("Lines:");
        for (i, line) in lines.iter().enumerate() {
            println!(
                "  {}: {:?} markers: {:?}",
                i,
                &text[line.range.clone()],
                line.markers
            );
        }

        // Line 3 ("> - foo") continuation should be "> - "
        let continuation = lines[2].continuation(&text);
        assert_eq!(continuation, "> - ");
    }

    #[test]
    fn test_code_block_in_blockquote() {
        let buf: Buffer = "> ```rust\n> fn main() {}\n> ```\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // First line should have both BlockQuote and CodeBlockFence markers
        assert!(lines[0].is_fence());
        assert!(lines[0].has_border());
    }
}

#[test]
fn test_code_block_fence_detection() {
    // Simple code block
    let buf: Buffer = "```rust\nfn main() {}\n```\n".parse().unwrap();
    let lines = extract_lines(&buf);

    println!("Simple code block:");
    for (i, line) in lines.iter().enumerate() {
        println!("  Line {}: is_fence={}", i, line.is_fence());
    }

    assert!(lines[0].is_fence(), "Line 0 should be fence");
    assert!(!lines[1].is_fence(), "Line 1 should not be fence");
    assert!(lines[2].is_fence(), "Line 2 should be fence");

    // Code block in blockquote
    let buf2: Buffer = "> ```rust\n> fn main() {}\n> ```\n".parse().unwrap();
    let lines2 = extract_lines(&buf2);

    println!("\nCode block in blockquote:");
    for (i, line) in lines2.iter().enumerate() {
        println!("  Line {}: is_fence={}", i, line.is_fence());
    }

    assert!(lines2[0].is_fence(), "Line 0 should be fence");
    assert!(!lines2[1].is_fence(), "Line 1 should not be fence");
    assert!(lines2[2].is_fence(), "Line 2 should be fence");
}

#[test]
fn test_code_block_content_markers() {
    // Code block in blockquote - check content line markers
    let buf: Buffer = "> ```rust\n> fn main() {}\n> ```\n".parse().unwrap();
    let lines = extract_lines(&buf);

    println!("Content line (line 1) markers: {:?}", lines[1].markers);
    println!(
        "Content line substitution: {:?}",
        lines[1].substitution(&buf.text())
    );
}

#[test]
fn test_simple_code_block_content_markers() {
    // Simple code block - check content line markers
    let buf: Buffer = "```rust\nfn main() {}\n```\n".parse().unwrap();
    let lines = extract_lines(&buf);

    println!("Content line (line 1) markers: {:?}", lines[1].markers);
    println!(
        "Content line substitution: {:?}",
        lines[1].substitution(&buf.text())
    );
    println!("Content line range: {:?}", lines[1].range);
    println!(
        "Content line text: {:?}",
        &buf.text()[lines[1].range.clone()]
    );
}

#[test]
fn test_cursor_in_code_block_detection() {
    use crate::marker::LineMarkers;

    fn compute_code_block_ranges(lines: &[LineMarkers]) -> Vec<(usize, Option<usize>)> {
        let mut ranges = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            if lines[i].is_fence() {
                let start = i;
                i += 1;
                let mut found_close = false;

                while i < lines.len() {
                    if lines[i].is_fence() {
                        ranges.push((start, Some(i)));
                        i += 1;
                        found_close = true;
                        break;
                    }
                    i += 1;
                }

                if !found_close {
                    ranges.push((start, None));
                }
            } else {
                i += 1;
            }
        }

        ranges
    }

    fn cursor_in_code_block(lines: &[LineMarkers], cursor_line: usize) -> bool {
        let ranges = compute_code_block_ranges(lines);
        for (start, end) in ranges {
            let block_end = end.unwrap_or(lines.len().saturating_sub(1));
            if cursor_line >= start && cursor_line <= block_end {
                return true;
            }
        }
        false
    }

    // Simple code block
    let buf: Buffer = "```rust\nfn main() {}\n```\n".parse().unwrap();
    let lines = extract_lines(&buf);

    println!("Simple code block:");
    let ranges = compute_code_block_ranges(&lines);
    println!("  Ranges: {:?}", ranges);
    println!("  Line 0 in block: {}", cursor_in_code_block(&lines, 0));
    println!("  Line 1 in block: {}", cursor_in_code_block(&lines, 1));
    println!("  Line 2 in block: {}", cursor_in_code_block(&lines, 2));

    assert!(cursor_in_code_block(&lines, 0));
    assert!(cursor_in_code_block(&lines, 1));
    assert!(cursor_in_code_block(&lines, 2));

    // Code block in blockquote
    let buf2: Buffer = "> ```rust\n> fn main() {}\n> ```\n".parse().unwrap();
    let lines2 = extract_lines(&buf2);

    println!("\nCode block in blockquote:");
    let ranges2 = compute_code_block_ranges(&lines2);
    println!("  Ranges: {:?}", ranges2);
    println!("  Line 0 in block: {}", cursor_in_code_block(&lines2, 0));
    println!("  Line 1 in block: {}", cursor_in_code_block(&lines2, 1));
    println!("  Line 2 in block: {}", cursor_in_code_block(&lines2, 2));

    assert!(cursor_in_code_block(&lines2, 0));
    assert!(cursor_in_code_block(&lines2, 1));
    assert!(cursor_in_code_block(&lines2, 2));
}

#[test]
fn test_code_block_after_tab() {
    // Simulate: type ```rust, enter, tab, type "fn foo()"
    let text = "```rust\n    fn foo()\n```\n";
    let buf: Buffer = text.parse().unwrap();
    let lines = extract_lines(&buf);

    println!("Text: {:?}", text);
    println!("\nLines:");
    for (i, line) in lines.iter().enumerate() {
        let line_text = &text[line.range.clone()];
        println!("  Line {}: range={:?} text={:?}", i, line.range, line_text);
        println!("    markers: {:?}", line.markers);
        println!("    is_fence: {}", line.is_fence());
        println!("    substitution: {:?}", line.substitution(text));
        println!("    marker_range: {:?}", line.marker_range());
    }

    // Print tree structure
    if let Some(tree) = buf.tree() {
        println!("\nBlock tree:");
        fn print_node(node: tree_sitter::Node, text: &str, indent: usize) {
            let spacing = "  ".repeat(indent);
            let preview: String = text
                [node.start_byte()..node.end_byte().min(node.start_byte() + 30)]
                .chars()
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
                    print_node(child, text, indent + 1);
                }
            }
        }
        print_node(tree.block_tree().root_node(), text, 0);
    }
}
