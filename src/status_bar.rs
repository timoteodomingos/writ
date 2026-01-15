use gpui::{App, Global, ReadGlobal, div, prelude::*, px, rems};

use crate::config::Config;
use crate::editor::EditorTheme;

/// Unordered list marker character for status bar display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnorderedListMarker {
    Minus,
    Star,
    Plus,
}

/// Ordered list marker style for status bar display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrderedListStyle {
    Dot,
    Parenthesis,
}

/// A single context marker for the status bar.
#[derive(Clone, Debug)]
pub enum ContextMarker {
    BlockQuote,
    UnorderedList(UnorderedListMarker),
    OrderedList {
        number: u32,
        style: OrderedListStyle,
    },
    CheckboxUnchecked,
    CheckboxChecked,
    CodeBlock(Option<String>), // language
}

impl ContextMarker {
    /// Convert marker to its string representation.
    pub fn as_str(&self) -> String {
        match self {
            ContextMarker::BlockQuote => ">".to_string(),
            ContextMarker::UnorderedList(marker) => match marker {
                UnorderedListMarker::Minus => "-".to_string(),
                UnorderedListMarker::Star => "*".to_string(),
                UnorderedListMarker::Plus => "+".to_string(),
            },
            ContextMarker::OrderedList { number, style } => match style {
                OrderedListStyle::Dot => format!("{}.", number),
                OrderedListStyle::Parenthesis => format!("{})", number),
            },
            ContextMarker::CheckboxUnchecked => "[ ]".to_string(),
            ContextMarker::CheckboxChecked => "[x]".to_string(),
            ContextMarker::CodeBlock(lang) => lang
                .as_ref()
                .map(|l| format!("```{}", l))
                .unwrap_or_else(|| "```".to_string()),
        }
    }
}

/// Status bar information updated by the editor on each render.
#[derive(Clone, Default)]
pub struct StatusBarInfo {
    /// Context markers for the current line
    pub context_markers: Vec<ContextMarker>,
    /// Current heading level (1-6), if cursor is under a heading
    pub heading_level: Option<u8>,
    /// Cursor line (1-indexed)
    pub cursor_line: usize,
    /// Cursor column (1-indexed)
    pub cursor_col: usize,
    /// Total number of lines in the document
    pub total_lines: usize,
    /// First visible line (0-indexed), for scroll percentage
    pub first_visible_line: usize,
    /// Last visible line (0-indexed), for scroll percentage
    pub last_visible_line: usize,
}

impl Global for StatusBarInfo {}

/// Render the status bar at the bottom of the editor.
pub fn status_bar(cx: &App) -> impl IntoElement {
    let info = StatusBarInfo::global(cx);
    let theme = EditorTheme::global(cx);
    let config = Config::global(cx);

    // Scroll indicator: Top/Bot/All or percentage
    let scroll_str = if info.total_lines <= 1
        || (info.first_visible_line == 0 && info.last_visible_line >= info.total_lines - 1)
    {
        "All".to_string()
    } else if info.first_visible_line == 0 {
        "Top".to_string()
    } else if info.last_visible_line >= info.total_lines - 1 {
        "Bot".to_string()
    } else {
        let pct = ((info.last_visible_line + 1) * 100) / info.total_lines;
        format!("{}%", pct)
    };

    // Position: Ln 42, Col 15
    let position_str = format!("Ln {}, Col {}", info.cursor_line, info.cursor_col);

    // Color palette for nesting depth (cycles when exhausted)
    let depth_colors = [
        theme.cyan,
        theme.purple,
        theme.green,
        theme.orange,
        theme.pink,
        theme.yellow,
    ];

    // Build colored marker elements, tracking depth
    // Each "level" is a blockquote, list item, or code block
    // Checkboxes share the same color as their parent list marker
    // Nested checkboxes after first show as `]` or `x]` (e.g., `- [x]] ]x]`)
    let mut depth = 0;
    let mut marker_elements: Vec<gpui::AnyElement> = Vec::new();
    let mut prev_was_checkbox = false;

    for (i, marker) in info.context_markers.iter().enumerate() {
        // Skip list marker if previous was checkbox
        let is_list_marker = matches!(
            marker,
            ContextMarker::UnorderedList(_) | ContextMarker::OrderedList { .. }
        );
        if is_list_marker && prev_was_checkbox {
            // Still increment depth for this level
            if i > 0 {
                depth += 1;
            }
            continue;
        }

        // Increment depth before block-level markers (except first)
        // This groups list marker + checkbox at the same depth
        if i > 0 {
            match marker {
                ContextMarker::BlockQuote
                | ContextMarker::UnorderedList(_)
                | ContextMarker::OrderedList { .. }
                | ContextMarker::CodeBlock(_) => {
                    depth += 1;
                }
                _ => {}
            }
        }

        // Determine display string and whether to add space
        let (display_str, needs_space) = match marker {
            // Nested checkbox: show as ` ]` or `x]` (space for unchecked, x for checked)
            ContextMarker::CheckboxUnchecked if prev_was_checkbox => (" ]".to_string(), false),
            ContextMarker::CheckboxChecked if prev_was_checkbox => ("x]".to_string(), false),
            // Normal marker
            _ => (marker.as_str(), true),
        };

        // Add space separator between markers (except for nested checkboxes)
        if !marker_elements.is_empty() && needs_space {
            marker_elements.push(div().child(" ").into_any_element());
        }

        let color = depth_colors[depth % depth_colors.len()];
        marker_elements.push(
            div()
                .text_color(color)
                .child(display_str)
                .into_any_element(),
        );

        prev_was_checkbox = matches!(
            marker,
            ContextMarker::CheckboxChecked | ContextMarker::CheckboxUnchecked
        );
    }

    div()
        .w_full()
        .py(rems(0.25))
        .px(rems(2.0))
        .bg(theme.background)
        .border_color(theme.selection)
        .border_t_1()
        .font_family(config.code_font.clone())
        .text_color(theme.comment)
        .child(
            div()
                .w_full()
                .max_w(px(800.0))
                .mx_auto()
                .flex()
                .flex_row()
                .justify_between()
                .child(
                    // Left: context markers with depth colors
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .flex()
                        .flex_row()
                        .children(marker_elements),
                )
                .child(
                    // Right: heading, position, lines, scroll
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .flex()
                        .flex_row()
                        .items_center()
                        .children(info.heading_level.map(|level| {
                            div()
                                .text_color(theme.cyan)
                                .mr(rems(1.0))
                                .child(format!("H{}", level))
                        }))
                        .child(position_str)
                        .child(div().mx(rems(0.5)).text_color(theme.selection).child("·"))
                        .child(format!("{} lines", info.total_lines))
                        .child(div().mx(rems(0.5)).text_color(theme.selection).child("·"))
                        .child(div().text_color(theme.purple).child(scroll_str)),
                ),
        )
}

/// Build the display string for context markers, handling nested checkbox compaction.
/// Returns a vector of (display_string, depth) tuples for testing/inspection.
pub fn build_context_display(markers: &[ContextMarker]) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut prev_was_checkbox = false;

    for (i, marker) in markers.iter().enumerate() {
        // Skip list marker if previous was checkbox
        let is_list_marker = matches!(
            marker,
            ContextMarker::UnorderedList(_) | ContextMarker::OrderedList { .. }
        );
        if is_list_marker && prev_was_checkbox {
            if i > 0 {
                depth += 1;
            }
            continue;
        }

        // Increment depth before block-level markers (except first)
        if i > 0 {
            match marker {
                ContextMarker::BlockQuote
                | ContextMarker::UnorderedList(_)
                | ContextMarker::OrderedList { .. }
                | ContextMarker::CodeBlock(_) => {
                    depth += 1;
                }
                _ => {}
            }
        }

        // Determine display string
        let display_str = match marker {
            ContextMarker::CheckboxUnchecked if prev_was_checkbox => " ]".to_string(),
            ContextMarker::CheckboxChecked if prev_was_checkbox => "x]".to_string(),
            _ => marker.as_str(),
        };

        result.push((display_str, depth));

        prev_was_checkbox = matches!(
            marker,
            ContextMarker::CheckboxChecked | ContextMarker::CheckboxUnchecked
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_blockquote() {
        assert_eq!(ContextMarker::BlockQuote.as_str(), ">");
    }

    #[test]
    fn as_str_unordered_list_minus() {
        assert_eq!(
            ContextMarker::UnorderedList(UnorderedListMarker::Minus).as_str(),
            "-"
        );
    }

    #[test]
    fn as_str_unordered_list_star() {
        assert_eq!(
            ContextMarker::UnorderedList(UnorderedListMarker::Star).as_str(),
            "*"
        );
    }

    #[test]
    fn as_str_unordered_list_plus() {
        assert_eq!(
            ContextMarker::UnorderedList(UnorderedListMarker::Plus).as_str(),
            "+"
        );
    }

    #[test]
    fn as_str_ordered_list_dot() {
        assert_eq!(
            ContextMarker::OrderedList {
                number: 1,
                style: OrderedListStyle::Dot
            }
            .as_str(),
            "1."
        );
        assert_eq!(
            ContextMarker::OrderedList {
                number: 42,
                style: OrderedListStyle::Dot
            }
            .as_str(),
            "42."
        );
    }

    #[test]
    fn as_str_ordered_list_parenthesis() {
        assert_eq!(
            ContextMarker::OrderedList {
                number: 1,
                style: OrderedListStyle::Parenthesis
            }
            .as_str(),
            "1)"
        );
        assert_eq!(
            ContextMarker::OrderedList {
                number: 10,
                style: OrderedListStyle::Parenthesis
            }
            .as_str(),
            "10)"
        );
    }

    #[test]
    fn as_str_checkbox_unchecked() {
        assert_eq!(ContextMarker::CheckboxUnchecked.as_str(), "[ ]");
    }

    #[test]
    fn as_str_checkbox_checked() {
        assert_eq!(ContextMarker::CheckboxChecked.as_str(), "[x]");
    }

    #[test]
    fn as_str_code_block_no_language() {
        assert_eq!(ContextMarker::CodeBlock(None).as_str(), "```");
    }

    #[test]
    fn as_str_code_block_with_language() {
        assert_eq!(
            ContextMarker::CodeBlock(Some("rust".to_string())).as_str(),
            "```rust"
        );
    }

    #[test]
    fn display_simple_list() {
        let markers = vec![ContextMarker::UnorderedList(UnorderedListMarker::Minus)];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("-".to_string(), 0)]);
    }

    #[test]
    fn display_nested_lists() {
        let markers = vec![
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
        ];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("-".to_string(), 0), ("-".to_string(), 1)]);
    }

    #[test]
    fn display_list_with_checkbox() {
        // - [x] displays as "- [x]" at same depth
        let markers = vec![
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxChecked,
        ];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("-".to_string(), 0), ("[x]".to_string(), 0)]);
    }

    #[test]
    fn display_nested_checkboxes_compact() {
        // - [x] - [ ] displays as "- [x] ]" (nested checkbox compacted)
        let markers = vec![
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxChecked,
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxUnchecked,
        ];
        let display = build_context_display(&markers);
        // After [x], the next - is skipped, depth increments, then [ ] shows as " ]"
        assert_eq!(
            display,
            vec![
                ("-".to_string(), 0),
                ("[x]".to_string(), 0),
                (" ]".to_string(), 1)
            ]
        );
    }

    #[test]
    fn display_nested_checked_checkbox_compact() {
        // - [ ] - [x] displays as "- [ ]x]"
        let markers = vec![
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxUnchecked,
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxChecked,
        ];
        let display = build_context_display(&markers);
        assert_eq!(
            display,
            vec![
                ("-".to_string(), 0),
                ("[ ]".to_string(), 0),
                ("x]".to_string(), 1)
            ]
        );
    }

    #[test]
    fn display_blockquote_list() {
        let markers = vec![
            ContextMarker::BlockQuote,
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
        ];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![(">".to_string(), 0), ("-".to_string(), 1)]);
    }

    #[test]
    fn display_ordered_list() {
        let markers = vec![ContextMarker::OrderedList {
            number: 3,
            style: OrderedListStyle::Dot,
        }];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("3.".to_string(), 0)]);
    }

    #[test]
    fn display_code_block() {
        let markers = vec![ContextMarker::CodeBlock(Some("python".to_string()))];
        let display = build_context_display(&markers);
        assert_eq!(display, vec![("```python".to_string(), 0)]);
    }

    #[test]
    fn display_deeply_nested() {
        // > - [x] - [ ] - [x]
        let markers = vec![
            ContextMarker::BlockQuote,
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxChecked,
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxUnchecked,
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::CheckboxChecked,
        ];
        let display = build_context_display(&markers);
        // > at depth 0, - [x] at depth 1, ] at depth 2, x] at depth 3
        assert_eq!(
            display,
            vec![
                (">".to_string(), 0),
                ("-".to_string(), 1),
                ("[x]".to_string(), 1),
                (" ]".to_string(), 2),
                ("x]".to_string(), 3),
            ]
        );
    }

    #[test]
    fn depth_cycles_after_six() {
        // 7 nested lists should cycle back to depth 0 for color
        let markers = vec![
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
            ContextMarker::UnorderedList(UnorderedListMarker::Minus),
        ];
        let display = build_context_display(&markers);
        assert_eq!(display.len(), 7);
        // Depths: 0, 1, 2, 3, 4, 5, 6
        assert_eq!(display[6].1, 6);
        // Color cycling happens in status_bar() with depth % 6
    }
}
