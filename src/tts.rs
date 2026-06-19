// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Narration -> audio.mp3 (one TTS call), with optional pitch-preserving tempo change.
//!
//! `audio.mp3` is the timeline clock for the whole reel: caption timing and scene durations are
//! all measured against it downstream, so this stage normalizes whatever the TTS provider returns
//! into a single mp3 at the requested tempo.

use std::path::Path;

use anyhow::Result;

use crate::ffmpeg;
use crate::openrouter::OpenRouter;

/// Synthesize `narration` to `out` (an .mp3 path). Handles whichever format the TTS
/// provider returns (mp3 or raw PCM) and applies the tempo change if `speed != 1.0`.
///
/// `speed` is the tempo multiplier (1.0 = native pace); the actual time-stretch is done by
/// ffmpeg's `atempo`, which preserves pitch so faster narration doesn't sound chipmunky.
pub async fn synthesize(or: &OpenRouter, narration: &str, out: &Path, speed: f64) -> Result<()> {
    let speech = or.text_to_speech(narration).await?;
    // Float compare with an epsilon — `speed` comes from a CLI default/parse, so exact 1.0
    // isn't guaranteed; treat anything within 1e-6 as "no tempo change requested".
    let unchanged_speed = (speed - 1.0).abs() < 1e-6;

    // Fast path: if the provider already gave us mp3 and no tempo change is needed, write the
    // bytes straight through and skip spawning ffmpeg entirely.
    if speech.format == "mp3" && unchanged_speed {
        std::fs::write(out, &speech.bytes)?;
        return Ok(());
    }

    // Otherwise stage the raw bytes to a sibling file and transcode to mp3. The `pcm` flag tells
    // ffmpeg the input is headerless raw samples (so it must be told the format), and `speed`
    // applies the `atempo` time-stretch in the same pass.
    let raw = out.with_file_name(format!("audio-raw.{}", speech.format));
    std::fs::write(&raw, &speech.bytes)?;
    ffmpeg::transcode_to_mp3(&raw, out, speech.format == "pcm", speed)?;
    Ok(())
}
