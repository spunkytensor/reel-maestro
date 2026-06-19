//! Shared data types passed between pipeline stages.

use serde::{Deserialize, Serialize};

/// A single word with its spoken time window, from speech-to-text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTiming {
    pub word: String,
    pub start_s: f64,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub title: String,
    pub narration: String,
    pub scenes: Vec<Scene>,
    /// A short instrumental-music description (mood/genre/tempo) for the soundtrack.
    pub music_prompt: String,
    /// Visual description of the recurring person/animal(s) and their fixed traits, used to
    /// keep them consistent across scenes. Empty when nothing recurs.
    pub cast: String,
    /// A concept for an eye-catching cover/thumbnail image for the whole reel. Optional so
    /// run folders written before this field still resume.
    #[serde(default)]
    pub poster_prompt: String,
    /// Best-fitting narrator voice gender for the story: "male", "female", or "neutral".
    /// Used to auto-pick a voice when none is set explicitly. Optional for older folders.
    #[serde(default)]
    pub narrator_gender: String,
}
