use fractional_index::FractionalIndex;
use slotmap::DefaultKey;
use strum::EnumDiscriminants;

use crate::document_next::RichText;

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
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub index: FractionalIndex,
    pub text: RichText,
}

impl Block {
    pub fn parent(&self) -> Option<DefaultKey> {
        match &self.kind {
            BlockKind::Heading { .. } => None,
            BlockKind::Paragraph { parent } => *parent,
            BlockKind::Code { parent, .. } => *parent,
        }
    }
}
