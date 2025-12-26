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
use crate::title_bar::FileInfo;
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

    fn save_file(&self, cx: &mut Context<Self>) {
        let file_info = FileInfo::global(cx);
        let path = file_info.path.clone();
        let markdown = self.state.document.to_markdown();
        if let Err(e) = std::fs::write(&path, &markdown) {
            eprintln!("Failed to save file: {}", e);
        } else {
            cx.set_global(FileInfo { path, dirty: false });
            cx.notify();
        }
    }

    fn mark_dirty(&self, cx: &mut Context<Self>) {
        let file_info = FileInfo::global(cx);
        if !file_info.dirty {
            let path = file_info.path.clone();
            cx.set_global(FileInfo { path, dirty: true });
            cx.notify();
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;

        // Handle save with Ctrl-S or Cmd-S
        if keystroke.key.as_str() == "s"
            && (keystroke.modifiers.control || keystroke.modifiers.platform)
        {
            self.save_file(cx);
            return;
        }

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
            // Mark dirty for actions that modify the document
            let modifies_document = !matches!(
                action,
                EditorAction::MoveCursor(_) | EditorAction::SetCursor { .. }
            );

            self.state.apply(action);

            if modifies_document {
                self.mark_dirty(cx);
            }
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
                    block = block.with_cursor_offset(self.state.cursor.offset);

                    let pending_marker = self.state.pending_marker_text();
                    if !pending_marker.is_empty() {
                        block = block.with_pending_inline_marker(pending_marker.to_string());
                    }

                    if let Some(block_marker) = self.state.pending_block_marker_text() {
                        block = block.with_pending_block_marker(block_marker);
                    }

                    if let Some(indicator) = self.state.active_styles_indicator() {
                        block = block.with_active_styles_indicator(indicator);
                    }
                }

                // Create on_layout callback that stores layout in editor
                let entity_for_layout = entity.clone();
                block = block.on_layout(Rc::new(move |layout, _window, cx| {
                    entity_for_layout.update(cx, |editor, _| {
                        editor.block_layouts.insert(block_key, layout);
                    });
                }));

                // Create on_click callback that sets cursor position and focuses editor
                let entity_for_click = entity.clone();
                block = block.on_click(Rc::new(move |char_index, window, cx| {
                    entity_for_click.update(cx, |editor, cx| {
                        editor.state.apply(EditorAction::SetCursor {
                            block_key,
                            offset: char_index,
                        });
                        editor.focus_handle.focus(window);
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
