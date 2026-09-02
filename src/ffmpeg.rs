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

use crate::config::Canvas;

/// The unified cinematic "house grade" applied to the whole reel: a gentle contrast/saturation
/// lift, a soft S-curve, a subtle vignette, and light temporal film grain. Ties independently
/// generated scenes into one look and reads as real footage rather than clean AI stills.
const GRADE: &str = "eq=contrast=1.04:saturation=1.05,\
                     curves=master='0/0 0.25/0.23 0.78/0.81 1/1',\
                     vignette=PI/4,noise=alls=6:allf=t,format=yuv420p";

/// Cap on the per-still exposure correction (in ffmpeg `eq=brightness` units) so the cross-scene
/// match nudges frames toward a common exposure without flattening intentional dark/bright scenes.
const EXPOSURE_CAP: f64 = 0.06;

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
    let tmp_path = dir.join(tmp);
    let reel_path = dir.join(reel);
    // Map only the main video (0:V:0 excludes any existing attached_pic) + audio, then add the
    // poster — so re-running on an already-embedded reel replaces the cover art rather than
    // accumulating extra streams.
    clean_up_temp_on_error(&tmp_path, || {
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
            bail!(
                "ffmpeg poster embed failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::fs::rename(&tmp_path, &reel_path).with_context(|| {
            format!(
                "renaming {} over {}",
                tmp_path.display(),
                reel_path.display()
            )
        })
    })
}

/// Remove a temporary file when its producing operation fails.
fn clean_up_temp_on_error<T>(tmp: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let result = operation();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
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
    /// Per-junction cross-dissolve flags, length `media.len() - 1`: element `j` requests a
    /// cross-dissolve between scene `j` and `j+1` (only set where both are stills). Empty or all
    /// `false` ⇒ plain hard cuts (the original `concat` path).
    pub dissolves: &'a [bool],
    /// Cross-dissolve length in seconds; clamped per junction so a dissolve never exceeds half of
    /// either neighbor's on-screen time.
    pub dissolve_seconds: f64,
    /// Apply the unified cinematic colour grade + film grain to the whole reel, and match scene
    /// stills' exposure to each other, so independently-generated frames read as one shoot.
    pub grade: bool,
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
    /// Output canvas (1080x1920 reel / 1920x1080 youtube); every filtergraph dimension
    /// derives from it.
    pub canvas: Canvas,
    /// Optional watermark image path (PNG with alpha), overlaid bottom-right, scaled to the canvas.
    pub watermark: Option<&'a str>,
    /// Output MP4 filename to write.
    pub output: &'a str,
}

/// Mean luma (0–255) of an image file, or `None` if it can't be read/decoded.
fn mean_luma(path: &Path) -> Option<f64> {
    let img = image::open(path).ok()?.to_luma8();
    let n = img.as_raw().len();
    if n == 0 {
        return None;
    }
    let sum: u64 = img.as_raw().iter().map(|&p| p as u64).sum();
    Some(sum as f64 / n as f64)
}

/// Mean luma of a clip's first frame, by extracting it to a temp JPEG and measuring it. Lets clips
/// participate in the cross-scene exposure match alongside stills. `None` if extraction fails.
fn clip_frame_luma(dir: &Path, name: &str) -> Option<f64> {
    let tmp = dir.join(".exposure-probe.jpg");
    let probe = tmp.file_name()?;
    let out = Command::new("ffmpeg")
        .current_dir(dir)
        .args(["-y", "-loglevel", "error", "-i", name, "-frames:v", "1"])
        .arg(probe)
        .output()
        .ok()?;
    let luma = out.status.success().then(|| mean_luma(&tmp)).flatten();
    let _ = std::fs::remove_file(&tmp);
    luma
}

/// Per-scene exposure correction (in `eq=brightness` units) that nudges each scene toward the
/// group's median brightness, so independently-generated frames — stills AND clips alike — match
/// instead of flickering lighter/darker between scenes. Unreadable scenes get 0.0 (no correction);
/// when fewer than two scenes are measurable there's nothing to match, so all corrections are 0.0.
fn exposure_deltas(dir: &Path, media: &[SceneMedia]) -> Vec<f64> {
    let lumas: Vec<Option<f64>> = media
        .iter()
        .map(|m| match m {
            SceneMedia::Still(name) => mean_luma(&dir.join(name)),
            SceneMedia::Clip(name) => clip_frame_luma(dir, name),
        })
        .collect();
    let mut present: Vec<f64> = lumas.iter().filter_map(|x| *x).collect();
    if present.len() < 2 {
        return vec![0.0; media.len()];
    }
    present.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let target = present[present.len() / 2]; // median brightness
    lumas
        .iter()
        .map(|x| match x {
            Some(l) => ((target - l) / 255.0).clamp(-EXPOSURE_CAP, EXPOSURE_CAP),
            None => 0.0,
        })
        .collect()
}

/// The per-scene Ken Burns chain for one still. zoompan rounds the pan/crop to whole INPUT
/// pixels each frame, which makes the motion step/shimmer at output; we smooth it by
/// supersampling the still to 4x the canvas (a 1px step becomes ~0.25px) and running zoompan at
/// 2x, then Lanczos-downscaling to the canvas (antialiases the remainder). Pure so the built
/// string — including the canvas-derived dimensions — is unit-testable.
fn still_chain(canvas: Canvas, i: usize, z: &str, eqx: &str, tb: &str) -> String {
    let (w, h) = (canvas.w, canvas.h);
    let (w4, h4) = (w * 4, h * 4);
    let (w2, h2) = (w * 2, h * 2);
    format!(
        "[{i}:v]scale={w4}:{h4}:force_original_aspect_ratio=increase:flags=lanczos,crop={w4}:{h4},\
         zoompan=z='{z}':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={w2}x{h2}:fps=30,\
         scale={w}:{h}:flags=lanczos,setsar=1,{eqx}format=yuv420p{tb}[v{i}]"
    )
}

/// The per-scene chain for one pre-rendered clip: fit to the canvas, retime by `factor` to fill
/// its slot (a clip shorter than the window plays slowed rather than freezing), trim to the
/// window. Pure for the same testability reason as `still_chain`.
fn clip_chain(canvas: Canvas, i: usize, factor: f64, dur: f64, eqx: &str, tb: &str) -> String {
    let (w, h) = (canvas.w, canvas.h);
    format!(
        "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},\
         setsar=1,setpts=PTS*{factor:.5},fps=30,tpad=stop_mode=clone:stop_duration=1,\
         trim=duration={dur:.3},setpts=PTS-STARTPTS,{eqx}format=yuv420p{tb}[v{i}]"
    )
}

/// The narration/music mix filter (ends in `[aout]`). `narr`/`mus` are input stream labels like
/// `[1:a]`. Forces every amix input to stereo: amix collapses its output to the fewest-channel
/// input, so a mono narration would otherwise downmix the (stereo) music to mono and discard its
/// stereo image; the sidechain detector still takes the raw mono narration (an input stream, so
/// ffmpeg auto-splits it). Duck mode dips the music gently under speech and recovers in gaps.
fn audio_mix_filter(narr: &str, mus: &str, duck: bool, music_volume: f64, total: f64) -> String {
    let fade_out = (total - 1.5).max(0.0);
    let vol = music_volume.max(0.0);
    if duck {
        format!(
            "{mus}aformat=channel_layouts=stereo,volume={vol},afade=t=in:st=0:d=1.5,afade=t=out:st={fade_out}:d=1.5[mv];\
             [mv]{narr}sidechaincompress=threshold=0.12:ratio=2:attack=25:release=350:mix=0.6[mduck];\
             {narr}aformat=channel_layouts=stereo[narrst];\
             [narrst][mduck]amix=inputs=2:duration=first:normalize=0[aout]"
        )
    } else {
        format!(
            "{mus}aformat=channel_layouts=stereo,volume={vol},afade=t=in:st=0:d=1.5,afade=t=out:st={fade_out}:d=1.5[m];\
             {narr}aformat=channel_layouts=stereo[narrst];\
             [narrst][m]amix=inputs=2:duration=first:normalize=0[aout]"
        )
    }
}

/// The video half of a render, shared by the single-pass reel and per-chapter segments:
/// per-scene input args and the filter parts that join, caption, grade, and watermark the scenes.
struct VideoGraph {
    /// `-i` (and still-loop) args, one group per scene (input indices 0..n), plus one more `-i`
    /// for the watermark image when present (input index n).
    input_args: Vec<String>,
    /// Filter parts producing the final video label.
    parts: Vec<String>,
    /// The label to `-map` (`[vwm]` when watermarked, else `[vfinal]` when graded, else `[vout]`).
    video_map: &'static str,
    /// Number of `-i` inputs in `input_args` (scenes + watermark). The caller appends audio
    /// inputs after these, so audio input indices start at `input_count`.
    input_count: usize,
}

/// Watermark width as a fraction of the canvas width, and its margin from the frame edges.
/// Width-relative so the logo scales sensibly for both the narrow reel and the wide youtube canvas.
const WATERMARK_WIDTH_FRAC: f64 = 0.06;
const WATERMARK_MARGIN_FRAC: f64 = 0.04;

/// The watermark overlay filter parts and the new video label. Scales the watermark input
/// (index `wm_input`) to a fraction of the canvas width — preserving aspect ratio and the PNG's
/// alpha — and composites it in the BOTTOM-RIGHT corner with a margin. Pure so the geometry is
/// unit-testable.
fn watermark_parts(canvas: Canvas, wm_input: usize, base: &str) -> (Vec<String>, &'static str) {
    let wm_w = ((canvas.w as f64) * WATERMARK_WIDTH_FRAC).round() as u32;
    let margin = ((canvas.w as f64) * WATERMARK_MARGIN_FRAC).round() as u32;
    let parts = vec![
        // `-1` keeps the watermark's aspect ratio; scale preserves the alpha channel.
        format!("[{wm_input}:v]scale={wm_w}:-1[wmk]"),
        // Bottom-right corner (W/H = main dims, w/h = watermark dims); overlay honours alpha,
        // and we re-assert yuv420p afterward for libx264.
        format!("{base}[wmk]overlay=W-w-{margin}:H-h-{margin},format=yuv420p[vwm]"),
    ];
    (parts, "[vwm]")
}

/// Build the scene inputs + video filtergraph: each scene (a Ken Burns still or a clip) fit to
/// the canvas and its window, joined with cuts/cross-dissolves, captioned, and graded.
// Mirrors the two options structs that feed it; a third struct would just rename their fields.
#[allow(clippy::too_many_arguments)]
fn build_video_graph(
    dir: &Path,
    media: &[SceneMedia],
    durations: &[f64],
    dissolves: &[bool],
    dissolve_seconds: f64,
    grade: bool,
    captions: Option<&str>,
    fontsdir: Option<&str>,
    canvas: Canvas,
    watermark: Option<&str>,
) -> VideoGraph {
    let n = media.len();

    // Per-junction cross-dissolve length (0.0 = hard cut). Clamp so a dissolve never exceeds half
    // of either neighbor's on-screen time, and drop sub-0.1s dissolves back to cuts (too short to
    // read). Index `j` is the junction between scene `j` and `j+1`.
    let xfade: Vec<f64> = (0..n.saturating_sub(1))
        .map(|j| {
            if dissolves.get(j).copied().unwrap_or(false) {
                let d = dissolve_seconds
                    .min(0.5 * durations[j])
                    .min(0.5 * durations[j + 1]);
                if d >= 0.1 {
                    d
                } else {
                    0.0
                }
            } else {
                0.0
            }
        })
        .collect();
    let any_dissolve = xfade.iter().any(|&d| d > 0.0);
    // Rendered length of scene `i`: its on-screen time plus the dissolve it fades OUT into the next
    // scene. Extending the outgoing still by exactly the overlap keeps the joined total equal to
    // sum(durations) (= the audio length), so the crossfade never steals hold time or desyncs.
    let seg_len = |i: usize| durations[i] + xfade.get(i).copied().unwrap_or(0.0);

    // One input per scene: looped still (capped at its duration) or a video clip.
    let mut input_args: Vec<String> = Vec::new();
    for (i, m) in media.iter().enumerate() {
        match m {
            SceneMedia::Still(img) => {
                input_args.extend(
                    ["-framerate", "30", "-loop", "1", "-t"]
                        .iter()
                        .map(|s| s.to_string()),
                );
                input_args.push(format!("{:.3}", seg_len(i)));
                input_args.push("-i".into());
                input_args.push(img.clone());
            }
            SceneMedia::Clip(clip) => {
                input_args.push("-i".into());
                input_args.push(clip.clone());
            }
        }
    }

    // Per-scene video filter, normalized to a canvas-sized 30fps segment of length durations[i].
    // When dissolving, pin every segment to a single timebase: `xfade` rejects inputs whose
    // timebases differ, and a `concat` stage upstream of an `xfade` emits a different timebase
    // than a raw segment, so without this a cut-before-a-dissolve fails to configure.
    let tb = if any_dissolve { ",settb=AVTB" } else { "" };
    // Cross-scene exposure match (part of the grade): nudge each still toward the group's median
    // brightness so independently-generated frames don't flicker lighter/darker between scenes.
    let exposure = if grade {
        exposure_deltas(dir, media)
    } else {
        vec![0.0; n]
    };
    let mut parts: Vec<String> = Vec::with_capacity(n + 4);
    for (i, m) in media.iter().enumerate() {
        let dur = durations[i];
        // Per-scene exposure correction toward the group median (empty when grading is off or the
        // correction is negligible). Applied to both stills and clips so they don't flicker.
        let eqx = if grade && exposure[i].abs() > 1e-4 {
            format!("eq=brightness={:.4},", exposure[i])
        } else {
            String::new()
        };
        match m {
            SceneMedia::Still(_) => {
                // Ken Burns. d=1 = one output frame per looped input frame, so the zoom
                // advances smoothly with the output-frame index `on` instead of exploding.
                //
                // Scale the per-frame zoom rate to THIS scene's frame count so the move spans the
                // whole window (1.0 → 1.12, or the reverse) instead of finishing early and freezing
                // for the rest of a long scene. (A fixed rate maxes out at a fixed time, ~6.7s.)
                let frames = (seg_len(i) * 30.0).max(2.0);
                let rate = 0.12 / (frames - 1.0);
                let z = if i % 2 == 0 {
                    format!("min(1.0+{rate:.6}*on,1.12)")
                } else {
                    format!("max(1.12-{rate:.6}*on,1.0)")
                };
                parts.push(still_chain(canvas, i, &z, &eqx, tb));
            }
            SceneMedia::Clip(name) => {
                // Guarantee the clip covers its whole slot with continuous motion. If the
                // generated clip is shorter than the window (e.g. the window exceeds the video
                // model's max clip length), retime it to fill the slot instead of freezing the
                // last frame; if it's longer, trim at native speed. A tiny tpad only absorbs
                // sub-frame rounding so `trim` always has enough to cut.
                let clip_dur = duration_s(&dir.join(name)).unwrap_or(dur);
                let factor = if clip_dur > 0.0 && clip_dur < dur {
                    dur / clip_dur
                } else {
                    1.0
                };
                parts.push(clip_chain(canvas, i, factor, dur, &eqx, tb));
            }
        }
    }
    if !any_dissolve {
        // No cross-dissolves: the original single-concat path, unchanged.
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
    } else {
        // Mixed cuts and cross-dissolves: fold the segments left-to-right, joining each next
        // segment with either xfade (cross-dissolve) or concat (hard cut). `acc_len` tracks the
        // accumulator's running length so each xfade `offset` lands at `acc_len - d`.
        let mut acc = "[v0]".to_string();
        let mut acc_len = seg_len(0);
        for i in 1..n {
            let d = xfade[i - 1];
            if d > 0.0 {
                let offset = acc_len - d;
                parts.push(format!(
                    "{acc}[v{i}]xfade=transition=fade:duration={d:.3}:offset={offset:.3}[j{i}]"
                ));
                acc_len += seg_len(i) - d;
            } else {
                parts.push(format!("{acc}[v{i}]concat=n=2:v=1:a=0[j{i}]"));
                acc_len += seg_len(i);
            }
            acc = format!("[j{i}]");
        }
        // Burn captions onto the joined video, or pass it through, producing [vout].
        match captions {
            Some(ass) => match fontsdir {
                Some(fd) => parts.push(format!("{acc}subtitles={ass}:fontsdir={fd}[vout]")),
                None => parts.push(format!("{acc}subtitles={ass}[vout]")),
            },
            None => parts.push(format!("{acc}null[vout]")),
        }
    }

    // Optional unified grade + grain + vignette over the whole video → [vfinal].
    let mut video_map = if grade {
        parts.push(format!("[vout]{GRADE}[vfinal]"));
        "[vfinal]"
    } else {
        "[vout]"
    };

    // Optional watermark overlay: the watermark image is the input right after the n scenes
    // (index n). Applied last so it sits above the grade and captions.
    let mut input_count = n;
    if let Some(wm) = watermark {
        input_args.push("-i".into());
        input_args.push(wm.to_string());
        let (wparts, wmap) = watermark_parts(canvas, n, video_map);
        parts.extend(wparts);
        video_map = wmap;
        input_count += 1;
    }

    VideoGraph {
        input_args,
        parts,
        video_map,
        input_count,
    }
}

/// The shared H.264 video encode settings: H.264 in the pixel format every phone/browser can
/// decode, preset veryfast/crf 20 (fast encode at visually-lossless quality), 30fps. Segments
/// and the single-pass reel must encode identically so segment concat can stream-copy.
const VIDEO_ENCODE_ARGS: [&str; 10] = [
    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "veryfast", "-crf", "20", "-r", "30",
];

/// Build the whole reel in a SINGLE ffmpeg pass: each scene (a Ken Burns still or a video
/// clip) is fit to its window, concatenated, captioned, and muxed with audio — one encode
/// total (no intermediate clips, no double-encoding). Used for reels (and any run without
/// chapters); chaptered long-form renders go through `render_segment` + `concat_segments`
/// instead so dozens of supersampled stills are never held in one filtergraph.
///
/// Runs with `dir` as the working directory so we can pass bare filenames and avoid
/// ffmpeg filter path-escaping headaches. `media`, `audio`, `ass`, and `output` are
/// filenames relative to `dir`; `fontsdir` is an absolute path libass searches.
pub fn render_reel(opts: RenderReelOptions<'_>) -> Result<()> {
    let RenderReelOptions {
        dir,
        media,
        durations,
        dissolves,
        dissolve_seconds,
        grade,
        audio,
        music,
        duck,
        music_volume,
        captions,
        fontsdir,
        canvas,
        watermark,
        output,
    } = opts;

    let graph = build_video_graph(
        dir,
        media,
        durations,
        dissolves,
        dissolve_seconds,
        grade,
        captions,
        fontsdir,
        canvas,
        watermark,
    );

    // Audio inputs follow the graph's video inputs (scenes + optional watermark).
    let base = graph.input_count;
    let mut args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];
    args.extend(graph.input_args);
    // Narration is the next input.
    args.push("-i".into());
    args.push(audio.to_string());
    // Optional music is the input after that, looped to cover the whole reel.
    if let Some(m) = music {
        args.extend(["-stream_loop", "-1", "-i"].iter().map(|s| s.to_string()));
        args.push(m.to_string());
    }

    let mut parts = graph.parts;
    // Audio: narration alone, or mixed under a soundtrack (ducked or fixed-low).
    let audio_map = if music.is_some() {
        let total: f64 = durations.iter().sum();
        parts.push(audio_mix_filter(
            &format!("[{base}:a]"),
            &format!("[{}:a]", base + 1),
            duck,
            music_volume,
            total,
        ));
        "[aout]".to_string()
    } else {
        format!("{base}:a")
    };

    // Final mux/encode: the shared video encode plus broadly-compatible delivery settings:
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
            graph.video_map,
            "-map",
            &audio_map,
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    args.extend(VIDEO_ENCODE_ARGS.iter().map(|s| s.to_string()));
    args.extend(
        [
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ar",
            "44100",
            // Always emit a 2-channel file; the no-music path maps mono narration directly, and
            // this keeps every reel a consistent stereo container (centered dual-mono if mono).
            "-ac",
            "2",
            "-shortest",
            "-movflags",
            "+faststart",
            output,
        ]
        .iter()
        .map(|s| s.to_string()),
    );

    run_ffmpeg_in(dir, &args, "final render")
}

/// Everything `render_segment` needs to render one chapter's video-only segment.
pub struct RenderSegmentOptions<'a> {
    pub dir: &'a Path,
    /// This chapter's scenes only (sliced from the full run).
    pub media: &'a [SceneMedia],
    pub durations: &'a [f64],
    pub dissolves: &'a [bool],
    pub dissolve_seconds: f64,
    pub grade: bool,
    /// Per-segment `.ass` file with times re-based to this segment's clock.
    pub captions: Option<&'a str>,
    pub fontsdir: Option<&'a str>,
    pub canvas: Canvas,
    /// Optional watermark image path, burned into each segment (so the stream-copy concat keeps
    /// it) at the same position/size as a single-pass reel.
    pub watermark: Option<&'a str>,
    pub output: &'a str,
}

/// Render one chapter as a video-only MP4 segment. Identical per-scene chains and encode
/// params to `render_reel` (so `concat_segments` can stream-copy the segments), just no audio —
/// the full-length mix is muxed once at concat time so music runs continuously across chapters.
pub fn render_segment(opts: RenderSegmentOptions<'_>) -> Result<()> {
    let graph = build_video_graph(
        opts.dir,
        opts.media,
        opts.durations,
        opts.dissolves,
        opts.dissolve_seconds,
        opts.grade,
        opts.captions,
        opts.fontsdir,
        opts.canvas,
        opts.watermark,
    );
    let mut args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];
    args.extend(graph.input_args);
    let filter = graph.parts.join(";");
    args.extend(
        ["-filter_complex", &filter, "-map", graph.video_map, "-an"]
            .iter()
            .map(|s| s.to_string()),
    );
    args.extend(VIDEO_ENCODE_ARGS.iter().map(|s| s.to_string()));
    args.push(opts.output.to_string());
    run_ffmpeg_in(opts.dir, &args, "segment render")
}

/// Mix the full-length narration + optional looped music into one audio file (aac in an m4a),
/// in its own pass — the chunked render muxes this over the concatenated video segments.
pub fn mix_audio(
    dir: &Path,
    audio: &str,
    music: Option<&str>,
    duck: bool,
    music_volume: f64,
    total: f64,
    output: &str,
) -> Result<()> {
    let mut args: Vec<String> = vec!["-y".into(), "-loglevel".into(), "error".into()];
    args.push("-i".into());
    args.push(audio.to_string());
    if let Some(m) = music {
        args.extend(["-stream_loop", "-1", "-i"].iter().map(|s| s.to_string()));
        args.push(m.to_string());
        args.push("-filter_complex".into());
        args.push(audio_mix_filter(
            "[0:a]",
            "[1:a]",
            duck,
            music_volume,
            total,
        ));
        args.extend(["-map", "[aout]"].iter().map(|s| s.to_string()));
    } else {
        args.extend(["-map", "0:a"].iter().map(|s| s.to_string()));
    }
    args.extend(
        [
            "-c:a", "aac", "-b:a", "192k", "-ar", "44100", "-ac", "2", "-t",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    args.push(format!("{total:.3}"));
    args.push(output.to_string());
    run_ffmpeg_in(dir, &args, "audio mix")
}

/// Concatenate the chapter video segments (stream copy — identical encode params by
/// construction) and mux the pre-mixed audio over them → the final MP4.
pub fn concat_segments(
    dir: &Path,
    segments: &[String],
    audio_mix: &str,
    output: &str,
) -> Result<()> {
    let list = write_concat_list(dir, "segments.txt", segments)?;
    let args: Vec<String> = [
        "-y",
        "-loglevel",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        &list,
        "-i",
        audio_mix,
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-c",
        "copy",
        "-shortest",
        "-movflags",
        "+faststart",
        output,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    run_ffmpeg_in(dir, &args, "segment concat")
}

/// Concatenate per-chapter narration audio into one mp3 with a single re-encode. Stream-copy
/// mp3 concat produces unreliable duration headers, and the output's ffprobe duration is the
/// master clock for the whole render, so the one lame pass is worth it.
pub fn concat_audio(dir: &Path, parts: &[String], output: &str) -> Result<()> {
    let list = write_concat_list(dir, "chapters.txt", parts)?;
    let args: Vec<String> = [
        "-y",
        "-loglevel",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        &list,
        "-c:a",
        "libmp3lame",
        "-q:a",
        "2",
        output,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    run_ffmpeg_in(dir, &args, "audio concat")
}

/// Write a concat-demuxer list file (one `file '<name>'` line per part) into `dir` and return
/// its filename. Filenames are our own generated names (no quotes to escape).
fn write_concat_list(dir: &Path, list_name: &str, parts: &[String]) -> Result<String> {
    let body: String = parts.iter().map(|p| format!("file '{p}'\n")).collect();
    std::fs::write(dir.join(list_name), body)
        .with_context(|| format!("could not write {list_name}"))?;
    Ok(list_name.to_string())
}

/// Run ffmpeg with `dir` as the working directory, surfacing stderr on failure.
fn run_ffmpeg_in(dir: &Path, args: &[String], what: &str) -> Result<()> {
    let out = Command::new("ffmpeg")
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to launch ffmpeg for {what}"))?;
    if !out.status.success() {
        bail!(
            "ffmpeg {what} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{clean_up_temp_on_error, clip_chain, still_chain, watermark_parts};
    use crate::config::Canvas;
    use anyhow::Context;

    const REEL: Canvas = Canvas { w: 1080, h: 1920 };
    const YT: Canvas = Canvas { w: 1920, h: 1080 };

    #[test]
    fn watermark_scales_to_canvas_width_and_sits_bottom_right() {
        // Reel: 6% of 1080 = 65 wide, 4% margin = 43. Overlaid onto the graded label.
        let (parts, map) = watermark_parts(REEL, 4, "[vfinal]");
        assert_eq!(map, "[vwm]");
        assert_eq!(parts[0], "[4:v]scale=65:-1[wmk]");
        assert_eq!(
            parts[1],
            "[vfinal][wmk]overlay=W-w-43:H-h-43,format=yuv420p[vwm]"
        );
        // Youtube: 6% of 1920 = 115 wide, 4% margin = 77 — a larger logo on the wider canvas.
        let (parts, _) = watermark_parts(YT, 9, "[vout]");
        assert_eq!(parts[0], "[9:v]scale=115:-1[wmk]");
        assert_eq!(
            parts[1],
            "[vout][wmk]overlay=W-w-77:H-h-77,format=yuv420p[vwm]"
        );
    }

    #[test]
    fn reel_still_chain_matches_original_literal() {
        // Regression lock: the canvas-parameterized builder must reproduce the original
        // hardcoded reel filter byte-for-byte (4x supersample, 2x zoompan, canvas downscale).
        let z = "min(1.0+0.001000*on,1.12)";
        let got = still_chain(REEL, 3, z, "", "");
        let want = "[3:v]scale=4320:7680:force_original_aspect_ratio=increase:flags=lanczos,crop=4320:7680,\
             zoompan=z='min(1.0+0.001000*on,1.12)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s=2160x3840:fps=30,\
             scale=1080:1920:flags=lanczos,setsar=1,format=yuv420p[v3]";
        assert_eq!(got, want);
    }

    #[test]
    fn reel_clip_chain_matches_original_literal() {
        let got = clip_chain(REEL, 1, 1.0, 6.0, "eq=brightness=0.0200,", ",settb=AVTB");
        let want = "[1:v]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,\
             setsar=1,setpts=PTS*1.00000,fps=30,tpad=stop_mode=clone:stop_duration=1,\
             trim=duration=6.000,setpts=PTS-STARTPTS,eq=brightness=0.0200,format=yuv420p,settb=AVTB[v1]";
        assert_eq!(got, want);
    }

    #[test]
    fn landscape_chains_derive_from_canvas() {
        let s = still_chain(YT, 0, "z", "", "");
        assert!(s.contains("scale=7680:4320"));
        assert!(s.contains("s=3840x2160"));
        assert!(s.contains("scale=1920:1080:flags=lanczos"));
        let c = clip_chain(YT, 0, 1.0, 4.0, "", "");
        assert!(c.contains("scale=1920:1080:force_original_aspect_ratio=increase"));
        assert!(c.contains("crop=1920:1080"));
    }

    #[test]
    fn rename_failure_removes_poster_temp_file() {
        let dir = std::env::temp_dir().join(format!(
            "reelmaestro_poster_rename_failure_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("reel-poster.mp4");
        let reel = dir.join("reel.mp4");
        std::fs::write(&tmp, b"temporary poster reel").unwrap();
        std::fs::create_dir(&reel).unwrap();

        let err = clean_up_temp_on_error(&tmp, || {
            std::fs::rename(&tmp, &reel)
                .with_context(|| format!("renaming {} over {}", tmp.display(), reel.display()))
        })
        .unwrap_err();

        assert!(err.to_string().contains("renaming"));
        assert!(!tmp.exists(), "failed rename left {tmp:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
