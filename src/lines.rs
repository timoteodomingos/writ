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

/// A single structural layer of a line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineLayer {
    /// What kind of layer this is
    pub kind: LayerKind,
    /// Byte range of this layer's marker in the buffer (to hide when cursor away)
    pub marker_range: Range<usize>,
}

impl LineLayer {
    /// Get the substitution text for this layer (shown when marker is hidden).
    pub fn substitution(&self) -> &'static str {
        match &self.kind {
            LayerKind::BlockQuote => "", // No text substitution, just border
            LayerKind::ListItem {
                ordered: false,
                checked: None,
            } => "• ",
            LayerKind::ListItem {
                ordered: false,
                checked: Some(false),
            } => "☐ ",
            LayerKind::ListItem {
                ordered: false,
                checked: Some(true),
            } => "☑ ",
            LayerKind::ListItem { ordered: true, .. } => "", // Keep number visible
            LayerKind::Heading(_) => "",                     // No substitution for headings
            LayerKind::CodeBlock { is_fence: true, .. } => "", // Fence lines rendered specially
            LayerKind::CodeBlock {
                is_fence: false, ..
            } => "", // Code content, no marker
        }
    }

    /// Get the continuation text for Smart Enter.
    pub fn continuation(&self) -> &'static str {
        match &self.kind {
            LayerKind::BlockQuote => "> ",
            LayerKind::ListItem {
                ordered: false,
                checked: None,
            } => "- ",
            LayerKind::ListItem {
                ordered: false,
                checked: Some(_),
            } => "- [ ] ",
            LayerKind::ListItem { ordered: true, .. } => "1. ", // Normalization fixes the number
            LayerKind::Heading(_) => "",
            LayerKind::CodeBlock { .. } => "",
        }
    }

    /// Whether this layer adds a left border when hidden.
    pub fn has_border(&self) -> bool {
        matches!(self.kind, LayerKind::BlockQuote)
    }

    /// Whether this layer's marker should be hidden (vs just substituted).
    pub fn hides_marker(&self) -> bool {
        match &self.kind {
            LayerKind::ListItem { ordered: true, .. } => false, // Keep numbers visible
            _ => true,
        }
    }
}

/// The type of a layer.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerKind {
    /// Blockquote (single level)
    BlockQuote,
    /// List item
    ListItem {
        ordered: bool,
        checked: Option<bool>,
    },
    /// Heading with level 1-6
    Heading(u8),
    /// Code block line
    CodeBlock {
        language: Option<String>,
        is_fence: bool,
    },
}

/// Information about a single line for rendering purposes.
#[derive(Debug, Clone, PartialEq)]
pub struct LineInfo {
    /// Byte range of this line in the buffer (excluding the newline)
    pub range: Range<usize>,
    /// The line number (0-indexed)
    pub line_number: usize,
    /// Stack of structural layers (outermost first, e.g., [BlockQuote, ListItem])
    pub layers: Vec<LineLayer>,
    /// URL for image-only lines (when line contains only an image)
    pub image_url: Option<String>,
    /// Alt text for image-only lines
    pub image_alt: Option<String>,
}

impl LineInfo {
    /// Check if this is a blank line.
    pub fn is_blank(&self) -> bool {
        self.layers.is_empty() && self.range.start == self.range.end
    }

    /// Check if this is a paragraph (no structural layers, but has content).
    pub fn is_paragraph(&self) -> bool {
        self.layers.is_empty() && self.range.start < self.range.end
    }

    /// Get the combined marker range (all layers' markers combined).
    pub fn marker_range(&self) -> Option<Range<usize>> {
        if self.layers.is_empty() {
            return None;
        }
        let start = self.layers.first()?.marker_range.start;
        let end = self.layers.last()?.marker_range.end;
        Some(start..end)
    }

    /// Get the innermost layer kind (for backwards compatibility).
    pub fn innermost_layer(&self) -> Option<&LayerKind> {
        self.layers.last().map(|l| &l.kind)
    }

    /// Check if any layer is a blockquote.
    pub fn has_blockquote(&self) -> bool {
        self.layers
            .iter()
            .any(|l| matches!(l.kind, LayerKind::BlockQuote))
    }

    /// Check if any layer adds a border.
    pub fn has_border(&self) -> bool {
        self.layers.iter().any(|l| l.has_border())
    }

    /// Get the combined substitution text for all layers.
    pub fn substitution(&self) -> String {
        self.layers.iter().map(|l| l.substitution()).collect()
    }

    /// Get the combined continuation text for Smart Enter.
    /// Includes leading indentation to maintain nesting level.
    pub fn continuation(&self, text: &str) -> String {
        if self.layers.is_empty() {
            return String::new();
        }

        // Get leading whitespace (indentation before first marker)
        let first_marker_start = self
            .layers
            .first()
            .map(|l| l.marker_range.start)
            .unwrap_or(self.range.start);
        let leading_whitespace = if first_marker_start > self.range.start {
            &text[self.range.start..first_marker_start]
        } else {
            ""
        };

        // Combine leading whitespace + all layer continuations
        let layer_continuations: String = self.layers.iter().map(|l| l.continuation()).collect();
        format!("{}{}", leading_whitespace, layer_continuations)
    }

    /// Check if the innermost layer is a heading.
    pub fn is_heading(&self) -> Option<u8> {
        match self.innermost_layer() {
            Some(LayerKind::Heading(level)) => Some(*level),
            _ => None,
        }
    }

    /// Check if the innermost layer is a code block.
    pub fn is_code_block(&self) -> Option<(&Option<String>, bool)> {
        match self.innermost_layer() {
            Some(LayerKind::CodeBlock { language, is_fence }) => Some((language, *is_fence)),
            _ => None,
        }
    }
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

impl LineInfo {
    /// Convert layers to LineKind for backwards compatibility.
    /// Returns the innermost significant layer as LineKind.
    pub fn kind(&self) -> LineKind {
        if self.is_blank() {
            return LineKind::Blank;
        }
        if self.is_paragraph() {
            return LineKind::Paragraph;
        }

        match self.innermost_layer() {
            Some(LayerKind::BlockQuote) => LineKind::BlockQuote,
            Some(LayerKind::ListItem { ordered, checked }) => LineKind::ListItem {
                ordered: *ordered,
                checked: *checked,
            },
            Some(LayerKind::Heading(level)) => LineKind::Heading(*level),
            Some(LayerKind::CodeBlock { language, is_fence }) => LineKind::CodeBlock {
                language: language.clone(),
                is_fence: *is_fence,
            },
            None => LineKind::Paragraph,
        }
    }
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

    // Now determine the layers for each line using tree-sitter
    lines
        .into_iter()
        .map(|(line_num, range)| {
            let layers = if let Some(tree) = &tree {
                build_layers(tree, &text, &range)
            } else {
                Vec::new()
            };

            // Check if this is an image-only line
            let (image_url, image_alt) = if let Some(tree) = &tree {
                detect_image_only_line(tree, &text, &range)
            } else {
                (None, None)
            };

            LineInfo {
                range,
                line_number: line_num,
                layers,
                image_url,
                image_alt,
            }
        })
        .collect()
}

/// Build the layer stack for a line by walking the tree-sitter AST.
fn build_layers(tree: &MarkdownTree, text: &str, range: &Range<usize>) -> Vec<LineLayer> {
    let root = tree.block_tree().root_node();
    let mut layers = Vec::new();

    // First, check if this is inside a fenced code block
    if let Some(code_block) = find_containing_code_block(&root, range.start) {
        let line_text = &text[range.clone()];
        let is_fence = line_text.trim_start().starts_with("```");
        let language = extract_code_block_language(&code_block, text);

        layers.push(LineLayer {
            kind: LayerKind::CodeBlock { language, is_fence },
            // Fence lines don't hide their content - they're styled specially in line_view
            marker_range: range.start..range.start,
        });
        return layers;
    }

    // For empty lines, return no layers
    if range.start == range.end {
        return layers;
    }

    // Walk through the line text to find markers and build layers
    let line_text = &text[range.clone()];
    let mut pos = 0;

    // Find blockquote markers (> )
    while pos < line_text.len() {
        let rest = &line_text[pos..];
        if rest.starts_with("> ") {
            layers.push(LineLayer {
                kind: LayerKind::BlockQuote,
                marker_range: (range.start + pos)..(range.start + pos + 2),
            });
            pos += 2;
        } else if rest.starts_with(">")
            && (rest.len() == 1 || rest.chars().nth(1).map(|c| c == '>').unwrap_or(false))
        {
            // Handle ">>" without spaces
            layers.push(LineLayer {
                kind: LayerKind::BlockQuote,
                marker_range: (range.start + pos)..(range.start + pos + 1),
            });
            pos += 1;
        } else {
            break;
        }
    }

    // Skip any remaining whitespace
    while pos < line_text.len() && line_text[pos..].starts_with(' ') {
        pos += 1;
    }

    let rest = &line_text[pos..];

    // Check for heading
    if rest.starts_with('#') {
        let hashes = rest.chars().take_while(|&c| c == '#').count();
        if hashes <= 6 && rest.chars().nth(hashes) == Some(' ') {
            layers.push(LineLayer {
                kind: LayerKind::Heading(hashes as u8),
                marker_range: (range.start + pos)..(range.start + pos + hashes + 1),
            });
            return layers;
        }
    }

    // Check for list item
    if rest.starts_with("- ") || rest.starts_with("* ") {
        let marker_len = 2;
        let after_marker = &rest[marker_len..];

        // Check for task list
        let (checked, total_marker_len) = if after_marker.starts_with("[ ] ") {
            (Some(false), marker_len + 4)
        } else if after_marker.starts_with("[x] ") || after_marker.starts_with("[X] ") {
            (Some(true), marker_len + 4)
        } else if after_marker.starts_with("[ ]") {
            (Some(false), marker_len + 3)
        } else if after_marker.starts_with("[x]") || after_marker.starts_with("[X]") {
            (Some(true), marker_len + 3)
        } else {
            (None, marker_len)
        };

        layers.push(LineLayer {
            kind: LayerKind::ListItem {
                ordered: false,
                checked,
            },
            marker_range: (range.start + pos)..(range.start + pos + total_marker_len),
        });
        return layers;
    }

    // Check for ordered list
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after_digits = &rest[digits.len()..];
        if after_digits.starts_with(". ") {
            layers.push(LineLayer {
                kind: LayerKind::ListItem {
                    ordered: true,
                    checked: None,
                },
                marker_range: (range.start + pos)..(range.start + pos + digits.len() + 2),
            });
            return layers;
        } else if after_digits.starts_with(".") {
            layers.push(LineLayer {
                kind: LayerKind::ListItem {
                    ordered: true,
                    checked: None,
                },
                marker_range: (range.start + pos)..(range.start + pos + digits.len() + 1),
            });
            return layers;
        }
    }

    // Check for empty list marker (- or * without content, recognized after pressing Enter)
    if rest == "-" || rest == "*" {
        layers.push(LineLayer {
            kind: LayerKind::ListItem {
                ordered: false,
                checked: None,
            },
            marker_range: (range.start + pos)..(range.start + pos + 1),
        });
        return layers;
    }

    layers
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
        if node.kind() == "fenced_code_block" {
            if node.start_byte() <= pos && pos <= node.end_byte() {
                return Some(node);
            }
        }

        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.start_byte() <= pos
                    && pos <= child.end_byte()
                    && let Some(found) = search(child, pos)
                {
                    return Some(found);
                }
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

    #[test]
    fn test_empty_buffer() {
        let buf: Buffer = "".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind(), LineKind::Blank);
        assert_eq!(lines[0].range, 0..0);
    }

    #[test]
    fn test_single_newline() {
        let buf: Buffer = "\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind(), LineKind::Blank);
        assert_eq!(lines[0].range, 0..0);
        assert_eq!(lines[1].kind(), LineKind::Blank);
        assert_eq!(lines[1].range, 1..1);
    }

    #[test]
    fn test_blank_line_between_paragraphs() {
        let buf: Buffer = "Hello\n\nWorld\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].kind(), LineKind::Paragraph);
        assert_eq!(lines[0].range, 0..5);
        assert_eq!(lines[1].kind(), LineKind::Blank);
        assert_eq!(lines[1].range, 6..6);
        assert_eq!(lines[2].kind(), LineKind::Paragraph);
        assert_eq!(lines[2].range, 7..12);
        assert_eq!(lines[3].kind(), LineKind::Blank);
        assert_eq!(lines[3].range, 13..13);
    }

    #[test]
    fn test_heading_line() {
        let buf: Buffer = "# Hello\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind(), LineKind::Heading(1));
        assert_eq!(lines[0].marker_range(), Some(0..2));
    }

    #[test]
    fn test_heading_levels() {
        let buf: Buffer = "# H1\n## H2\n### H3\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind(), LineKind::Heading(1));
        assert_eq!(lines[1].kind(), LineKind::Heading(2));
        assert_eq!(lines[2].kind(), LineKind::Heading(3));
    }

    #[test]
    fn test_unordered_list() {
        let buf: Buffer = "- Item 1\n- Item 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            lines[0].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(lines[0].marker_range(), Some(0..2));
        assert_eq!(
            lines[1].kind(),
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
            lines[0].kind(),
            LineKind::ListItem {
                ordered: true,
                checked: None
            }
        );
        assert_eq!(lines[0].marker_range(), Some(0..3));
    }

    #[test]
    fn test_task_list() {
        let buf: Buffer = "- [ ] Unchecked\n- [x] Checked\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(
            lines[0].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: Some(false)
            }
        );
        assert_eq!(
            lines[1].kind(),
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
            lines[0].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(
            lines[1].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        // Nested list item has 1 layer with indentation preserved
        assert_eq!(lines[1].layers.len(), 1);
        assert_eq!(
            lines[2].kind(),
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

        assert_eq!(lines[0].kind(), LineKind::BlockQuote);
        assert_eq!(lines[0].marker_range(), Some(0..2));
        assert_eq!(lines[1].kind(), LineKind::BlockQuote);
    }

    #[test]
    fn test_nested_blockquote() {
        let buf: Buffer = "> Level 1\n> > Level 2\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert_eq!(lines[0].kind(), LineKind::BlockQuote);
        assert_eq!(lines[0].layers.len(), 1);
        assert_eq!(lines[0].marker_range(), Some(0..2));
        assert_eq!(lines[1].kind(), LineKind::BlockQuote);
        assert_eq!(lines[1].layers.len(), 2);
        assert_eq!(lines[1].marker_range(), Some(10..14));
    }

    #[test]
    fn test_list_in_blockquote() {
        let buf: Buffer = "> - Item 1\n>   - Nested item\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // First line: has blockquote and list item layers
        assert_eq!(
            lines[0].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(lines[0].layers.len(), 2);

        // Second line: also has blockquote and list item
        assert_eq!(
            lines[1].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(lines[1].layers.len(), 2);
    }

    #[test]
    fn test_blockquote_in_list() {
        let buf: Buffer = "- > Quoted item\n".parse().unwrap();
        let lines = extract_lines(&buf);

        // Layer-based: list item is detected, "> " is just content
        assert_eq!(
            lines[0].kind(),
            LineKind::ListItem {
                ordered: false,
                checked: None
            }
        );
        assert_eq!(lines[0].layers.len(), 1);
    }

    #[test]
    fn test_code_block() {
        let buf: Buffer = "```rust\nlet x = 1;\n```\n".parse().unwrap();
        let lines = extract_lines(&buf);

        assert!(matches!(
            lines[0].kind(),
            LineKind::CodeBlock { is_fence: true, .. }
        ));
        assert!(matches!(
            lines[1].kind(),
            LineKind::CodeBlock {
                is_fence: false,
                ..
            }
        ));
        assert!(matches!(
            lines[2].kind(),
            LineKind::CodeBlock { is_fence: true, .. }
        ));
    }
}
