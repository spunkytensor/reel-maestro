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

/// A recurring visual entity whose appearance must stay fixed across scenes — either a
/// person/animal (a [`Script::characters`] entry) or a place (a [`Script::locations`] entry).
/// `id` is a short slug the scenes reference; `description` is a fully-specified canonical visual
/// spec (worn up/down, sleeve length, decor, palette, …) reused verbatim everywhere it appears,
/// so the model can't free-fill the unspecified bits differently each time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Short, stable slug (e.g. `"man"`, `"date"`, `"restaurant"`) that scenes reference.
    pub id: String,
    /// Canonical, fully-specified visual description; the consistency anchor.
    pub description: String,
}

/// One visual beat: a slice of narration and the image to show during it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    /// The portion of the narration this scene covers (used to find its time window).
    pub line: String,
    /// A vivid, vertical-friendly prompt for the image generator.
    pub image_prompt: String,
    /// Ids of the recurring [`Script::characters`] that appear in this scene (a subset). Each is
    /// conditioned on its reference portrait (identity lock) so the same person carries through.
    /// Empty = no recurring character here, so the scene is generated independently and any
    /// people render as distinct individuals (the old `features_cast == false` behavior).
    //
    // `#[serde(default)]`: added after the format shipped. Older `script.json` files lack it (and
    // carry a now-ignored `features_cast` bool); they resume fine since resume reuses existing
    // images rather than regenerating, so per-scene conditioning never runs on old runs.
    #[serde(default)]
    pub cast_ids: Vec<String>,
    /// Id of the recurring [`Script::locations`] entry this scene is set in, or `""` for none.
    /// When set, the scene is also conditioned on that location's establishing reference image.
    #[serde(default)]
    pub location_id: String,
    /// How this scene enters from the PREVIOUS one: `"dissolve"` (cross-fade) or `"cut"`/`""`
    /// (hard cut). Only honored when both neighbors are Ken Burns stills; the first scene's value
    /// is ignored. Empty = cut, so older `script.json` files render with hard cuts as before.
    #[serde(default)]
    pub transition: String,
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
    /// Recurring people/animals, each with fixed visual traits, kept consistent across scenes by
    /// conditioning every scene that lists them (via [`Scene::cast_ids`]) on a per-character
    /// reference portrait. Empty when nothing recurs.
    #[serde(default)]
    pub characters: Vec<Entity>,
    /// Recurring locations/settings, each with a fixed look, kept consistent by conditioning
    /// scenes set there (via [`Scene::location_id`]) on a per-location establishing image.
    #[serde(default)]
    pub locations: Vec<Entity>,
    /// Legacy single-cast description from before multi-character support. Read-only back-compat:
    /// older `script.json` files set this string; [`Script::normalize_entities`] folds it into
    /// `characters` so those runs still behave like one recurring character.
    #[serde(default)]
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

impl Script {
    /// Fold a legacy `cast` string (older `script.json`) into `characters` so pre-multi-character
    /// runs behave like a single recurring character. No-op once `characters` is populated.
    pub fn normalize_entities(&mut self) {
        if self.characters.is_empty() && !self.cast.trim().is_empty() {
            self.characters.push(Entity {
                id: "main".to_string(),
                description: self.cast.trim().to_string(),
            });
        }
    }
}
