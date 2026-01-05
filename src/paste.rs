//! Context-aware paste handling for markdown.
//!
//! This module transforms pasted content to intelligently merge with the
//! current document structure, respecting container markers and markdown semantics.

use crate::buffer::Buffer;
use crate::marker::{LineMarkers, MarkerKind};

/// Context about the cursor position for paste operations.
#[derive(Debug, Clone)]
pub struct PasteContext {
    /// Whether the cursor is at a block boundary (empty line with only markers).
    pub at_block_boundary: bool,
    /// Whether the cursor is inside a code block.
    pub in_code_block: bool,
    /// The container prefix to apply to each line (e.g., "> " for blockquotes).
    pub container_prefix: String,
    /// Number of blockquote levels in current context.
    pub blockquote_depth: usize,
    /// Whether we're inside a list item.
    pub in_list: bool,
}

impl PasteContext {
    /// Analyze the buffer at cursor position to determine paste context.
    pub fn from_buffer(buffer: &Buffer, cursor_offset: usize) -> Self {
        let line_idx = buffer.byte_to_line(cursor_offset);
        let lines = buffer.lines();
        let current_line = lines.get(line_idx);

        // Check if in code block
        let in_code_block = Self::is_in_code_block(lines, line_idx);

        // Determine container prefix and depth
        let (container_prefix, blockquote_depth, in_list) = if let Some(line) = current_line {
            let prefix = line.continuation_rope(buffer.rope());
            let bq_depth = line
                .markers
                .iter()
                .filter(|m| matches!(m.kind, MarkerKind::BlockQuote))
                .count();
            let has_list = line.markers.iter().any(|m| {
                matches!(
                    m.kind,
                    MarkerKind::ListItem { .. } | MarkerKind::TaskList { .. }
                )
            });
            (prefix, bq_depth, has_list)
        } else {
            (String::new(), 0, false)
        };

        // Check if at block boundary: cursor at content_start on a line with no content
        let at_block_boundary = if let Some(line) = current_line {
            let content_start = line.content_start();
            let content_end = line.range.end;
            // At block boundary if cursor is at content start and there's no content after markers
            cursor_offset == content_start && content_start >= content_end
        } else {
            true
        };

        Self {
            at_block_boundary,
            in_code_block,
            container_prefix,
            blockquote_depth,
            in_list,
        }
    }

    /// Check if a line index is inside a code block.
    fn is_in_code_block(lines: &[LineMarkers], target_line: usize) -> bool {
        let mut in_code = false;
        for (idx, line) in lines.iter().enumerate() {
            if idx > target_line {
                break;
            }

            for marker in &line.markers {
                if let MarkerKind::CodeBlockFence { is_opening, .. } = marker.kind {
                    in_code = is_opening;
                }
            }

            // Check if on the opening fence line itself - cursor is "in" code block
            // only if it's after the fence line
            if idx == target_line {
                // If we just saw an opening fence on this line, we're not inside yet
                let has_opening_fence = line.markers.iter().any(|m| {
                    matches!(
                        m.kind,
                        MarkerKind::CodeBlockFence {
                            is_opening: true,
                            ..
                        }
                    )
                });
                if has_opening_fence {
                    return false;
                }
                // If we just saw a closing fence on this line, we're still inside
                // (the fence itself is still part of the code block context)
                let has_closing_fence = line.markers.iter().any(|m| {
                    matches!(
                        m.kind,
                        MarkerKind::CodeBlockFence {
                            is_opening: false,
                            ..
                        }
                    )
                });
                if has_closing_fence {
                    return true;
                }
            }
        }
        in_code
    }
}

/// Preprocess pasted content: trim and normalize line endings.
fn preprocess(text: &str) -> String {
    text.trim().replace("\r\n", "\n").replace('\r', "\n")
}

/// Count leading blockquote markers in a line.
fn count_blockquote_markers(line: &str) -> usize {
    let mut count = 0;
    let mut chars = line.chars().peekable();

    loop {
        // Skip whitespace
        while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
            chars.next();
        }
        // Check for >
        if chars.peek() == Some(&'>') {
            chars.next();
            count += 1;
            // Skip optional space after >
            if chars.peek() == Some(&' ') {
                chars.next();
            }
        } else {
            break;
        }
    }
    count
}

/// Strip N levels of blockquote markers from a line.
fn strip_blockquote_markers(line: &str, levels: usize) -> String {
    if levels == 0 {
        return line.to_string();
    }

    let mut result = line;
    for _ in 0..levels {
        result = result.trim_start();
        if result.starts_with('>') {
            result = &result[1..];
            if result.starts_with(' ') {
                result = &result[1..];
            }
        } else {
            break;
        }
    }
    result.to_string()
}

/// Check if a line starts with a heading marker.
fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#')
        && trimmed
            .chars()
            .take_while(|&c| c == '#')
            .count()
            .min(6)
            .gt(&0)
        && trimmed
            .chars()
            .nth(trimmed.chars().take_while(|&c| c == '#').count())
            .map_or_else(|| true, |c| c == ' ' || c == '\t')
}

/// Strip heading marker from a line.
fn strip_heading_marker(line: &str) -> String {
    let trimmed = line.trim_start();
    if !is_heading(line) {
        return line.to_string();
    }
    let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
    let after_hashes = &trimmed[hash_count..];
    if let Some(stripped) = after_hashes.strip_prefix(' ') {
        stripped.to_string()
    } else {
        after_hashes.to_string()
    }
}

/// Check if a line starts a fenced code block.
fn is_code_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Check if a line is a list item (-, *, +, or ordered).
fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    // Unordered: -, *, + followed by space
    if trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed == "-"
        || trimmed == "*"
        || trimmed == "+"
    {
        return true;
    }
    // Task list: - [ ] or - [x]
    if trimmed.starts_with("- [ ] ")
        || trimmed.starts_with("- [x] ")
        || trimmed.starts_with("- [X] ")
    {
        return true;
    }
    // Ordered: digits followed by . or ) and space
    let mut chars = trimmed.chars().peekable();
    let mut has_digit = false;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        chars.next();
        has_digit = true;
    }
    if has_digit
        && let Some(c) = chars.next()
        && (c == '.' || c == ')')
    {
        return chars.next().is_none_or(|c| c == ' ');
    }
    false
}

/// Strip blockquote markers from a line based on context depth.
fn strip_markers_from_line(line: &str, ctx: &PasteContext) -> String {
    let mut processed = line.to_string();

    // Strip matching blockquote markers
    if ctx.blockquote_depth > 0 {
        let line_bq_depth = count_blockquote_markers(&processed);
        let to_strip = line_bq_depth.min(ctx.blockquote_depth);
        processed = strip_blockquote_markers(&processed, to_strip);
    }

    // Strip heading markers if in a container
    if (ctx.blockquote_depth > 0 || ctx.in_list) && is_heading(&processed) {
        processed = strip_heading_marker(&processed);
    }

    // Strip leading whitespace (indented code blocks become plain text)
    processed.trim_start().to_string()
}

/// Transform pasted content based on context.
pub fn transform_paste(text: &str, ctx: &PasteContext) -> String {
    let preprocessed = preprocess(text);

    // If pasting into a code block, just apply container prefix literally
    if ctx.in_code_block {
        return apply_prefix_to_lines(&preprocessed, &ctx.container_prefix);
    }

    // Split into blocks (separated by double newlines)
    let blocks: Vec<&str> = preprocessed.split("\n\n").collect();

    let mut result_blocks = Vec::new();

    for block in blocks {
        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            continue;
        }

        // Check if this block is a code fence
        let is_code_block_start = is_code_fence(lines[0]);

        if is_code_block_start {
            if ctx.at_block_boundary {
                // Preserve code block structure, but strip markers from each line first
                let processed_lines: Vec<String> = lines
                    .iter()
                    .map(|line| strip_markers_from_line(line, ctx))
                    .collect();
                let processed_block = processed_lines.join("\n");
                result_blocks.push(processed_block);
            } else {
                // Collapse to inline: strip markers, then join all lines with spaces
                let processed_lines: Vec<String> = lines
                    .iter()
                    .map(|line| strip_markers_from_line(line, ctx))
                    .collect();
                let inline = processed_lines.join(" ");
                result_blocks.push(inline);
            }
            continue;
        }

        // Process each line in the block
        let processed_lines: Vec<String> = lines
            .iter()
            .map(|line| strip_markers_from_line(line, ctx))
            .collect();

        // Check if first processed line is a list item
        let first_is_list = processed_lines
            .first()
            .map(|l| is_list_item(l))
            .unwrap_or(false);

        // If at content position (not block boundary), join with spaces
        if !ctx.at_block_boundary && !first_is_list {
            let joined = processed_lines.join(" ");
            result_blocks.push(joined);
        } else {
            // At block boundary or list: preserve structure
            // For lists, each item should be on its own line with blank line between
            if first_is_list {
                // Join list items with blank lines
                let list_result = processed_lines.join("\n\n");
                result_blocks.push(list_result);
            } else {
                result_blocks.push(processed_lines.join("\n"));
            }
        }
    }

    // Join blocks with double newlines, then apply container prefix
    let joined = result_blocks.join("\n\n");

    // Apply container prefix to each line
    apply_prefix_to_lines(&joined, &ctx.container_prefix)
}

/// Apply a prefix to each line in the text.
fn apply_prefix_to_lines(text: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return text.to_string();
    }

    text.lines()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                // First line: cursor is already positioned, don't add prefix
                line.to_string()
            } else if line.is_empty() {
                // Empty line in block separator: just use continuation marker for blockquotes
                // Extract just the blockquote part of the prefix ("> " repeated)
                let bq_prefix: String = prefix
                    .chars()
                    .collect::<String>()
                    .split_inclusive(' ')
                    .filter(|s| s.trim() == ">" || s.starts_with('>'))
                    .collect();
                if bq_prefix.is_empty() {
                    String::new()
                } else {
                    bq_prefix.trim_end().to_string()
                }
            } else {
                format!("{}{}", prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a simple context for testing
    fn ctx(
        at_block_boundary: bool,
        in_code_block: bool,
        prefix: &str,
        bq_depth: usize,
    ) -> PasteContext {
        PasteContext {
            at_block_boundary,
            in_code_block,
            container_prefix: prefix.to_string(),
            blockquote_depth: bq_depth,
            in_list: false,
        }
    }

    #[test]
    fn test_preprocess_trim() {
        assert_eq!(preprocess("  hello  "), "hello");
        assert_eq!(preprocess("\n\nhello\n\n"), "hello");
    }

    #[test]
    fn test_preprocess_crlf() {
        assert_eq!(preprocess("a\r\nb"), "a\nb");
        assert_eq!(preprocess("a\rb"), "a\nb");
    }

    #[test]
    fn test_count_blockquote_markers() {
        assert_eq!(count_blockquote_markers("hello"), 0);
        assert_eq!(count_blockquote_markers("> hello"), 1);
        assert_eq!(count_blockquote_markers("> > hello"), 2);
        assert_eq!(count_blockquote_markers(">> hello"), 2);
        assert_eq!(count_blockquote_markers("  > hello"), 1);
    }

    #[test]
    fn test_strip_blockquote_markers() {
        assert_eq!(strip_blockquote_markers("> hello", 1), "hello");
        assert_eq!(strip_blockquote_markers("> > hello", 1), "> hello");
        assert_eq!(strip_blockquote_markers("> > hello", 2), "hello");
        assert_eq!(strip_blockquote_markers("hello", 1), "hello");
    }

    #[test]
    fn test_is_heading() {
        assert!(is_heading("# Title"));
        assert!(is_heading("## Title"));
        assert!(is_heading("### Title"));
        assert!(is_heading("  # Title"));
        assert!(!is_heading("hello"));
        assert!(!is_heading("#hashtag"));
    }

    #[test]
    fn test_strip_heading_marker() {
        assert_eq!(strip_heading_marker("# Title"), "Title");
        assert_eq!(strip_heading_marker("## Title"), "Title");
        assert_eq!(strip_heading_marker("hello"), "hello");
    }

    #[test]
    fn test_is_code_fence() {
        assert!(is_code_fence("```"));
        assert!(is_code_fence("```rust"));
        assert!(is_code_fence("~~~"));
        assert!(is_code_fence("  ```"));
        assert!(!is_code_fence("hello"));
    }

    #[test]
    fn test_is_list_item() {
        assert!(is_list_item("- item"));
        assert!(is_list_item("* item"));
        assert!(is_list_item("+ item"));
        assert!(is_list_item("1. item"));
        assert!(is_list_item("10. item"));
        assert!(is_list_item("1) item"));
        assert!(is_list_item("- [ ] task"));
        assert!(is_list_item("- [x] task"));
        assert!(!is_list_item("hello"));
        assert!(!is_list_item("-hello"));
    }

    // Example 1: Plain text into blockquote
    #[test]
    fn test_plain_text_into_blockquote() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("line 1\nline 2", &ctx);
        assert_eq!(result, "line 1 line 2");
    }

    // Example 2: Paragraphs into blockquote
    #[test]
    fn test_paragraphs_into_blockquote() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("para 1\n\npara 2", &ctx);
        assert_eq!(result, "para 1\n>\n> para 2");
    }

    // Example 3: Blockquote into blockquote at block boundary
    #[test]
    fn test_blockquote_into_blockquote_block_boundary() {
        let ctx = ctx(true, false, "> ", 1);
        let result = transform_paste("> quoted", &ctx);
        assert_eq!(result, "quoted");
    }

    // Example 4: Nested quote into blockquote
    #[test]
    fn test_nested_quote_into_blockquote() {
        let ctx = ctx(true, false, "> ", 1);
        let result = transform_paste("> > nested", &ctx);
        assert_eq!(result, "> nested");
    }

    // Example 9: Heading into blockquote
    #[test]
    fn test_heading_into_blockquote() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("# Title", &ctx);
        assert_eq!(result, "Title");
    }

    // Example 10: Heading at root
    #[test]
    fn test_heading_at_root() {
        let ctx = ctx(true, false, "", 0);
        let result = transform_paste("# Title", &ctx);
        assert_eq!(result, "# Title");
    }

    // Example 14: Trailing newlines trimmed
    #[test]
    fn test_trailing_newlines_trimmed() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("text\n\n", &ctx);
        assert_eq!(result, "text");
    }

    // Example 15: Indented code becomes text
    #[test]
    fn test_indented_code_becomes_text() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("    code", &ctx);
        assert_eq!(result, "code");
    }

    // Example 16: CRLF line endings
    #[test]
    fn test_crlf_line_endings() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("a\r\nb", &ctx);
        assert_eq!(result, "a b");
    }

    // Example 17: URL preserved
    #[test]
    fn test_url_preserved() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("https://example.com", &ctx);
        assert_eq!(result, "https://example.com");
    }

    // Example 18: Markdown link preserved
    #[test]
    fn test_markdown_link_preserved() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("[text](url)", &ctx);
        assert_eq!(result, "[text](url)");
    }

    // Example 8: Code with > into code block (literal paste)
    #[test]
    fn test_paste_into_code_block() {
        let ctx = ctx(false, true, "> ", 1);
        let result = transform_paste("> text", &ctx);
        // In code block: literal paste with prefix on subsequent lines
        assert_eq!(result, "> text");
    }

    #[test]
    fn test_multiline_paste_into_code_block() {
        let ctx = ctx(false, true, "> ", 1);
        let result = transform_paste("line 1\nline 2", &ctx);
        assert_eq!(result, "line 1\n> line 2");
    }

    // Example 6: Code block at boundary preserved
    #[test]
    fn test_code_block_at_boundary() {
        let ctx = ctx(true, false, "> ", 1);
        let result = transform_paste("```rust\ncode\n```", &ctx);
        assert_eq!(result, "```rust\n> code\n> ```");
    }

    // Example 7: Code block mid-line becomes inline
    #[test]
    fn test_code_block_midline() {
        let ctx = ctx(false, false, "> ", 1);
        let result = transform_paste("```rust\ncode\n```", &ctx);
        assert_eq!(result, "```rust code ```");
    }

    // Example 5: List into blockquote
    #[test]
    fn test_list_into_blockquote() {
        let ctx = ctx(true, false, "> ", 1);
        let result = transform_paste("> - item 1\n> - item 2", &ctx);
        // Strip outer >, list items separated by blank lines
        assert_eq!(result, "- item 1\n>\n> - item 2");
    }
}
