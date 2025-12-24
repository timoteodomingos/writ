mod block;
mod container;
mod parser;
mod rich_text;

pub use block::*;
pub use container::*;
pub use parser::*;
pub use rich_text::*;
use slotmap::{DefaultKey, SlotMap};

pub struct Document {
    pub blocks: SlotMap<DefaultKey, Block>,
    pub containers: SlotMap<DefaultKey, Container>,
}
