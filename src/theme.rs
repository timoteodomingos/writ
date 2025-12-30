use gpui::{Global, Rgba, rgb, rgba};

use crate::highlight::HIGHLIGHT_NAMES;

#[allow(dead_code)]
pub struct Theme {
    pub background: Rgba,
    pub current_line: Rgba,
    pub selection: Rgba,
    pub foreground: Rgba,
    pub comment: Rgba,
    pub red: Rgba,
    pub orange: Rgba,
    pub yellow: Rgba,
    pub green: Rgba,
    pub cyan: Rgba,
    pub purple: Rgba,
    pub pink: Rgba,
}

impl Global for Theme {}

impl Theme {
    /// Map a tree-sitter highlight capture name to a color.
    ///
    /// Capture names follow conventions like:
    /// - `keyword`, `keyword.control` -> pink
    /// - `function`, `function.definition` -> green
    /// - `type`, `type.builtin` -> cyan
    /// - `string`, `string.escape` -> yellow
    /// - `number`, `boolean` -> purple
    /// - `comment`, `comment.doc` -> comment gray
    /// - `variable`, `property` -> foreground
    /// - `operator`, `punctuation` -> foreground
    pub fn color_for_capture(&self, capture: &str) -> Rgba {
        // Handle specific sub-captures first
        match capture {
            // self keyword (variable.special in Zed's query)
            "variable.special" => return self.purple,
            // Function parameters
            "variable.parameter" => return self.orange,
            // Brackets and parens
            "punctuation.bracket" => return self.foreground,
            // # in attribute macros
            "punctuation.special" => return self.pink,
            // Escape sequences in strings
            "string.escape" => return self.pink,
            // Lifetimes
            "lifetime" => return self.pink,
            _ => {}
        }

        // Split by '.' to handle nested captures like "function.definition"
        let base = capture.split('.').next().unwrap_or(capture);

        match base {
            // Keywords (pink in Dracula)
            "keyword" => self.pink,

            // Functions (green in Dracula)
            "function" => self.green,

            // Types (cyan in Dracula)
            "type" => self.cyan,

            // Strings (yellow in Dracula)
            "string" => self.yellow,

            // Numbers and booleans (purple in Dracula)
            "number" | "boolean" => self.purple,

            // Comments (gray/comment color)
            "comment" => self.comment,

            // Constants (purple)
            "constant" => self.purple,

            // Operators (pink for visibility)
            "operator" => self.pink,

            // Attributes (pink for macro-like appearance)
            "attribute" => self.pink,

            // Properties/fields (cyan, same as types)
            "property" => self.cyan,

            // Punctuation (foreground)
            "punctuation" => self.foreground,

            // Default to foreground
            _ => self.foreground,
        }
    }

    /// Map a tree-sitter highlight ID to a color.
    ///
    /// The highlight ID is an index into HIGHLIGHT_NAMES.
    pub fn color_for_highlight(&self, highlight_id: usize) -> Rgba {
        let capture = HIGHLIGHT_NAMES.get(highlight_id).copied().unwrap_or("");
        self.color_for_capture(capture)
    }
}

pub fn dracula() -> Theme {
    Theme {
        background: rgb(0x282A36),
        current_line: rgb(0x6272A4),
        selection: rgba(0x44475A99), // Semi-transparent selection
        foreground: rgb(0xF8F8F2),
        comment: rgb(0x6272A4),
        red: rgb(0xFF5555),
        orange: rgb(0xFFB86C),
        yellow: rgb(0xF1FA8C),
        green: rgb(0x50FA7B),
        cyan: rgb(0x8BE9FD),
        purple: rgb(0xBD93F9),
        pink: rgb(0xFF79C6),
    }
}
