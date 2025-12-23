use gpui::{App, Entity, Window, div, prelude::*, rems};
use ropey::{Rope, RopeSlice};

pub fn line_is_empty(line: &RopeSlice) -> bool {
    line.len_chars() == 0 || line.len_chars() == 1 && line.char(0) == '\n'
}

pub struct State {
    rope: Rope,
}

pub struct Editor {
    state: Entity<State>,
}

impl Editor {
    pub fn new(cx: &mut App, rope: Rope) -> Self {
        let state = cx.new(|_| State { rope });
        Self { state }
    }
}

impl Render for Editor {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let rope = &self.state.read(cx).rope;
        div().flex().flex_col().items_start().children(
            rope.lines()
                .filter(|line| !line_is_empty(line))
                .enumerate()
                .map(|(index, line)| EditorLine {
                    index,
                    line: line.slice(..line.len_chars() - 1).to_string(),
                }),
        )
    }
}

#[derive(IntoElement)]
pub struct EditorLine {
    index: usize,
    line: String,
}

impl RenderOnce for EditorLine {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .id(("editor-line", self.index))
            .line_height(rems(1.4))
            .child(self.line)
    }
}
