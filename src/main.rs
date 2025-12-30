use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use gpui::{
    Application, Bounds, Entity, FocusHandle, Focusable, KeyBinding, Point, Size, Timer, Window,
    WindowBounds, WindowDecorations, WindowOptions, div, prelude::*, rems,
};
use writ::{
    config::Config,
    demo::{DemoAction, DemoTiming, demo_script},
    editor::Editor,
    http, theme,
    title_bar::FileInfo,
    window::{CloseWindow, Quit, window_shadow},
};

fn load_file(file: &std::path::Path) -> String {
    std::fs::read_to_string(file).unwrap_or_default()
}

/// Run the demo script, scheduling actions with delays.
fn run_demo(editor: Entity<Editor>, cx: &mut gpui::App) {
    let script = demo_script();
    let timing = DemoTiming::default();

    // Flatten the script into individual timed events
    let mut events: Vec<(Duration, DemoAction)> = Vec::new();
    let mut current_delay = Duration::from_millis(500); // Initial delay before starting

    for action in script {
        match &action {
            DemoAction::Type(text) => {
                // Each character gets its own event
                for c in text.chars() {
                    events.push((current_delay, DemoAction::Type(c.to_string())));
                    current_delay += timing.char_delay;
                }
            }
            DemoAction::Wait(ms) => {
                current_delay += Duration::from_millis(*ms);
            }
            _ => {
                events.push((current_delay, action.clone()));
                current_delay += timing.key_delay;
            }
        }
    }

    // Schedule each event
    for (delay, action) in events {
        let editor = editor.clone();
        cx.spawn(async move |cx| {
            Timer::after(delay).await;
            let _ = cx.update(|cx| {
                if let Some(window_handle) = cx.windows().first().copied() {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        editor.update(cx, |editor, cx| {
                            match action {
                                DemoAction::Type(s) => {
                                    for c in s.chars() {
                                        editor.demo_type_char(c, window, cx);
                                    }
                                }
                                DemoAction::Enter => editor.demo_enter(window, cx),
                                DemoAction::ShiftEnter => editor.demo_shift_enter(window, cx),
                                DemoAction::Backspace => editor.demo_backspace(window, cx),
                                DemoAction::Move(dir) => editor.demo_move(&dir, window, cx),
                                DemoAction::Wait(_) => {} // Already handled in timing
                            }
                        });
                    });
                }
            });
        })
        .detach();
    }
}

pub struct Root {
    focus_handle: FocusHandle,
    editor: Entity<Editor>,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .bg(cx.global::<theme::Theme>().background)
                .child(self.editor.clone()),
        )
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn main() {
    let config = Config::parse()
        .validate()
        .expect("Failed to validate config");

    let demo_mode = config.demo;
    let file_path = config
        .file
        .clone()
        .unwrap_or_else(|| PathBuf::from("demo.md"));
    let content = if demo_mode {
        String::new()
    } else {
        load_file(&file_path)
    };

    let app = Application::new().with_http_client(http::Client::new());

    app.run(move |cx| {
        cx.set_global(theme::dracula());
        cx.set_global(FileInfo {
            path: file_path.clone(),
            dirty: false,
        });
        cx.set_global(config);
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

                // Create editor with file content
                let editor = cx.new(|cx| Editor::new(&content, cx));

                // Start demo if in demo mode
                if demo_mode {
                    run_demo(editor.clone(), cx);
                }

                cx.new(|_| Root {
                    focus_handle,
                    editor,
                })
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
