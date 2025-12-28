use anyhow::Result;
use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(short, long)]
    pub file: PathBuf,
}

impl Args {
    pub fn validate(self) -> Result<Self> {
        // File doesn't need to exist - we'll create it on save
        Ok(self)
    }
}
