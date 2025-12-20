mod args;
mod editor;
mod theme;
mod window;

use std::fs::File;

use anyhow::Result;
use clap::Parser;
use gpui::{
    Application, Bounds, Entity, Point, Size, Window, WindowBounds, WindowDecorations,
    WindowOptions, div, prelude::*, rems,
};
use ropey::Rope;

use crate::{args::Args, editor::Editor, window::window_shadow};

pub struct Container {
    editor: Entity<Editor>,
}

impl Render for Container {
    fn render(&mut self, _window: &mut Window, _ct: &mut Context<Self>) -> impl IntoElement {
        window_shadow().child(
            div()
                .id("container")
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
        cx.spawn(async move |cx| {
            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::new(0.0.into(), 0.0.into()),
                    size: Size::new(800.0.into(), 800.0.into()),
                })),
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            };

            cx.open_window(window_options, |_window, cx| {
                let editor = cx.new(|cx| Editor::new(cx, rope));
                cx.new(|_| Container { editor })
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
