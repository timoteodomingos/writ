use gpui::{
    div, prelude::*, rems, App, ClickEvent, ElementId, Fill, Global, MouseButton, ReadGlobal,
    Window,
};

use crate::theme::Theme;

pub struct FileInfo {
    pub path: std::path::PathBuf,
    pub dirty: bool,
}

impl Global for FileInfo {}

fn traffic_light(
    id: impl Into<ElementId>,
    bg: impl Into<Fill>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.opacity(0.7))
        .child(div().w(rems(1.0)).h(rems(1.0)).rounded_full().bg(bg))
        .on_click(on_click)
}

pub fn title_bar(cx: &mut App) -> impl IntoElement {
    let theme = Theme::global(cx);
    let file_info = FileInfo::global(cx);
    let file_name = file_info
        .path
        .file_name()
        .expect("file name")
        .display()
        .to_string();
    let title = if file_info.dirty {
        format!("* {}", file_name)
    } else {
        file_name
    };
    div()
        .id("title-bar")
        .w_full()
        .py(rems(0.5))
        .px(rems(1.0))
        .border_color(theme.selection)
        .border_b_1()
        .flex()
        .flex_row()
        .justify_between()
        .child(
            div()
                .w_full()
                .child(title)
                .on_mouse_down(MouseButton::Left, |_e, window, _| {
                    window.start_window_move();
                }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(rems(0.5))
                .child(traffic_light(
                    "minimize-button",
                    theme.orange,
                    |_, window, _| {
                        window.minimize_window();
                    },
                ))
                .child(traffic_light(
                    "maximize-button",
                    theme.green,
                    |_, window, _| {
                        window.zoom_window();
                    },
                ))
                .child(traffic_light("quit-button", theme.red, |_, window, _| {
                    window.remove_window();
                })),
        )
}
