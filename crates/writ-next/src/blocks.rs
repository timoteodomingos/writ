//! Block extraction from tree-sitter AST.
//!
//! This module extracts semantic blocks from the markdown AST for rendering.
//! Each block represents a top-level element like a paragraph, heading, list, etc.

use std::ops::Range;

use crate::buffer::Buffer;
use crate::parser::MarkdownTree;
use crate::render::{StyledRegion, TextStyle};
use tree_sitter::Node;

/// A rendered block extracted from the AST.
#[derive(Debug, Clone)]
pub enum RenderBlock {
    /// A paragraph of text.
    Paragraph {
        /// Full range in buffer
        range: Range<usize>,
        /// Inline styles within the paragraph
        inline_styles: Vec<StyledRegion>,
    },

    /// A heading (h1-h6).
    Heading {
        /// Heading level (1-6)
        level: u8,
        /// Full range including marker and content
        range: Range<usize>,
        /// Range of the marker (e.g., "## ")
        marker_range: Range<usize>,
        /// Range of the content text
        content_range: Range<usize>,
        /// Inline styles within the heading
        inline_styles: Vec<StyledRegion>,
    },

    /// A list item.
    ListItem {
        /// Full range of this item
        range: Range<usize>,
        /// Range of the marker (e.g., "- " or "1. ")
        marker_range: Range<usize>,
        /// Range of the content
        content_range: Range<usize>,
        /// Inline styles within the content
        inline_styles: Vec<StyledRegion>,
        /// Nested blocks (for nested lists, etc.)
        nested_blocks: Vec<RenderBlock>,
    },

    /// A blockquote.
    BlockQuote {
        /// Full range
        range: Range<usize>,
        /// Range of the marker ("> ")
        marker_range: Range<usize>,
        /// Nested blocks inside the quote
        nested_blocks: Vec<RenderBlock>,
    },

    /// A fenced code block.
    CodeBlock {
        /// Full range including fences
        range: Range<usize>,
        /// The language identifier (e.g., "rust")
        language: Option<String>,
        /// Range of the code content (excluding fences)
        content_range: Range<usize>,
    },
}

impl RenderBlock {
    /// Get the full range of this block in the buffer.
    pub fn range(&self) -> Range<usize> {
        match self {
            RenderBlock::Paragraph { range, .. } => range.clone(),
            RenderBlock::Heading { range, .. } => range.clone(),
            RenderBlock::ListItem { range, .. } => range.clone(),
            RenderBlock::BlockQuote { range, .. } => range.clone(),
            RenderBlock::CodeBlock { range, .. } => range.clone(),
        }
    }

    /// Check if a cursor position is within this block.
    pub fn contains_cursor(&self, cursor: usize) -> bool {
        let range = self.range();
        cursor >= range.start && cursor < range.end
    }
}

/// Extract all top-level blocks from a buffer.
pub fn extract_blocks(buffer: &Buffer) -> Vec<RenderBlock> {
    let Some(tree) = buffer.tree() else {
        // No parse tree - treat entire buffer as one paragraph
        let text = buffer.text();
        return vec![RenderBlock::Paragraph {
            range: 0..text.len(),
            inline_styles: vec![],
        }];
    };

    let text = buffer.text();
    let mut blocks = Vec::new();

    // Walk the block tree at the top level
    let root = tree.block_tree().root_node();
    collect_blocks_from_node(&root, tree, &text, &mut blocks);

    blocks
}

/// Recursively collect blocks from AST nodes.
fn collect_blocks_from_node(
    node: &Node,
    tree: &MarkdownTree,
    text: &str,
    blocks: &mut Vec<RenderBlock>,
) {
    let kind = node.kind();

    match kind {
        "atx_heading" => {
            if let Some(block) = extract_heading(node, tree, text) {
                blocks.push(block);
            }
        }
        "paragraph" => {
            if let Some(block) = extract_paragraph(node, tree, text) {
                blocks.push(block);
            }
        }
        "list" => {
            // A list contains list_items - extract each as a block
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    if child.kind() == "list_item" {
                        if let Some(block) = extract_list_item(&child, tree, text) {
                            blocks.push(block);
                        }
                    }
                }
            }
        }
        "block_quote" => {
            if let Some(block) = extract_blockquote(node, tree, text) {
                blocks.push(block);
            }
        }
        "fenced_code_block" => {
            if let Some(block) = extract_code_block(node, text) {
                blocks.push(block);
            }
        }
        // Container nodes - recurse into children
        "document" | "section" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    collect_blocks_from_node(&child, tree, text, blocks);
                }
            }
        }
        _ => {
            // Unknown block type - skip for now
        }
    }
}

/// Extract a heading block.
fn extract_heading(node: &Node, tree: &MarkdownTree, text: &str) -> Option<RenderBlock> {
    let range = node.start_byte()..node.end_byte();

    // Find the marker and determine level
    let mut marker_end = node.start_byte();
    let mut level: u8 = 1;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let child_kind = child.kind();
            if child_kind.starts_with("atx_h") && child_kind.ends_with("_marker") {
                // Extract level from marker name (atx_h1_marker -> 1)
                if let Some(level_char) = child_kind.chars().nth(5) {
                    if let Some(l) = level_char.to_digit(10) {
                        level = l as u8;
                    }
                }
                marker_end = child.end_byte();
                break;
            }
        }
    }

    // Skip space after marker
    let content_start = if marker_end < text.len() && text.as_bytes().get(marker_end) == Some(&b' ')
    {
        marker_end + 1
    } else {
        marker_end
    };

    // Content ends before trailing newline
    let content_end = if range.end > 0 && text.as_bytes().get(range.end - 1) == Some(&b'\n') {
        range.end - 1
    } else {
        range.end
    };

    // Collect inline styles from the heading content
    let inline_styles = collect_inline_styles_for_node(node, tree, text);

    Some(RenderBlock::Heading {
        level,
        range,
        marker_range: node.start_byte()..content_start,
        content_range: content_start..content_end,
        inline_styles,
    })
}

/// Extract a paragraph block.
fn extract_paragraph(node: &Node, tree: &MarkdownTree, text: &str) -> Option<RenderBlock> {
    let range = node.start_byte()..node.end_byte();
    let inline_styles = collect_inline_styles_for_node(node, tree, text);

    Some(RenderBlock::Paragraph {
        range,
        inline_styles,
    })
}

/// Extract a list item block.
fn extract_list_item(node: &Node, tree: &MarkdownTree, text: &str) -> Option<RenderBlock> {
    let range = node.start_byte()..node.end_byte();

    // Find the list marker
    let mut marker_range = node.start_byte()..node.start_byte();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            let child_kind = child.kind();
            if child_kind.starts_with("list_marker") {
                marker_range = child.start_byte()..child.end_byte();
                break;
            }
        }
    }

    let content_start = marker_range.end;

    // Collect inline styles from paragraph children
    let mut inline_styles = Vec::new();
    let mut nested_blocks = Vec::new();

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "paragraph" => {
                    inline_styles.extend(collect_inline_styles_for_node(&child, tree, text));
                }
                "list" => {
                    // Nested list - extract its items recursively
                    for j in 0..child.child_count() {
                        if let Some(list_item) = child.child(j as u32) {
                            if list_item.kind() == "list_item" {
                                if let Some(block) = extract_list_item(&list_item, tree, text) {
                                    nested_blocks.push(block);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Content range is from after marker to end of first paragraph
    // (before any nested content)
    let content_end = if nested_blocks.is_empty() {
        range.end
    } else {
        nested_blocks
            .first()
            .map(|b| b.range().start)
            .unwrap_or(range.end)
    };

    Some(RenderBlock::ListItem {
        range,
        marker_range,
        content_range: content_start..content_end,
        inline_styles,
        nested_blocks,
    })
}

/// Extract a blockquote block.
fn extract_blockquote(node: &Node, tree: &MarkdownTree, text: &str) -> Option<RenderBlock> {
    let range = node.start_byte()..node.end_byte();

    // Find the blockquote marker
    let mut marker_range = node.start_byte()..node.start_byte();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "block_quote_marker" {
                marker_range = child.start_byte()..child.end_byte();
                break;
            }
        }
    }

    // Collect nested blocks (paragraphs, etc. inside the quote)
    let mut nested_blocks = Vec::new();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "paragraph" => {
                    if let Some(block) = extract_paragraph(&child, tree, text) {
                        nested_blocks.push(block);
                    }
                }
                "block_quote" => {
                    // Nested blockquote
                    if let Some(block) = extract_blockquote(&child, tree, text) {
                        nested_blocks.push(block);
                    }
                }
                _ => {}
            }
        }
    }

    Some(RenderBlock::BlockQuote {
        range,
        marker_range,
        nested_blocks,
    })
}

/// Extract a fenced code block.
fn extract_code_block(node: &Node, text: &str) -> Option<RenderBlock> {
    let range = node.start_byte()..node.end_byte();

    let mut language = None;
    let mut content_range = range.clone();

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            match child.kind() {
                "info_string" => {
                    // Look for language child
                    for j in 0..child.child_count() {
                        if let Some(lang_node) = child.child(j as u32) {
                            if lang_node.kind() == "language" {
                                language = Some(
                                    text[lang_node.start_byte()..lang_node.end_byte()].to_string(),
                                );
                            }
                        }
                    }
                }
                "code_fence_content" => {
                    content_range = child.start_byte()..child.end_byte();
                }
                _ => {}
            }
        }
    }

    Some(RenderBlock::CodeBlock {
        range,
        language,
        content_range,
    })
}

/// Collect inline styles from a node that may have inline content.
fn collect_inline_styles_for_node(
    node: &Node,
    tree: &MarkdownTree,
    text: &str,
) -> Vec<StyledRegion> {
    let mut styles = Vec::new();

    // Look for inline children and their inline trees
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "inline" {
                if let Some(inline_tree) = tree.inline_tree(&child) {
                    collect_inline_styles_recursive(inline_tree.root_node(), text, &mut styles);
                }
            }
        }
    }

    styles
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
    })
}

/// Extract a styled region from a code span.
fn extract_code_span_region(node: &Node) -> Option<StyledRegion> {
    let full_start = node.start_byte();
    let full_end = node.end_byte();

    let mut content_start = full_start;
    let mut content_end = full_end;

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "code_span_delimiter" {
                if child.start_byte() == full_start {
                    content_start = child.end_byte();
                } else if child.end_byte() == full_end {
                    content_end = child.start_byte();
                }
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
    fn test_extract_heading() {
        let buf: Buffer = "# Hello World".parse().unwrap();
        let blocks = extract_blocks(&buf);

        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            RenderBlock::Heading {
                level,
                marker_range,
                content_range,
                ..
            } => {
                assert_eq!(*level, 1);
                assert_eq!(*marker_range, 0..2); // "# "
                assert_eq!(*content_range, 2..13); // "Hello World"
            }
            _ => panic!("Expected Heading block"),
        }
    }

    #[test]
    fn test_extract_paragraph() {
        let buf: Buffer = "Hello world".parse().unwrap();
        let blocks = extract_blocks(&buf);

        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            RenderBlock::Paragraph { range, .. } => {
                assert_eq!(*range, 0..11);
            }
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_extract_multiple_blocks() {
        let buf: Buffer = "# Heading\n\nParagraph".parse().unwrap();
        let blocks = extract_blocks(&buf);

        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], RenderBlock::Heading { .. }));
        assert!(matches!(&blocks[1], RenderBlock::Paragraph { .. }));
    }

    #[test]
    fn test_extract_list() {
        let buf: Buffer = "- Item 1\n- Item 2".parse().unwrap();
        let blocks = extract_blocks(&buf);

        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            RenderBlock::ListItem { marker_range, .. } => {
                assert_eq!(*marker_range, 0..2); // "- "
            }
            _ => panic!("Expected ListItem block"),
        }
    }

    #[test]
    fn test_extract_blockquote() {
        let buf: Buffer = "> Quote".parse().unwrap();
        let blocks = extract_blocks(&buf);

        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            RenderBlock::BlockQuote { marker_range, .. } => {
                assert_eq!(*marker_range, 0..2); // "> "
            }
            _ => panic!("Expected BlockQuote block"),
        }
    }

    #[test]
    fn test_extract_code_block() {
        let buf: Buffer = "```rust\ncode\n```".parse().unwrap();
        let blocks = extract_blocks(&buf);

        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            RenderBlock::CodeBlock { language, .. } => {
                assert_eq!(language.as_deref(), Some("rust"));
            }
            _ => panic!("Expected CodeBlock block"),
        }
    }
}
