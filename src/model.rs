// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Shared data types passed between pipeline stages.
//!
//! These structs are the contract between stages: the LLM scriptwriter emits a [`Script`] as
//! JSON, [`Scene`]s drive image/video generation, and [`WordTiming`]s come back from the
//! transcriber to drive captions. All derive `Serialize`/`Deserialize` so a run folder's
//! `script.json` / `words.json` can be written once and re-read on `--from` resume.

use serde::{Deserialize, Serialize};

/// A single word with its spoken time window, from speech-to-text.
///
/// Produced by `transcribe` and consumed by `captions`/`assemble` to time each word's on-screen
/// highlight against the narration audio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTiming {
    /// The transcribed word (as spoken in the narration).
    pub word: String,
    /// Start of the word in the audio, in seconds from the beginning of the track.
    pub start_s: f64,
    /// End of the word in the audio, in seconds from the beginning of the track.
    pub end_s: f64,
}

/// One visual beat: a slice of narration and the image to show during it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    /// The portion of the narration this scene covers (used to find its time window).
    pub line: String,
    /// A vivid, vertical-friendly prompt for the image generator.
    pub image_prompt: String,
}

/// The full plan for one video.
///
/// This is the LLM's structured output and the single source of truth for the rest of the run:
/// it is serialized to `script.json` so a run can be resumed without re-calling the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    /// Short human title; also slugified into the run folder name.
    pub title: String,
    /// The full narration text, spoken verbatim by the TTS stage.
    pub narration: String,
    /// Ordered visual beats covering the narration, one image (or clip) each.
    pub scenes: Vec<Scene>,
    /// A short instrumental-music description (mood/genre/tempo) for the soundtrack.
    pub music_prompt: String,
    /// Visual description of the recurring person/animal(s) and their fixed traits, used to
    /// keep them consistent across scenes. Empty when nothing recurs.
    pub cast: String,
    /// A concept for an eye-catching cover/thumbnail image for the whole reel.
    //
    // `#[serde(default)]`: this field was added after the format shipped, so older `script.json`
    // files lack it. Defaulting to "" lets those runs still deserialize (resume) instead of
    // failing; callers treat an empty value as "fall back to the hook scene".
    #[serde(default)]
    pub poster_prompt: String,
    /// Best-fitting narrator voice gender for the story: "male", "female", or "neutral".
    /// Used to auto-pick a voice when none is set explicitly.
    //
    // `#[serde(default)]` for the same back-compat reason as `poster_prompt`: empty means
    // "no preference", which `pick_voice` maps to the default voice.
    #[serde(default)]
    pub narrator_gender: String,
}
