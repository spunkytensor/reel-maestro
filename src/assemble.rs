// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Final assembly stage of the pipeline.
//!
//! By the time we get here every input asset already exists on disk: the narration
//! `audio.mp3`, one still image per scene, and optionally an AI video clip per scene
//! plus a background music track. This module's job is to glue them into the finished
//! `reel.mp4`:
//!   1. Divide the audio timeline into one window per scene, proportional to how many
//!      narration words that scene speaks (so the visuals track the voiceover).
//!   2. Write the burned-in subtitle file (`reel.ass`) from the word timings.
//!   3. Hand the per-scene media + durations to `ffmpeg::render_reel`, which does the
//!      heavy lifting in a single pass: Ken Burns pan/zoom on stills, concat of all
//!      scenes, caption burn-in, and the narration/music audio mix.
//!
//! The actual ffmpeg filtergraph construction lives in `ffmpeg.rs`; this module owns the
//! scene-timing math and decides *which* asset (clip vs. still) represents each scene.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::captions;
use crate::ffmpeg;
use crate::model::{Scene, WordTiming};

/// Where DejaVu ships on Debian/Ubuntu. libass needs a fonts directory to render captions;
/// on Linux we point it here, and on macOS/Windows (where this path is absent) we fall back
/// to the OS font provider instead — see the `fontsdir` handling in `build`.
const FONTS_DIR: &str = "/usr/share/fonts/truetype/dejavu";

/// Everything `build` needs to assemble the reel. Borrowed (not owned) because the caller
/// already holds all of these and assembly is a single synchronous pass.
pub struct BuildOptions<'a> {
    /// Run folder. All inputs live here and `reel.mp4` / `reel.ass` are written here.
    pub dir: &'a Path,
    /// The scenes, in order. Used for per-scene word counts (timing) — not the text itself.
    pub scenes: &'a [Scene],
    /// One still image per scene, parallel to `scenes`. Always present (the Ken Burns fallback).
    pub images: &'a [PathBuf],
    /// Optional AI video clip per scene, parallel to `scenes`. `Some` wins over the still.
    pub clips: &'a [Option<PathBuf>],
    /// Word-level timings used to build the caption file. Empty ⇒ no captions written.
    pub words: &'a [WordTiming],
    /// The narration track; its total duration defines the length of the whole reel.
    pub audio: &'a Path,
    /// Optional background soundtrack mixed under the narration.
    pub music: Option<&'a Path>,
    /// `true` = sidechain-duck the music under speech; `false` = hold it at a constant low level.
    pub duck: bool,
    /// Music gain multiplier (≥ 0). Higher is louder.
    pub music_volume: f64,
    /// Master switch for burning captions. When `false` no subtitle file is produced.
    pub captions_on: bool,
}

/// Build `reel.mp4` in `dir`. `images` are scene stills in order; `audio` and the
/// produced `reel.ass` live in `dir`.
pub fn build(opts: BuildOptions<'_>) -> Result<PathBuf> {
    let BuildOptions {
        dir,
        scenes,
        images,
        clips,
        words,
        audio,
        music,
        duck,
        music_volume,
        captions_on,
    } = opts;

    if images.is_empty() {
        bail!("no scene images to assemble");
    }
    // Slice the audio timeline into one duration per scene (see `scene_durations`).
    let durations = scene_durations(scenes, audio)?;

    // Write captions only when enabled and there are timed words to show. The returned
    // `Some(name)` signals render_reel to burn this ASS file in; `None` skips the subtitles
    // filter entirely (e.g. silent/no-narration reels have no words to caption).
    let ass_name = "reel.ass";
    let captions = if captions_on && !words.is_empty() {
        std::fs::write(dir.join(ass_name), captions::build_ass(words))?;
        Some(ass_name)
    } else {
        None
    };

    // Decide each scene's visual source: an AI video clip if one was produced for that
    // index, otherwise its still (which render_reel animates with Ken Burns). render_reel
    // works in `dir`, so we pass bare file names rather than full paths.
    let basename = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let media: Vec<ffmpeg::SceneMedia> = images
        .iter()
        .enumerate()
        .map(|(i, img)| match clips.get(i).and_then(|c| c.as_ref()) {
            Some(clip) => ffmpeg::SceneMedia::Clip(basename(clip)),
            None => ffmpeg::SceneMedia::Still(basename(img)),
        })
        .collect();

    let audio_name = audio.file_name().unwrap().to_string_lossy().into_owned();
    let music_name = music.map(basename);

    // FONTS_DIR is a Linux path; on macOS/Windows it won't exist, so only pass it
    // when present and otherwise let libass fall back to the system font provider.
    let fontsdir = Path::new(FONTS_DIR).exists().then_some(FONTS_DIR);

    let output = "reel.mp4";
    ffmpeg::render_reel(ffmpeg::RenderReelOptions {
        dir,
        media: &media,
        durations: &durations,
        audio: &audio_name,
        music: music_name.as_deref(),
        duck,
        music_volume,
        captions,
        fontsdir,
        output,
    })?;
    Ok(dir.join(output))
}

/// Per-scene durations in seconds, each scene's slice of the audio timeline (proportional to
/// its narration word count). Every duration is floored at 0.5s so a one-word scene still gets
/// a visible beat. Exposed (not private) so the video step can size its Veo clips to match the
/// exact slot each scene will occupy in the final reel.
pub fn scene_durations(scenes: &[Scene], audio: &Path) -> Result<Vec<f64>> {
    let total = ffmpeg::duration_s(audio)?;
    Ok(scene_windows(scenes, total)
        .into_iter()
        .map(|(start, end)| (end - start).max(0.5))
        .collect())
}

/// Assign each scene a `[start, end)` window (in seconds) over the total audio duration,
/// proportional to its narration word count — so a scene that speaks twice as many words gets
/// roughly twice the screen time, keeping visuals loosely in sync with the voiceover.
///
/// Boundaries are computed from the *cumulative* word count rather than by summing per-scene
/// durations, so rounding never drifts and adjacent windows always meet exactly. The final
/// scene is pinned to `total` so the visuals cover the audio to the very end with no gap.
fn scene_windows(scenes: &[Scene], total: f64) -> Vec<(f64, f64)> {
    // Word count per scene; `.max(1)` guarantees a blank/empty line still claims a share
    // (and avoids a divide-by-zero if every line were empty).
    let counts: Vec<usize> = scenes
        .iter()
        .map(|s| s.line.split_whitespace().count().max(1))
        .collect();
    let sum: usize = counts.iter().sum();

    let mut windows = Vec::with_capacity(scenes.len());
    let mut cum = 0usize; // words consumed by all preceding scenes
    for (i, &c) in counts.iter().enumerate() {
        // Fraction of the timeline up to this scene's start = words before it / total words.
        let start = total * (cum as f64) / (sum as f64);
        cum += c;
        let end = if i == scenes.len() - 1 {
            total // pin the last scene to the end so visuals fully cover the audio
        } else {
            total * (cum as f64) / (sum as f64)
        };
        windows.push((start, end));
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Scene, WordTiming};
    use std::process::Command;

    /// Full render-path smoke test using synthetic inputs — NO network/API calls.
    /// Requires `ffmpeg`/`ffprobe` on PATH. Ignored by default; run explicitly with:
    ///   cargo test render_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn render_smoke() {
        let dir = std::env::temp_dir().join("reelmaestro_render_smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Three synthetic scene stills (solid colors), full 1080x1920 canvas.
        let colors = [[180u8, 60, 60], [60, 140, 180], [80, 170, 90]];
        let mut images = Vec::new();
        for (i, c) in colors.iter().enumerate() {
            let img = image::RgbImage::from_pixel(1080, 1920, image::Rgb(*c));
            let p = dir.join(format!("scene-{i:02}.jpg"));
            img.save(&p).unwrap();
            images.push(p);
        }

        // A 6-second tone as stand-in narration audio.
        let audio = dir.join("audio.mp3");
        let ok = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=330:duration=6",
            ])
            .arg(&audio)
            .status()
            .expect("ffmpeg must be installed to run this test");
        assert!(ok.success(), "failed to synthesize test audio");

        // Synthetic scenes + word timings spanning the 6s.
        let scenes = vec![
            Scene {
                line: "one two three".into(),
                image_prompt: String::new(),
            },
            Scene {
                line: "four five six".into(),
                image_prompt: String::new(),
            },
            Scene {
                line: "seven eight nine".into(),
                image_prompt: String::new(),
            },
        ];
        let labels = [
            "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        ];
        let words: Vec<WordTiming> = labels
            .iter()
            .enumerate()
            .map(|(i, w)| WordTiming {
                word: w.to_string(),
                start_s: i as f64 * (6.0 / 9.0),
                end_s: (i as f64 + 1.0) * (6.0 / 9.0),
            })
            .collect();

        let no_clips = vec![None; images.len()];
        let reel = build(BuildOptions {
            dir: &dir,
            scenes: &scenes,
            images: &images,
            clips: &no_clips,
            words: &words,
            audio: &audio,
            music: None,
            duck: true,
            music_volume: 0.5,
            captions_on: true,
        })
        .unwrap();
        assert!(reel.exists(), "reel.mp4 was not produced");

        // Verify the output is a real ~6s 1080x1920 video.
        let dur = ffmpeg::duration_s(&reel).unwrap();
        assert!((dur - 6.0).abs() < 1.0, "unexpected duration: {dur}");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&reel)
            .output()
            .unwrap();
        let dims = String::from_utf8_lossy(&probe.stdout);
        assert!(
            dims.trim().starts_with("1080,1920"),
            "unexpected dims: {dims}"
        );
        println!(
            "render_smoke OK -> {} ({dur:.1}s, {})",
            reel.display(),
            dims.trim()
        );
    }

    /// Exercises the soundtrack mixing filter graph (both duck and low modes) with a
    /// synthetic music track — NO network. Requires ffmpeg. Run with:
    ///   cargo test music_mix_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn music_mix_smoke() {
        let dir = std::env::temp_dir().join("reelmaestro_music_smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let img = image::RgbImage::from_pixel(1080, 1920, image::Rgb([90, 90, 120]));
        let scene_path = dir.join("scene-00.jpg");
        img.save(&scene_path).unwrap();

        // 6s narration tone, and a longer 10s music tone (tests looping/trim).
        let audio = dir.join("audio.mp3");
        Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=330:duration=6",
            ])
            .arg(&audio)
            .status()
            .expect("ffmpeg required")
            .success()
            .then_some(())
            .unwrap();
        let music = dir.join("music.wav");
        Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=180:duration=10",
            ])
            .arg(&music)
            .status()
            .unwrap()
            .success()
            .then_some(())
            .unwrap();

        let scenes = vec![Scene {
            line: "one two three four five six".into(),
            image_prompt: String::new(),
        }];
        let words: Vec<WordTiming> = (0..6)
            .map(|i| WordTiming {
                word: format!("w{i}"),
                start_s: i as f64,
                end_s: i as f64 + 1.0,
            })
            .collect();

        for duck in [true, false] {
            let reel = build(BuildOptions {
                dir: &dir,
                scenes: &scenes,
                images: std::slice::from_ref(&scene_path),
                clips: &[None],
                words: &words,
                audio: &audio,
                music: Some(&music),
                duck,
                music_volume: 0.6,
                captions_on: true,
            })
            .unwrap();
            assert!(reel.exists());
            // Confirm a real audio stream survived the mix.
            let probe = Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=codec_name",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&reel)
                .output()
                .unwrap();
            let codec = String::from_utf8_lossy(&probe.stdout);
            assert!(
                codec.trim() == "aac",
                "no audio after mix (duck={duck}): {codec:?}"
            );
            let dur = ffmpeg::duration_s(&reel).unwrap();
            assert!(
                (dur - 6.0).abs() < 1.0,
                "duck={duck} unexpected duration {dur}"
            );
            println!(
                "music_mix_smoke OK (duck={duck}) -> {dur:.1}s, audio={}",
                codec.trim()
            );
        }
    }

    /// Exercises the video-clip render branch (a real mp4 scene mixed with a still) — NO
    /// network. Requires ffmpeg. Run with:
    ///   cargo test video_mode_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn video_mode_smoke() {
        let dir = std::env::temp_dir().join("reelmaestro_video_smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Scene 0 = a synthetic video clip; scene 1 = a still.
        let clip = dir.join("scene-00.mp4");
        Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=720x1280:rate=30:duration=6",
            ])
            .arg(&clip)
            .status()
            .expect("ffmpeg required")
            .success()
            .then_some(())
            .unwrap();
        let still = dir.join("scene-01.jpg");
        image::RgbImage::from_pixel(1080, 1920, image::Rgb([70, 120, 70]))
            .save(&still)
            .unwrap();

        let audio = dir.join("audio.mp3");
        Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=330:duration=8",
            ])
            .arg(&audio)
            .status()
            .unwrap()
            .success()
            .then_some(())
            .unwrap();

        let scenes = vec![
            Scene {
                line: "one two three four".into(),
                image_prompt: String::new(),
            },
            Scene {
                line: "five six seven eight".into(),
                image_prompt: String::new(),
            },
        ];
        let words: Vec<WordTiming> = (0..8)
            .map(|i| WordTiming {
                word: format!("w{i}"),
                start_s: i as f64,
                end_s: i as f64 + 1.0,
            })
            .collect();
        let images = vec![dir.join("scene-00.jpg"), still]; // scene 0 image unused (clip wins)
        let clips = vec![Some(clip), None];

        let reel = build(BuildOptions {
            dir: &dir,
            scenes: &scenes,
            images: &images,
            clips: &clips,
            words: &words,
            audio: &audio,
            music: None,
            duck: true,
            music_volume: 0.5,
            captions_on: true,
        })
        .unwrap();
        assert!(reel.exists());
        let dur = ffmpeg::duration_s(&reel).unwrap();
        assert!((dur - 8.0).abs() < 1.0, "unexpected duration {dur}");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "csv=p=0",
            ])
            .arg(&reel)
            .output()
            .unwrap();
        let streams = String::from_utf8_lossy(&probe.stdout);
        assert!(
            streams.contains("video") && streams.contains("audio"),
            "missing streams: {streams:?}"
        );
        println!("video_mode_smoke OK -> {dur:.1}s (clip scene + still scene)");
    }

    /// Silent-narration + no-captions path: a silent timeline, no ASS burned. NO network.
    /// Requires ffmpeg. Run: cargo test no_narration_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn no_narration_smoke() {
        let dir = std::env::temp_dir().join("reelmaestro_silent_smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let still = dir.join("scene-00.jpg");
        image::RgbImage::from_pixel(1080, 1920, image::Rgb([60, 60, 90]))
            .save(&still)
            .unwrap();

        // Silent timeline (what main builds when --no-narration is set), 8s for one scene.
        let audio = dir.join("audio.mp3");
        ffmpeg::silent_track(&audio, 8.0).unwrap();

        let scenes = vec![Scene {
            line: "one two three four".into(),
            image_prompt: String::new(),
        }];
        // No words, captions disabled.
        let reel = build(BuildOptions {
            dir: &dir,
            scenes: &scenes,
            images: &[still],
            clips: &[None],
            words: &[],
            audio: &audio,
            music: None,
            duck: true,
            music_volume: 0.8,
            captions_on: false,
        })
        .unwrap();
        assert!(reel.exists());
        assert!(
            !dir.join("reel.ass").exists(),
            "captions file should not be written"
        );
        let dur = ffmpeg::duration_s(&reel).unwrap();
        assert!((dur - 8.0).abs() < 1.0, "unexpected duration {dur}");
        println!("no_narration_smoke OK -> {dur:.1}s, no captions burned");
    }

    /// A clip SHORTER than its slot must keep moving across the whole slot (retimed), not
    /// freeze the last frame. We render a 3s clip into an 8s slot and assert late frames
    /// still differ from earlier ones. NO network; requires ffmpeg.
    /// Run: cargo test clip_fills_slot_without_freezing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn clip_fills_slot_without_freezing() {
        let dir = std::env::temp_dir().join("reelmaestro_freeze_smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 3s moving clip, 8s timeline → one 8s slot the clip is far too short for.
        let clip = dir.join("scene-00.mp4");
        Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=720x1280:rate=30:duration=3",
            ])
            .arg(&clip)
            .status()
            .expect("ffmpeg required")
            .success()
            .then_some(())
            .unwrap();
        let audio = dir.join("audio.mp3");
        ffmpeg::silent_track(&audio, 8.0).unwrap();

        let scenes = vec![Scene {
            line: "a b c d e f g h".into(),
            image_prompt: String::new(),
        }];
        let images = vec![dir.join("scene-00.jpg")]; // unused (clip wins)
        let reel = build(BuildOptions {
            dir: &dir,
            scenes: &scenes,
            images: &images,
            clips: &[Some(clip)],
            words: &[],
            audio: &audio,
            music: None,
            duck: true,
            music_volume: 0.8,
            captions_on: false,
        })
        .unwrap();

        let dur = ffmpeg::duration_s(&reel).unwrap();
        assert!((dur - 8.0).abs() < 1.0, "slot not filled: {dur}");

        // Frames late in the slot (well past the clip's native 3s) must still differ from a
        // mid-slot frame — i.e. motion continued instead of freezing.
        let grab = |t: &str, out: &str| {
            Command::new("ffmpeg")
                .args(["-y", "-loglevel", "error", "-ss", t, "-i"])
                .arg(&reel)
                .args(["-frames:v", "1"])
                .arg(dir.join(out))
                .status()
                .unwrap();
            std::fs::read(dir.join(out)).unwrap()
        };
        let mid = grab("5.0", "f_mid.png");
        let late = grab("7.5", "f_late.png");
        assert_ne!(
            mid, late,
            "frames identical past the clip length → it froze"
        );
        println!("clip_fills_slot_without_freezing OK -> {dur:.1}s, motion continuous");
    }

    /// Poster extraction + cover-art embedding. NO network; requires ffmpeg.
    /// Run: cargo test poster_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn poster_smoke() {
        let dir = std::env::temp_dir().join("reelmaestro_poster_smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let still = dir.join("scene-00.jpg");
        image::RgbImage::from_pixel(1080, 1920, image::Rgb([120, 80, 60]))
            .save(&still)
            .unwrap();
        let audio = dir.join("audio.mp3");
        ffmpeg::silent_track(&audio, 6.0).unwrap();
        let scenes = vec![Scene {
            line: "a b c d".into(),
            image_prompt: String::new(),
        }];
        let reel = build(BuildOptions {
            dir: &dir,
            scenes: &scenes,
            images: &[still],
            clips: &[None],
            words: &[],
            audio: &audio,
            music: None,
            duck: true,
            music_volume: 0.8,
            captions_on: false,
        })
        .unwrap();

        // Extract a poster at the hook midpoint and verify it's a 1080x1920 image.
        let poster = dir.join("poster.jpg");
        ffmpeg::poster_frame(&reel, &poster, 3.0).unwrap();
        let img = image::open(&poster).unwrap();
        assert_eq!((img.width(), img.height()), (1080, 1920));

        // Embed as cover art and confirm the reel now has a second (attached_pic) video stream.
        ffmpeg::embed_poster(&dir, "reel.mp4", "poster.jpg").unwrap();
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v",
                "-show_entries",
                "stream=codec_type:stream_disposition=attached_pic",
                "-of",
                "csv=p=0",
            ])
            .arg(&reel)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&probe.stdout);
        let vstreams = out.lines().filter(|l| l.contains("video")).count();
        assert!(
            vstreams >= 2,
            "expected a cover-art video stream, got: {out:?}"
        );
        assert!(
            out.contains(",1"),
            "no attached_pic disposition found: {out:?}"
        );
        // Reel still plays and keeps its duration.
        assert!((ffmpeg::duration_s(&reel).unwrap() - 6.0).abs() < 1.0);
        println!("poster_smoke OK -> poster.jpg 1080x1920 + embedded cover art");
    }
}
