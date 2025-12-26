use slotmap::DefaultKey;
use strum::EnumDiscriminants;

use crate::document::RichText;

#[derive(Debug, Clone, PartialEq, EnumDiscriminants)]
pub enum BlockKind {
    Heading {
        level: usize,
        id: Option<String>,
    },
    Paragraph {
        parent: Option<DefaultKey>,
    },
    Code {
        parent: Option<DefaultKey>,
        language: Option<String>,
    },
    HorizontalRule,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub text: RichText,
}

impl Block {
    pub fn parent(&self) -> Option<DefaultKey> {
        match &self.kind {
            BlockKind::Heading { .. } => None,
            BlockKind::Paragraph { parent } => *parent,
            BlockKind::Code { parent, .. } => *parent,
            BlockKind::HorizontalRule => None,
        }
    }

    /// Extract plain text from this block (ignoring styles)
    pub fn plain_text(&self) -> String {
        self.text.chunks.iter().map(|c| c.text.as_str()).collect()
    }
}
