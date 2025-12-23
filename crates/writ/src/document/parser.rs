use pulldown_cmark::{Event, Parser as MarkdownParser, Tag, TagEnd};
use strum::IntoDiscriminant;

use crate::document::{
    Block, Document, TextStyleDiscriminants,
    block::{BlockId, BlockKind},
    rich_text::{StyleSet, TextStyle},
};

#[derive(Default)]
pub struct Parser {
    document: Document,
    /// Stack of styles, outermost first
    style_stack: Vec<TextStyle>,
    /// Current block being built
    current_block: Option<BlockId>,
    /// Stack of list types: true = ordered, false = unordered
    list_stack: Vec<bool>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    fn current_styles(&self) -> StyleSet {
        StyleSet {
            styles: self.style_stack.clone(),
        }
    }

    fn push_block(&mut self, kind: BlockKind) -> BlockId {
        let id = self.document.insert_last_child(self.current_block, kind);
        self.current_block = Some(id);
        id
    }

    fn pop_block(&mut self) {
        if let Some(block_id) = self.current_block {
            self.current_block = self.document.blocks[block_id].parent;
        }
    }

    fn peek_block(&mut self) -> Option<&Block> {
        self.current_block
            .and_then(|block_id| self.document.blocks.get(block_id))
    }

    fn push_text(&mut self, text: &str) {
        if let Some(block_id) = self.current_block {
            let styles = self.current_styles();
            self.document.blocks[block_id].content.push(text, styles);
        }
    }

    fn push_style(&mut self, style: TextStyle) {
        self.style_stack.push(style);
    }

    fn pop_style(&mut self, expected: TextStyleDiscriminants) {
        if let Some(style) = self.style_stack.pop() {
            if style.discriminant() != expected {
                panic!("Unexpected style pop");
            }
        } else {
            panic!("Style stack underflow");
        }
    }

    pub fn parse(mut self, parser: MarkdownParser) -> Document {
        for event in parser {
            println!("Event: {:#?}", event);
            match event {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => {
                        if !self
                            .peek_block()
                            .is_some_and(|b| b.kind.is_list_item() && b.content.is_empty())
                        {
                            self.push_block(BlockKind::Paragraph);
                        }
                    }
                    Tag::Heading { level, id, .. } => {
                        self.push_block(BlockKind::Heading {
                            level: level as u8,
                            id: id.map(|s| s.to_string()),
                        });
                    }
                    Tag::BlockQuote(_) => {
                        todo!("BlockQuote")
                    }
                    Tag::CodeBlock(_code_block_kind) => {
                        todo!("CodeBlock")
                    }
                    Tag::List(start) => {
                        self.list_stack.push(start.is_some());
                    }
                    Tag::Item => {
                        self.push_block(if *self.list_stack.last().expect("No list in stack") {
                            BlockKind::NumberedItem
                        } else {
                            BlockKind::BulletItem
                        });
                    }
                    Tag::Emphasis => {
                        self.push_style(TextStyle::Italic);
                    }
                    Tag::Strong => {
                        self.push_style(TextStyle::Bold);
                    }
                    Tag::Strikethrough => {
                        self.push_style(TextStyle::Strikethrough);
                    }
                    Tag::Link { dest_url, .. } => {
                        self.push_style(TextStyle::Link {
                            url: dest_url.to_string(),
                        });
                    }
                    other => todo!("Start tag: {other:?}"),
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Paragraph => {
                        if self
                            .peek_block()
                            .is_some_and(|b| b.kind == BlockKind::Paragraph)
                        {
                            self.pop_block();
                        }
                    }
                    TagEnd::Heading(_) => {
                        self.pop_block();
                    }
                    TagEnd::List(_) => {
                        self.list_stack.pop();
                    }
                    TagEnd::Item => {
                        self.pop_block();
                    }
                    TagEnd::Emphasis => {
                        self.pop_style(TextStyleDiscriminants::Italic);
                    }
                    TagEnd::Strong => {
                        self.pop_style(TextStyleDiscriminants::Bold);
                    }
                    TagEnd::Strikethrough => {
                        self.pop_style(TextStyleDiscriminants::Strikethrough);
                    }
                    TagEnd::Link => {
                        self.pop_style(TextStyleDiscriminants::Link);
                    }
                    other => todo!("End tag: {other:?}"),
                },
                Event::Text(text) => {
                    self.push_text(&text);
                }
                Event::Code(code) => {
                    // Inline code is a style that wraps just this text
                    self.push_style(TextStyle::Code);
                    self.push_text(&code);
                    self.style_stack.pop();
                }
                Event::SoftBreak => {
                    self.push_text(" ");
                }
                Event::HardBreak => {
                    self.push_text("\n");
                }
                other => todo!("Event: {other:?}"),
            }
        }

        self.document
    }
}
