//! writ - A Typora-style markdown editor component for GPUI
//!
//! This crate provides an embeddable markdown editor with:
//! - Live inline rendering (markers hidden when cursor is away)
//! - Syntax highlighting for code blocks
//! - Streaming support for AI chat applications
//!
//! # Quick Start
//!
//! ```ignore
//! use writ::{Editor, EditorConfig};
//!
//! // Create with default config
//! let editor = cx.new(|cx| Editor::new("# Hello", cx));
//!
//! // Or with custom config
//! let config = EditorConfig::default();
//! let editor = cx.new(|cx| Editor::with_config("# Hello", config, cx));
//! ```
//!
//! # Streaming (for AI chat)
//!
//! ```ignore
//! editor.update(cx, |e, cx| e.begin_streaming(cx));
//! for token in stream {
//!     editor.update(cx, |e, cx| e.append(&token, cx));
//! }
//! editor.update(cx, |e, cx| e.end_streaming(cx));
//! ```

// Primary public API
pub use editor::{Direction, Editor, EditorAction, EditorConfig, EditorTheme};

// Internal modules - exposed for the writ application but not part of stable API
pub mod buffer;
pub mod config;
pub mod cursor;
pub mod demo;
pub mod editor;
pub mod highlight;
pub mod http;
pub mod line_view;
pub mod lines;
pub mod parser;
pub mod render;
pub mod theme;
pub mod title_bar;
pub mod window;
