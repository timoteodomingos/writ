use gpui::{
    App, Context, FocusHandle, Focusable, HighlightStyle, InteractiveText, IntoElement,
    KeyDownEvent, ReadGlobal, Render, StyledText, Window, div, prelude::*, px, rems,
};
use slotmap::DefaultKey;

use crate::editor_state::{Direction, EditorAction, EditorState};
use crate::theme::Theme;

/// GPUI wrapper around EditorState
pub struct EditorView {
    pub state: EditorState,
    focus_handle: FocusHandle,
}

impl EditorView {
    pub fn new(state: EditorState, cx: &mut Context<Self>) -> Self {
        Self {
            state,
            focus_handle: cx.focus_handle(),
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = match event.keystroke.key.as_str() {
            "left" => Some(EditorAction::MoveCursor(Direction::Left)),
            "right" => Some(EditorAction::MoveCursor(Direction::Right)),
            "up" => Some(EditorAction::MoveCursor(Direction::Up)),
            "down" => Some(EditorAction::MoveCursor(Direction::Down)),
            "home" => Some(EditorAction::MoveCursor(Direction::Home)),
            "end" => Some(EditorAction::MoveCursor(Direction::End)),
            "backspace" => Some(EditorAction::Backspace),
            "delete" => Some(EditorAction::Delete),
            key if key.len() == 1
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform =>
            {
                Some(EditorAction::InsertText(key.to_string()))
            }
            _ => None,
        };

        if let Some(action) = action {
            self.state.apply(action);
            cx.notify();
        }
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx);
        let theme_foreground = theme.foreground;

        // Collect block data first to avoid borrow issues
        let blocks_iter =
            self.state
                .document
                .block_order
                .values()
                .enumerate()
                .map(|(block_idx, block_key)| {
                    let block_key = *block_key;
                    let block = &self.state.document.blocks[block_key];
                    let is_cursor_block = block_key == self.state.cursor.block_key;
                    let cursor_offset = if is_cursor_block {
                        Some(self.state.cursor.offset)
                    } else {
                        None
                    };

                    let plain_text: String =
                        block.text.chunks.iter().map(|c| c.text.as_str()).collect();
                    let highlights = block.text.to_highlights(theme);

                    (block_idx, block_key, plain_text, highlights, cursor_offset)
                });

        // Now create elements with cx using a for loop to avoid closure borrow issues
        let mut block_elements = Vec::new();
        for (block_idx, block_key, plain_text, highlights, cursor_offset) in blocks_iter {
            let element = render_block(
                block_idx,
                block_key,
                plain_text,
                highlights,
                cursor_offset,
                theme,
            );
            block_elements.push(element);
        }

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .flex()
            .flex_col()
            .gap(rems(0.5))
            .text_color(theme_foreground)
            .children(block_elements)
    }
}

fn render_block(
    block_idx: usize,
    _block_key: DefaultKey,
    plain_text: String,
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    cursor_offset: Option<usize>,
    theme: &Theme,
) -> impl IntoElement {
    let styled_text = StyledText::new(plain_text.clone()).with_highlights(highlights);

    // For now, render without click handling - we'll add it later using a different approach
    let text_element = InteractiveText::new(("block", block_idx), styled_text);

    // Container with cursor overlay
    let container = div().line_height(rems(1.4)).relative();

    if let Some(offset) = cursor_offset {
        let before = plain_text[..offset.min(plain_text.len())].to_string();

        container.child(text_element).child(
            // Cursor overlay
            div()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_row()
                .child(div().invisible().child(before))
                .child(div().w(px(2.0)).h(rems(1.2)).bg(theme.foreground)),
        )
    } else {
        container.child(text_element)
    }
}
