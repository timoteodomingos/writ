use slotmap::DefaultKey;

#[derive(PartialEq, Eq)]
pub enum ContainerKind {
    NumberedList,
    BulletedList,
    ListItem,
    Quote,
}

pub struct Container {
    pub kind: ContainerKind,
    pub parent: Option<DefaultKey>,
}
