// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Optional AI-generated video scenes (image-to-video via Veo). Each chosen scene's still
//! is animated into a short clip; failures fall back to `None` so the scene stays a still.

use std::path::{Path, PathBuf};

use futures::stream::{self, StreamExt};

use crate::model::Scene;
use crate::openrouter::{self, OpenRouter};

/// Cap on simultaneous in-flight Veo requests. Keeps us from hammering the provider (and
/// running up cost) while still overlapping the slow video generations.
const MAX_CONCURRENT: usize = 4;

/// Animate the first `video_count` scenes into clips. Returns a per-scene vector aligned to
/// `scenes`: `Some(clip path)` where a clip was produced, `None` where the scene should stay
/// a Ken Burns still (either not selected, or generation failed).
pub async fn generate(
    or: &OpenRouter,
    scenes: &[Scene],
    images: &[PathBuf],
    durations: &[f64],
    video_count: usize,
    resolution: &str,
    dir: &Path,
) -> Vec<Option<PathBuf>> {
    // Only the first `video_count` scenes get animated; the rest remain stills. `min` guards
    // against a caller asking for more clips than there are scenes.
    let jobs: Vec<usize> = (0..video_count.min(scenes.len())).collect();

    // Fan the jobs out concurrently (up to MAX_CONCURRENT) and collect (scene index, result)
    // pairs. `buffer_unordered` lets fast clips complete without waiting on slow ones, so we
    // carry the index through to re-sort into scene order afterward.
    let made: Vec<(usize, Option<PathBuf>)> = stream::iter(jobs)
        .map(|i| async move {
            // Veo Lite accepts only 4, 6, or 8s clips; size up to the scene's window.
            let duration = snap_duration(durations[i]);
            // Seed the motion prompt with the scene's image prompt, then nudge toward gentle,
            // cinematic movement so clips don't jump around or contradict the still.
            let prompt = format!(
                "{}. Subtle natural motion, cinematic, slow gentle camera move.",
                scenes[i].image_prompt
            );
            // Use the already-generated still as the first frame (image-to-video) so the clip
            // animates the exact image the user previewed. A read failure just drops the frame.
            let frame = std::fs::read(&images[i])
                .ok()
                .map(|b| openrouter::data_url_from_image(&b));

            match or
                .generate_video(&prompt, frame.as_deref(), duration, resolution)
                .await
            {
                Ok(bytes) => {
                    let path = dir.join(format!("scene-{i:02}.mp4"));
                    match std::fs::write(&path, &bytes) {
                        Ok(()) => (i, Some(path)),
                        Err(e) => {
                            eprintln!("  scene {i}: writing clip failed ({e}); using still");
                            (i, None)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  scene {i}: video generation failed ({e}); using still");
                    (i, None)
                }
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    // Re-index the unordered results back into a per-scene vector (default `None` = still).
    let mut out = vec![None; scenes.len()];
    for (i, clip) in made {
        out[i] = clip;
    }
    out
}

/// Total Veo seconds that will be billed for `video_count` scenes (for a cost estimate).
pub fn billed_seconds(durations: &[f64], video_count: usize) -> u32 {
    durations
        .iter()
        .take(video_count)
        .map(|&d| snap_duration(d))
        .sum()
}

/// Veo Lite accepts only discrete clip lengths (4, 6, 8s). Round the scene's window *up* to
/// the next supported length so the clip still covers the window, capped at the 8s maximum.
fn snap_duration(d: f64) -> u32 {
    const SUPPORTED: [u32; 3] = [4, 6, 8];
    // Round the (possibly fractional) window up to whole seconds, flooring at 0 to avoid a
    // negative-to-u32 wrap, then pick the first supported length that's at least that long.
    let want = d.ceil().max(0.0) as u32;
    SUPPORTED.into_iter().find(|&s| s >= want).unwrap_or(8) // nothing fits → cap at the 8s max
}

#[cfg(test)]
mod tests {
    use super::snap_duration;

    #[test]
    fn snaps_to_supported_veo_durations() {
        assert_eq!(snap_duration(0.0), 4);
        assert_eq!(snap_duration(3.2), 4);
        assert_eq!(snap_duration(4.0), 4);
        assert_eq!(snap_duration(4.1), 6); // 5s window -> 6, not the rejected 5
        assert_eq!(snap_duration(6.0), 6);
        assert_eq!(snap_duration(6.4), 8); // 7s window -> 8, not the rejected 7
        assert_eq!(snap_duration(8.0), 8);
        assert_eq!(snap_duration(20.0), 8); // capped at the model max
    }
}
