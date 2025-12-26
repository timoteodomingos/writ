use slotmap::DefaultKey;
use strum::EnumDiscriminants;

#[derive(Debug, Clone, PartialEq, Eq, EnumDiscriminants)]
pub enum ContainerKind {
    NumberedList,
    BulletedList,
    /// A list item. `checked` is None for regular items,
    /// Some(false) for unchecked checkboxes, Some(true) for checked.
    ListItem {
        checked: Option<bool>,
    },
    Quote,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub kind: ContainerKind,
    pub parent: Option<DefaultKey>,
}
