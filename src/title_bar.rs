use gpui::{
    App, ClickEvent, ElementId, Fill, MouseButton, ReadGlobal, Window, div, prelude::*, rems,
};

use crate::theme::Theme;

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
                .child("this is the custom titlebar")
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
