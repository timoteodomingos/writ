mod block;
mod container;
mod parser;
mod rich_text;

pub use block::*;
pub use container::*;
use itertools::Itertools;
pub use parser::*;
use pulldown_cmark::Parser as MarkdownParser;
pub use rich_text::*;
use slotmap::{DefaultKey, SlotMap};

pub struct Document {
    pub blocks: SlotMap<DefaultKey, Block>,
    pub containers: SlotMap<DefaultKey, Container>,
}

impl Document {
    pub fn from_markdown(markdown: &str) -> Document {
        let parser = MarkdownParser::new(markdown);
        Parser::default().parse(parser)
    }

    pub fn to_markdown(&self) -> String {
        let sorted_blocks = self
            .blocks
            .iter()
            .sorted_by_key(|(key, block)| block.index.clone())
            .map(|(key, _)| key);
        let mut result = String::new();
        for block in sorted_blocks {
            result.push_str(&self.block_to_markdown(block));
        }
        result
    }

    pub fn get_markdown_prefix(&self, container: &Container, key: DefaultKey) -> String {
        todo!()
    }

    fn block_to_markdown(&self, key: DefaultKey) -> String {
        let block = self.blocks.get(key).unwrap();
        let text = block.text.to_markdown();
        match &block.kind {
            BlockKind::Heading { level, .. } => format!("{} {}", "#".repeat(*level), text),
            BlockKind::Paragraph { parent } => todo!(),
            BlockKind::Code { parent, .. } => todo!(),
        };
        todo!()
    }
}
