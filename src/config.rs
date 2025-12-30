use anyhow::Result;
use std::path::PathBuf;

use clap::Parser;
use gpui::Global;

/// Platform-specific default fonts
#[cfg(target_os = "windows")]
const DEFAULT_TEXT_FONT: &str = "Segoe UI";
#[cfg(target_os = "windows")]
const DEFAULT_CODE_FONT: &str = "Consolas";

#[cfg(target_os = "macos")]
const DEFAULT_TEXT_FONT: &str = ".AppleSystemUIFont";
#[cfg(target_os = "macos")]
const DEFAULT_CODE_FONT: &str = "Menlo";

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const DEFAULT_TEXT_FONT: &str = "Liberation Sans";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const DEFAULT_CODE_FONT: &str = "Liberation Mono";

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// File to open (not required in demo mode)
    #[arg(short, long, required_unless_present = "demo")]
    pub file: Option<PathBuf>,

    /// Run in demo mode with scripted input
    #[arg(long)]
    pub demo: bool,

    /// Font for regular text
    #[arg(long, env = "WRIT_TEXT_FONT", default_value = DEFAULT_TEXT_FONT)]
    pub text_font: String,

    /// Font for code blocks and inline code
    #[arg(long, env = "WRIT_CODE_FONT", default_value = DEFAULT_CODE_FONT)]
    pub code_font: String,
}

impl Global for Config {}

impl Config {
    pub fn validate(self) -> Result<Self> {
        // File doesn't need to exist - we'll create it on save
        Ok(self)
    }
}
