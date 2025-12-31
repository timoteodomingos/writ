use std::path::PathBuf;

use gpui::Rems;

use super::theme::{DEFAULT_CODE_FONT, DEFAULT_TEXT_FONT, EditorTheme};

#[derive(Clone)]
pub struct EditorConfig {
    pub theme: EditorTheme,
    pub text_font: String,
    pub code_font: String,
    pub base_path: Option<PathBuf>,
    pub padding_x: Rems,
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
