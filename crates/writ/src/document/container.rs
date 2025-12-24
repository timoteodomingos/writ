use slotmap::DefaultKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerKind {
    NumberedList,
    BulletedList,
    ListItem,
    Quote,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub kind: ContainerKind,
    pub parent: Option<DefaultKey>,
}
