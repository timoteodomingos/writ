//! Line-based rendering model.
//!
//! This module provides line-by-line rendering where each line in the buffer
//! gets rendered as a separate element, with styling determined by tree-sitter.
//!
//! Lines are modeled as a stack of layers, where each layer represents a
//! structural element (blockquote, list item, heading, etc.). For example:
//! - `> - item` has layers: [BlockQuote, ListItem]
//! - `> > text` has layers: [BlockQuote, BlockQuote]
//! - `# Title` has layers: [Heading(1)]
//!
//! Each layer knows:
//! - Its marker range (what to hide when cursor is away)
//! - Its substitution (what to show instead, e.g., bullet for list)
//! - Its styling contribution (border for blockquote, bold for heading)
//! - Its continuation text for Smart Enter

use crate::buffer::Buffer;
use crate::parser::MarkdownTree;
use crate::render::{StyledRegion, TextStyle};
use std::ops::Range;
use tree_sitter::Node;

// ============================================================================
// New tree-sitter based marker detection
// ============================================================================

/// Information about a marker found on a line (internal use only).
#[derive(Debug, Clone, PartialEq)]
struct MarkerInfo {
    /// The type of marker
    pub kind: MarkerKind,
    /// Byte range of this marker in the buffer
    pub range: Range<usize>,
}

/// The type of a marker node (internal use only).
#[derive(Debug, Clone, PartialEq)]
enum MarkerKind {
    /// Blockquote marker `> ` or continuation
    BlockQuote,
    /// Unordered list marker `- ` or `* `
    UnorderedList,
    /// Ordered list marker `1. `
    OrderedList,
    /// Task list marker `[ ]` or `[x]`
    TaskList { checked: bool },
    /// Heading marker `# `, `## `, etc.
    Heading(u8),
}

impl MarkerInfo {
    /// Create a MarkerInfo from a tree-sitter node.
    fn from_node(node: &Node, text: &str) -> Option<Self> {
        let kind = node.kind();
        let range = node.start_byte()..node.end_byte();

        let marker_kind = match kind {
            "block_quote_marker" => MarkerKind::BlockQuote,
            "block_continuation" => {
                // Only count as blockquote if it contains '>'
                let content = &text[range.clone()];
                if content.contains('>') {
                    MarkerKind::BlockQuote
                } else {
                    return None; // This is list indentation, not a blockquote
                }
            }
            "list_marker_minus" | "list_marker_star" | "list_marker_plus" => {
                MarkerKind::UnorderedList
            }
            "list_marker_dot" | "list_marker_parenthesis" => MarkerKind::OrderedList,
            "task_list_marker_unchecked" => MarkerKind::TaskList { checked: false },
            "task_list_marker_checked" => MarkerKind::TaskList { checked: true },
            k if k.starts_with("atx_h") && k.ends_with("_marker") => {
                // Extract level from "atx_h1_marker", "atx_h2_marker", etc.
                let level = k.chars().nth(5)?.to_digit(10)? as u8;
                MarkerKind::Heading(level)
            }
            _ => return None,
        };

        Some(MarkerInfo {
            kind: marker_kind,
            range,
        })
    }

    /// Get the substitution text for this marker (shown when marker is hidden).
    pub fn substitution(&self) -> &'static str {
        match &self.kind {
            MarkerKind::BlockQuote => "", // No text substitution, just border
            MarkerKind::UnorderedList => "• ",
            MarkerKind::OrderedList => "", // Keep number visible
            MarkerKind::TaskList { checked: false } => "☐ ",
            MarkerKind::TaskList { checked: true } => "☑ ",
            MarkerKind::Heading(_) => "",
        }
    }

    /// Get the continuation text for Smart Enter.
    pub fn continuation(&self) -> &'static str {
        match &self.kind {
            MarkerKind::BlockQuote => "> ",
            MarkerKind::UnorderedList => "- ",
            MarkerKind::OrderedList => "1. ", // Normalization fixes the number
            MarkerKind::TaskList { .. } => "- [ ] ",
            MarkerKind::Heading(_) => "",
        }
    }

    /// Whether this marker adds a left border when hidden.
    pub fn has_border(&self) -> bool {
        matches!(self.kind, MarkerKind::BlockQuote)
    }
}

/// Find all markers on a line by traversing the tree.
/// Returns markers sorted by byte position.
fn find_markers_on_line(
    root: &Node,
    text: &str,
    line_start: usize,
    line_end: usize,
) -> Vec<MarkerInfo> {
    let mut markers = Vec::new();
    collect_markers_recursive(root, text, line_start, line_end, &mut markers);
    markers.sort_by_key(|m| m.range.start);
    markers
}

fn collect_markers_recursive(
    node: &Node,
    text: &str,
    line_start: usize,
    line_end: usize,
    markers: &mut Vec<MarkerInfo>,
) {
    // Skip nodes that don't overlap with our line
    if node.end_byte() <= line_start || node.start_byte() >= line_end {
        return;
    }

    // Check if this node is a marker on our line
    if node.start_byte() >= line_start && node.start_byte() < line_end {
        if let Some(marker) = MarkerInfo::from_node(node, text) {
            markers.push(marker);
        }
    }

    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            collect_markers_recursive(&child, text, line_start, line_end, markers);
        }
    }
}

/// Information about a single line for rendering purposes.
/// All marker-derived fields are pre-computed during line extraction.
#[derive(Debug, Clone, PartialEq)]
pub struct LineInfo {
    /// Byte range of this line in the buffer (excluding the newline)
    pub range: Range<usize>,
    /// The line number (0-indexed)
    pub line_number: usize,
    /// URL for image-only lines (when line contains only an image)
    pub image_url: Option<String>,
    /// Alt text for image-only lines
    pub image_alt: Option<String>,
    /// The kind of line (heading, list item, etc.)
    pub kind: LineKind,
    /// Combined byte range of all markers on this line (to hide when cursor away)
    pub marker_range: Option<Range<usize>>,
    /// Whether this line should show a left border (blockquote)
    pub has_border: bool,
    /// Substitution text when markers are hidden (e.g., "• " for bullets)
    pub substitution: String,
    /// Continuation text for smart enter (e.g., "- " for list items)
    pub continuation: String,
}

// Keep LineKind for backwards compatibility during transition
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

/// Compute LineKind from markers.
fn compute_kind_from_markers(markers: &[MarkerInfo]) -> LineKind {
    // Check for specific marker types in reverse order (innermost first)
    for marker in markers.iter().rev() {
        match &marker.kind {
            MarkerKind::Heading(level) => return LineKind::Heading(*level),
            MarkerKind::TaskList { checked } => {
                return LineKind::ListItem {
                    ordered: false,
                    checked: Some(*checked),
                };
            }
            MarkerKind::UnorderedList => {
                return LineKind::ListItem {
                    ordered: false,
                    checked: None,
                };
            }
            MarkerKind::OrderedList => {
                return LineKind::ListItem {
                    ordered: true,
                    checked: None,
                };
            }
            MarkerKind::BlockQuote => return LineKind::BlockQuote,
        }
    }
    LineKind::Paragraph
}

/// Compute LineKind for a line, checking for code blocks first.
fn compute_line_kind(
    tree: Option<&MarkdownTree>,
    text: &str,
    range: &Range<usize>,
    markers: &[MarkerInfo],
) -> LineKind {
    // Check for empty line
    if range.start == range.end {
        return LineKind::Blank;
    }

    // Check for code block first
    if let Some(tree) = tree {
        let root = tree.block_tree().root_node();
        if let Some(code_block) = find_containing_code_block(&root, range.start) {
            let language = extract_code_block_language(&code_block, text);
            let is_fence = code_block.start_byte() == range.start
                || (range.end <= code_block.end_byte()
                    && text[range.clone()].trim().starts_with("```"));
            return LineKind::CodeBlock { language, is_fence };
        }
    }

    compute_kind_from_markers(markers)
}

/// Compute the combined marker range (for hiding when cursor is away).
fn compute_marker_range(markers: &[MarkerInfo]) -> Option<Range<usize>> {
    if markers.is_empty() {
        return None;
    }
    Some(markers.first()?.range.start..markers.last()?.range.end)
}

/// Compute whether line should show a left border.
fn compute_has_border(markers: &[MarkerInfo]) -> bool {
    markers.iter().any(|m| m.has_border())
}

/// Compute substitution text when markers are hidden.
fn compute_substitution(markers: &[MarkerInfo]) -> String {
    markers.iter().map(|m| m.substitution()).collect()
}

/// Compute continuation text for smart enter.
fn compute_continuation(text: &str, line_range: &Range<usize>, markers: &[MarkerInfo]) -> String {
    if markers.is_empty() {
        return String::new();
    }

    // Leading whitespace before first marker
    let first_start = markers.first().unwrap().range.start;
    let leading = if first_start > line_range.start {
        &text[line_range.start..first_start]
    } else {
        ""
    };

    // Each marker's continuation text
    let marker_text: String = markers.iter().map(|m| m.continuation()).collect();
    format!("{}{}", leading, marker_text)
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

    // Build LineInfo for each line
    lines
        .into_iter()
        .map(|(line_num, range)| {
            // Check if this is an image-only line
            let (image_url, image_alt) = if let Some(tree) = &tree {
                detect_image_only_line(tree, &text, &range)
            } else {
                (None, None)
            };

            // Find markers on this line
            let markers = if let Some(tree) = &tree {
                let root = tree.block_tree().root_node();
                find_markers_on_line(&root, &text, range.start, range.end)
            } else {
                Vec::new()
            };

            // Compute all marker-derived fields
            let kind = compute_line_kind(tree.as_deref(), &text, &range, &markers);
            let marker_range = compute_marker_range(&markers);
            let has_border = compute_has_border(&markers);
            let substitution = compute_substitution(&markers);
            let continuation = compute_continuation(&text, &range, &markers);

            LineInfo {
                range,
                line_number: line_num,
                image_url,
                image_alt,
                kind,
                marker_range,
                has_border,
                substitution,
                continuation,
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
        link_url: None,
    })
}

/// Extract a styled region from a code span.
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
    })
}

/// Extract a styled region from a link node (inline_link, shortcut_link, etc.).
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
    })
}

/// Extract a styled region from an image node.
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
    })
}

/// Extract the language from a fenced_code_block node.
fn extract_code_block_language(node: &tree_sitter::Node, text: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32)
            && child.kind() == "info_string"
        {
            for j in 0..child.child_count() {
                if let Some(lang_node) = child.child(j as u32)
                    && lang_node.kind() == "language"
                {
                    let lang = &text[lang_node.start_byte()..lang_node.end_byte()];
                    return Some(lang.to_string());
                }
            }
            let info = &text[child.start_byte()..child.end_byte()];
            let trimmed = info.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Find a fenced_code_block node that contains the given position.
fn find_containing_code_block<'a>(
    root: &tree_sitter::Node<'a>,
    pos: usize,
) -> Option<tree_sitter::Node<'a>> {
    fn search<'a>(node: tree_sitter::Node<'a>, pos: usize) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == "fenced_code_block" && node.start_byte() <= pos && pos <= node.end_byte()
        {
            return Some(node);
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && child.start_byte() <= pos
                && pos <= child.end_byte()
                && let Some(found) = search(child, pos)
            {
                return Some(found);
            }
        }

        None
    }

    search(*root, pos)
}

/// Detect if a line contains only an image (no other content except whitespace).
fn detect_image_only_line(
    tree: &MarkdownTree,
    text: &str,
    range: &Range<usize>,
) -> (Option<String>, Option<String>) {
    let root = tree.block_tree().root_node();

    fn find_inline_in_range<'a>(node: Node<'a>, range: &Range<usize>) -> Option<Node<'a>> {
        if node.end_byte() <= range.start || node.start_byte() >= range.end {
            return None;
        }

        if node.kind() == "inline" {
            return Some(node);
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32)
                && let Some(inline) = find_inline_in_range(child, range)
            {
                return Some(inline);
            }
        }
        None
    }

    let Some(inline_node) = find_inline_in_range(root, range) else {
        return (None, None);
    };

    let Some(inline_tree) = tree.inline_tree(&inline_node) else {
        return (None, None);
    };

    let inline_root = inline_tree.root_node();
    let mut image_node: Option<Node> = None;
    let mut has_other_constructs = false;

    for i in 0..inline_root.child_count() {
        if let Some(child) = inline_root.child(i as u32) {
            match child.kind() {
                "image" => {
                    if image_node.is_some() {
                        has_other_constructs = true;
                    } else {
                        image_node = Some(child);
                    }
                }
                _ => {
                    has_other_constructs = true;
                }
            }
        }
    }

    if has_other_constructs {
        return (None, None);
    }

    let Some(img) = image_node else {
        return (None, None);
    };

    let inline_start = inline_root.start_byte();
    let inline_end = inline_root.end_byte();
    let img_start = img.start_byte();
    let img_end = img.end_byte();

    let text_before = &text[inline_start..img_start];
    if !text_before.trim().is_empty() {
        return (None, None);
    }

    let text_after = &text[img_end..inline_end];
    if !text_after.trim().is_empty() {
        return (None, None);
    }

    let mut url: Option<String> = None;
    let mut alt: Option<String> = None;

    for i in 0..img.child_count() {
        if let Some(child) = img.child(i as u32) {
            match child.kind() {
                "image_description" => {
                    let desc_start = child.start_byte();
                    let desc_end = child.end_byte();
                    let desc_text = &text[desc_start..desc_end];
                    let inner = desc_text.trim_start_matches('[').trim_end_matches(']');
                    alt = Some(inner.to_string());
                }
                "link_destination" => {
                    url = Some(text[child.start_byte()..child.end_byte()].to_string());
                }
                _ => {}
            }
        }
    }

    (url, alt)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Tests for new tree-sitter based marker detection
    // ========================================================================

    #[test]
    fn test_find_markers_simple_list() {
        let buf: Buffer = "- Item 1\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 0: "- Item 1" (bytes 0-8)
        let markers = find_markers_on_line(&root, &text, 0, 8);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, MarkerKind::UnorderedList);
        assert_eq!(markers[0].range, 0..2);
    }

    #[test]
    fn test_find_markers_nested_list() {
        let buf: Buffer = "- Item 1\n  - Nested\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 0: "- Item 1" (bytes 0-8)
        let markers = find_markers_on_line(&root, &text, 0, 8);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].range, 0..2);

        // Line 1: "  - Nested" (bytes 9-19)
        // The marker should be at bytes 11-13 (the "- "), not including leading spaces
        let markers = find_markers_on_line(&root, &text, 9, 19);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, MarkerKind::UnorderedList);
        assert_eq!(markers[0].range, 11..13);
    }

    #[test]
    fn test_find_markers_blockquote() {
        let buf: Buffer = "> Quote\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = find_markers_on_line(&root, &text, 0, 7);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, MarkerKind::BlockQuote);
        assert_eq!(markers[0].range, 0..2);
    }

    #[test]
    fn test_find_markers_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 0: "> Level 1" (bytes 0-9)
        let markers = find_markers_on_line(&root, &text, 0, 9);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].range, 0..2);

        // Line 1: "> > Level 2" (bytes 10-21)
        // Should find TWO blockquote markers
        let markers = find_markers_on_line(&root, &text, 10, 21);
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().all(|m| m.kind == MarkerKind::BlockQuote));
    }

    #[test]
    fn test_find_markers_list_in_blockquote() {
        let buf: Buffer = "> - Item\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = find_markers_on_line(&root, &text, 0, 8);
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].kind, MarkerKind::BlockQuote);
        assert_eq!(markers[1].kind, MarkerKind::UnorderedList);
    }

    #[test]
    fn test_find_markers_heading() {
        let buf: Buffer = "# Heading\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        let markers = find_markers_on_line(&root, &text, 0, 9);
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].kind, MarkerKind::Heading(1));
    }

    #[test]
    fn test_find_markers_task_list() {
        let buf: Buffer = "- [ ] Unchecked\n- [x] Checked\n".parse().unwrap();
        let text = buf.text();
        let tree = buf.tree().unwrap();
        let root = tree.block_tree().root_node();

        // Line 0: "- [ ] Unchecked" (bytes 0-15)
        let markers = find_markers_on_line(&root, &text, 0, 15);
        assert_eq!(markers.len(), 2); // list marker + task marker
        assert_eq!(markers[0].kind, MarkerKind::UnorderedList);
        assert_eq!(markers[1].kind, MarkerKind::TaskList { checked: false });

        // Line 1: "- [x] Checked" (bytes 16-29)
        let markers = find_markers_on_line(&root, &text, 16, 29);
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[1].kind, MarkerKind::TaskList { checked: true });
    }

    #[test]
    fn test_continuation_nested_list() {
        let buf: Buffer = "- Item 1\n  - Nested\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Line 1: "  - Nested" - should continue with "  - "
        assert_eq!(lines[1].continuation, "  - ");
    }

    #[test]
    fn test_kind_nested_list() {
        let buf: Buffer = "- Item 1\n  - Nested\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Line 1: "  - Nested" should be recognized as a list item
        assert_eq!(
            lines[1].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
    }

    #[test]
    fn test_marker_range_nested_list() {
        let buf: Buffer = "- Item 1\n  - Nested\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Line 1: marker should be at bytes 11-13 (the "- "), not 9-13
        assert_eq!(lines[1].marker_range, Some(11..13));
    }

    // ========================================================================
    // Tests using pre-computed fields
    // ========================================================================

    #[test]
    fn test_empty_buffer() {
        let buf: Buffer = "".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, LineKind::Blank);
        assert_eq!(lines[0].range, 0..0);
    }

    #[test]
    fn test_single_newline() {
        let buf: Buffer = "\n".parse().unwrap();
        let lines = extract_lines(&buf);

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

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].kind, LineKind::Paragraph);
        assert_eq!(lines[0].range, 0..5);
        assert_eq!(lines[1].kind, LineKind::Blank);
        assert_eq!(lines[1].range, 6..6);
        assert_eq!(lines[2].kind, LineKind::Paragraph);
        assert_eq!(lines[2].range, 7..12);
        assert_eq!(lines[3].kind, LineKind::Blank);
        assert_eq!(lines[3].range, 13..13);
    }

    #[test]
    fn test_heading_line() {
        let buf: Buffer = "# Hello\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, LineKind::Heading(1));
        // tree-sitter's atx_h1_marker is just "#", not "# "
        assert_eq!(lines[0].marker_range, Some(0..1));
    }

    #[test]
    fn test_heading_levels() {
        let buf: Buffer = "# H1\n## H2\n### H3\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::Heading(1));
        assert_eq!(lines[1].kind, LineKind::Heading(2));
        assert_eq!(lines[2].kind, LineKind::Heading(3));
    }

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
        assert_eq!(lines[0].marker_range, Some(0..2));
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
        assert_eq!(lines[0].marker_range, Some(0..3));
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
        assert_eq!(
            lines[2].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
    }

    #[test]
    fn test_blockquote() {
        let buf: Buffer = "> Quote line 1\n> Quote line 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::BlockQuote);
        assert_eq!(lines[0].marker_range, Some(0..2));
        assert_eq!(lines[1].kind, LineKind::BlockQuote);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind, LineKind::BlockQuote);
        assert_eq!(lines[0].marker_range, Some(0..2));
        assert_eq!(lines[1].kind, LineKind::BlockQuote);
        // Nested blockquote: marker range covers both "> " markers
        assert_eq!(lines[1].marker_range, Some(10..14));
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item 1\n>   - Nested item\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // First line: list item inside blockquote
        assert_eq!(
            lines[0].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );

        // Second line: nested list item inside blockquote
        assert_eq!(
            lines[1].kind,
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
    }

    #[test]
    fn test_blockquote_in_list() {
        let buf: Buffer = "- > Quoted item\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // List item with blockquote content - kind is based on innermost block
        // With tree-based approach, we find both markers
        let kind = &lines[0].kind;
        // The innermost is BlockQuote since it appears after the list marker
        assert!(
            *kind == LineKind::BlockQuote
                || *kind
                    == LineKind::ListItem {
                        ordered: false,
                        checked: None
                    }
        );
    }

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
}
