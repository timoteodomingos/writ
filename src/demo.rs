use std::time::Duration;

use crate::editor::EditorAction;

#[derive(Clone, Debug)]
pub enum DemoStep {
    Action(EditorAction),
    Type(String),
    Wait(u64),
}

pub fn demo_script() -> Vec<DemoStep> {
    use DemoStep::*;
    use EditorAction as A;

    vec![
        // Start with a heading
        Type("# Welcome to writ".into()),
        Action(A::Enter),
        Wait(500),
        // Explain what it is
        Type("A **hybrid** markdown editor with _inline_ rendering.".into()),
        Wait(600),
        Action(A::Enter),
        Wait(500),
        // Lists
        Type("## Lists".into()),
        Action(A::Enter),
        Wait(500),
        Type("- First item".into()),
        Action(A::Enter),
        Type("Second item".into()),
        Action(A::Enter),
        Action(A::Tab),
        Type("Nested item".into()),
        Action(A::Enter),
        Type("Another nested".into()),
        Action(A::ShiftEnter),
        Type("Nested paragraph".into()),
        Action(A::Enter),
        Action(A::ShiftTab),
        Action(A::ShiftTab),
        Type("- Back to top level".into()),
        Action(A::Enter),
        Wait(800),
        Action(A::Enter),
        // Task list
        Type("Tasks work too:".into()),
        Action(A::Enter),
        Type("- [x] Learn writ".into()),
        Action(A::Enter),
        Type("Write documentation".into()),
        Action(A::Enter),
        Type("Record demo".into()),
        Action(A::Enter),
        Action(A::Enter),
        // Blockquotes
        Type("## Blockquotes".into()),
        Action(A::Enter),
        Wait(500),
        Type("> Blockquotes hide the `>` marker and show a border instead.".into()),
        Action(A::Enter),
        Wait(800),
        Action(A::Enter),
        // Nesting
        Type("## Nesting".into()),
        Action(A::Enter),
        Wait(500),
        Type("> Nested structures work too:".into()),
        Action(A::Enter),
        Type("- A list inside a blockquote".into()),
        Action(A::Enter),
        Type("With multiple items".into()),
        Action(A::Enter),
        Type("And more".into()),
        Action(A::Enter),
        Action(A::Enter),
        Wait(800),
        // Code blocks
        Type("## Code".into()),
        Action(A::Enter),
        Wait(500),
        Type("It uses a `monospace` font.".into()),
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
        Wait(500),
        // Links
        Type("## Links".into()),
        Action(A::Enter),
        Wait(500),
        Type("Check out [the repo](https://github.com/wilfreddenton/writ)!".into()),
        Wait(1000),
        Action(A::Enter),
        Type("Embed images:".into()),
        Action(A::Enter),
        Type("![Hello, World!](https://upload.wikimedia.org/wikipedia/commons/9/97/The_Earth_seen_from_Apollo_17.jpg)".into()),
        Action(A::Enter),
        Wait(2000),
        // Finish
        Type("---".into()),
        Action(A::Enter),
        Type("_That's writ!_".into()),
        Wait(500),
        Action(A::Enter),
    ]
}

pub struct DemoTiming {
    pub char_delay: Duration,
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
