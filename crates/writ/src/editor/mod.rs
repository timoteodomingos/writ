mod block;
mod state;

pub use state::{Cursor, Direction, EditorAction, EditorState};

use std::rc::Rc;

use gpui::{
    App, Context, FocusHandle, Focusable, IntoElement, KeyDownEvent, ReadGlobal, Render,
    TextLayout, Window, div, prelude::*, rems,
};
use slotmap::{DefaultKey, SecondaryMap};

use crate::theme::Theme;
use block::{Block, CursorInfo};

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
        let keystroke = &event.keystroke;

        // First check for special keys
        let action = match keystroke.key.as_str() {
            "left" => Some(EditorAction::MoveCursor(Direction::Left)),
            "right" => Some(EditorAction::MoveCursor(Direction::Right)),
            "up" => Some(EditorAction::MoveCursor(Direction::Up)),
            "down" => Some(EditorAction::MoveCursor(Direction::Down)),
            "home" => Some(EditorAction::MoveCursor(Direction::Home)),
            "end" => Some(EditorAction::MoveCursor(Direction::End)),
            "backspace" => Some(EditorAction::Backspace),
            "delete" => Some(EditorAction::Delete),
            "enter" => Some(EditorAction::Enter),
            "space" if !keystroke.modifiers.control && !keystroke.modifiers.platform => {
                Some(EditorAction::InsertText(" ".to_string()))
            }
            _ => {
                // For text input, use key_char (handles shift for capitals, etc.)
                if !keystroke.modifiers.control
                    && !keystroke.modifiers.alt
                    && !keystroke.modifiers.platform
                {
                    keystroke
                        .key_char
                        .as_ref()
                        .map(|c| EditorAction::InsertText(c.clone()))
                } else {
                    None
                }
            }
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
                let doc_block = &self.state.document.blocks[block_key];
                let is_cursor_block = block_key == self.state.cursor.block_key;

                let mut block = Block::from_document_block(block_idx, doc_block, theme);

                // Add cursor info if this is the cursor block
                if is_cursor_block {
                    block = block.with_cursor(CursorInfo {
                        offset: self.state.cursor.offset,
                        pending_marker: self.state.pending_marker_text().to_string(),
                        pending_block_marker: self.state.pending_block_marker_text(),
                    });
                }

                // Create on_layout callback that stores layout in editor
                let entity_for_layout = entity.clone();
                block = block.on_layout(Rc::new(move |layout, _window, cx| {
                    entity_for_layout.update(cx, |editor, _| {
                        editor.block_layouts.insert(block_key, layout);
                    });
                }));

                // Create on_click callback that sets cursor position
                let entity_for_click = entity.clone();
                block = block.on_click(Rc::new(move |char_index, _window, cx| {
                    entity_for_click.update(cx, |editor, cx| {
                        editor.state.apply(EditorAction::SetCursor {
                            block_key,
                            offset: char_index,
                        });
                        cx.notify();
                    });
                }));

                block
            })
            .collect();

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .flex()
            .flex_col()
            .gap(rems(0.5))
            .font_family("Iosevka Aile")
            .text_color(theme.foreground)
            .children(block_views)
    }
}
