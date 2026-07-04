// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Resolves runtime configuration: CLI flag > environment variable > quality-tier default.

use anyhow::{bail, Context, Result};
use clap::ValueEnum;

use crate::Cli;

/// Output format: a vertical short-form reel or a landscape long-form YouTube video. Drives the
/// canvas geometry, script structure (single-shot vs chaptered), caption styling, poster aspect,
/// and the video-model default.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Vertical 9:16 short (~30-60s) for TikTok/Reels/Shorts — the original pipeline.
    Reel,
    /// Landscape 16:9 long-form (~1-12 min) for YouTube: chaptered script, per-chapter TTS and
    /// rendering, 1280x720 thumbnail, and a `youtube.md` metadata file.
    Youtube,
}

/// A render canvas in pixels. The single source of truth for output geometry — every dimension
/// downstream (crops, filtergraph scales, caption PlayRes) derives from it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub w: u32,
    pub h: u32,
}

impl Canvas {
    /// The aspect-ratio string the OpenRouter image/video APIs expect.
    pub fn aspect_str(self) -> &'static str {
        if self.w > self.h {
            "16:9"
        } else {
            "9:16"
        }
    }
}

/// The scene/render canvas for a format.
pub fn canvas(f: Format) -> Canvas {
    match f {
        Format::Reel => Canvas { w: 1080, h: 1920 },
        Format::Youtube => Canvas { w: 1920, h: 1080 },
    }
}

/// The poster/thumbnail canvas for a format. YouTube thumbnails are 1280x720 (16:9), not the
/// video canvas size; reels reuse the scene canvas (a vertical cover image).
pub fn poster_canvas(f: Format) -> Canvas {
    match f {
        Format::Reel => Canvas { w: 1080, h: 1920 },
        Format::Youtube => Canvas { w: 1280, h: 720 },
    }
}

/// Spoken-narration pace the length budgets assume.
pub const WORDS_PER_MINUTE: f64 = 145.0;

/// Target narration word range for a long-form video of `minutes` (±15% around the pace).
pub fn word_budget(minutes: f64) -> (usize, usize) {
    let target = minutes * WORDS_PER_MINUTE;
    (
        (target * 0.85).round() as usize,
        (target * 1.15).round() as usize,
    )
}

/// Target scene-count range for `minutes` of content: one scene per ~7-9s of narration,
/// floored at 8 so even a 1-minute video gets real visual rhythm.
pub fn scene_budget(minutes: f64) -> (usize, usize) {
    let secs = minutes * 60.0;
    let lo = ((secs / 9.0).round() as usize).max(8);
    let hi = ((secs / 7.0).round() as usize).max(lo);
    (lo, hi)
}

/// Chapter count for `minutes` of content: roughly one per minute, clamped to 2-12 so even a
/// short long-form video gets structure and a 12-minute one stays manageable (≤12 script calls).
pub fn chapter_count(minutes: f64) -> usize {
    (minutes.round() as usize).clamp(2, 12)
}

/// Quality/cost tier. Supplies the *defaults* for the model choices and validation depth —
/// an explicit per-model flag or env var always overrides the tier's pick.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Quality {
    /// Cheapest models, validation off — fast, rough previews (~3-5x cheaper than standard).
    Draft,
    /// The regular defaults: best quality-per-dollar balance.
    Standard,
    /// Best models, 1080p video, deepest validation — for final renders.
    Premium,
}

/// The per-tier defaults, resolved by [`tier_defaults`]. Pure data (no env reads) so tier
/// selection is unit-testable.
pub struct TierDefaults {
    pub text_model: &'static str,
    pub image_model: &'static str,
    pub judge_model: &'static str,
    pub video_model: &'static str,
    pub video_resolution: &'static str,
    pub validate_scene: usize,
}

/// Map a quality tier to its model/validation defaults.
///
/// Model picks (July 2026): the judge is always a cheap multimodal Gemini Flash — judging is an
/// image-heavy, constrained scoring task where Flash performs on par with the (10x pricier)
/// script model. Draft swaps the scriptwriter to Haiku and images to Nano Banana 2 (about half
/// the image cost; see the cost table in `.env.example`). Premium upgrades the scriptwriter to
/// Opus and video to Veo 3.1 Fast at 1080p.
pub fn tier_defaults(q: Quality) -> TierDefaults {
    match q {
        Quality::Draft => TierDefaults {
            text_model: "anthropic/claude-haiku-4-5",
            image_model: "google/gemini-3.1-flash-image",
            judge_model: "google/gemini-3.1-flash-lite",
            video_model: "google/veo-3.1-lite",
            video_resolution: "720p",
            validate_scene: 0,
        },
        Quality::Standard => TierDefaults {
            text_model: "anthropic/claude-sonnet-4-6",
            image_model: "google/gemini-3-pro-image",
            judge_model: "google/gemini-2.5-flash",
            video_model: "google/veo-3.1-lite",
            video_resolution: "720p",
            validate_scene: 2,
        },
        Quality::Premium => TierDefaults {
            text_model: "anthropic/claude-opus-4-8",
            image_model: "google/gemini-3-pro-image",
            judge_model: "google/gemini-2.5-flash",
            video_model: "google/veo-3.1-fast",
            video_resolution: "1080p",
            validate_scene: 3,
        },
    }
}

/// Estimated video cost per second by model and resolution (July 2026 OpenRouter pricing:
/// Veo 3.1 Lite $0.05/$0.08, Fast $0.10/$0.12, Standard $0.40; Wan 2.6 from $0.04; Seedance
/// 2.0 Fast from ~$0.054, plain ~$0.067; Kling v3.0 from $0.126). An unrecognized model falls
/// back to the cheapest rate so an estimate still prints rather than nothing.
pub fn video_cost_per_second(model: &str, resolution: &str) -> f64 {
    let hd = resolution.trim().starts_with("1080");
    if model.contains("veo-3.1-lite") {
        if hd {
            0.08
        } else {
            0.05
        }
    } else if model.contains("veo-3.1-fast") {
        if hd {
            0.12
        } else {
            0.10
        }
    } else if model.contains("veo") {
        0.40
    } else if model.contains("wan-2.6") {
        0.04
    } else if model.contains("seedance-2.0-fast") {
        0.054
    } else if model.contains("seedance") {
        0.067
    } else if model.contains("kling") {
        0.126
    } else {
        0.05
    }
}

/// Estimated flat cost per generated music track (Lyria bills per track, not per second).
pub fn music_cost(_model: &str) -> f64 {
    0.08
}

/// Fully resolved settings for one run. Every field has already been collapsed from the
/// CLI-flag > env-var > quality-tier-default precedence (see `load`), so the rest of the
/// program reads concrete values and never touches the environment again.
pub struct Config {
    /// OpenRouter API key (required; the only setting with no default).
    pub api_key: String,
    /// Output format: vertical short reel or landscape long-form YouTube video.
    pub format: Format,
    /// Target video length in minutes (youtube format only; reels ignore it).
    pub minutes: f64,
    /// Model IDs routed to OpenRouter for each generation step.
    pub text_model: String,
    /// Multimodal model for the consistency QA judge (separate from `text_model` because
    /// judging is a cheap-model task).
    pub judge_model: String,
    pub image_model: String,
    pub tts_model: String,
    pub music_model: String,
    pub video_model: String,
    /// Whether `video_model` came from an explicit `--video-model`/env (not a format/tier
    /// default). On `--from` resume, a non-explicit model is re-derived from the *stored*
    /// format so a youtube run's stills rehydrate with the youtube video default (Wan) even
    /// when the resume command omits `--format`.
    pub video_model_explicit: bool,
    /// Explicit TTS voice; `None` means auto-select by the script's narrator gender.
    pub voice: Option<String>,
    /// Veo clip resolution ("720p" / "1080p").
    pub video_resolution: String,
    /// Per-scene validation effort: 0 = off, 2/3 = candidates judged per scene.
    pub validate_scene: usize,
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
    /// Resolve every setting for this run. The API key is mandatory only for runs that will call
    /// OpenRouter; a plain `--from` re-stitch is local/free.
    pub fn load(cli: &Cli, needs_api: bool) -> Result<Config> {
        let api_key = if needs_api {
            let key = std::env::var("OPENROUTER_API_KEY").context(
                "OPENROUTER_API_KEY is not set (put it in a .env file or your environment)",
            )?;
            validate_api_key(&key)?;
            key
        } else {
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default()
        };

        let quality = cli
            .quality
            .or_else(quality_from_env)
            .unwrap_or(Quality::Standard);
        let tier = tier_defaults(quality);

        let format = cli.format.or_else(format_from_env).unwrap_or(Format::Reel);
        let minutes = positive_finite(
            match cli.minutes {
                Some(v) => v,
                None => parse_env_f64("REELMAESTRO_MINUTES")?.unwrap_or(3.0),
            },
            "minutes",
        )?;
        match format {
            Format::Youtube if !(1.0..=12.0).contains(&minutes) => {
                bail!("--minutes must be between 1 and 12 (got {minutes})");
            }
            Format::Reel if cli.minutes.is_some() => {
                eprintln!("  note: --minutes only applies to --format youtube; ignoring");
            }
            _ => {}
        }
        // Long-form defaults to Wan 2.6: cheaper per second than Veo Lite and its clips run up
        // to 15s, so long scene windows aren't slow-motion-stretched. Reels keep the tier's
        // (Veo) pick; an explicit --video-model/env always wins over both.
        let tier_video_model = match format {
            Format::Youtube => "alibaba/wan-2.6",
            Format::Reel => tier.video_model,
        };

        // Apply the precedence `CLI flag > env var > tier default` for one string setting.
        let pick = |flag: &Option<String>, env: &str, default: &str| -> String {
            flag.clone()
                .or_else(|| std::env::var(env).ok())
                .unwrap_or_else(|| default.to_string())
        };

        Ok(Config {
            api_key,
            format,
            minutes,
            text_model: pick(&cli.text_model, "REELMAESTRO_TEXT_MODEL", tier.text_model),
            judge_model: pick(
                &cli.judge_model,
                "REELMAESTRO_JUDGE_MODEL",
                tier.judge_model,
            ),
            image_model: pick(
                &cli.image_model,
                "REELMAESTRO_IMAGE_MODEL",
                tier.image_model,
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
                tier_video_model,
            ),
            video_model_explicit: cli.video_model.is_some()
                || std::env::var("REELMAESTRO_VIDEO_MODEL").is_ok(),
            voice: cli
                .voice
                .clone()
                .or_else(|| std::env::var("REELMAESTRO_VOICE").ok()),
            video_resolution: parse_video_resolution(&pick(
                &cli.video_resolution,
                "REELMAESTRO_VIDEO_RESOLUTION",
                tier.video_resolution,
            ))?,
            validate_scene: match cli.validate_scene {
                Some(v) => v,
                None => match std::env::var("REELMAESTRO_VALIDATE_SCENE") {
                    Ok(v) => crate::parse_validate_scene(&v)
                        .map_err(anyhow::Error::msg)
                        .with_context(|| "REELMAESTRO_VALIDATE_SCENE is invalid")?,
                    Err(std::env::VarError::NotPresent) => tier.validate_scene,
                    Err(e) => return Err(e).context("could not read REELMAESTRO_VALIDATE_SCENE"),
                },
            },
            whisper_cmd: pick(
                &cli.whisper_cmd,
                "REELMAESTRO_WHISPER_CMD",
                "whisper_timestamped",
            ),
            whisper_model: pick(&cli.whisper_model, "REELMAESTRO_WHISPER_MODEL", "base"),
            no_captions: cli.no_captions || env_flag("REELMAESTRO_NO_CAPTIONS"),
            no_narration: cli.no_narration || env_flag("REELMAESTRO_NO_NARRATION"),
            scene_seconds: positive_finite(
                match cli.scene_seconds {
                    Some(v) => v,
                    None => parse_env_f64("REELMAESTRO_SCENE_SECONDS")?.unwrap_or(4.0),
                },
                "scene seconds",
            )?,
        })
    }
}

fn positive_finite(value: f64, name: &str) -> Result<f64> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        bail!("{name} must be finite and positive, got {value:?}")
    }
}

fn validate_api_key(key: &str) -> Result<()> {
    let trimmed = key.trim();
    if trimmed.is_empty()
        || trimmed == "sk-or-v1-..."
        || trimmed.contains("replace-with")
        || trimmed.contains("replace_me")
        || trimmed.contains("your-openrouter-api-key")
        || trimmed.contains("YOUR_")
        || trimmed.contains('<')
        || trimmed.eq_ignore_ascii_case("placeholder")
    {
        bail!("OPENROUTER_API_KEY is empty or still a placeholder");
    }
    Ok(())
}

fn parse_env_f64(name: &str) -> Result<Option<f64>> {
    match std::env::var(name) {
        Ok(v) => v
            .parse::<f64>()
            .map(Some)
            .with_context(|| format!("{name} must be a number (got {v:?})")),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(e).with_context(|| format!("could not read {name}")),
    }
}

fn parse_video_resolution(v: &str) -> Result<String> {
    crate::parse_video_resolution(v).map_err(anyhow::Error::msg)
}

/// Read the output format from `REELMAESTRO_FORMAT` (reel/youtube), if set and valid.
fn format_from_env() -> Option<Format> {
    let v = std::env::var("REELMAESTRO_FORMAT").ok()?;
    match v.trim().to_lowercase().as_str() {
        "reel" => Some(Format::Reel),
        "youtube" => Some(Format::Youtube),
        other => {
            eprintln!("  note: unknown REELMAESTRO_FORMAT {other:?}; using reel");
            None
        }
    }
}

/// Read the quality tier from `REELMAESTRO_QUALITY` (draft/standard/premium), if set and valid.
fn quality_from_env() -> Option<Quality> {
    let v = std::env::var("REELMAESTRO_QUALITY").ok()?;
    match v.trim().to_lowercase().as_str() {
        "draft" => Some(Quality::Draft),
        "standard" => Some(Quality::Standard),
        "premium" => Some(Quality::Premium),
        other => {
            eprintln!("  note: unknown REELMAESTRO_QUALITY {other:?}; using standard");
            None
        }
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

#[cfg(test)]
mod tests {
    use super::{
        canvas, chapter_count, music_cost, poster_canvas, scene_budget, tier_defaults,
        video_cost_per_second, word_budget, Canvas, Format, Quality,
    };

    #[test]
    fn canvases_track_format() {
        assert_eq!(canvas(Format::Reel), Canvas { w: 1080, h: 1920 });
        assert_eq!(canvas(Format::Youtube), Canvas { w: 1920, h: 1080 });
        assert_eq!(canvas(Format::Reel).aspect_str(), "9:16");
        assert_eq!(canvas(Format::Youtube).aspect_str(), "16:9");
        // YouTube thumbnails are 1280x720, not the video canvas; reels reuse the scene canvas.
        assert_eq!(poster_canvas(Format::Youtube), Canvas { w: 1280, h: 720 });
        assert_eq!(poster_canvas(Format::Reel), Canvas { w: 1080, h: 1920 });
    }

    #[test]
    fn length_budgets_scale_with_minutes() {
        // 3 minutes at 145 wpm: ±15% around 435 words.
        assert_eq!(word_budget(3.0), (370, 500));
        // One scene per 7-9s: 180s → 20-26 scenes.
        assert_eq!(scene_budget(3.0), (20, 26));
        // Scene floor: even 1 minute gets at least 8 scenes.
        assert!(scene_budget(1.0).0 >= 8);
        // ~1 chapter/minute, clamped 2-12.
        assert_eq!(chapter_count(1.0), 2);
        assert_eq!(chapter_count(5.0), 5);
        assert_eq!(chapter_count(20.0), 12);
    }

    #[test]
    fn tiers_scale_models_and_validation() {
        let draft = tier_defaults(Quality::Draft);
        let standard = tier_defaults(Quality::Standard);
        let premium = tier_defaults(Quality::Premium);

        // Standard is today's defaults, unchanged.
        assert_eq!(standard.text_model, "anthropic/claude-sonnet-4-6");
        assert_eq!(standard.image_model, "google/gemini-3-pro-image");
        assert_eq!(standard.video_model, "google/veo-3.1-lite");
        assert_eq!(standard.video_resolution, "720p");
        assert_eq!(standard.validate_scene, 2);

        // Draft trades quality for cost and skips validation.
        assert_eq!(draft.text_model, "anthropic/claude-haiku-4-5");
        assert_eq!(draft.image_model, "google/gemini-3.1-flash-image");
        assert_eq!(draft.validate_scene, 0);

        // Premium upgrades the scriptwriter, video tier, and validation depth.
        assert_eq!(premium.text_model, "anthropic/claude-opus-4-8");
        assert_eq!(premium.video_model, "google/veo-3.1-fast");
        assert_eq!(premium.video_resolution, "1080p");
        assert_eq!(premium.validate_scene, 3);

        // The judge is always a cheap multimodal model, never the script model.
        for tier in [draft, standard, premium] {
            assert!(tier.judge_model.contains("flash"));
        }
    }

    #[test]
    fn video_cost_tracks_model_and_resolution() {
        assert_eq!(video_cost_per_second("google/veo-3.1-lite", "720p"), 0.05);
        assert_eq!(video_cost_per_second("google/veo-3.1-lite", "1080p"), 0.08);
        assert_eq!(video_cost_per_second("google/veo-3.1-fast", "720p"), 0.10);
        assert_eq!(video_cost_per_second("google/veo-3.1-fast", "1080p"), 0.12);
        assert_eq!(video_cost_per_second("google/veo-3.1", "720p"), 0.40);
        // The lower-cost non-Veo entries (fast checked before plain seedance).
        assert_eq!(video_cost_per_second("alibaba/wan-2.6", "720p"), 0.04);
        assert_eq!(
            video_cost_per_second("bytedance/seedance-2.0-fast", "720p"),
            0.054
        );
        assert_eq!(
            video_cost_per_second("bytedance/seedance-2.0", "720p"),
            0.067
        );
        assert_eq!(
            video_cost_per_second("kwaivgi/kling-v3.0-std", "720p"),
            0.126
        );
        // Unknown model → cheapest-rate fallback so an estimate still prints.
        assert_eq!(video_cost_per_second("someone/other-video", "720p"), 0.05);
    }

    #[test]
    fn music_cost_is_flat_per_track() {
        assert_eq!(music_cost("google/lyria-3-pro-preview"), 0.08);
    }
}
