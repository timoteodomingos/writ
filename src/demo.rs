//! Demo mode: scripted input automation for screen recording.

use std::time::Duration;

/// A single action in the demo script.
#[derive(Clone, Debug)]
pub enum DemoAction {
    /// Type a string of text (character by character with delay).
    Type(String),
    /// Press Enter.
    Enter,
    /// Press Shift+Enter (smart enter).
    ShiftEnter,
    /// Press Backspace.
    Backspace,
    /// Move cursor: "left", "right", "up", "down".
    Move(String),
    /// Wait for a duration (milliseconds).
    Wait(u64),
}

/// The demo script showing off writ's features.
pub fn demo_script() -> Vec<DemoAction> {
    use DemoAction::*;

    vec![
        // Start with a heading
        Type("# Welcome to writ".into()),
        Enter,
        Wait(800),
        Enter,
        // Explain what it is
        Type("A **hybrid** markdown editor with _inline_ rendering.".into()),
        Wait(600),
        Enter,
        Wait(200),
        Enter,
        Wait(500),
        // Lists
        Type("## Lists".into()),
        Enter,
        Wait(500),
        Enter,
        Type("- First item".into()),
        ShiftEnter,
        Type("Second item".into()),
        Enter,
        Type("    - Nested item".into()),
        ShiftEnter,
        Type("Another nested".into()),
        Enter,
        Type("- Back to top level".into()),
        Enter,
        Wait(800),
        Enter,
        // Task list
        Type("Tasks work too:".into()),
        Enter,
        Type("- [ ] Learn writ".into()),
        ShiftEnter,
        Type("Write documentation".into()),
        ShiftEnter,
        Type("Record demo".into()),
        Enter,
        Wait(1000),
        Enter,
        // Blockquotes
        Type("## Blockquotes".into()),
        Enter,
        Wait(500),
        Enter,
        Type("> Blockquotes hide the `>` marker".into()),
        ShiftEnter,
        Type("and show a border instead.".into()),
        Enter,
        Wait(800),
        Enter,
        // Nesting
        Type("## Nesting".into()),
        Enter,
        Wait(500),
        Enter,
        Type("> Nested structures work too:".into()),
        Enter,
        Type("> ".into()),
        Enter,
        Type("> - A list inside a blockquote".into()),
        ShiftEnter,
        Type("With multiple items".into()),
        ShiftEnter,
        Type("And more".into()),
        Enter,
        Wait(800),
        // Move up to show the nested markers
        Move("up".into()),
        Move("up".into()),
        Wait(600),
        // Move back down
        Move("down".into()),
        Move("down".into()),
        Wait(500),
        Enter,
        // Code blocks
        Type("## Code".into()),
        Enter,
        Wait(500),
        Enter,
        Type("Inline `code` uses a monospace font.".into()),
        Enter,
        Enter,
        Type("```rust".into()),
        Enter,
        Type("fn main() {".into()),
        Enter,
        Type("    println!(\"Hello, writ!\");".into()),
        Enter,
        Type("}".into()),
        Enter,
        Type("```".into()),
        Enter,
        // Now cursor is outside the block - fences disappear
        Wait(1000),
        // Move back up into the code block to show fences reappearing
        Move("up".into()),
        Wait(400),
        Move("up".into()),
        Wait(800),
        // Move back down outside the block
        Move("down".into()),
        Move("down".into()),
        Wait(500),
        Enter,
        // Links
        Type("## Links".into()),
        Enter,
        Wait(500),
        Enter,
        Type("Check out [the repo](https://github.com/wilfred/writ)!".into()),
        Wait(1000),
        Enter,
        Enter,
        // Finish
        Type("---".into()),
        Enter,
        Enter,
        Type("_That's writ!_".into()),
        Wait(500),
        Enter,
    ]
}

/// Timing configuration for the demo.
pub struct DemoTiming {
    /// Delay between each character when typing.
    pub char_delay: Duration,
    /// Delay after special keys (enter, etc).
    pub key_delay: Duration,
}

impl Default for DemoTiming {
    fn default() -> Self {
        Self {
            char_delay: Duration::from_millis(50),
            key_delay: Duration::from_millis(150),
        }
    }
}
