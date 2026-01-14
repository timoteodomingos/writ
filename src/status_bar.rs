use gpui::{App, Global, ReadGlobal, div, prelude::*, px, rems};

use crate::config::Config;
use crate::editor::EditorTheme;

/// Status bar information updated by the editor on each render.
#[derive(Clone, Default)]
pub struct StatusBarInfo {
    /// Context markers string, e.g. "> - - _"
    pub context_markers: String,
    /// Current heading level (1-6), if cursor is under a heading
    pub heading_level: Option<u8>,
    /// Cursor line (1-indexed)
    pub cursor_line: usize,
    /// Cursor column (1-indexed)
    pub cursor_col: usize,
    /// Total number of lines in the document
    pub total_lines: usize,
}

impl Global for StatusBarInfo {}

/// Render the status bar at the bottom of the editor.
pub fn status_bar(cx: &App) -> impl IntoElement {
    let info = StatusBarInfo::global(cx);
    let theme = EditorTheme::global(cx);
    let config = Config::global(cx);

    // Build heading indicator (e.g. "###" for h3)
    let heading_str = info
        .heading_level
        .map(|level| "#".repeat(level as usize))
        .unwrap_or_default();

    // Left side: context markers + heading
    let left_content = if info.context_markers.is_empty() && heading_str.is_empty() {
        String::new()
    } else if info.context_markers.is_empty() {
        heading_str.clone()
    } else if heading_str.is_empty() {
        info.context_markers.clone()
    } else {
        format!("{}  {}", info.context_markers, heading_str)
    };

    // Right side: position info
    let percentage = if info.total_lines > 0 {
        (info.cursor_line * 100) / info.total_lines
    } else {
        0
    };
    let right_content = format!(
        "Ln {}, Col {}  {}%",
        info.cursor_line, info.cursor_col, percentage
    );

    div()
        .w_full()
        .py(rems(0.25))
        .px(rems(2.0)) // Padding on outer container, matching editor
        .bg(theme.background)
        .border_color(theme.selection)
        .border_t_1()
        .font_family(config.code_font.clone())
        .text_color(theme.comment)
        .child(
            // Inner container matching line layout
            div()
                .w_full()
                .max_w(px(800.0))
                .mx_auto()
                .flex()
                .flex_row()
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(left_content),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .child(right_content),
                ),
        )
}
