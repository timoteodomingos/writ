//! Agent view component with split-pane layout.
//!
//! Provides a horizontal split between the document editor (left) and
//! an optional chat panel (right) for agent responses.

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, Rems, Render, Window, actions, div,
    prelude::*, px, rgb,
};

use crate::editor::{Editor, EditorConfig};

actions!(agent_view, [ToggleChatPanel]);

/// Container managing the document editor and optional chat panel.
pub struct AgentView {
    /// The main document editor (left pane).
    document_editor: Entity<Editor>,
    /// The chat panel editor (right pane), read-only.
    chat_editor: Entity<Editor>,
    /// The prompt input editor (bottom of chat pane).
    prompt_editor: Entity<Editor>,
    /// Whether the chat panel is currently visible.
    chat_panel_visible: bool,
    /// Focus handle for the agent view.
    focus_handle: FocusHandle,
}

impl AgentView {
    /// Create a new agent view with the given document content and editor config.
    pub fn new(document_content: &str, config: EditorConfig, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        // Create the main document editor
        let document_editor =
            cx.new(|cx| Editor::with_config(document_content, config.clone(), cx));

        // Create the chat editor with hardcoded sample content for now
        let chat_content = r#"# Agent Response

This is a **sample response** from the AI agent.

## Features

- Markdown rendering works
- Code blocks are highlighted
- Lists render properly

```rust
fn main() {
    println!("Hello from the agent!");
}
```

> The chat panel uses the same Editor component
> but in read-only mode.

Try toggling this panel with the keybinding!
"#;
        let chat_editor = cx.new(|cx| {
            let mut editor = Editor::with_config(chat_content, config.clone(), cx);
            // Block input to make it read-only
            editor.set_input_blocked(true);
            // Not primary - don't update global status bar/title bar
            editor.set_primary(false);
            editor
        });

        // Create the prompt input editor with compact padding and no max width
        let prompt_config = EditorConfig {
            padding_x: Rems(1.0),
            padding_top: Rems(1.0),
            padding_bottom: Rems(1.0),
            max_line_width: None,
            ..config
        };
        let prompt_editor = cx.new(|cx| {
            let mut editor = Editor::with_config("", prompt_config, cx);
            // Not primary - don't update global status bar/title bar
            editor.set_primary(false);
            editor
        });

        Self {
            document_editor,
            chat_editor,
            prompt_editor,
            chat_panel_visible: false,
            focus_handle,
        }
    }

    /// Get a reference to the document editor.
    pub fn document_editor(&self) -> &Entity<Editor> {
        &self.document_editor
    }

    /// Get a reference to the chat editor.
    pub fn chat_editor(&self) -> &Entity<Editor> {
        &self.chat_editor
    }

    /// Toggle the chat panel visibility.
    pub fn toggle_chat_panel(&mut self, cx: &mut Context<Self>) {
        self.chat_panel_visible = !self.chat_panel_visible;
        cx.notify();
    }

    /// Check if the chat panel is visible.
    pub fn is_chat_panel_visible(&self) -> bool {
        self.chat_panel_visible
    }

    /// Set chat panel visibility.
    pub fn set_chat_panel_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.chat_panel_visible != visible {
            self.chat_panel_visible = visible;
            cx.notify();
        }
    }
}

impl Focusable for AgentView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chat_visible = self.chat_panel_visible;

        div()
            .id("agent-view")
            .key_context("AgentView")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleChatPanel, _window, cx| {
                this.toggle_chat_panel(cx);
            }))
            .flex()
            .flex_row()
            .size_full()
            // Document editor - takes full width when chat hidden, or flex-1 when chat visible
            .child(
                div()
                    .id("document-pane")
                    .when(chat_visible, |d| d.flex_1().min_w_0())
                    .when(!chat_visible, |d| d.size_full())
                    .overflow_hidden()
                    .child(self.document_editor.clone()),
            )
            // Chat panel - only rendered when visible
            .when(chat_visible, |d| {
                d.child(
                    // Divider
                    div().w(px(1.0)).h_full().bg(rgb(0x44475a)),
                )
                .child(
                    div()
                        .id("chat-pane")
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        // Chat content area
                        .child(
                            div()
                                .id("chat-content")
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .child(self.chat_editor.clone()),
                        )
                        // Divider between chat and prompt
                        .child(div().h(px(1.0)).w_full().bg(rgb(0x44475a)))
                        // Prompt input area
                        .child(
                            div()
                                .id("prompt-input")
                                .h(px(100.0))
                                .min_h(px(100.0))
                                .overflow_hidden()
                                .child(self.prompt_editor.clone()),
                        ),
                )
            })
    }
}
