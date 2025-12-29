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
    /// Heading level (1-6), or 0 for non-heading text
    pub heading_level: u8,
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

    /// Create a heading style.
    pub fn heading(level: u8) -> Self {
        Self {
            heading_level: level,
            bold: true, // Headings are bold
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
            heading_level: self.heading_level.max(other.heading_level),
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

/// Convert a visual offset (in rendered text) to a buffer offset.
///
/// The `spans` should be computed with the current cursor position.
/// Returns the buffer byte offset corresponding to the visual position.
pub fn visual_to_buffer_offset(spans: &[RenderSpan], visual_offset: usize) -> usize {
    let mut visual_pos = 0;

    for span in spans {
        let span_visual_len = span.text.len();
        if visual_pos + span_visual_len > visual_offset {
            // Click is within this span
            let offset_in_span = visual_offset - visual_pos;
            return span.buffer_range.start + offset_in_span.min(span.text.len());
        }
        visual_pos += span_visual_len;
    }

    // Past the end - return end of last span or 0
    spans.last().map(|s| s.buffer_range.end).unwrap_or(0)
}

/// Convert a buffer offset to a visual offset (in rendered text).
///
/// The `spans` should be computed with the cursor at `buffer_offset`.
/// Returns the visual position corresponding to the buffer offset.
pub fn buffer_to_visual_offset(spans: &[RenderSpan], buffer_offset: usize) -> usize {
    let mut visual_pos = 0;

    for span in spans {
        if buffer_offset <= span.buffer_range.start {
            // Cursor is before this span
            break;
        } else if buffer_offset <= span.buffer_range.end {
            // Cursor is within this span
            let offset_in_buffer = buffer_offset - span.buffer_range.start;
            visual_pos += offset_in_buffer.min(span.text.len());
            break;
        } else {
            // Cursor is after this span
            visual_pos += span.text.len();
        }
    }

    visual_pos
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
    let mut block_regions: Vec<BlockRegion> = Vec::new();

    // Walk the block tree to find inline nodes and block regions
    let mut block_cursor = tree.walk();
    collect_inline_styles(
        tree,
        &text,
        &mut block_cursor,
        cursor_offset,
        &mut styled_regions,
    );

    // Reset cursor and collect block regions
    let mut block_cursor = tree.walk();
    collect_block_regions(tree, &text, &mut block_cursor, &mut block_regions);

    // Collect all boundary points where styles might change
    // Include both full_range boundaries (for marker visibility) and content_range boundaries
    let mut boundaries: Vec<usize> = Vec::new();
    boundaries.push(0);
    boundaries.push(text.len());

    // Add boundaries from inline styled regions
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

    // Add boundaries from block regions
    for region in &block_regions {
        // For block-level elements, visibility is based on whether cursor is on the same line
        let cursor_on_line =
            cursor_offset >= region.full_range.start && cursor_offset < region.full_range.end;

        if cursor_on_line {
            // Show markers - full range
            boundaries.push(region.full_range.start);
            boundaries.push(region.full_range.end);
        } else {
            // Hide markers - add marker boundaries for hiding
            boundaries.push(region.full_range.start);
            boundaries.push(region.marker_range.end);
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

        // Check inline styled regions
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

        // Check block regions
        if !is_hidden {
            for region in &block_regions {
                let cursor_on_line = cursor_offset >= region.full_range.start
                    && cursor_offset < region.full_range.end;

                if !cursor_on_line {
                    // Markers are hidden - check if this byte range is in the marker area
                    let in_marker =
                        start >= region.marker_range.start && end <= region.marker_range.end;

                    if in_marker {
                        is_hidden = true;
                        break;
                    }
                }
            }
        }

        if is_hidden {
            continue;
        }

        // Compute merged style for this range
        let mut style = TextStyle::default();

        // Apply inline styles
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

        // Apply block styles (headings, etc.)
        for region in &block_regions {
            let cursor_on_line =
                cursor_offset >= region.full_range.start && cursor_offset < region.full_range.end;

            // Determine which range to use for styling
            let style_range = if cursor_on_line {
                // When cursor is on line, style the entire line (excluding trailing newline)
                region.full_range.start..region.content_range.end
            } else {
                // When cursor is elsewhere, only style the content
                region.content_range.clone()
            };

            // Apply style if this span overlaps the style range
            if start < style_range.end && end > style_range.start {
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
pub struct StyledRegion {
    /// The full range including markers (e.g., "**bold**")
    pub full_range: Range<usize>,
    /// The content range without markers (e.g., "bold")
    pub content_range: Range<usize>,
    /// The style to apply
    pub style: TextStyle,
    /// URL for links (None for non-link regions)
    pub link_url: Option<String>,
}

/// A block-level region with line-based marker visibility.
#[derive(Debug, Clone)]
struct BlockRegion {
    /// The full range of the block (entire line/lines)
    full_range: Range<usize>,
    /// The marker range to hide (e.g., "# " for headings)
    marker_range: Range<usize>,
    /// The content range (text after marker)
    content_range: Range<usize>,
    /// The style to apply to content
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

/// Walk the tree and collect block-level regions (headings, lists, blockquotes).
fn collect_block_regions(
    tree: &MarkdownTree,
    text: &str,
    cursor: &mut MarkdownCursor,
    regions: &mut Vec<BlockRegion>,
) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        // Handle ATX headings (# Heading)
        if kind.starts_with("atx_heading") || kind == "atx_heading" {
            if let Some(region) = extract_heading_region(&node, text) {
                regions.push(region);
            }
        }

        // Recurse into children
        if cursor.goto_first_child() {
            collect_block_regions(tree, text, cursor, regions);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extract a BlockRegion from a heading node.
fn extract_heading_region(node: &Node, text: &str) -> Option<BlockRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    // Find the heading level marker (atx_h1_marker, atx_h2_marker, etc.)
    let mut marker_end = full_start;
    let mut heading_level: u8 = 1;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let child_kind = child.kind();
            if child_kind.starts_with("atx_h") && child_kind.ends_with("_marker") {
                // Extract level from marker name (atx_h1_marker -> 1)
                if let Some(level_char) = child_kind.chars().nth(5) {
                    if let Some(level) = level_char.to_digit(10) {
                        heading_level = level as u8;
                    }
                }
                marker_end = child.end_byte();
                break;
            }
        }
    }

    // Skip the space after the marker
    let content_start = if marker_end < text.len() && text.as_bytes().get(marker_end) == Some(&b' ')
    {
        marker_end + 1
    } else {
        marker_end
    };

    // Content ends before the trailing newline (if any)
    let content_end = if full_end > 0 && text.as_bytes().get(full_end - 1) == Some(&b'\n') {
        full_end - 1
    } else {
        full_end
    };

    Some(BlockRegion {
        full_range: full_start..full_end,
        marker_range: full_start..content_start, // includes "# "
        content_range: content_start..content_end,
        style: TextStyle::heading(heading_level),
    })
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
        link_url: None,
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
        link_url: None,
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

    #[test]
    fn test_visual_to_buffer_offset_plain_text() {
        let buf: Buffer = "hello world".parse().unwrap();
        let spans = compute_render_spans(&buf, 0);

        // Plain text: visual offset == buffer offset
        assert_eq!(visual_to_buffer_offset(&spans, 0), 0);
        assert_eq!(visual_to_buffer_offset(&spans, 5), 5);
        assert_eq!(visual_to_buffer_offset(&spans, 11), 11);
    }

    #[test]
    fn test_visual_to_buffer_offset_with_hidden_markers() {
        let buf: Buffer = "hello **bold** world".parse().unwrap();
        // Cursor at 0 (outside bold), so markers are hidden
        // Visual: "hello bold world" (16 chars)
        // Buffer: "hello **bold** world" (20 chars)
        let spans = compute_render_spans(&buf, 0);

        // "hello " -> visual 0-5, buffer 0-5
        assert_eq!(visual_to_buffer_offset(&spans, 0), 0);
        assert_eq!(visual_to_buffer_offset(&spans, 5), 5);

        // "bold" -> visual 6-9, buffer 8-11 (markers hidden)
        assert_eq!(visual_to_buffer_offset(&spans, 6), 8); // 'b' in bold
        assert_eq!(visual_to_buffer_offset(&spans, 9), 11); // 'd' in bold

        // " world" -> visual 10-15, buffer 14-19
        assert_eq!(visual_to_buffer_offset(&spans, 10), 14);
    }

    #[test]
    fn test_buffer_to_visual_offset_with_hidden_markers() {
        let buf: Buffer = "hello **bold** world".parse().unwrap();
        // Cursor at 0 (outside bold), so markers are hidden
        let spans = compute_render_spans(&buf, 0);

        // "hello " -> buffer 0-5, visual 0-5
        assert_eq!(buffer_to_visual_offset(&spans, 0), 0);
        assert_eq!(buffer_to_visual_offset(&spans, 5), 5);

        // "bold" -> buffer 8-11, visual 6-9
        assert_eq!(buffer_to_visual_offset(&spans, 8), 6); // 'b' in bold
        assert_eq!(buffer_to_visual_offset(&spans, 11), 9); // 'd' in bold

        // " world" -> buffer 14-19, visual 10-15
        assert_eq!(buffer_to_visual_offset(&spans, 14), 10);
    }

    #[test]
    fn test_visual_to_buffer_offset_cursor_inside() {
        let buf: Buffer = "hello **bold** world".parse().unwrap();
        // Cursor at 10 (inside bold), so markers are shown
        // Visual: "hello **bold** world" (20 chars)
        let spans = compute_render_spans(&buf, 10);

        // With markers shown, visual == buffer
        assert_eq!(visual_to_buffer_offset(&spans, 0), 0);
        assert_eq!(visual_to_buffer_offset(&spans, 6), 6); // first *
        assert_eq!(visual_to_buffer_offset(&spans, 8), 8); // 'b' in bold
        assert_eq!(visual_to_buffer_offset(&spans, 14), 14); // space after **
    }

    fn print_tree(buf: &Buffer) {
        let tree = buf.tree().unwrap();
        let text = buf.text();

        fn print_node(node: &tree_sitter::Node, text: &str, depth: usize) {
            let indent = "  ".repeat(depth);
            let content = &text[node.start_byte()..node.end_byte()];
            let preview: String = content.chars().take(40).collect();
            eprintln!(
                "{}{}: {:?} [{}-{}]",
                indent,
                node.kind(),
                preview,
                node.start_byte(),
                node.end_byte()
            );
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    print_node(&child, text, depth + 1);
                }
            }
        }

        eprintln!("\nBlock tree:");
        print_node(&tree.block_tree().root_node(), &text, 0);
    }

    #[test]
    fn test_debug_heading_structure() {
        let buf: Buffer = "# Heading\n\nText".parse().unwrap();
        print_tree(&buf);
    }

    #[test]
    fn test_debug_list_structure() {
        let buf: Buffer = "- Item 1\n- Item 2\n  - Nested".parse().unwrap();
        print_tree(&buf);
    }

    #[test]
    fn test_debug_blockquote_structure() {
        let buf: Buffer = "> Quote line 1\n> Quote line 2".parse().unwrap();
        print_tree(&buf);
    }

    #[test]
    fn test_debug_code_block_structure() {
        let buf: Buffer = "```rust\nfn main() {}\n```".parse().unwrap();
        print_tree(&buf);
    }

    #[test]
    fn test_debug_mixed_blocks() {
        let buf: Buffer = "# Heading\n\nParagraph text.\n\n- List item\n\n> Quote"
            .parse()
            .unwrap();
        print_tree(&buf);
    }

    #[test]
    fn test_heading_cursor_outside() {
        // When cursor is outside the heading line, "# " should be hidden
        let buf: Buffer = "# Heading\n\nText".parse().unwrap();
        // Cursor at "Text" (offset 11)
        let spans = compute_render_spans(&buf, 12);

        // Should have: "Heading" (bold, no #) + "\n\n" + "Text"
        // The "# " marker should be hidden
        assert!(
            !spans.iter().any(|s| s.text.contains("#")),
            "Heading marker should be hidden when cursor is outside"
        );

        // Find the heading text
        let heading_span = spans.iter().find(|s| s.text.contains("Heading")).unwrap();
        assert!(heading_span.style.bold, "Heading should be bold");
        assert_eq!(
            heading_span.style.heading_level, 1,
            "Should be heading level 1"
        );
    }

    #[test]
    fn test_heading_cursor_inside() {
        // When cursor is on the heading line, "# " should be visible
        let buf: Buffer = "# Heading\n\nText".parse().unwrap();
        // Cursor inside heading (offset 5, in "Heading")
        let spans = compute_render_spans(&buf, 5);

        // Should show "# Heading" with markers visible
        let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(
            full_text.contains("# "),
            "Heading marker should be visible when cursor is on line"
        );

        // The heading content should still be styled
        let heading_span = spans.iter().find(|s| s.text.contains("Heading")).unwrap();
        assert!(heading_span.style.bold, "Heading should be bold");
    }

    #[test]
    fn test_heading_level_2() {
        let buf: Buffer = "## Second Level\n\nText".parse().unwrap();
        // Cursor outside
        let spans = compute_render_spans(&buf, 20);

        // "## " should be hidden
        assert!(
            !spans.iter().any(|s| s.text.contains("#")),
            "Heading markers should be hidden"
        );

        let heading_span = spans.iter().find(|s| s.text.contains("Second")).unwrap();
        assert_eq!(heading_span.style.heading_level, 2);
    }
}
