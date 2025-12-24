mod block;
mod state;

pub use state::{Cursor, Direction, EditorAction, EditorState};

use gpui::{
    App, Context, FocusHandle, Focusable, IntoElement, KeyDownEvent, ReadGlobal, Render,
    TextLayout, Window, div, prelude::*, rems,
};
use slotmap::{DefaultKey, SecondaryMap};

use crate::theme::Theme;
use block::Block;

/// The main editor GPUI entity
pub struct Editor {
    pub state: EditorState,
    focus_handle: FocusHandle,
    /// Text layouts for each block, used for click-to-position calculations
    pub(crate) block_layouts: SecondaryMap<DefaultKey, TextLayout>,
}

impl Editor {
    pub fn new(state: EditorState, cx: &mut Context<Self>) -> Self {
        Self {
            state,
            focus_handle: cx.focus_handle(),
            block_layouts: SecondaryMap::new(),
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

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx);
        let entity = cx.entity().clone();

        // Collect block views
        let block_views: Vec<_> = self
            .state
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

                Block {
                    block_idx,
                    block_key,
                    plain_text,
                    highlights,
                    cursor_offset,
                    foreground_color: theme.foreground,
                    editor: entity.clone(),
                }
            })
            .collect();

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .flex()
            .flex_col()
            .gap(rems(0.5))
            .text_color(theme.foreground)
            .children(block_views)
    }
}
