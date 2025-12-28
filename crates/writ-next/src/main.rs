use clap::Parser;
use gpui::{
    Application, Bounds, FocusHandle, Focusable, KeyBinding, Point, Size, Window, WindowBounds,
    WindowDecorations, WindowOptions, div, prelude::*, rems,
};
use writ_next::{
    args::Args,
    http, theme,
    title_bar::FileInfo,
    window::{CloseWindow, Quit, window_shadow},
};

pub struct Root {
    focus_handle: FocusHandle,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        window_shadow().child(
            div()
                .id("root")
                .track_focus(&self.focus_handle)
                .on_action(|CloseWindow, window, _| {
                    window.remove_window();
                })
                .on_action(|Quit, _, cx| {
                    cx.quit();
                })
                .overflow_scroll()
                .px(rems(2.0))
                .py(rems(1.6))
                .flex()
                .flex_col()
                .size_full()
                .child("writ-next: empty editor placeholder"),
        )
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn main() {
    let args = Args::parse()
        .validate()
        .expect("Failed to validate arguments");

    let app = Application::new().with_http_client(http::Client::new());

    app.run(move |cx| {
        cx.set_global(theme::dracula());
        cx.set_global(FileInfo {
            path: args.file,
            dirty: false,
        });
        cx.bind_keys([
            KeyBinding::new("ctrl-w", CloseWindow, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::new(0.0.into(), 0.0.into()),
                    size: Size::new(600.0.into(), 600.0.into()),
                })),
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            };

            cx.open_window(window_options, |window, cx| {
                let focus_handle = cx.focus_handle();
                focus_handle.focus(window);
                cx.new(|_| Root { focus_handle })
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
