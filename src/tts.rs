// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Narration -> audio.mp3 (one TTS call), with optional pitch-preserving tempo change.

use std::path::Path;

use anyhow::Result;

use crate::ffmpeg;
use crate::openrouter::OpenRouter;

/// Synthesize `narration` to `out` (an .mp3 path). Handles whichever format the TTS
/// provider returns (mp3 or raw PCM) and applies the tempo change if `speed != 1.0`.
pub async fn synthesize(or: &OpenRouter, narration: &str, out: &Path, speed: f64) -> Result<()> {
    let speech = or.text_to_speech(narration).await?;
    let unchanged_speed = (speed - 1.0).abs() < 1e-6;

    // mp3 at native speed needs no ffmpeg pass; everything else is transcoded to mp3.
    if speech.format == "mp3" && unchanged_speed {
        std::fs::write(out, &speech.bytes)?;
        return Ok(());
    }

    let raw = out.with_file_name(format!("audio-raw.{}", speech.format));
    std::fs::write(&raw, &speech.bytes)?;
    ffmpeg::transcode_to_mp3(&raw, out, speech.format == "pcm", speed)?;
    Ok(())
}
