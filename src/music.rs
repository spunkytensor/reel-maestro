// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Optional AI-generated background soundtrack (e.g. Lyria 3 via OpenRouter).
//!
//! Thin wrapper over the OpenRouter music call: it just persists the returned audio bytes to the
//! run folder. The caller (`main::resolve_music`) owns retry/fallback policy, since this preview
//! model is flaky.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::openrouter::OpenRouter;

/// Generate a soundtrack from `prompt` and write it into `dir`, returning its path.
///
/// The provider decides the audio container (wav/mp3/...); we name the file `music.<format>`
/// using `track.format` so the extension always matches the actual bytes — `assemble`/`resume`
/// then locate it by trying the known extensions.
pub async fn generate(or: &OpenRouter, prompt: &str, dir: &Path) -> Result<PathBuf> {
    let track = or.generate_music(prompt).await?;
    let path = dir.join(format!("music.{}", track.format));
    std::fs::write(&path, &track.bytes)?;
    Ok(path)
}
