use std::ops::Range;

use crate::{
    buffer::Buffer,
    parser::{MarkdownCursor, MarkdownTree},
};
use tree_sitter::Node;

/// A text style to apply during rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
}

impl TextStyle {
    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Default::default()
        }
    }

    pub fn italic() -> Self {
        Self {
            italic: true,
            ..Default::default()
        }
    }

    pub fn code() -> Self {
        Self {
            code: true,
            ..Default::default()
        }
    }

    pub fn strikethrough() -> Self {
        Self {
            strikethrough: true,
            ..Default::default()
        }
    }

    /// Merge another style into this one.
    pub fn merge(&self, other: &TextStyle) -> Self {
        Self {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            code: self.code || other.code,
            strikethrough: self.strikethrough || other.strikethrough,
        }
    }
}

/// A span of text to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSpan {
    /// The text content to display
    pub text: String,
    /// The style to apply
    pub style: TextStyle,
    /// The byte range in the original buffer this span came from
    pub buffer_range: Range<usize>,
}

/// Compute the render spans for a buffer given a cursor position.
///
/// This walks the tree-sitter AST and determines:
/// - Which bytes are visible (markers hidden when cursor is away)
/// - What styles apply to each span
///
/// The cursor position determines marker visibility:
/// - If cursor is inside a styled span (including markers), show markers
/// - If cursor is outside, hide markers but apply styling to content
pub fn compute_render_spans(buffer: &Buffer, cursor_offset: usize) -> Vec<RenderSpan> {
    let text = buffer.text();

    let Some(tree) = buffer.tree() else {
        // No parse tree, return plain text
        return vec![RenderSpan {
            text: text.clone(),
            style: TextStyle::default(),
            buffer_range: 0..text.len(),
        }];
    };

    let mut styled_regions: Vec<StyledRegion> = Vec::new();

    // Walk the block tree to find inline nodes
    let mut block_cursor = tree.walk();
    collect_inline_styles(
        tree,
        &text,
        &mut block_cursor,
        cursor_offset,
        &mut styled_regions,
    );

    // Collect all boundary points where styles might change
    // Include both full_range boundaries (for marker visibility) and content_range boundaries
    let mut boundaries: Vec<usize> = Vec::new();
    boundaries.push(0);
    boundaries.push(text.len());

    for region in &styled_regions {
        let cursor_inside =
            cursor_offset >= region.full_range.start && cursor_offset <= region.full_range.end;

        if cursor_inside {
            // Show markers - use full range
            boundaries.push(region.full_range.start);
            boundaries.push(region.full_range.end);
        } else {
            // Hide markers - use content range, but also mark full_range for hiding
            boundaries.push(region.full_range.start);
            boundaries.push(region.content_range.start);
            boundaries.push(region.content_range.end);
            boundaries.push(region.full_range.end);
        }
    }

    boundaries.sort();
    boundaries.dedup();

    // Build spans between consecutive boundaries
    let mut spans = Vec::new();

    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];

        if start >= end || start >= text.len() {
            continue;
        }

        let end = end.min(text.len());

        // Check if this range is hidden (inside a marker area when cursor is outside)
        let mut is_hidden = false;
        for region in &styled_regions {
            let cursor_inside =
                cursor_offset >= region.full_range.start && cursor_offset <= region.full_range.end;

            if !cursor_inside {
                // Markers are hidden - check if this byte range overlaps the marker area
                let in_opening_marker = start >= region.full_range.start
                    && start < region.content_range.start
                    && end <= region.content_range.start;
                let in_closing_marker = start >= region.content_range.end
                    && start < region.full_range.end
                    && end <= region.full_range.end;

                if in_opening_marker || in_closing_marker {
                    is_hidden = true;
                    break;
                }
            }
        }

        if is_hidden {
            continue;
        }

        // Compute merged style for this range
        let mut style = TextStyle::default();
        for region in &styled_regions {
            let cursor_inside =
                cursor_offset >= region.full_range.start && cursor_offset <= region.full_range.end;

            let style_range = if cursor_inside {
                &region.full_range
            } else {
                &region.content_range
            };

            if style_range.start <= start && end <= style_range.end {
                style = style.merge(&region.style);
            }
        }

        spans.push(RenderSpan {
            text: text[start..end].to_string(),
            style,
            buffer_range: start..end,
        });
    }

    // Filter out empty spans
    spans.retain(|s| !s.text.is_empty());

    spans
}

/// A styled region found in the AST.
#[derive(Debug, Clone)]
struct StyledRegion {
    /// The full range including markers (e.g., "**bold**")
    full_range: Range<usize>,
    /// The content range without markers (e.g., "bold")
    content_range: Range<usize>,
    /// The style to apply
    style: TextStyle,
}

/// Walk the tree and collect styled regions from inline content.
fn collect_inline_styles(
    tree: &MarkdownTree,
    text: &str,
    cursor: &mut MarkdownCursor,
    _cursor_offset: usize,
    regions: &mut Vec<StyledRegion>,
) {
    loop {
        let node = cursor.node();

        // Check if this block node has an inline tree
        if (node.kind() == "inline" || node.kind() == "heading_content")
            && let Some(inline_tree) = tree.inline_tree(&node)
        {
            collect_inline_nodes(inline_tree.root_node(), text, regions);
        }

        // Recurse into children
        if cursor.goto_first_child() {
            collect_inline_styles(tree, text, cursor, _cursor_offset, regions);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Collect styled regions from an inline tree node.
fn collect_inline_nodes(node: Node, text: &str, regions: &mut Vec<StyledRegion>) {
    match node.kind() {
        "emphasis" => {
            if let Some(region) = extract_emphasis_region(node, text, TextStyle::italic()) {
                regions.push(region);
            }
            // Also recurse into children for nested styles
            recurse_into_children(node, text, regions);
        }
        "strong_emphasis" => {
            if let Some(region) = extract_emphasis_region(node, text, TextStyle::bold()) {
                regions.push(region);
            }
            // Also recurse into children for nested styles
            recurse_into_children(node, text, regions);
        }
        "code_span" => {
            if let Some(region) = extract_code_span_region(node, text) {
                regions.push(region);
            }
            // Code spans typically don't have nested styles, but recurse anyway
            recurse_into_children(node, text, regions);
        }
        "strikethrough" => {
            if let Some(region) = extract_emphasis_region(node, text, TextStyle::strikethrough()) {
                regions.push(region);
            }
            // Also recurse into children for nested styles
            recurse_into_children(node, text, regions);
        }
        _ => {
            // Recurse into children for other node types
            recurse_into_children(node, text, regions);
        }
    }
}

/// Helper to recurse into child nodes.
fn recurse_into_children(node: Node, text: &str, regions: &mut Vec<StyledRegion>) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_inline_nodes(child, text, regions);
        }
    }
}

/// Extract a styled region from an emphasis-like node (emphasis, strong_emphasis, strikethrough).
fn extract_emphasis_region(node: Node, _text: &str, style: TextStyle) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    // Find delimiter children to determine content bounds.
    // For strong_emphasis (**bold**), there are multiple delimiter children:
    // e.g., "*" at 6-7, "*" at 7-8 for opening, "*" at 12-13, "*" at 13-14 for closing.
    // We need to find where all opening delimiters end and where closing delimiters start.
    let mut content_start = full_start;
    let mut content_end = full_end;

    // Collect all delimiter positions
    let mut delimiters: Vec<(usize, usize)> = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let kind = child.kind();
            if kind == "emphasis_delimiter" || kind.ends_with("_delimiter") {
                delimiters.push((child.start_byte(), child.end_byte()));
            }
        }
    }

    // Opening delimiters are contiguous from the start
    for &(start, end) in &delimiters {
        if start == content_start {
            content_start = end;
        }
    }

    // Closing delimiters are contiguous to the end
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

/// Extract a styled region from a code_span node.
fn extract_code_span_region(node: Node, _text: &str) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    // Find code_span_delimiter children
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_no_styles() {
        let buf: Buffer = "hello world".parse().unwrap();
        let spans = compute_render_spans(&buf, 0);

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello world");
        assert_eq!(spans[0].style, TextStyle::default());
    }

    #[test]
    fn test_bold_cursor_outside() {
        let buf: Buffer = "hello **bold** world".parse().unwrap();
        // Cursor at start, outside the bold span
        let spans = compute_render_spans(&buf, 0);

        // Should have: "hello " + "bold" (styled, markers hidden) + " world"
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "hello ");
        assert_eq!(spans[1].text, "bold");
        assert!(spans[1].style.bold);
        assert_eq!(spans[2].text, " world");
    }

    #[test]
    fn test_bold_cursor_inside() {
        let buf: Buffer = "hello **bold** world".parse().unwrap();
        // Cursor inside the bold span (offset 8 is in "bold")
        let spans = compute_render_spans(&buf, 10);

        // Should have: "hello " + "**bold**" (styled, markers shown) + " world"
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "hello ");
        assert_eq!(spans[1].text, "**bold**");
        assert!(spans[1].style.bold);
        assert_eq!(spans[2].text, " world");
    }

    #[test]
    fn test_italic_cursor_outside() {
        let buf: Buffer = "hello *italic* world".parse().unwrap();
        let spans = compute_render_spans(&buf, 0);

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "hello ");
        assert_eq!(spans[1].text, "italic");
        assert!(spans[1].style.italic);
        assert_eq!(spans[2].text, " world");
    }

    #[test]
    fn test_code_cursor_outside() {
        let buf: Buffer = "hello `code` world".parse().unwrap();
        let spans = compute_render_spans(&buf, 0);

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "hello ");
        assert_eq!(spans[1].text, "code");
        assert!(spans[1].style.code);
        assert_eq!(spans[2].text, " world");
    }

    #[test]
    fn test_multiple_styles() {
        let buf: Buffer = "**bold** and *italic*".parse().unwrap();
        let spans = compute_render_spans(&buf, 100); // cursor far away

        // "bold" + " and " + "italic"
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "bold");
        assert!(spans[0].style.bold);
        assert_eq!(spans[1].text, " and ");
        assert_eq!(spans[2].text, "italic");
        assert!(spans[2].style.italic);
    }

    #[test]
    fn test_cursor_at_marker_edge() {
        let buf: Buffer = "hello **bold** world".parse().unwrap();
        // Cursor right at the opening **
        let spans = compute_render_spans(&buf, 6);

        // Should show markers since cursor is at edge
        assert_eq!(spans[1].text, "**bold**");
    }

    #[test]
    fn test_nested_styles_cursor_outside() {
        let buf: Buffer = "*italic **bold***".parse().unwrap();
        let spans = compute_render_spans(&buf, 100); // cursor far away

        // With cursor outside, markers hidden:
        // "italic " (italic) + "bold" (bold+italic)
        // The outer italic covers everything, inner bold covers "bold"
        for span in &spans {
            eprintln!(
                "span: {:?} bold={} italic={}",
                span.text, span.style.bold, span.style.italic
            );
        }

        // Find the "bold" part - it should have both bold and italic
        let bold_span = spans.iter().find(|s| s.text.contains("bold")).unwrap();
        assert!(bold_span.style.bold, "bold text should be bold");
        assert!(
            bold_span.style.italic,
            "bold text inside italic should also be italic"
        );

        // Find the "italic " part (without "bold") - should only be italic
        let italic_span = spans
            .iter()
            .find(|s| s.text.contains("italic") && !s.text.contains("bold"))
            .unwrap();
        assert!(italic_span.style.italic, "italic text should be italic");
        assert!(
            !italic_span.style.bold,
            "italic-only text should not be bold"
        );
    }
}
