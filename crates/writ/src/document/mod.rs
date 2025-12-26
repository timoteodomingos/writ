mod block;
mod container;
mod parser;
mod rich_text;

pub use block::*;
pub use container::*;
use fractional_index::FractionalIndex;
pub use parser::*;
use pulldown_cmark::Parser as MarkdownParser;
pub use rich_text::*;
use slotmap::{DefaultKey, SlotMap};
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct Document {
    pub blocks: SlotMap<DefaultKey, Block>,
    pub block_order: BTreeMap<FractionalIndex, DefaultKey>,
    pub containers: SlotMap<DefaultKey, Container>,
}

impl Document {
    pub fn from_markdown(markdown: &str) -> Document {
        let parser = MarkdownParser::new(markdown);
        Parser::default().parse(parser)
    }

    pub fn to_markdown(&self) -> String {
        let mut result = String::new();
        let mut container_counts: HashMap<DefaultKey, usize> = HashMap::new();
        let mut prev_block_key: Option<DefaultKey> = None;

        for k in self.block_order.values() {
            let key = *k;
            let sibling_idx = self.sibling_index(key);

            // Add separator between blocks
            if let Some(prev_key) = prev_block_key {
                result.push_str(self.block_separator(prev_key, key));
            }

            let prefix = match self.blocks[key].parent() {
                Some(parent_key) => {
                    self.container_prefix(parent_key, Some(sibling_idx), &mut container_counts)
                }
                None => String::new(),
            };

            result.push_str(&self.block_to_markdown(key, &prefix));
            prev_block_key = Some(key);
        }

        result.push('\n');
        result
    }

    fn container_prefix(
        &self,
        container_key: DefaultKey,
        index: Option<usize>,
        container_counts: &mut HashMap<DefaultKey, usize>,
    ) -> String {
        let container = &self.containers[container_key];

        let parent_prefix = match container.parent {
            Some(parent_key) => self.container_prefix(parent_key, None, container_counts),
            None => String::new(),
        };

        match container.kind {
            ContainerKind::ListItem => {
                let list_key = container.parent.expect("ListItem must have a parent list");
                let list = &self.containers[list_key];

                match index {
                    Some(0) => {
                        // First block in this list item - emit marker
                        // Get and increment the count for this list
                        let count = container_counts.get(&list_key).copied().unwrap_or(0);
                        container_counts.insert(list_key, count + 1);

                        match list.kind {
                            ContainerKind::BulletedList => format!("{}- ", parent_prefix),
                            ContainerKind::NumberedList => {
                                format!("{}{}. ", parent_prefix, count + 1)
                            }
                            _ => panic!("ListItem parent must be a list"),
                        }
                    }
                    Some(_) => {
                        // Continuation block (not first in list item) - needs indentation
                        // Indent by the width of the marker that was used for this list item
                        let count = container_counts.get(&list_key).copied().unwrap_or(1);
                        let marker_width = match list.kind {
                            ContainerKind::BulletedList => 2, // "- "
                            ContainerKind::NumberedList => {
                                // "N. " where N is the item number
                                let digits = count.to_string().len();
                                digits + 2 // digits + ". "
                            }
                            _ => panic!("ListItem parent must be a list"),
                        };
                        format!("{}{}", parent_prefix, " ".repeat(marker_width))
                    }
                    None => {
                        // Traversing through - indent by the marker width
                        let count = container_counts.get(&list_key).copied().unwrap_or(1);
                        let marker_width = match list.kind {
                            ContainerKind::BulletedList => 2, // "- "
                            ContainerKind::NumberedList => {
                                let digits = count.to_string().len();
                                digits + 2
                            }
                            _ => panic!("ListItem parent must be a list"),
                        };
                        format!("{}{}", parent_prefix, " ".repeat(marker_width))
                    }
                }
            }
            ContainerKind::BulletedList | ContainerKind::NumberedList => parent_prefix,
            ContainerKind::Quote => {
                format!("{}> ", parent_prefix)
            }
        }
    }

    fn block_to_markdown(&self, key: DefaultKey, prefix: &str) -> String {
        let block = &self.blocks[key];
        let text = block.text.to_markdown();

        match &block.kind {
            BlockKind::Heading { level, .. } => {
                format!("{} {}", "#".repeat(*level), text)
            }
            BlockKind::Paragraph { .. } => {
                format!("{}{}", prefix, text)
            }
            BlockKind::Code { language, .. } => {
                let lang = language.as_deref().unwrap_or("");
                if prefix.is_empty() {
                    format!("```{}\n{}```", lang, text)
                } else {
                    // In a list - indent matches the prefix
                    let indented_content = text
                        .lines()
                        .map(|line| format!("{}{}", prefix, line))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("{}```{}\n{}\n{}```", prefix, lang, indented_content, prefix)
                }
            }
        }
    }

    fn get_path(&self, key: DefaultKey) -> HashSet<DefaultKey> {
        let mut path = HashSet::new();
        let mut parent = self.containers[key].parent;
        while let Some(p) = parent {
            path.insert(p);
            parent = self.containers[p].parent;
        }
        path
    }

    /// Determine the separator between two consecutive blocks
    fn block_separator(&self, prev_key: DefaultKey, curr_key: DefaultKey) -> &'static str {
        let prev_block = &self.blocks[prev_key];
        let curr_block = &self.blocks[curr_key];
        let prev_parent = prev_block.parent();
        let curr_parent = curr_block.parent();

        // Single newline only when:
        // - Both blocks are in list items
        // - Current block is the first in its list item (gets a marker)
        // - Previous block was also the first (and only) in its list item
        // - List items container paths intersect
        if let Some(pp) = prev_parent
            && self.containers[pp].kind == ContainerKind::ListItem
            && let Some(cp) = curr_parent
            && self.containers[cp].kind == ContainerKind::ListItem
            && !self.get_path(pp).is_disjoint(&self.get_path(cp))
            && self.sibling_index(prev_key) == 0
            && self.sibling_index(curr_key) == 0
            && prev_parent != curr_parent
        {
            return "\n";
        }
        // Default: double newline between blocks
        "\n\n"
    }

    /// Returns the 0-based index of a block among its siblings.
    /// Siblings are blocks that share the same immediate parent container.
    fn sibling_index(&self, block_key: DefaultKey) -> usize {
        let block = &self.blocks[block_key];
        let parent = block.parent();

        self.block_order
            .iter()
            .filter(|(_, k)| self.blocks[**k].parent() == parent)
            .position(|(_, k)| k == &block_key)
            .unwrap()
    }

    /// Insert a new block after the given block, returns the new block's key
    pub fn insert_block_after(&mut self, after_key: DefaultKey, block: Block) -> DefaultKey {
        // Find the FractionalIndex of after_key
        let after_index = self
            .block_order
            .iter()
            .find(|(_, k)| **k == after_key)
            .map(|(idx, _)| idx.clone())
            .expect("Block not found in block_order");

        // Find the next block's index (if any)
        let next_index = self
            .block_order
            .range(&after_index..)
            .nth(1)
            .map(|(idx, _)| idx.clone());

        // Generate new FractionalIndex between after_key and next block
        let new_index = match next_index {
            Some(ref next) => FractionalIndex::new_between(&after_index, next)
                .expect("Failed to create FractionalIndex between blocks"),
            None => FractionalIndex::new_after(&after_index),
        };

        // Insert into blocks SlotMap and block_order BTreeMap
        let new_key = self.blocks.insert(block);
        self.block_order.insert(new_index, new_key);

        new_key
    }

    /// Remove a block from the document
    pub fn remove_block(&mut self, block_key: DefaultKey) {
        // Find and remove from block_order
        let index_to_remove = self
            .block_order
            .iter()
            .find(|(_, k)| **k == block_key)
            .map(|(idx, _)| idx.clone());

        if let Some(index) = index_to_remove {
            self.block_order.remove(&index);
        }

        // Remove from blocks SlotMap
        self.blocks.remove(block_key);
    }
}
