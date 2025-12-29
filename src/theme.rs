use gpui::{Global, Rgba, rgb, rgba};

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
