use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub heading_level: u8,
}

impl TextStyle {
    pub fn bold() -> Self {
        Self {
            bold: true,
            ..Default::default()
        }
    }

    pub fn italic() -> Self {
        Self {
            italic: true,
            ..Default::default()
        }
    }

    pub fn code() -> Self {
        Self {
            code: true,
            ..Default::default()
        }
    }

    pub fn strikethrough() -> Self {
        Self {
            strikethrough: true,
            ..Default::default()
        }
    }

    pub fn heading(level: u8) -> Self {
        Self {
            heading_level: level,
            bold: true, // Headings are bold
            ..Default::default()
        }
    }

    pub fn merge(&self, other: &TextStyle) -> Self {
        Self {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            code: self.code || other.code,
            strikethrough: self.strikethrough || other.strikethrough,
            heading_level: self.heading_level.max(other.heading_level),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyledRegion {
    pub full_range: Range<usize>,
    pub content_range: Range<usize>,
    pub style: TextStyle,
    pub link_url: Option<String>,
}
