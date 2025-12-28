//! Block view component for rendering individual blocks.

use gpui::{IntoElement, SharedString, StyledText, div, prelude::*};

use crate::blocks::RenderBlock;

/// A view component for rendering a single block.
pub struct BlockView<'a> {
    /// The block to render
    block: &'a RenderBlock,
    /// The full buffer text (for extracting block content)
    text: &'a str,
}

impl<'a> BlockView<'a> {
    /// Create a new block view.
    pub fn new(block: &'a RenderBlock, text: &'a str) -> Self {
        Self { block, text }
    }

    /// Get the text content to display for this block.
    fn display_text(&self) -> String {
        match self.block {
            RenderBlock::Paragraph { range, .. } => self.text[range.clone()].to_string(),
            RenderBlock::Heading { range, .. } => {
                // For now, show the full heading including marker
                self.text[range.clone()].to_string()
            }
            RenderBlock::ListItem { range, .. } => self.text[range.clone()].to_string(),
            RenderBlock::BlockQuote { range, .. } => self.text[range.clone()].to_string(),
            RenderBlock::CodeBlock { range, .. } => self.text[range.clone()].to_string(),
        }
    }
}

impl IntoElement for BlockView<'_> {
    type Element = gpui::Div;

    fn into_element(self) -> Self::Element {
        let display_text = self.display_text();
        let shared_text: SharedString = display_text.into();
        let styled_text = StyledText::new(shared_text);

        div().child(styled_text)
    }
}
