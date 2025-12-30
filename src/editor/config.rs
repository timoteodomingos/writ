use std::path::PathBuf;

use gpui::Rems;

use super::theme::{DEFAULT_CODE_FONT, DEFAULT_TEXT_FONT, EditorTheme};

/// Configuration for the editor.
#[derive(Clone)]
pub struct EditorConfig {
    /// Theme colors
    pub theme: EditorTheme,
    /// Font for regular text
    pub text_font: String,
    /// Font for code blocks and inline code
    pub code_font: String,
    /// Base path for resolving relative image URLs
    pub base_path: Option<PathBuf>,
    /// Horizontal padding (left and right)
    pub padding_x: Rems,
    /// Vertical padding (top and bottom) - rendered as spacers that scroll with content
    pub padding_y: Rems,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            theme: EditorTheme::default(),
            text_font: DEFAULT_TEXT_FONT.to_string(),
            code_font: DEFAULT_CODE_FONT.to_string(),
            base_path: None,
            padding_x: Rems(0.0),
            padding_y: Rems(0.0),
        }
    }
}
