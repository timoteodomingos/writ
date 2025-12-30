use std::path::PathBuf;

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
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            theme: EditorTheme::default(),
            text_font: DEFAULT_TEXT_FONT.to_string(),
            code_font: DEFAULT_CODE_FONT.to_string(),
            base_path: None,
        }
    }
}
