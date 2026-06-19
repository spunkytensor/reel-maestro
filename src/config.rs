// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Resolves runtime configuration: CLI flag > environment variable > built-in default.

use anyhow::{Context, Result};

use crate::Cli;

/// Fully resolved settings for one run. Every field has already been collapsed from the
/// CLI-flag > env-var > default precedence (see `load`), so the rest of the program reads
/// concrete values and never touches the environment again.
pub struct Config {
    /// OpenRouter API key (required; the only setting with no default).
    pub api_key: String,
    /// Model IDs routed to OpenRouter for each generation step.
    pub text_model: String,
    pub image_model: String,
    pub tts_model: String,
    pub music_model: String,
    pub video_model: String,
    /// Explicit TTS voice; `None` means auto-select by the script's narrator gender.
    pub voice: Option<String>,
    /// Local whisper-timestamped command used for real word-level caption timings.
    pub whisper_cmd: String,
    /// Whisper model size/name passed to that command (e.g. `base`, `small`, `large-v3`).
    pub whisper_model: String,
    /// Don't burn captions into the video.
    pub no_captions: bool,
    /// Don't synthesize spoken narration (silent or music-only video).
    pub no_narration: bool,
    /// Per-scene seconds when narration is disabled (no audio to derive timing from).
    pub scene_seconds: f64,
}

impl Config {
    /// Resolve every setting for this run. The API key is mandatory and fails fast if missing;
    /// everything else falls back through env var to a built-in default.
    pub fn load(cli: &Cli) -> Result<Config> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .context("OPENROUTER_API_KEY is not set (put it in a .env file or your environment)")?;

        // Apply the precedence `CLI flag > env var > default` for one string setting.
        let pick = |flag: &Option<String>, env: &str, default: &str| -> String {
            flag.clone()
                .or_else(|| std::env::var(env).ok())
                .unwrap_or_else(|| default.to_string())
        };

        Ok(Config {
            api_key,
            text_model: pick(
                &cli.text_model,
                "REELMAESTRO_TEXT_MODEL",
                "anthropic/claude-sonnet-4-6",
            ),
            image_model: pick(
                &cli.image_model,
                "REELMAESTRO_IMAGE_MODEL",
                "google/gemini-3.1-flash-image",
            ),
            tts_model: pick(
                &cli.tts_model,
                "REELMAESTRO_TTS_MODEL",
                "google/gemini-3.1-flash-tts-preview",
            ),
            music_model: pick(
                &cli.music_model,
                "REELMAESTRO_MUSIC_MODEL",
                "google/lyria-3-pro-preview",
            ),
            video_model: pick(
                &cli.video_model,
                "REELMAESTRO_VIDEO_MODEL",
                "google/veo-3.1-lite",
            ),
            voice: cli
                .voice
                .clone()
                .or_else(|| std::env::var("REELMAESTRO_VOICE").ok()),
            whisper_cmd: pick(
                &cli.whisper_cmd,
                "REELMAESTRO_WHISPER_CMD",
                "whisper_timestamped",
            ),
            whisper_model: pick(&cli.whisper_model, "REELMAESTRO_WHISPER_MODEL", "base"),
            no_captions: cli.no_captions || env_flag("REELMAESTRO_NO_CAPTIONS"),
            no_narration: cli.no_narration || env_flag("REELMAESTRO_NO_NARRATION"),
            scene_seconds: cli
                .scene_seconds
                .or_else(|| {
                    std::env::var("REELMAESTRO_SCENE_SECONDS")
                        .ok()?
                        .parse()
                        .ok()
                })
                .unwrap_or(4.0),
        })
    }
}

/// Read a boolean env var: true for "1", "true", "yes", "on" (case-insensitive).
fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            matches!(
                v.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}
