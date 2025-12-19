use std::rc::Rc;

use gpui_component::{ThemeConfig, ThemeSet};

pub const DRACULA_THEME: &str = include_str!("../themes/dracula.json");

pub fn dracula_theme() -> Rc<ThemeConfig> {
    Rc::new(
        serde_json::from_str::<ThemeSet>(DRACULA_THEME)
            .expect("Failed to parse theme")
            .themes[0]
            .clone(),
    )
}
