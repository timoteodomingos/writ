use slotmap::DefaultKey;
use strum::EnumDiscriminants;

use crate::document_next::RichText;

#[derive(Debug, Clone, PartialEq, EnumDiscriminants)]
pub enum BlockKind {
    Heading {
        level: u8,
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

pub struct Block {
    pub kind: BlockKind,
    pub text: RichText,
}
