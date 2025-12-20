use anyhow::{Result, bail};
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
        if !self.file.exists() {
            bail!("File '{}' does not exist", self.file.display())
        } else {
            Ok(self)
        }
    }
}
