use gpui::{App, Global, ReadGlobal, div, prelude::*, px, rems};

use crate::config::Config;
use crate::editor::EditorTheme;

/// A single context marker for the status bar.
#[derive(Clone, Debug)]
pub enum ContextMarker {
    BlockQuote,
    UnorderedList,
    OrderedList,
    CheckboxUnchecked,
    CheckboxChecked,
    Indent,
    CodeBlock(Option<String>), // language
}

impl ContextMarker {
    /// Convert marker to its string representation.
    pub fn as_str(&self) -> String {
        match self {
            ContextMarker::BlockQuote => ">".to_string(),
            ContextMarker::UnorderedList => "•".to_string(),
            ContextMarker::OrderedList => "1.".to_string(),
            ContextMarker::CheckboxUnchecked => "[ ]".to_string(),
            ContextMarker::CheckboxChecked => "[x]".to_string(),
            ContextMarker::Indent => "  ".to_string(),
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
    let scroll_str = if info.total_lines <= 1 {
        "All".to_string()
    } else if info.first_visible_line == 0 && info.last_visible_line >= info.total_lines - 1 {
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

    // Build context marker string
    let marker_str: String = info
        .context_markers
        .iter()
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(" ");

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
                    // Left: context markers
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(marker_str),
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
