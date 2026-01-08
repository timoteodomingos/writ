//! Context-aware paste handling for markdown.
//!
//! This module handles pasted content with minimal transformation:
//! - Normalizes line endings (CRLF → LF)
//! - Normalizes curly quotes to straight quotes
//! - Preserves content literally in code blocks
//! - Adds blockquote prefixes to continuation lines when in a blockquote

use crate::buffer::Buffer;
use crate::marker::{LineMarkers, MarkerKind};

/// Context about the cursor position for paste operations.
#[derive(Debug, Clone)]
pub struct PasteContext {
    /// Whether the cursor is inside a code block.
    pub in_code_block: bool,
    /// The blockquote prefix to apply to continuation lines (e.g., "> " or "> > ").
    pub blockquote_prefix: String,
}

impl PasteContext {
    /// Analyze the buffer at cursor position to determine paste context.
    pub fn from_buffer(buffer: &Buffer, cursor_offset: usize) -> Self {
        let line_idx = buffer.byte_to_line(cursor_offset);
        let lines = buffer.lines();
        let current_line = lines.get(line_idx);

        // Check if in code block
        let in_code_block = Self::is_in_code_block(lines, line_idx);

        // Build blockquote prefix from markers
        let blockquote_prefix = if let Some(line) = current_line {
            let bq_depth = line
                .markers
                .iter()
                .filter(|m| matches!(m.kind, MarkerKind::BlockQuote))
                .count();
            "> ".repeat(bq_depth)
        } else {
            String::new()
        };

        Self {
            in_code_block,
            blockquote_prefix,
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

/// Normalize line endings and curly quotes.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace(['\u{201C}', '\u{201D}'], "\"") // curly double quotes " "
        .replace(['\u{2018}', '\u{2019}'], "'") // curly single quotes ' '
}

/// Transform pasted content based on context.
pub fn transform_paste(text: &str, ctx: &PasteContext) -> String {
    let normalized = normalize(text);

    // In code block: paste literally
    if ctx.in_code_block {
        return normalized;
    }

    // No blockquote: paste as-is
    if ctx.blockquote_prefix.is_empty() {
        return normalized;
    }

    // In blockquote: prefix continuation lines
    normalized
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                // First line: cursor is already positioned, no prefix needed
                line.to_string()
            } else if line.is_empty() {
                // Empty line: just the blockquote marker without trailing space
                ctx.blockquote_prefix.trim_end().to_string()
            } else {
                format!("{}{}", ctx.blockquote_prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(in_code_block: bool, blockquote_prefix: &str) -> PasteContext {
        PasteContext {
            in_code_block,
            blockquote_prefix: blockquote_prefix.to_string(),
        }
    }

    // Scenario 1: Paste "hello world" on normal line
    #[test]
    fn test_paste_simple_text_normal_line() {
        let ctx = ctx(false, "");
        let result = transform_paste("hello world", &ctx);
        assert_eq!(result, "hello world");
    }

    // Scenario 2: Paste multiline on normal line
    #[test]
    fn test_paste_multiline_normal_line() {
        let ctx = ctx(false, "");
        let result = transform_paste("line 1\nline 2", &ctx);
        assert_eq!(result, "line 1\nline 2");
    }

    // Scenario 3: Paste "> quoted" in code block (literal)
    #[test]
    fn test_paste_blockquote_in_code_block() {
        let ctx = ctx(true, "");
        let result = transform_paste("> quoted", &ctx);
        assert_eq!(result, "> quoted");
    }

    // Scenario 4: Paste "# heading" in code block (literal)
    #[test]
    fn test_paste_heading_in_code_block() {
        let ctx = ctx(true, "");
        let result = transform_paste("# heading", &ctx);
        assert_eq!(result, "# heading");
    }

    // Scenario 5: Paste "hello" in blockquote
    #[test]
    fn test_paste_simple_text_in_blockquote() {
        let ctx = ctx(false, "> ");
        let result = transform_paste("hello", &ctx);
        assert_eq!(result, "hello");
    }

    // Scenario 6: Paste multiline in blockquote
    #[test]
    fn test_paste_multiline_in_blockquote() {
        let ctx = ctx(false, "> ");
        let result = transform_paste("line 1\nline 2", &ctx);
        assert_eq!(result, "line 1\n> line 2");
    }

    // Scenario 7: Paste paragraphs in blockquote
    #[test]
    fn test_paste_paragraphs_in_blockquote() {
        let ctx = ctx(false, "> ");
        let result = transform_paste("para 1\n\npara 2", &ctx);
        assert_eq!(result, "para 1\n>\n> para 2");
    }

    // Scenario 8: Paste multiline in nested blockquote
    #[test]
    fn test_paste_multiline_in_nested_blockquote() {
        let ctx = ctx(false, "> > ");
        let result = transform_paste("line 1\nline 2", &ctx);
        assert_eq!(result, "line 1\n> > line 2");
    }

    // Scenario 9: Paste multiline on list item (no markers added)
    #[test]
    fn test_paste_multiline_on_list_item() {
        let ctx = ctx(false, "");
        let result = transform_paste("line 1\nline 2", &ctx);
        assert_eq!(result, "line 1\nline 2");
    }

    // Scenario 10: Paste curly quotes → straight quotes
    #[test]
    fn test_paste_curly_quotes_normalized() {
        let ctx = ctx(false, "");
        let result = transform_paste("\u{201C}hello\u{201D}", &ctx);
        assert_eq!(result, "\"hello\"");
    }

    // Additional: single curly quotes
    #[test]
    fn test_paste_single_curly_quotes_normalized() {
        let ctx = ctx(false, "");
        let result = transform_paste("\u{2018}hello\u{2019}", &ctx);
        assert_eq!(result, "'hello'");
    }

    // Additional: CRLF normalization
    #[test]
    fn test_paste_crlf_normalized() {
        let ctx = ctx(false, "");
        let result = transform_paste("line 1\r\nline 2", &ctx);
        assert_eq!(result, "line 1\nline 2");
    }

    // Additional: CR normalization
    #[test]
    fn test_paste_cr_normalized() {
        let ctx = ctx(false, "");
        let result = transform_paste("line 1\rline 2", &ctx);
        assert_eq!(result, "line 1\nline 2");
    }
}
