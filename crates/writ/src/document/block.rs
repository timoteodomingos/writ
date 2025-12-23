use fractional_index::FractionalIndex;
use slotmap::{DefaultKey, SlotMap};

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

    /// Compute depth by walking up the parent chain
    fn depth(&self, blocks: &SlotMap<BlockId, Block>) -> usize {
        let mut depth = 0;
        let mut current = self.parent;
        while let Some(parent_id) = current {
            depth += 1;
            current = blocks[parent_id].parent;
        }
        depth
    }

    /// Get the 1-based position among siblings (for numbered lists)
    fn sibling_position(&self, id: BlockId, blocks: &SlotMap<BlockId, Block>) -> usize {
        let siblings = Self::children_of(self.parent, blocks);
        siblings.iter().position(|&sid| sid == id).unwrap() + 1
    }

    /// Get children of a parent, sorted by order (duplicated from Document to avoid circular dep)
    fn children_of(parent: Option<BlockId>, blocks: &SlotMap<BlockId, Block>) -> Vec<BlockId> {
        let mut children: Vec<_> = blocks
            .iter()
            .filter(|(_, b)| b.parent == parent)
            .map(|(id, b)| (id, &b.order))
            .collect();

        children.sort_by(|(_, a), (_, b)| a.cmp(b));
        children.into_iter().map(|(id, _)| id).collect()
    }

    pub fn to_markdown(&self, id: BlockId, blocks: &SlotMap<BlockId, Block>) -> String {
        let content = self.content.to_markdown();
        let depth = self.depth(blocks);

        match &self.kind {
            BlockKind::Paragraph => content,
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
                let num = self.sibling_position(id, blocks);
                format!("{}{}. {}", "  ".repeat(depth), num, content)
            }
        }
    }
}
