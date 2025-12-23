use fractional_index::FractionalIndex;
use slotmap::DefaultKey;

use crate::document::{ToMarkdown, rich_text::RichText};

pub type BlockId = DefaultKey;

#[derive(Default, Debug, Clone, PartialEq)]
pub enum BlockKind {
    #[default]
    Paragraph,
    Heading {
        level: u8,
        id: Option<String>,
    },
    CodeBlock {
        language: Option<String>,
    },
    Quote,
    BulletItem,
    NumberedItem,
}

impl BlockKind {
    pub fn is_list_item(&self) -> bool {
        matches!(self, BlockKind::BulletItem | BlockKind::NumberedItem)
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub parent: Option<BlockId>,
    pub order: FractionalIndex,
    pub kind: BlockKind,
    pub content: RichText,
}

impl Block {
    pub fn new(kind: BlockKind, order: FractionalIndex) -> Self {
        Self {
            parent: None,
            order,
            kind,
            content: RichText::new(),
        }
    }

    pub fn to_markdown(&self, depth: usize, index: usize) -> String {
        let content = self.content.to_markdown();

        match &self.kind {
            BlockKind::Paragraph => {
                format!("{}{}", "  ".repeat(depth), content)
            }
            BlockKind::Heading { level, .. } => {
                format!("{} {}", "#".repeat(*level as usize), content)
            }
            BlockKind::CodeBlock { language } => {
                let lang = language.as_deref().unwrap_or("");
                format!("```{}\n{}\n```", lang, content)
            }
            BlockKind::Quote => format!("> {}", content),
            BlockKind::BulletItem => format!("{}- {}", "  ".repeat(depth), content),
            BlockKind::NumberedItem => {
                format!("{}{}. {}", "  ".repeat(depth), index + 1, content)
            }
        }
    }
}
