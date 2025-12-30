//! Shared types for text styling and rendering.

use std::ops::Range;

/// A text style to apply during rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    /// Heading level (1-6), or 0 for non-heading text
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

    /// Create a heading style.
    pub fn heading(level: u8) -> Self {
        Self {
            heading_level: level,
            bold: true, // Headings are bold
            ..Default::default()
        }
    }

    /// Merge another style into this one.
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

/// A styled region within inline content.
/// Represents text that has styling (bold, italic, code, link, etc.)
/// with separate ranges for the full syntax and the visible content.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledRegion {
    /// The full range including markers (e.g., "**bold**")
    pub full_range: Range<usize>,
    /// The content range without markers (e.g., "bold")
    pub content_range: Range<usize>,
    /// The style to apply
    pub style: TextStyle,
    /// URL for links (None for non-link regions)
    pub link_url: Option<String>,
}
