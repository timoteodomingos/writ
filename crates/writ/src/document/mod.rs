mod block;
mod parser;
mod rich_text;

pub use block::*;
pub use parser::*;
pub use rich_text::*;

use std::fs;

use anyhow::Result;
use fractional_index::FractionalIndex;
use pulldown_cmark::Parser as MarkdownParser;
use slotmap::SlotMap;

pub trait ToMarkdown {
    fn to_markdown(&self) -> String;
}

#[derive(Default, Debug, Clone)]
pub struct Document {
    pub blocks: SlotMap<BlockId, Block>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get children of a parent (None for root), sorted by order
    pub fn children(&self, parent: Option<BlockId>) -> Vec<BlockId> {
        let mut children: Vec<_> = self
            .blocks
            .iter()
            .filter(|(_, b)| b.parent == parent)
            .map(|(id, b)| (id, &b.order))
            .collect();

        children.sort_by(|(_, a), (_, b)| a.cmp(b));
        children.into_iter().map(|(id, _)| id).collect()
    }

    /// Get the previous sibling of a block
    pub fn previous_sibling(&self, block_id: BlockId) -> Option<BlockId> {
        let block = &self.blocks[block_id];
        let siblings = self.children(block.parent);
        let pos = siblings.iter().position(|&id| id == block_id)?;
        if pos > 0 {
            Some(siblings[pos - 1])
        } else {
            None
        }
    }

    /// Get the next sibling of a block
    pub fn next_sibling(&self, block_id: BlockId) -> Option<BlockId> {
        let block = &self.blocks[block_id];
        let siblings = self.children(block.parent);
        let pos = siblings.iter().position(|&id| id == block_id)?;
        siblings.get(pos + 1).copied()
    }

    /// Insert a new block after the given block (as sibling)
    pub fn insert_after(&mut self, after: BlockId, kind: BlockKind) -> BlockId {
        let parent = self.blocks[after].parent;
        let after_order = &self.blocks[after].order;

        let order = match self.next_sibling(after) {
            Some(next) => {
                FractionalIndex::new_between(after_order, &self.blocks[next].order).unwrap()
            }
            None => FractionalIndex::new_after(after_order),
        };

        self.blocks.insert(Block {
            parent,
            order,
            kind,
            content: RichText::new(),
        })
    }

    /// Insert a new block as the last child of a parent
    pub fn insert_last_child(&mut self, parent: Option<BlockId>, kind: BlockKind) -> BlockId {
        let children = self.children(parent);

        let order = match children.last() {
            Some(&last) => FractionalIndex::new_after(&self.blocks[last].order),
            None => FractionalIndex::default(),
        };

        self.blocks.insert(Block {
            parent,
            order,
            kind,
            content: RichText::new(),
        })
    }

    /// Indent: make block a child of its previous sibling
    pub fn indent(&mut self, block_id: BlockId) -> bool {
        let Some(prev) = self.previous_sibling(block_id) else {
            return false;
        };

        let new_order = match self.children(Some(prev)).last() {
            Some(&last) => FractionalIndex::new_after(&self.blocks[last].order),
            None => FractionalIndex::default(),
        };

        let block = &mut self.blocks[block_id];
        block.parent = Some(prev);
        block.order = new_order;
        true
    }

    /// Outdent: make block a sibling of its parent
    pub fn outdent(&mut self, block_id: BlockId) -> bool {
        let Some(parent_id) = self.blocks[block_id].parent else {
            return false;
        };

        let grandparent = self.blocks[parent_id].parent;
        let parent_order = &self.blocks[parent_id].order;

        let new_order = match self.next_sibling(parent_id) {
            Some(uncle) => {
                FractionalIndex::new_between(parent_order, &self.blocks[uncle].order).unwrap()
            }
            None => FractionalIndex::new_after(parent_order),
        };

        let block = &mut self.blocks[block_id];
        block.parent = grandparent;
        block.order = new_order;
        true
    }

    /// Delete a block
    pub fn delete(&mut self, block_id: BlockId) -> Option<Block> {
        self.blocks.remove(block_id)
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_markdown(&content)
    }

    pub fn from_markdown(markdown: &str) -> Result<Self> {
        let parser = MarkdownParser::new(markdown);
        let doc = Parser::new().parse(parser);
        Ok(doc)
    }

    fn blocks_to_markdown(&self, parent: Option<BlockId>) -> String {
        let children = self.children(parent);
        let mut parts = Vec::new();

        for child_id in children {
            let block = &self.blocks[child_id];
            let block_md = block.to_markdown(child_id, &self.blocks);

            let children_md = self.blocks_to_markdown(Some(child_id));

            let full_block = if children_md.is_empty() {
                block_md
            } else {
                let block_children = self.children(Some(child_id));
                let first_child_is_list_item = block_children
                    .first()
                    .is_some_and(|&id| self.blocks[id].kind.is_list_item());

                let separator = if block.kind.is_list_item() && first_child_is_list_item {
                    "\n"
                } else {
                    "\n\n"
                };
                format!("{}{}{}", block_md, separator, children_md)
            };

            parts.push((child_id, full_block));
        }

        // Join blocks: list items use single newline, others use double
        let mut result = String::new();
        for (i, (id, md)) in parts.iter().enumerate() {
            if i > 0 {
                let prev_id = parts[i - 1].0;
                let prev_is_list_item = self.blocks[prev_id].kind.is_list_item();
                let curr_is_list_item = self.blocks[*id].kind.is_list_item();

                if prev_is_list_item && curr_is_list_item {
                    result.push('\n');
                } else {
                    result.push_str("\n\n");
                }
            }
            result.push_str(md);
        }

        result
    }

    pub fn save_to_file(&self, path: &std::path::Path) -> Result<()> {
        let markdown = self.to_markdown();
        fs::write(path, markdown)?;
        Ok(())
    }
}

impl ToMarkdown for Document {
    fn to_markdown(&self) -> String {
        let mut result = self.blocks_to_markdown(None);
        // add newline at the end of the document
        result.push('\n');
        result
    }
}
