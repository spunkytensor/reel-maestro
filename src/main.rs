// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Reel Maestro — turn an idea into a vertical TikTok-style video with AI-generated
//! audio, images, and captions, using a single OpenRouter API key.

mod assemble;
mod captions;
mod config;
mod extract;
mod ffmpeg;
mod images;
mod metadata;
mod model;
mod music;
mod openrouter;
mod script;
mod transcribe;
mod tts;
mod video;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};

use config::Config;
use openrouter::OpenRouter;

/// Generate a vertical short video from a topic, an article URL, or your own script.
#[derive(Parser, Debug)]
#[command(name = "reelmaestro", version, about)]
pub struct Cli {
    /// A topic/idea; the AI writes the whole script.
    #[arg(long, conflicts_with_all = ["script", "url"])]
    topic: Option<String>,

    /// Path to a text file containing your finished narration (used verbatim).
    #[arg(long, conflicts_with_all = ["topic", "url"])]
    script: Option<PathBuf>,

    /// Path to a text file of notes/brief the AI writes a script FROM (unlike --script,
    /// which is used verbatim).
    #[arg(long, conflicts_with_all = ["topic", "script", "url", "from"])]
    brief: Option<PathBuf>,

    /// An article URL; the AI extracts the gist and writes a script.
    #[arg(long, conflicts_with_all = ["topic", "script"])]
    url: Option<String>,

    /// Resume a previous run folder: reuse its script, audio, captions, and images, and only
    /// re-render. Pair with --video to upgrade an image preview to video without regenerating.
    #[arg(long, conflicts_with_all = ["topic", "script", "url"])]
    from: Option<PathBuf>,

    /// Output directory (a per-video subfolder is created inside it).
    #[arg(long, default_value = "out")]
    out: PathBuf,

    /// TTS voice name (model-dependent).
    #[arg(long)]
    voice: Option<String>,

    /// Narration tempo multiplier (0.5–2.0); 1.0 keeps the TTS pace.
    #[arg(long, default_value_t = 1.0)]
    speed: f64,

    /// AI-generate a background soundtrack (OpenRouter music model, ~$0.08).
    #[arg(long)]
    music_gen: bool,

    /// Use this audio file as the background soundtrack (overrides --music-gen).
    #[arg(long)]
    music: Option<PathBuf>,

    /// How the soundtrack sits under the narration.
    #[arg(long, value_enum, default_value_t = MixMode::Duck)]
    mix: MixMode,

    /// Background music gain (0.0–1.0+). Higher = louder music.
    #[arg(long, default_value_t = 0.6)]
    music_volume: f64,

    /// Skip image generation and stop right after writing word timings (cheap caption-timing
    /// test: runs only script + TTS + word timing, no image/video/music/assembly calls).
    #[arg(long)]
    no_images: bool,

    /// Render ALL scenes as AI video clips (Veo image-to-video). Cost depends on the video
    /// model/resolution — the default Veo 3.1 Lite is ~$0.05/sec at 720p.
    #[arg(long)]
    video: bool,

    /// Render only the first N scenes as video clips; the rest stay Ken Burns stills.
    #[arg(long)]
    video_scenes: Option<usize>,

    /// Video clip resolution (720p or 1080p). Default comes from the quality tier (720p except
    /// --quality premium).
    #[arg(long)]
    video_resolution: Option<String>,

    /// Don't burn captions into the video.
    #[arg(long)]
    no_captions: bool,

    /// Disable cross-dissolve transitions (use hard cuts between every scene).
    #[arg(long)]
    no_dissolve: bool,

    /// Per-scene consistency validation: generate candidates and keep the one a vision model judges
    /// most consistent, re-rolling drifting frames. `off` = one candidate, no judging; `2` or `3` =
    /// up to that many candidates at up to N× the image cost. Default comes from the quality tier
    /// (2 for standard, off for draft, 3 for premium).
    #[arg(long, value_name = "off|2|3", value_parser = parse_validate_scene)]
    validate_scene: Option<usize>,

    /// Quality/cost tier that presets the model defaults: `draft` (cheapest models, validation
    /// off), `standard` (the regular defaults), `premium` (best models, 1080p video, deepest
    /// validation). Explicit --*-model flags and env vars still override the tier's picks.
    #[arg(long, value_enum)]
    quality: Option<config::Quality>,

    /// Output format: `reel` (vertical 9:16 short, the default) or `youtube` (landscape 16:9
    /// long-form with chapters, a 1280x720 thumbnail, and a youtube.md metadata file).
    #[arg(long, value_enum)]
    format: Option<config::Format>,

    /// Target video length in minutes (youtube format only; 1-12, default 3).
    #[arg(long)]
    minutes: Option<f64>,

    /// Disable the cinematic colour grade / film grain applied to the final video.
    #[arg(long)]
    no_grade: bool,

    /// Cross-dissolve length in seconds for scriptwriter-flagged still-to-still transitions.
    #[arg(long, default_value_t = 0.5)]
    dissolve_seconds: f64,

    /// Don't generate spoken narration — produce a silent or music-only video.
    #[arg(long)]
    no_narration: bool,

    /// Per-scene seconds when narration is disabled (default 4.0).
    #[arg(long)]
    scene_seconds: Option<f64>,

    /// Which scene the preview poster frame is taken from (default 0 = hook).
    #[arg(long)]
    poster_scene: Option<usize>,

    /// Generate poster.jpg but don't embed it as the MP4's cover art.
    #[arg(long)]
    no_embed_poster: bool,

    /// Disable automatic character-consistency conditioning across scenes.
    #[arg(long)]
    no_consistency: bool,

    /// Use this image as the recurring character reference (overrides the generated portrait).
    #[arg(long)]
    character_ref: Option<PathBuf>,

    /// Overlay this watermark image (a PNG with alpha) on the final video, scaled to the format
    /// and placed bottom-right. Works on fresh runs and `--from` resumes.
    #[arg(long)]
    watermark: Option<PathBuf>,

    #[arg(long)]
    text_model: Option<String>,
    #[arg(long)]
    image_model: Option<String>,
    #[arg(long)]
    tts_model: Option<String>,
    #[arg(long)]
    music_model: Option<String>,
    /// Multimodal model for the scene-consistency judge (cheaper than the script model).
    #[arg(long)]
    judge_model: Option<String>,

    /// Local command that produces word-level timestamps (default: `whisper_timestamped`).
    #[arg(long)]
    whisper_cmd: Option<String>,
    /// Whisper model for local word timing (e.g. `base`, `small`, `large-v3`).
    #[arg(long)]
    whisper_model: Option<String>,
    #[arg(long)]
    video_model: Option<String>,
}

/// Parse `--validate-scene`: `off` → 0 (validation disabled, one candidate); `2`/`3` → that many
/// candidates judged per scene. `1` is rejected — "keep the most consistent" needs ≥2 candidates.
/// `pub(crate)` so config.rs applies the same rule to `REELMAESTRO_VALIDATE_SCENE`.
pub(crate) fn parse_validate_scene(s: &str) -> Result<usize, String> {
    match s {
        "off" | "0" => Ok(0),
        "2" => Ok(2),
        "3" => Ok(3),
        _ => Err(format!("expected `off`, `2`, or `3` (got {s:?})")),
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum MixMode {
    /// Music automatically dips under the narration (sidechain ducking).
    Duck,
    /// Music held at a constant low volume.
    Low,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    if let Err(e) = run(&cli).await {
        eprintln!("\nerror: {e:#}");
        std::process::exit(1);
    }
    Ok(())
}

async fn run(cli: &Cli) -> Result<()> {
    // ffmpeg's atempo filter only accepts 0.5–2.0; reject out-of-range speeds
    // upfront so the user gets a clear message instead of a cryptic ffmpeg error.
    // (music_volume needs no check — it's clamped to >= 0 at the ffmpeg call.)
    if !(0.5..=2.0).contains(&cli.speed) {
        bail!("--speed must be between 0.5 and 2.0 (got {})", cli.speed);
    }
    let cfg = Config::load(cli)?;
    let mut or = OpenRouter::new(&cfg)?;

    // Resolve/validate the watermark up front (fail fast, before any generation) into an
    // absolute path — the render runs with the run folder as its working directory.
    let watermark = match &cli.watermark {
        Some(p) => Some(resolve_watermark(p)?),
        None => None,
    };

    let resume = cli.from.is_some();

    // 1. Script ---------------------------------------------------------------
    // Resume mode loads the prior run's script.json; fresh mode writes a new one.
    let (mut script, dir) = if let Some(from) = &cli.from {
        let bytes = std::fs::read(from.join("script.json")).with_context(|| {
            format!(
                "could not read {}/script.json (is this a Reel Maestro run folder?)",
                from.display()
            )
        })?;
        let script: model::Script =
            serde_json::from_slice(&bytes).context("invalid script.json")?;
        println!(
            "→ resuming {} ({} scenes)",
            from.display(),
            script.scenes.len()
        );
        (script, from.clone())
    } else {
        println!("→ writing script ({}) ...", or.text_model);
        let yt = cfg.format == config::Format::Youtube;
        let script = if let Some(topic) = &cli.topic {
            if yt {
                script::youtube_from_topic(&or, topic, cfg.minutes).await?
            } else {
                script::from_topic(&or, topic).await?
            }
        } else if let Some(path) = &cli.brief {
            // The file's contents are the brief/notes the AI writes a script FROM.
            let brief = std::fs::read_to_string(path)
                .with_context(|| format!("could not read brief file {}", path.display()))?;
            if yt {
                script::youtube_from_brief(&or, brief.trim(), cfg.minutes).await?
            } else {
                script::from_brief(&or, brief.trim()).await?
            }
        } else if let Some(path) = &cli.script {
            // The file's contents are used verbatim as the narration.
            let narration = std::fs::read_to_string(path)
                .with_context(|| format!("could not read script file {}", path.display()))?;
            if yt {
                script::youtube_from_narration(&or, narration.trim(), cfg.minutes).await?
            } else {
                script::from_narration(&or, narration.trim()).await?
            }
        } else if let Some(url) = &cli.url {
            println!("  fetching {url} ...");
            let text = extract::fetch_article(url).await?;
            if yt {
                script::youtube_from_article(&or, &text, cfg.minutes).await?
            } else {
                script::from_article(&or, &text).await?
            }
        } else {
            bail!("provide exactly one of --topic, --brief, --script, --url, or --from")
        };
        println!("  title: {}", script.title);
        println!(
            "  {} scenes, {} narration words",
            script.scenes.len(),
            script.narration.split_whitespace().count()
        );
        if !script.chapters.is_empty() {
            let est = script.narration.split_whitespace().count() as f64 / config::WORDS_PER_MINUTE;
            println!("  {} chapters, ~{est:.1} min", script.chapters.len());
        }
        let dir = cli
            .out
            .join(format!("{}_{}", timestamp(), slug(&script.title)));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("script.json"), serde_json::to_vec_pretty(&script)?)?;
        (script, dir)
    };

    // Fold any legacy single-`cast` string (older resumed `script.json`) into `characters` so the
    // rest of the pipeline only deals with the multi-character/location model.
    script.normalize_entities();

    // The effective format: fresh runs stamp the config's format into the script; resumed runs
    // read it back from script.json so render geometry always matches the stored assets — a
    // conflicting --format on resume is noted and deferred to the script's.
    let format = if resume {
        let stored = if script.format == "youtube" {
            config::Format::Youtube
        } else {
            config::Format::Reel
        };
        if cli.format.is_some_and(|f| f != stored) {
            eprintln!(
                "  note: --format conflicts with this run folder's script.json; \
                 using the script's format so geometry matches its assets"
            );
        }
        stored
    } else {
        if cfg.format == config::Format::Youtube && script.format != "youtube" {
            script.format = "youtube".to_string();
        }
        cfg.format
    };
    let canvas = config::canvas(format);
    // The aspect the image/video APIs are asked for must track the effective format too.
    or.aspect = canvas.aspect_str().to_string();
    // ...and so must the video-model default. On a `--from` resume that omits `--format`, the
    // config resolved the model from the (defaulted) reel format; re-derive the youtube default
    // (Wan) from the stored format so rehydrating a youtube preview with --video uses the same
    // model the first run would have. An explicit --video-model/env still wins.
    if !cfg.video_model_explicit && format == config::Format::Youtube {
        or.video_model = "alibaba/wan-2.6".to_string();
    }

    // Voice: honor an explicit --voice/REELMAESTRO_VOICE; otherwise auto-pick a male/female
    // voice from the script's narrator gender.
    if cfg.voice.is_none() {
        or.voice = pick_voice(&script.narrator_gender).to_string();
        if !script.narrator_gender.trim().is_empty() {
            println!(
                "  voice: {} (auto, {} narrator)",
                or.voice, script.narrator_gender
            );
        }
    }

    // 2. Audio + word timings -------------------------------------------------
    // Word timings drive BOTH captions and word-aligned scene cuts, so we compute them whenever
    // there is narration — even with --no-captions (only the burned-in subtitles are suppressed
    // downstream in `assemble`). Resume reuses the prior audio.mp3 + words.json. Fresh narration is
    // synthesized and timed; the preview TTS occasionally truncates a longer narration, so we detect
    // that via whisper coverage and re-synthesize, keeping the most complete take (so audio.mp3 and
    // words.json always describe the same audio).
    let audio = dir.join("audio.mp3");
    let words_path = dir.join("words.json");
    let words: Vec<model::WordTiming> = if resume {
        if !audio.exists() {
            bail!("{} has no audio.mp3 to resume from", dir.display());
        }
        match std::fs::read(&words_path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("could not parse {}", words_path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !cfg.no_narration && !script.narration.trim().is_empty() {
                    eprintln!(
                        "  note: {} is missing; re-deriving word timings from {} ...",
                        words_path.display(),
                        audio.display()
                    );
                    transcribe::word_timings(&cfg, &audio, &script.narration, &words_path)?.words
                } else {
                    Vec::new()
                }
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not read {}", words_path.display()));
            }
        }
    } else if cfg.no_narration {
        let total = cfg.scene_seconds * script.scenes.len() as f64;
        println!("→ no narration: building silent {total:.1}s timeline ...");
        ffmpeg::silent_track(&audio, total)?;
        Vec::new()
    } else {
        // Synthesize the WHOLE narration in one TTS call — this keeps a single consistent voice
        // for the entire video. (Generative TTS like Gemini re-samples the speaker on each
        // independent call, so splitting a long-form narration per chapter makes the voice audibly
        // change at chapter seams.) For long-form, if a single call still comes back truncated —
        // a very long script can exceed the preview TTS output cap — fall back to per-chapter
        // synthesis, trading a possible voice seam for a complete narration (the lesser evil).
        println!("→ synthesizing narration ({}) ...", or.tts_model);
        let (words, coverage) =
            synthesize_with_coverage(&or, &cfg, &script.narration, &audio, &words_path, cli.speed)
                .await?;
        if !script.chapters.is_empty() && coverage < MIN_TTS_COVERAGE {
            eprintln!(
                "  note: single-call narration still truncated after retries; falling back to \
                 per-chapter synthesis (the voice may vary slightly between chapters) ..."
            );
            synthesize_per_chapter(&or, &cfg, &script, &dir, &audio, &words_path, cli.speed).await?
        } else {
            std::fs::write(&words_path, serde_json::to_vec_pretty(&words)?)?;
            words
        }
    };

    // Caption-timing test mode (fresh runs only): stop before any image/video/music calls.
    if cli.no_images && !resume {
        println!(
            "\n✓ done (--no-images): word timings written to {}",
            dir.join("words.json").display()
        );
        return Ok(());
    }

    // 4. Images ---------------------------------------------------------------
    // Resume reuses the previewed stills so the video matches exactly what you approved.
    let images: Vec<PathBuf> = if resume {
        let imgs: Vec<PathBuf> = (0..script.scenes.len())
            .map(|i| dir.join(format!("scene-{i:02}.jpg")))
            .collect();
        for p in &imgs {
            if !p.exists() {
                bail!("missing {} — cannot resume", p.display());
            }
        }
        println!("→ reusing {} preview images", imgs.len());
        imgs
    } else {
        println!(
            "→ generating {} images ({}) ...",
            script.scenes.len(),
            or.image_model
        );
        let consistency = !cli.no_consistency;
        if consistency
            && (cli.character_ref.is_some()
                || !script.characters.is_empty()
                || !script.locations.is_empty())
        {
            if let Some(p) = &cli.character_ref {
                println!("  character consistency on (reference: {})", p.display());
            }
            if !script.characters.is_empty() {
                let who: Vec<&str> = script.characters.iter().map(|c| c.id.as_str()).collect();
                println!(
                    "  character consistency on (characters: {})",
                    who.join(", ")
                );
            }
            if !script.locations.is_empty() {
                let where_: Vec<&str> = script.locations.iter().map(|l| l.id.as_str()).collect();
                println!(
                    "  location consistency on (locations: {})",
                    where_.join(", ")
                );
            }
        }
        let validate = cfg.validate_scene; // 0 = off, 2/3 = candidates/scene
        if validate >= 2 {
            println!(
                "  scene validation on: up to {validate} candidates/scene, keeping the most \
                 consistent (judge: {})",
                or.judge_model
            );
        } else {
            println!("  scene validation off: one candidate/scene");
        }
        images::generate(
            &or,
            &script.scenes,
            &script.characters,
            &script.locations,
            cli.character_ref.as_deref(),
            consistency,
            validate,
            canvas,
            &dir,
        )
        .await?
    };

    // 5. Video scenes (optional, non-fatal) -----------------------------------
    let durations = assemble::scene_durations(&script.scenes, &words, &audio)?;
    let video_count = match cli.video_scenes {
        Some(n) => n.min(script.scenes.len()),
        None if cli.video => script.scenes.len(),
        None => 0,
    };
    let clips = if video_count > 0 {
        // Only scenes whose clip is missing get (re)generated and billed; existing scene-NN.mp4
        // clips are reused (delete one to regenerate just that scene). Estimate the real cost.
        let to_make: Vec<usize> = (0..video_count)
            .filter(|&i| !dir.join(format!("scene-{i:02}.mp4")).exists())
            .collect();
        if to_make.is_empty() {
            println!(
                "→ reusing {video_count} existing video clip(s) (delete a scene-NN.mp4 to regenerate it)"
            );
        } else {
            let secs = video::billed_seconds_for(&or.video_model, &durations, &to_make);
            let reused = video_count - to_make.len();
            let reuse_note = if reused > 0 {
                format!(", reusing {reused}")
            } else {
                String::new()
            };
            println!(
                "→ generating {} video scene(s){reuse_note} ({}, ~{secs}s ≈ ${:.2}) ...",
                to_make.len(),
                or.video_model,
                secs as f64 * config::video_cost_per_second(&or.video_model, &cfg.video_resolution)
            );
        }
        video::generate(
            &or,
            &script.scenes,
            &script.characters,
            &script.locations,
            &images,
            &durations,
            video_count,
            &cfg.video_resolution,
            &dir,
        )
        .await
    } else {
        vec![None; script.scenes.len()]
    };

    // 6. Soundtrack (optional, non-fatal) -------------------------------------
    // On resume, reuse the preview's soundtrack unless a new one was explicitly requested.
    let music = if resume && cli.music.is_none() && !cli.music_gen {
        existing_music(&dir)
    } else {
        resolve_music(cli, &or, &script.music_prompt, &dir).await
    };

    // 7. Assemble -------------------------------------------------------------
    println!("→ assembling video ...");
    let duck = cli.mix == MixMode::Duck;
    let reel = assemble::build(assemble::BuildOptions {
        dir: &dir,
        scenes: &script.scenes,
        images: &images,
        clips: &clips,
        words: &words,
        audio: &audio,
        music: music.as_deref(),
        duck,
        music_volume: cli.music_volume,
        captions_on: !cfg.no_captions,
        dissolve: !cli.no_dissolve,
        dissolve_seconds: cli.dissolve_seconds,
        grade: !cli.no_grade,
        canvas,
        caption_style: captions::CaptionStyle::for_format(format),
        chapters: &script.chapters,
        watermark: watermark.as_deref(),
    })?;

    // Long-form: write upload-ready YouTube metadata (title, description, tags, chapter
    // timestamps) alongside the video. Non-fatal.
    if !script.chapters.is_empty() {
        match metadata::write_youtube_md(&dir, &script, &durations) {
            Ok(p) => println!("  metadata: {}", p.display()),
            Err(e) => eprintln!("  note: writing youtube.md failed ({e})"),
        }
    }

    // 8. Poster — a custom, enticing thumbnail (non-fatal) --------------------
    // Generate a purpose-built cover image (clean, no captions). Resume reuses an existing
    // poster so a re-stitch stays free. If generation fails, fall back to a reel frame.
    let poster = dir.join("poster.jpg");
    if !(resume && poster.exists()) {
        println!("→ generating poster ({}) ...", or.image_model);
        let refs = poster_refs(&dir);
        let concept = poster_concept(&script, format);
        let protagonist = script
            .characters
            .first()
            .map(|c| c.description.as_str())
            .unwrap_or("");
        if images::generate_poster(
            &or,
            &concept,
            protagonist,
            &refs,
            config::poster_canvas(format),
            &dir,
        )
        .await
        .is_none()
        {
            eprintln!("  note: custom poster generation failed; using a reel frame instead");
            let t = poster_time(&durations, cli.poster_scene.unwrap_or(0));
            let _ = ffmpeg::poster_frame(&reel, &poster, t);
        }
    }
    if poster.exists() {
        if !cli.no_embed_poster {
            // Embed into whichever reel was just built (reel.mp4 or the video upgrade reel-video.mp4).
            let reel_name = reel
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("reel.mp4");
            if let Err(e) = ffmpeg::embed_poster(&dir, reel_name, "poster.jpg") {
                eprintln!("  note: embedding poster as cover art failed ({e})");
            }
        }
        println!("  poster: {}", poster.display());
    }

    println!("\n✓ done: {}", reel.display());
    Ok(())
}

/// Whisper coverage below this means the TTS take looks truncated — trigger a re-synthesis, and
/// (for long-form) fall back from a single whole-narration call to per-chapter synthesis.
const MIN_TTS_COVERAGE: f64 = 0.85;

/// Synthesize `narration` to `audio_path`, retrying when the take looks truncated: up to 3
/// attempts, keeping the take whisper heard the most of (audio and timings stay consistent as a
/// pair). `words_scratch` is where whisper's per-take timing JSON lands — the caller decides
/// whether that file is the run's words.json or a discarded scratch file. Returns the best
/// take's word timings and the coverage of that take (so the caller can decide whether a single
/// call was good enough or a fallback is warranted).
async fn synthesize_with_coverage(
    or: &OpenRouter,
    cfg: &Config,
    narration: &str,
    audio_path: &std::path::Path,
    words_scratch: &std::path::Path,
    speed: f64,
) -> Result<(Vec<model::WordTiming>, f64)> {
    const TTS_ATTEMPTS: usize = 3;
    let mut best: Option<(Vec<model::WordTiming>, f64, Vec<u8>)> = None;
    for attempt in 1..=TTS_ATTEMPTS {
        tts::synthesize(or, narration, audio_path, speed).await?;
        let t = transcribe::word_timings(cfg, audio_path, narration, words_scratch)?;
        println!(
            "  {} words timed (~{:.0}% of the text spoken)",
            t.words.len(),
            t.coverage * 100.0
        );
        // Keep the most complete take together with its audio so the two stay consistent.
        if best
            .as_ref()
            .map(|(_, c, _)| t.coverage > *c)
            .unwrap_or(true)
        {
            best = Some((t.words, t.coverage, std::fs::read(audio_path)?));
        }
        if t.coverage >= MIN_TTS_COVERAGE {
            break;
        }
        if attempt < TTS_ATTEMPTS {
            eprintln!(
                "  note: narration audio looks truncated (preview TTS cut it short); \
                 re-synthesizing ({}/{TTS_ATTEMPTS}) ...",
                attempt + 1
            );
        } else {
            eprintln!(
                "  warning: narration still truncated after {TTS_ATTEMPTS} attempts; \
                 using the most complete take"
            );
        }
    }
    let (words, coverage, audio_bytes) = best.expect("at least one TTS attempt ran");
    // The last synth on disk may have been a worse take — restore the best take's audio so
    // downstream sees the take the returned timings describe.
    std::fs::write(audio_path, &audio_bytes)?;
    Ok((words, coverage))
}

/// Fallback long-form TTS: synthesize each chapter separately, concatenate, and time the joined
/// track once. Used ONLY when a single whole-narration call truncated — per-chapter calls keep
/// the truncation-retry loop working on ~1 minute of speech at a time, at the cost of possible
/// voice drift between chapters (a generative TTS re-samples the speaker on each independent
/// call, so the seams can shift). Preferred path is the single call, which keeps one voice.
async fn synthesize_per_chapter(
    or: &OpenRouter,
    cfg: &Config,
    script: &model::Script,
    dir: &std::path::Path,
    audio: &std::path::Path,
    words_path: &std::path::Path,
    speed: f64,
) -> Result<Vec<model::WordTiming>> {
    let scratch = dir.join(".words-scratch.json");
    let mut parts: Vec<String> = Vec::new();
    for (c, chapter) in script.chapters.iter().enumerate() {
        println!(
            "→ synthesizing chapter {}/{} ({}) ...",
            c + 1,
            script.chapters.len(),
            or.tts_model
        );
        let name = format!("chapter-{c:02}.mp3");
        synthesize_with_coverage(
            or,
            cfg,
            &chapter.narration,
            &dir.join(&name),
            &scratch,
            speed,
        )
        .await?;
        parts.push(name);
    }
    let _ = std::fs::remove_file(&scratch);
    // Concat into the exact file the caller named as `audio` (and that `transcribe` reads below),
    // deriving the name from the path rather than hardcoding it so the two can't drift apart.
    let audio_name = audio
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.mp3");
    ffmpeg::concat_audio(dir, &parts, audio_name)?;
    // One whisper pass over the joined track gives the timeline-global word timings.
    println!(
        "→ timing narration ({} {}) ...",
        cfg.whisper_cmd, cfg.whisper_model
    );
    let t = transcribe::word_timings(cfg, audio, &script.narration, words_path)?;
    println!(
        "  {} words timed (~{:.0}% of the script spoken)",
        t.words.len(),
        t.coverage * 100.0
    );
    std::fs::write(words_path, serde_json::to_vec_pretty(&t.words)?)?;
    Ok(t.words)
}

/// Validate the `--watermark` path and resolve it to an absolute path (the render runs with the
/// run folder as its working directory, so a relative path wouldn't resolve there). Fails fast on
/// a missing/unreadable file, and warns — but does not fail — when the image has no alpha channel
/// (it would overlay as an opaque rectangle rather than a transparent logo).
fn resolve_watermark(path: &std::path::Path) -> Result<PathBuf> {
    let abs = std::fs::canonicalize(path)
        .with_context(|| format!("could not find watermark image {}", path.display()))?;
    match image::open(&abs) {
        Ok(img) => {
            if !img.color().has_alpha() {
                eprintln!(
                    "  note: watermark {} has no alpha channel; it will overlay as an opaque \
                     image (use a PNG with transparency for a clean logo)",
                    abs.display()
                );
            }
        }
        Err(e) => bail!(
            "could not read watermark {} as an image: {e}",
            abs.display()
        ),
    }
    Ok(abs)
}

/// Reference images for the poster: the character portrait (if any) so the poster's cast
/// matches the reel. Empty when there's no recurring character.
fn poster_refs(dir: &std::path::Path) -> Vec<String> {
    std::fs::read(dir.join("character-ref.jpg"))
        .ok()
        .map(|b| openrouter::data_url_from_image(&b))
        .into_iter()
        .collect()
}

/// The poster image concept: the script's `poster_prompt`, or a fallback built from the hook
/// scene for older runs that predate that field. Always nudged toward an enticing thumbnail,
/// in the format's aspect.
fn poster_concept(script: &model::Script, format: config::Format) -> String {
    let base = if !script.poster_prompt.trim().is_empty() {
        script.poster_prompt.clone()
    } else {
        let hook = script
            .scenes
            .first()
            .map(|s| s.image_prompt.as_str())
            .unwrap_or("");
        format!(
            "An eye-catching cover image for \"{}\": {hook}",
            script.title
        )
    };
    match format {
        config::Format::Reel => format!(
            "{base} A striking, high-contrast vertical thumbnail with an expressive focal \
             subject and broad appeal that entices viewers to watch."
        ),
        config::Format::Youtube => format!(
            "{base} A striking, high-contrast landscape 16:9 YouTube thumbnail with one \
             expressive focal subject and a bold composition readable at small sizes, that \
             entices viewers to click."
        ),
    }
}

/// Timestamp (seconds) of a scene's midpoint on the reel timeline — used to pick a poster
/// frame. Clamps an out-of-range scene index; returns 0 for an empty timeline.
fn poster_time(durations: &[f64], scene: usize) -> f64 {
    if durations.is_empty() {
        return 0.0;
    }
    let scene = scene.min(durations.len() - 1);
    let start: f64 = durations[..scene].iter().sum();
    start + durations[scene] * 0.5
}

/// Resolve the background soundtrack: a user file if given, else an AI-generated track if
/// requested, else none. Generation is non-fatal — a failure just drops the music.
async fn resolve_music(
    cli: &Cli,
    or: &OpenRouter,
    music_prompt: &str,
    dir: &std::path::Path,
) -> Option<PathBuf> {
    if let Some(file) = &cli.music {
        return Some(file.clone());
    }
    if !cli.music_gen {
        return None;
    }
    // Lyria bills a flat fee per track (unlike Veo's per-second cost), so the estimate is a
    // fixed figure rather than a computed one.
    println!(
        "→ generating soundtrack ({}, ~${:.2}) ...",
        or.music_model,
        config::music_cost(&or.music_model)
    );
    println!("  prompt: {music_prompt}");

    // Lyria is a flaky preview model, so retry a few times before giving up.
    const ATTEMPTS: usize = 3;
    for attempt in 1..=ATTEMPTS {
        match music::generate(or, music_prompt, dir).await {
            Ok(path) => {
                println!("  ✓ soundtrack added");
                return Some(path);
            }
            Err(e) => {
                eprintln!("  soundtrack attempt {attempt}/{ATTEMPTS} failed: {e}");
                if attempt < ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    }
    // Make the "no music" outcome impossible to miss in the run output.
    eprintln!(
        "\n  ⚠️  NO SOUNDTRACK — {} failed after {ATTEMPTS} attempts; the reel will have no music.\n",
        or.music_model
    );
    None
}

/// Find a previously generated/supplied soundtrack file in a resumed run folder.
fn existing_music(dir: &std::path::Path) -> Option<PathBuf> {
    ["wav", "mp3", "ogg", "flac"]
        .iter()
        .map(|e| dir.join(format!("music.{e}")))
        .find(|p| p.exists())
}

/// Date-time stamp `YYYYMMDD_HHMMSS` (UTC) used to prefix output folder names so runs sort
/// chronologically and never collide. Computed in pure Rust from the system clock — no `date`
/// subprocess — so it's portable and produces identical output on every OS.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (hour, min, sec) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (year, month, day) = civil_from_days(days);

    format!("{year:04}{month:02}{day:02}_{hour:02}{min:02}{sec:02}")
}

/// Convert days since the Unix epoch (1970-01-01) to a `(year, month, day)`
/// civil date. Algorithm from Howard Hinnant's `civil_from_days`, valid for the
/// proleptic Gregorian calendar across the full range we care about.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Map a narrator gender to a Gemini TTS voice. Male → a male voice; everything else keeps
/// the warm female default. (Only used when no voice is set explicitly.)
fn pick_voice(gender: &str) -> &'static str {
    match gender.trim().to_lowercase().as_str() {
        "male" => "Puck", // bright, conversational male voice
        _ => "Kore",      // female / neutral → warm default
    }
}

/// Turn a title into a filesystem-friendly slug.
fn slug(title: &str) -> String {
    let mut s: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "reel".to_string()
    } else {
        s.chars().take(60).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, pick_voice, poster_time};

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1)); // Unix epoch
        assert_eq!(civil_from_days(59), (1970, 3, 1)); // 1970 not a leap year
        assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // leap day
        assert_eq!(civil_from_days(20_544), (2026, 4, 1));
    }

    #[test]
    fn voice_follows_narrator_gender() {
        assert_eq!(pick_voice("male"), "Puck");
        assert_eq!(pick_voice("MALE"), "Puck");
        assert_eq!(pick_voice("female"), "Kore");
        assert_eq!(pick_voice("neutral"), "Kore");
        assert_eq!(pick_voice(""), "Kore");
    }

    #[test]
    fn poster_time_picks_scene_midpoint() {
        let d = vec![4.0, 6.0, 2.0];
        assert!((poster_time(&d, 0) - 2.0).abs() < 1e-9); // hook midpoint
        assert!((poster_time(&d, 1) - 7.0).abs() < 1e-9); // 4 + 6/2
        assert!((poster_time(&d, 2) - 11.0).abs() < 1e-9); // 4 + 6 + 2/2
        assert!((poster_time(&d, 9) - 11.0).abs() < 1e-9); // out-of-range clamps to last
        assert_eq!(poster_time(&[], 0), 0.0); // empty timeline
    }
}
