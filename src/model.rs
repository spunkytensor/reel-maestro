// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Shared data types passed between pipeline stages.
//!
//! These structs are the contract between stages: the LLM scriptwriter emits a [`Script`] as
//! JSON, [`Scene`]s drive image/video generation, and [`WordTiming`]s come back from the
//! transcriber to drive captions. All derive `Serialize`/`Deserialize` so a run folder's
//! `script.json` / `words.json` can be written once and re-read on `--from` resume.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// Short, stable slug (e.g. `"man"`, `"date"`, `"restaurant"`) that scenes reference.
    pub id: String,
    /// Canonical, fully-specified visual description; the consistency anchor.
    pub description: String,
}

/// One visual beat: a slice of narration and the image to show during it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    /// Stable identity of this visual beat across reordering and immutable revisions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
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
    /// Video direction for this scene (one camera move + the subject's small action), written by
    /// the scriptwriter with full story context. Used only in `--video` mode; the video prompt
    /// falls back to generic gentle motion when empty.
    //
    // `#[serde(default)]` for the same back-compat reason as `transition`: older `script.json`
    // files lack it, and an empty value means "use the generic motion default".
    #[serde(default)]
    pub motion_prompt: String,
}

/// One chapter of a long-form (youtube-format) video: a titled span of the flat
/// [`Script::scenes`] list plus the narration it covers. Chapters drive per-chapter TTS,
/// chunked rendering, and the YouTube chapter timestamps in `youtube.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    /// Short chapter title (shown as a YouTube chapter marker).
    pub title: String,
    /// One-line summary from the outline (context for neighboring chapters; not rendered).
    #[serde(default)]
    pub summary: String,
    /// This chapter's narration. All chapters' narrations concatenate (space-joined) to
    /// [`Script::narration`].
    pub narration: String,
    /// Index of this chapter's first scene in the flat [`Script::scenes`] list.
    pub scene_start: usize,
    /// How many consecutive scenes belong to this chapter.
    pub scene_count: usize,
}

/// The full plan for one video.
///
/// This is the LLM's structured output and the single source of truth for the rest of the run:
/// it is serialized to `script.json` so a run can be resumed without re-calling the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Output format this script was written for: `""` (= reel, all older runs) or
    /// `"youtube"`. Resume reads this so render geometry always matches the stored assets.
    #[serde(default)]
    pub format: String,
    /// Long-form chapter structure (youtube format). Empty for reels — every downstream stage
    /// takes the original single-pass path when this is empty.
    #[serde(default)]
    pub chapters: Vec<Chapter>,
    /// YouTube video description (youtube format only; written to `youtube.md`).
    #[serde(default)]
    pub description: String,
    /// YouTube tags (youtube format only; written to `youtube.md`).
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Script {
    /// Assign deterministic IDs to scenes imported from the legacy index-only format. Existing
    /// IDs are never changed. Duplicate legacy scenes receive deterministic occurrence suffixes.
    pub fn ensure_scene_ids(&mut self) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for scene in &mut self.scenes {
            if !scene.id.trim().is_empty() {
                continue;
            }
            let input = serde_json::json!({
                "line": scene.line,
                "image_prompt": scene.image_prompt,
                "cast_ids": scene.cast_ids,
                "location_id": scene.location_id,
                "motion_prompt": scene.motion_prompt,
            });
            let digest = Sha256::digest(serde_json::to_vec(&input).expect("scene is serializable"));
            let base = format!("scene-{}", hex_prefix(&digest, 16));
            let occurrence = seen.entry(base.clone()).or_default();
            scene.id = if *occurrence == 0 {
                base
            } else {
                format!("{base}-{}", *occurrence + 1)
            };
            *occurrence += 1;
        }
    }

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

fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    bytes
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .take(chars)
        .map(|nibble| char::from_digit(nibble.into(), 16).unwrap())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Scene, Script, WordTiming};

    #[test]
    fn words_json_requires_valid_word_timings_and_round_trips() {
        let valid = br#"[{"word":"hello","start_s":0.0,"end_s":0.5}]"#;
        let words: Vec<WordTiming> = serde_json::from_slice(valid).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "hello");
        assert_eq!(words[0].start_s, 0.0);
        assert_eq!(words[0].end_s, 0.5);
        assert_eq!(serde_json::to_vec(&words).unwrap(), valid);

        assert!(serde_json::from_slice::<Vec<WordTiming>>(br#"not json"#).is_err());
    }

    #[test]
    fn legacy_scene_ids_are_cryptographic_deterministic_and_persisted() {
        let scene = Scene {
            id: String::new(),
            line: "hello world".into(),
            image_prompt: "a sunrise".into(),
            cast_ids: Vec::new(),
            location_id: String::new(),
            transition: String::new(),
            motion_prompt: String::new(),
        };
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "title":"test", "narration":"hello world hello world", "scenes":[scene.clone(), scene],
            "music_prompt":""
        }))
        .unwrap();
        script.ensure_scene_ids();
        assert!(script.scenes[0].id.starts_with("scene-"));
        assert_eq!(script.scenes[0].id.len(), "scene-".len() + 16);
        assert_eq!(script.scenes[1].id, format!("{}-2", script.scenes[0].id));
        let ids: Vec<_> = script.scenes.iter().map(|scene| scene.id.clone()).collect();
        script.ensure_scene_ids();
        assert_eq!(
            ids,
            script
                .scenes
                .iter()
                .map(|scene| scene.id.clone())
                .collect::<Vec<_>>()
        );
    }
}
