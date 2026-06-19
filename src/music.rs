//! Optional AI-generated background soundtrack (e.g. Lyria 3 via OpenRouter).

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::openrouter::OpenRouter;

/// Generate a soundtrack from `prompt` and write it into `dir`, returning its path.
pub async fn generate(or: &OpenRouter, prompt: &str, dir: &Path) -> Result<PathBuf> {
    let track = or.generate_music(prompt).await?;
    let path = dir.join(format!("music.{}", track.format));
    std::fs::write(&path, &track.bytes)?;
    Ok(path)
}
