
use gpui::{
    div, prelude::*, px, rems, App, Entity, HighlightStyle, InteractiveText, IntoElement,
    RenderOnce, StyledText, Window,
};
use slotmap::DefaultKey;

use super::{Editor, EditorAction};

/// A view for rendering a single block of text
#[derive(IntoElement)]
pub struct Block {
    pub block_idx: usize,
    pub block_key: DefaultKey,
    pub plain_text: String,
    pub highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    pub cursor_offset: Option<usize>,
    pub foreground_color: gpui::Rgba,
    pub editor: Entity<Editor>,
}

impl RenderOnce for Block {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let styled_text =
            StyledText::new(self.plain_text.clone()).with_highlights(self.highlights);

        let block_key = self.block_key;
        let hover_entity = self.editor.clone();
        let click_entity = self.editor;

        let text_element =
            InteractiveText::new(("block", self.block_idx), styled_text).on_hover(
                move |char_index, _event, _window, cx: &mut App| {
                    hover_entity.update(cx, |editor, _cx| {
                        editor.hover_position = char_index.map(|idx| (block_key, idx));
                    });
                },
            );

        // Container with cursor overlay and click handling
        let container = div()
            .id(("block-container", self.block_idx))
            .line_height(rems(1.4))
            .relative()
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |_event, _window, cx: &mut App| {
                    click_entity.update(cx, |editor, cx| {
                        if let Some((block_key, char_index)) = editor.hover_position {
                            editor.state.apply(EditorAction::SetCursor {
                                block_key,
                                offset: char_index,
                            });
                            cx.notify();
                        }
                    });
                },
            );

        if let Some(offset) = self.cursor_offset {
            let before = self.plain_text[..offset.min(self.plain_text.len())].to_string();

            container.child(text_element).child(
                // Cursor overlay
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .flex()
                    .flex_row()
                    .child(div().invisible().child(before))
                    .child(div().w(px(2.0)).h(rems(1.2)).bg(self.foreground_color)),
            )
        } else {
            container.child(text_element)
        }
    }
}
