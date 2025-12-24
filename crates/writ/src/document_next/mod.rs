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
use std::collections::HashMap;

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
        let sorted_blocks: Vec<_> = self
            .blocks
            .iter()
            .sorted_by_key(|(_, block)| block.index.clone())
            .map(|(key, _)| key)
            .collect();

        let mut result = String::new();
        let mut container_counts: HashMap<DefaultKey, usize> = HashMap::new();
        let mut prev_block_key: Option<DefaultKey> = None;

        for key in sorted_blocks {
            let sibling_idx = self.sibling_index(key);

            // Add separator between blocks
            if let Some(prev_key) = prev_block_key {
                result.push_str(self.block_separator(prev_key, key));
            }

            let prefix = match self.blocks[key].parent() {
                Some(parent_key) => {
                    self.container_prefix(parent_key, Some(sibling_idx), 0, &mut container_counts)
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
        indent_level: usize,
        container_counts: &mut HashMap<DefaultKey, usize>,
    ) -> String {
        let container = &self.containers[container_key];

        // Only ListItems contribute to indentation when we traverse through them
        let indent_increment = match container.kind {
            ContainerKind::ListItem if index.is_none() => 1,
            _ => 0,
        };

        let parent_prefix = match container.parent {
            Some(parent_key) => self.container_prefix(
                parent_key,
                None,
                indent_level + indent_increment,
                container_counts,
            ),
            None => "  ".repeat(indent_level),
        };

        match container.kind {
            ContainerKind::ListItem => {
                match index {
                    Some(0) => {
                        // First block in this list item - emit marker
                        let list_key = container.parent.expect("ListItem must have a parent list");
                        let list = &self.containers[list_key];

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
                        format!("{}  ", parent_prefix)
                    }
                    None => {
                        // Traversing through - indentation handled via indent_level
                        parent_prefix
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
        let prev_in_list =
            prev_parent.is_some_and(|p| self.containers[p].kind == ContainerKind::ListItem);
        let curr_in_list =
            curr_parent.is_some_and(|p| self.containers[p].kind == ContainerKind::ListItem);

        if prev_in_list
            && curr_in_list
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

        self.blocks
            .iter()
            .filter(|(_, b)| b.parent() == parent)
            .sorted_by_key(|(_, b)| b.index.clone())
            .position(|(k, _)| k == block_key)
            .unwrap()
    }
}
