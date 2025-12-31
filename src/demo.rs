//! Demo mode: scripted input automation for screen recording.

use std::time::Duration;

use crate::editor::{Direction, EditorAction};

/// A demo step: either an editor action or a wait.
#[derive(Clone, Debug)]
pub enum DemoStep {
    /// Execute an editor action.
    Action(EditorAction),
    /// Type a string (expands to multiple Type actions).
    Type(String),
    /// Wait for a duration (milliseconds).
    Wait(u64),
}

/// The demo script showing off writ's features.
pub fn demo_script() -> Vec<DemoStep> {
    use DemoStep::*;
    use Direction::*;
    use EditorAction as A;

    vec![
        // Start with a heading
        Type("# Welcome to writ".into()),
        Action(A::Enter),
        Wait(500),
        Action(A::Enter),
        // Explain what it is
        Type("A **hybrid** markdown editor with _inline_ rendering.".into()),
        Wait(600),
        Action(A::Enter),
        Wait(200),
        Action(A::Enter),
        Wait(500),
        // Lists
        Type("## Lists".into()),
        Action(A::Enter),
        Wait(500),
        Action(A::Enter),
        Type("- First item".into()),
        Action(A::ShiftEnter),
        Type("Second item".into()),
        Action(A::Enter),
        Type("    - Nested item".into()),
        Action(A::ShiftEnter),
        Type("Another nested".into()),
        Action(A::Enter),
        Type("- Back to top level".into()),
        Action(A::Enter),
        Wait(800),
        Action(A::Enter),
        // Task list
        Type("Tasks work too:".into()),
        Action(A::Enter),
        Type("- [x] Learn writ".into()),
        Action(A::ShiftEnter),
        Type("Write documentation".into()),
        Action(A::ShiftEnter),
        Type("Record demo".into()),
        Action(A::Enter),
        Wait(1000),
        Action(A::Enter),
        // Blockquotes
        Type("## Blockquotes".into()),
        Action(A::Enter),
        Wait(500),
        Action(A::Enter),
        Type("> Blockquotes hide the `>` marker".into()),
        Action(A::ShiftEnter),
        Type("and show a border instead.".into()),
        Action(A::Enter),
        Wait(800),
        Action(A::Enter),
        // Nesting
        Type("## Nesting".into()),
        Action(A::Enter),
        Wait(500),
        Action(A::Enter),
        Type("> Nested structures work too:".into()),
        Action(A::Enter),
        Type("> ".into()),
        Action(A::Enter),
        Type("> - A list inside a blockquote".into()),
        Action(A::ShiftEnter),
        Type("With multiple items".into()),
        Action(A::ShiftEnter),
        Type("And more".into()),
        Action(A::Enter),
        Wait(800),
        // Move up to show the nested markers
        Action(A::Move(Up)),
        Action(A::Move(Up)),
        Wait(600),
        // Move back down
        Action(A::Move(Down)),
        Action(A::Move(Down)),
        Wait(500),
        Action(A::Enter),
        // Code blocks
        Type("## Code".into()),
        Action(A::Enter),
        Wait(500),
        Action(A::Enter),
        Type("It uses a `monospace` font.".into()),
        Action(A::Enter),
        Action(A::Enter),
        Type("```rust".into()),
        Action(A::Enter),
        Type("fn main() {".into()),
        Action(A::Enter),
        Type("    println!(\"Hello, writ!\");".into()),
        Action(A::Enter),
        Type("}".into()),
        Action(A::Enter),
        Type("```".into()),
        Action(A::Enter),
        // Now cursor is outside the block - fences disappear
        Wait(1000),
        // Move back up into the code block to show fences reappearing
        Action(A::Move(Up)),
        Wait(400),
        Action(A::Move(Up)),
        Wait(800),
        // Move back down outside the block
        Action(A::Move(Down)),
        Action(A::Move(Down)),
        Wait(500),
        Action(A::Enter),
        // Links
        Type("## Links".into()),
        Action(A::Enter),
        Wait(500),
        Action(A::Enter),
        Type("Check out [the repo](https://github.com/wilfreddenton/writ)!".into()),
        Wait(1000),
        Action(A::Enter),
        Action(A::Enter),
        Type("Embed images:".into()),
        Action(A::Enter),
        Action(A::Enter),
        Type("![Hello, World!](https://upload.wikimedia.org/wikipedia/commons/9/97/The_Earth_seen_from_Apollo_17.jpg)".into()),
        Wait(500),
        Action(A::Enter),
        Wait(1000),
        Action(A::Enter),
        // Finish
        Type("---".into()),
        Action(A::Enter),
        Action(A::Enter),
        Type("_That's writ!_".into()),
        Wait(500),
        Action(A::Enter),
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
