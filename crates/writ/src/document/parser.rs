use std::collections::BTreeMap;

use fractional_index::FractionalIndex;
use pulldown_cmark::{CodeBlockKind, Event, Parser as MarkdownParser, Tag, TagEnd};
use slotmap::{DefaultKey, SlotMap};
use strum::IntoDiscriminant;

use crate::document::{
    Block, BlockKind, BlockKindDiscriminants, Container, ContainerKind, Document, RichText,
    StyleSet, TextStyle, TextStyleDiscriminants,
};

#[derive(Default)]
pub struct Parser {
    blocks: SlotMap<DefaultKey, Block>,
    block_order: BTreeMap<FractionalIndex, DefaultKey>,
    containers: SlotMap<DefaultKey, Container>,

    style_stack: Vec<TextStyle>,
    container_stack: Vec<DefaultKey>,
    current_block: Option<DefaultKey>,
}

impl Parser {
    fn current_styles(&self) -> StyleSet {
        StyleSet {
            styles: self.style_stack.clone(),
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

    fn push_text(&mut self, text: &str) {
        match self.current_block {
            Some(key) => {
                let styles = self.current_styles();
                self.blocks[key].text.push(text, styles);
            }
            None => panic!("No current block"),
        }
    }

    fn get_parent(&self) -> Option<DefaultKey> {
        self.container_stack.last().copied()
    }

    fn push_container(&mut self, kind: ContainerKind) {
        let key = self.containers.insert(Container {
            kind,
            parent: self.get_parent(),
        });
        self.container_stack.push(key);
        // part of handling the special case for list item text
        self.current_block = None;
    }

    fn pop_container(&mut self, expected_kind: ContainerKind) {
        match self.container_stack.pop() {
            Some(key) => {
                if self.containers[key].kind != expected_kind {
                    panic!("Unexpected container kind");
                }
            }
            None => panic!("No containers in stack"),
        };
        // part of handling the special case for list item text
        self.current_block = None;
    }

    fn push_block(&mut self, kind: BlockKind) {
        let index = self
            .block_order
            .last_key_value()
            .map_or(FractionalIndex::default(), |(last_index, _)| {
                FractionalIndex::new_after(last_index)
            });
        let key = self.blocks.insert(Block {
            kind,
            text: RichText::default(),
        });
        self.block_order.insert(index, key);
        self.current_block = Some(key);
    }

    fn clear_current_block(&mut self, expected_kind: BlockKindDiscriminants) {
        match self.current_block {
            Some(key) => {
                if self.blocks[key].kind.discriminant() != expected_kind {
                    panic!("Unexpected block kind");
                }
            }
            None => panic!("No block in progress"),
        }
        self.current_block = None;
    }

    pub fn parse(&mut self, parser: MarkdownParser) -> Document {
        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::List(start) => {
                        self.push_container(if start.is_some() {
                            ContainerKind::NumberedList
                        } else {
                            ContainerKind::BulletedList
                        });
                    }
                    Tag::Item => {
                        self.push_container(ContainerKind::ListItem);
                    }
                    Tag::BlockQuote(_) => {
                        self.push_container(ContainerKind::Quote);
                    }
                    Tag::Heading { level, id, .. } => {
                        self.push_block(BlockKind::Heading {
                            level: level as usize,
                            id: id.map(|id| id.to_string()),
                        });
                    }
                    Tag::Paragraph => {
                        self.push_block(BlockKind::Paragraph {
                            parent: self.get_parent(),
                        });
                    }
                    Tag::CodeBlock(CodeBlockKind::Fenced(language)) => {
                        self.push_block(BlockKind::Code {
                            language: Some(language.to_string()),
                            parent: self.get_parent(),
                        });
                    }
                    Tag::CodeBlock(CodeBlockKind::Indented) => {
                        self.push_block(BlockKind::Code {
                            language: None,
                            parent: self.get_parent(),
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
                    TagEnd::List(numbered) => {
                        self.pop_container(if numbered {
                            ContainerKind::NumberedList
                        } else {
                            ContainerKind::BulletedList
                        });
                    }
                    TagEnd::Item => {
                        self.pop_container(ContainerKind::ListItem);
                    }
                    TagEnd::BlockQuote(_) => {
                        self.pop_container(ContainerKind::Quote);
                    }
                    TagEnd::Heading(_) => {
                        self.clear_current_block(BlockKindDiscriminants::Heading);
                    }
                    TagEnd::Paragraph => {
                        self.clear_current_block(BlockKindDiscriminants::Paragraph);
                    }
                    TagEnd::CodeBlock => {
                        self.clear_current_block(BlockKindDiscriminants::Code);
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
                    if self.current_block.is_none() {
                        self.push_block(BlockKind::Paragraph {
                            parent: self.get_parent(),
                        });
                    }
                    self.push_text(&text);
                }
                Event::Code(code) => {
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

        Document {
            blocks: self.blocks.clone(),
            block_order: self.block_order.clone(),
            containers: self.containers.clone(),
        }
    }
}
