// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Small wrappers around the `ffmpeg` and `ffprobe` binaries (must be on PATH).
//!
//! Reel Maestro shells out to the system ffmpeg/ffprobe rather than linking a media library:
//! it keeps the build dependency-light and lets users upgrade codecs independently. Everything
//! here is a thin command-builder — the interesting media logic lives in the filtergraph
//! strings (especially `render_reel`), which are documented inline where they're built.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Run ffmpeg with the given args; fail loudly with stderr if it errors.
///
/// Every call gets `-y` (overwrite output without prompting — we own the run folder) and
/// `-loglevel error` (suppress ffmpeg's banner/progress chatter; only real errors reach us,
/// which we then surface verbatim on failure).
fn run_ffmpeg(args: &[&str]) -> Result<()> {
    let out = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error"])
        .args(args)
        .output()
        .context("failed to launch ffmpeg (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!("ffmpeg failed:\n{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

/// Duration of a media file in seconds, via ffprobe.
pub fn duration_s(path: &Path) -> Result<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            // `csv=p=0` prints just the bare value (no "duration=" key prefix), so stdout is a
            // single float we can parse directly.
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .context("failed to launch ffprobe (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!("ffprobe failed:\n{}", String::from_utf8_lossy(&out.stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .parse::<f64>()
        .with_context(|| format!("could not parse duration from ffprobe output: {text:?}"))
}

/// Transcode an audio file to mp3, optionally changing tempo (pitch-preserving).
/// Set `raw_pcm` when the input is headerless 24kHz mono signed-16-bit PCM.
pub fn transcode_to_mp3(input: &Path, output: &Path, raw_pcm: bool, speed: f64) -> Result<()> {
    let mut args: Vec<String> = Vec::new();
    if raw_pcm {
        args.extend(
            ["-f", "s16le", "-ar", "24000", "-ac", "1"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    args.push("-i".into());
    args.push(input.to_string_lossy().into_owned());
    if (speed - 1.0).abs() > 1e-6 {
        args.push("-filter:a".into());
        args.push(format!("atempo={speed}"));
    }
    args.push(output.to_string_lossy().into_owned());

    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_ffmpeg(&refs)
}

/// Write a silent stereo MP3 of the given length. Used as the timeline/clock for reels with
/// no spoken narration (music, if any, is mixed over it just like it would be over speech).
pub fn silent_track(output: &Path, seconds: f64) -> Result<()> {
    run_ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "anullsrc=channel_layout=stereo:sample_rate=44100",
        "-t",
        &format!("{seconds:.3}"),
        &output.to_string_lossy(),
    ])
}

/// Extract a single high-quality JPEG frame from `reel` at `at_s` seconds → `out`.
pub fn poster_frame(reel: &Path, out: &Path, at_s: f64) -> Result<()> {
    run_ffmpeg(&[
        "-ss",
        &format!("{at_s:.3}"),
        "-i",
        &reel.to_string_lossy(),
        "-frames:v",
        "1",
        "-q:v",
        "2",
        &out.to_string_lossy(),
    ])
}

/// Attach `poster` to `reel` as MP4 cover art (so players/file browsers show it as the
/// thumbnail), without re-encoding. ffmpeg can't edit in place, so we write a temp file and
/// rename it over the reel. Runs with `dir` as the working directory; `reel`/`poster` are
/// filenames relative to it.
pub fn embed_poster(dir: &Path, reel: &str, poster: &str) -> Result<()> {
    let tmp = "reel-poster.mp4";
    // Map only the main video (0:V:0 excludes any existing attached_pic) + audio, then add the
    // poster — so re-running on an already-embedded reel replaces the cover art rather than
    // accumulating extra streams.
    let out = Command::new("ffmpeg")
        .current_dir(dir)
        .args([
            "-y",
            "-loglevel",
            "error",
            "-i",
            reel,
            "-i",
            poster,
            "-map",
            "0:V:0",
            "-map",
            "0:a?",
            "-map",
            "1",
            "-c",
            "copy",
            "-disposition:v:1",
            "attached_pic",
            tmp,
        ])
        .output()
        .context("failed to launch ffmpeg to embed poster")?;
    if !out.status.success() {
        let _ = std::fs::remove_file(dir.join(tmp));
        bail!(
            "ffmpeg poster embed failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    std::fs::rename(dir.join(tmp), dir.join(reel))
        .context("could not replace reel with poster-embedded version")?;
    Ok(())
}

/// A scene's visual source: a still (animated with Ken Burns) or a pre-made video clip.
/// Each variant carries the filename (relative to the render working dir) of its source.
pub enum SceneMedia {
    /// A generated still image, panned/zoomed (Ken Burns) to add motion.
    Still(String),
    /// A pre-rendered video clip (e.g. an AI image-to-video scene), used as-is.
    Clip(String),
}

/// Everything `render_reel` needs to build the final MP4 in one ffmpeg pass. Grouped into a
/// struct so the long call site reads as named fields rather than a wall of positional args.
pub struct RenderReelOptions<'a> {
    /// Working directory for the render; all relative filenames below resolve against it.
    pub dir: &'a Path,
    /// One visual source per scene, in order.
    pub media: &'a [SceneMedia],
    /// On-screen seconds for each scene (same length/order as `media`).
    pub durations: &'a [f64],
    /// Narration audio filename — the timeline the video is cut to.
    pub audio: &'a str,
    /// Optional background soundtrack filename, looped to cover the whole reel.
    pub music: Option<&'a str>,
    /// When `true`, sidechain-duck the music under speech; otherwise hold it at a fixed low level.
    pub duck: bool,
    /// Music gain (0.0–1.0+); clamped to >= 0 at the filter.
    pub music_volume: f64,
    /// Optional `.ass` subtitle filename to burn captions in; `None` leaves the video clean.
    pub captions: Option<&'a str>,
    /// Extra font directory for libass to search. `None` lets libass fall back to
    /// the system font provider (fontconfig/CoreText/DirectWrite).
    pub fontsdir: Option<&'a str>,
    /// Output MP4 filename to write.
    pub output: &'a str,
}

/// Build the whole reel in a SINGLE ffmpeg pass: each scene (a Ken Burns still or a video
/// clip) is fit to its window, concatenated, captioned, and muxed with audio — one encode
/// total (no intermediate clips, no double-encoding).
///
/// Runs with `dir` as the working directory so we can pass bare filenames and avoid
/// ffmpeg filter path-escaping headaches. `media`, `audio`, `ass`, and `output` are
/// filenames relative to `dir`; `fontsdir` is an absolute path libass searches.
pub fn render_reel(opts: RenderReelOptions<'_>) -> Result<()> {
    let RenderReelOptions {
        dir,
        media,
        durations,
        audio,
        music,
        duck,
        music_volume,
        captions,
        fontsdir,
        output,
    } = opts;

    let n = media.len();
    let mut args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];

    // One input per scene: looped still (capped at its duration) or a video clip.
    for (i, m) in media.iter().enumerate() {
        match m {
            SceneMedia::Still(img) => {
                args.extend(
                    ["-framerate", "30", "-loop", "1", "-t"]
                        .iter()
                        .map(|s| s.to_string()),
                );
                args.push(format!("{:.3}", durations[i]));
                args.push("-i".into());
                args.push(img.clone());
            }
            SceneMedia::Clip(clip) => {
                args.push("-i".into());
                args.push(clip.clone());
            }
        }
    }
    // Narration is input n.
    args.push("-i".into());
    args.push(audio.to_string());
    // Optional music is input n+1, looped to cover the whole reel.
    if let Some(m) = music {
        args.extend(["-stream_loop", "-1", "-i"].iter().map(|s| s.to_string()));
        args.push(m.to_string());
    }

    // Per-scene video filter, normalized to a 1080x1920 30fps segment of length durations[i].
    let mut parts: Vec<String> = Vec::with_capacity(n + 4);
    for (i, m) in media.iter().enumerate() {
        let dur = durations[i];
        match m {
            SceneMedia::Still(_) => {
                // Ken Burns. d=1 = one output frame per looped input frame, so the zoom
                // advances smoothly with the output-frame index `on` instead of exploding.
                // zoompan rounds the pan/crop to whole INPUT pixels each frame, which makes
                // the motion step/shimmer at output. Smooth it two ways: supersample the
                // still to 4x (a 1px step becomes ~0.25px at output) and run zoompan at 2x
                // output, then Lanczos-downscale to 1080x1920 (antialiases the remainder).
                let z = if i % 2 == 0 {
                    "min(1.0+0.0006*on,1.12)"
                } else {
                    "max(1.12-0.0006*on,1.0)"
                };
                parts.push(format!(
                    "[{i}:v]scale=4320:7680:force_original_aspect_ratio=increase:flags=lanczos,crop=4320:7680,\
                     zoompan=z='{z}':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s=2160x3840:fps=30,\
                     scale=1080:1920:flags=lanczos,setsar=1,format=yuv420p[v{i}]"
                ));
            }
            SceneMedia::Clip(name) => {
                // Guarantee the clip covers its whole slot with continuous motion. If the
                // generated clip is shorter than the window (e.g. the window exceeds Veo's
                // 8s max), retime it to fill the slot instead of freezing the last frame; if
                // it's longer, trim at native speed. A tiny tpad only absorbs sub-frame
                // rounding so `trim` always has enough to cut.
                let clip_dur = duration_s(&dir.join(name)).unwrap_or(dur);
                let factor = if clip_dur > 0.0 && clip_dur < dur {
                    dur / clip_dur
                } else {
                    1.0
                };
                parts.push(format!(
                    "[{i}:v]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,\
                     setsar=1,setpts=PTS*{factor:.5},fps=30,tpad=stop_mode=clone:stop_duration=1,\
                     trim=duration={dur:.3},setpts=PTS-STARTPTS,format=yuv420p[v{i}]"
                ));
            }
        }
    }
    let concat_inputs: String = (0..n).map(|i| format!("[v{i}]")).collect();
    match captions {
        Some(ass) => {
            parts.push(format!("{concat_inputs}concat=n={n}:v=1:a=0[cat]"));
            match fontsdir {
                Some(fd) => parts.push(format!("[cat]subtitles={ass}:fontsdir={fd}[vout]")),
                None => parts.push(format!("[cat]subtitles={ass}[vout]")),
            }
        }
        None => {
            parts.push(format!("{concat_inputs}concat=n={n}:v=1:a=0[vout]"));
        }
    }

    // Audio: narration alone, or mixed under a soundtrack (ducked or fixed-low).
    let audio_map = if music.is_some() {
        let total: f64 = durations.iter().sum();
        let fade_out = (total - 1.5).max(0.0);
        let narr = format!("[{n}:a]");
        let mus = format!("[{}:a]", n + 1);
        let vol = music_volume.max(0.0);
        if duck {
            // Music plays at full `vol` and dips only gently under speech, then recovers
            // in gaps. Low ratio + high threshold + mix<1 keep the duck shallow so the
            // music stays clearly audible throughout.
            parts.push(format!(
                "{mus}volume={vol},afade=t=in:st=0:d=1.5,afade=t=out:st={fade_out}:d=1.5[mv];\
                 [mv]{narr}sidechaincompress=threshold=0.12:ratio=2:attack=25:release=350:mix=0.6[mduck];\
                 {narr}[mduck]amix=inputs=2:duration=first:normalize=0[aout]"
            ));
        } else {
            // Music held at a constant `vol` under the narration.
            parts.push(format!(
                "{mus}volume={vol},afade=t=in:st=0:d=1.5,afade=t=out:st={fade_out}:d=1.5[m];\
                 {narr}[m]amix=inputs=2:duration=first:normalize=0[aout]"
            ));
        }
        "[aout]".to_string()
    } else {
        format!("{n}:a")
    };

    // Final mux/encode. Stream selection plus broadly-compatible delivery settings:
    //   libx264 + yuv420p   — H.264 in the pixel format every phone/browser can decode.
    //   preset veryfast/crf 20 — fast encode at visually-lossless quality for short reels.
    //   aac 192k / 44.1kHz  — standard audio for MP4.
    //   -shortest           — end the file when the shortest mapped stream ends (the narration
    //                         clock), so a looped soundtrack can't run past the visuals.
    //   +faststart          — move the moov atom to the front so the MP4 streams/previews
    //                         without a full download (important for social platforms).
    let filter = parts.join(";");
    args.extend(
        [
            "-filter_complex",
            &filter,
            "-map",
            "[vout]",
            "-map",
            &audio_map,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-crf",
            "20",
            "-r",
            "30",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ar",
            "44100",
            "-shortest",
            "-movflags",
            "+faststart",
            output,
        ]
        .iter()
        .map(|s| s.to_string()),
    );

    let out = Command::new("ffmpeg")
        .current_dir(dir)
        .args(&args)
        .output()
        .context("failed to launch ffmpeg for final render")?;
    if !out.status.success() {
        bail!(
            "ffmpeg render failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}
