use gpui::{
    App, Entity, HighlightStyle, InteractiveText, IntoElement, MouseDownEvent, RenderOnce,
    StyledText, Window, div, prelude::*, px, rems,
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let styled_text = StyledText::new(self.plain_text.clone()).with_highlights(self.highlights);

        // Store the text layout in the Editor for click position calculations
        let text_layout = styled_text.layout().clone();
        let text_len = self.plain_text.len();
        self.editor.update(cx, |editor, _cx| {
            editor.block_layouts.insert(self.block_key, text_layout);
        });

        let block_key = self.block_key;
        let click_entity = self.editor;

        let text_element = InteractiveText::new(("block", self.block_idx), styled_text);

        // Container with cursor overlay and click handling
        let container = div()
            .id(("block-container", self.block_idx))
            .line_height(rems(1.4))
            .relative()
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |event: &MouseDownEvent, _window, cx: &mut App| {
                    click_entity.update(cx, |editor, cx| {
                        // Get the text layout for this block
                        if let Some(layout) = editor.block_layouts.get(block_key) {
                            // index_for_position returns Ok(idx) for clicks on text,
                            // Err(idx) for clicks in margins (with closest index)
                            let char_index = match layout.index_for_position(event.position) {
                                Ok(idx) => idx,
                                Err(idx) => idx.min(text_len), // Clamp to text length for end-of-line
                            };
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
