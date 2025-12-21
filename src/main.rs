mod args;
mod editor;
mod theme;
mod title_bar;
mod window;

use std::fs::File;

use anyhow::Result;
use clap::Parser;
use gpui::{
    Application, Bounds, Entity, FocusHandle, KeyBinding, Point, Size, Window, WindowBounds,
    WindowDecorations, WindowOptions, div, prelude::*, rems,
};
use ropey::Rope;

use crate::{
    args::Args,
    editor::Editor,
    window::{CloseWindow, Quit, window_shadow},
};

pub struct Root {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _ct: &mut Context<Self>) -> impl IntoElement {
        window_shadow().child(
            div()
                .id("root")
                .track_focus(&self.focus_handle)
                .on_action(|CloseWindow, window, _| {
                    println!("Window close requested!");
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
                .child(self.editor.clone()),
        )
    }
}

fn load_file(file: &std::path::Path) -> Result<Rope> {
    Ok(Rope::from_reader(File::open(file)?)?)
}

fn main() {
    let args = Args::parse()
        .validate()
        .expect("Failed to validate arguments");
    let rope = load_file(&args.file).expect("Failed to load file");
    // for line in rope.lines() {
    //     println!("{:#?}", line);
    // }
    let app = Application::new();

    app.run(move |cx| {
        cx.set_global(theme::dracula());
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
                    size: Size::new(800.0.into(), 800.0.into()),
                })),
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            };

            cx.open_window(window_options, |window, cx| {
                let editor = cx.new(|cx| Editor::new(cx, rope));
                let focus_handle = cx.focus_handle();
                focus_handle.focus(window);
                cx.new(|_| Root {
                    editor,
                    focus_handle,
                })
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
