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
    #[arg(long, default_value_t = 0.8)]
    music_volume: f64,

    /// Skip image generation and stop right after writing word timings (cheap caption-timing
    /// test: runs only script + TTS + word timing, no image/video/music/assembly calls).
    #[arg(long)]
    no_images: bool,

    /// Render ALL scenes as AI video clips (Veo image-to-video). Costs ~$0.05/sec.
    #[arg(long)]
    video: bool,

    /// Render only the first N scenes as video clips; the rest stay Ken Burns stills.
    #[arg(long)]
    video_scenes: Option<usize>,

    /// Video clip resolution (e.g. 720p, 1080p).
    #[arg(long, default_value = "720p")]
    video_resolution: String,

    /// Don't burn captions into the video.
    #[arg(long)]
    no_captions: bool,

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

    #[arg(long)]
    text_model: Option<String>,
    #[arg(long)]
    image_model: Option<String>,
    #[arg(long)]
    tts_model: Option<String>,
    #[arg(long)]
    music_model: Option<String>,

    /// Local command that produces word-level timestamps (default: `whisper_timestamped`).
    #[arg(long)]
    whisper_cmd: Option<String>,
    /// Whisper model for local word timing (e.g. `base`, `small`, `large-v3`).
    #[arg(long)]
    whisper_model: Option<String>,
    #[arg(long)]
    video_model: Option<String>,
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

    let resume = cli.from.is_some();

    // 1. Script ---------------------------------------------------------------
    // Resume mode loads the prior run's script.json; fresh mode writes a new one.
    let (script, dir) = if let Some(from) = &cli.from {
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
        let script = if let Some(topic) = &cli.topic {
            script::from_topic(&or, topic).await?
        } else if let Some(path) = &cli.brief {
            // The file's contents are the brief/notes the AI writes a script FROM.
            let brief = std::fs::read_to_string(path)
                .with_context(|| format!("could not read brief file {}", path.display()))?;
            script::from_brief(&or, brief.trim()).await?
        } else if let Some(path) = &cli.script {
            // The file's contents are used verbatim as the narration.
            let narration = std::fs::read_to_string(path)
                .with_context(|| format!("could not read script file {}", path.display()))?;
            script::from_narration(&or, narration.trim()).await?
        } else if let Some(url) = &cli.url {
            println!("  fetching {url} ...");
            let text = extract::fetch_article(url).await?;
            script::from_article(&or, &text).await?
        } else {
            bail!("provide exactly one of --topic, --brief, --script, --url, or --from")
        };
        println!("  title: {}", script.title);
        println!(
            "  {} scenes, {} narration words",
            script.scenes.len(),
            script.narration.split_whitespace().count()
        );
        let dir = cli
            .out
            .join(format!("{}_{}", timestamp(), slug(&script.title)));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("script.json"), serde_json::to_vec_pretty(&script)?)?;
        (script, dir)
    };

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

    // 2. Audio ----------------------------------------------------------------
    // Resume reuses the prior audio. Fresh: synthesize the voiceover (the timeline clock), or
    // build a silent track sized to scene_seconds per scene for a music-only/silent reel.
    let audio = dir.join("audio.mp3");
    if resume {
        if !audio.exists() {
            bail!("{} has no audio.mp3 to resume from", dir.display());
        }
    } else if cfg.no_narration {
        let total = cfg.scene_seconds * script.scenes.len() as f64;
        println!("→ no narration: building silent {total:.1}s timeline ...");
        ffmpeg::silent_track(&audio, total)?;
    } else {
        println!("→ synthesizing narration ({}) ...", or.tts_model);
        tts::synthesize(&or, &script.narration, &audio, cli.speed).await?;
    }

    // 3. Word timings ---------------------------------------------------------
    // Captions need spoken narration to time against. Resume reuses words.json from the
    // preview; fresh runs time them with local whisper-timestamped (see transcribe.rs).
    let words = if cfg.no_narration || cfg.no_captions {
        Vec::new()
    } else if resume {
        std::fs::read(dir.join("words.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    } else {
        println!(
            "→ timing captions ({} {}) ...",
            cfg.whisper_cmd, cfg.whisper_model
        );
        let w = transcribe::word_timings(&cfg, &audio, &script.narration, &dir.join("words.json"))?;
        println!("  {} words timed", w.len());
        w
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
        if consistency && (cli.character_ref.is_some() || !script.cast.trim().is_empty()) {
            match &cli.character_ref {
                Some(p) => println!("  character consistency on (reference: {})", p.display()),
                None => println!("  character consistency on (cast: {})", script.cast),
            }
        }
        images::generate(
            &or,
            &script.scenes,
            &script.cast,
            cli.character_ref.as_deref(),
            consistency,
            &dir,
        )
        .await?
    };

    // 5. Video scenes (optional, non-fatal) -----------------------------------
    let durations = assemble::scene_durations(&script.scenes, &audio)?;
    let video_count = match cli.video_scenes {
        Some(n) => n.min(script.scenes.len()),
        None if cli.video => script.scenes.len(),
        None => 0,
    };
    let clips = if video_count > 0 {
        let secs = video::billed_seconds(&durations, video_count);
        println!(
            "→ generating {video_count} video scene(s) ({}, ~{secs}s ≈ ${:.2}) ...",
            or.video_model,
            secs as f64 * 0.05
        );
        video::generate(
            &or,
            &script.scenes,
            &images,
            &durations,
            video_count,
            &cli.video_resolution,
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
    })?;

    // 8. Poster — a custom, enticing thumbnail (non-fatal) --------------------
    // Generate a purpose-built cover image (clean, no captions). Resume reuses an existing
    // poster so a re-stitch stays free. If generation fails, fall back to a reel frame.
    let poster = dir.join("poster.jpg");
    if !(resume && poster.exists()) {
        println!("→ generating poster ({}) ...", or.image_model);
        let refs = poster_refs(&dir);
        let concept = poster_concept(&script);
        if images::generate_poster(&or, &concept, &script.cast, &refs, &dir)
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
            if let Err(e) = ffmpeg::embed_poster(&dir, "reel.mp4", "poster.jpg") {
                eprintln!("  note: embedding poster as cover art failed ({e})");
            }
        }
        println!("  poster: {}", poster.display());
    }

    println!("\n✓ done: {}", reel.display());
    Ok(())
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
/// scene for older runs that predate that field. Always nudged toward an enticing thumbnail.
fn poster_concept(script: &model::Script) -> String {
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
    format!(
        "{base} A striking, high-contrast vertical thumbnail with an expressive focal subject \
         and broad appeal that entices viewers to watch."
    )
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
    println!("→ generating soundtrack ({}) ...", or.music_model);
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

/// Local date-time stamp `YYYYMMDD_HHMMSS` for naming output folders. Uses the system
/// `date` command (local time); falls back to a UNIX-seconds prefix if unavailable.
/// `YYYYMMDD_HHMMSS` (UTC) for naming output directories. Computed in pure Rust
/// so it's portable and cheap — no `date` subprocess, identical on every OS.
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
