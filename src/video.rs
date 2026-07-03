// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Optional AI-generated video scenes (image-to-video via Veo). Each chosen scene's still
//! is animated into a short clip; failures fall back to `None` so the scene stays a still.

use std::path::{Path, PathBuf};

use futures::stream::{self, StreamExt};

use crate::images;
use crate::model::{Entity, Scene};
use crate::openrouter::{self, OpenRouter};

/// Cap on simultaneous in-flight Veo requests. Keeps us from hammering the provider (and
/// running up cost) while still overlapping the slow video generations.
const MAX_CONCURRENT: usize = 4;

/// Recurring Veo failure modes worth suppressing on every clip. Sent through OpenRouter's
/// provider passthrough (Veo has no top-level negative-prompt field); the submit falls back to
/// omitting it if the provider rejects the passthrough.
const NEGATIVE_PROMPT: &str = "text, captions, subtitles, watermark, logo, morphing, warping, \
     extra limbs, distorted face, flickering";

/// Generic motion used when the scriptwriter provided no `motion_prompt` (older `script.json`
/// resumes, or pre-upgrade scripts).
const DEFAULT_MOTION: &str =
    "Animate this image with subtle, natural motion and a slow, gentle camera move.";

/// Animate the first `video_count` scenes into clips. Returns a per-scene vector aligned to
/// `scenes`: `Some(clip path)` where a clip was produced, `None` where the scene should stay
/// a Ken Burns still (either not selected, or generation failed).
// Mirrors `images::generate`: one call site, loosely-related pipeline inputs.
#[allow(clippy::too_many_arguments)]
pub async fn generate(
    or: &OpenRouter,
    scenes: &[Scene],
    characters: &[Entity],
    locations: &[Entity],
    images: &[PathBuf],
    durations: &[f64],
    video_count: usize,
    resolution: &str,
    dir: &Path,
) -> Vec<Option<PathBuf>> {
    // Only the first `video_count` scenes get animated; the rest remain stills. `min` guards
    // against a caller asking for more clips than there are scenes.
    let jobs: Vec<usize> = (0..video_count.min(scenes.len())).collect();

    // Veo's negativePrompt rides a Veo-specific provider passthrough; skip it entirely for a
    // non-Veo --video-model override rather than send a field its provider never defined.
    let negative = or
        .video_model
        .starts_with("google/veo")
        .then_some(NEGATIVE_PROMPT);

    // Fan the jobs out concurrently (up to MAX_CONCURRENT) and collect (scene index, result)
    // pairs. `buffer_unordered` lets fast clips complete without waiting on slow ones, so we
    // carry the index through to re-sort into scene order afterward.
    let made: Vec<(usize, Option<PathBuf>)> = stream::iter(jobs)
        .map(|i| async move {
            // Reuse a clip already on disk instead of paying to regenerate it. This is what makes
            // selective regeneration work: delete just the scene-NN.mp4 you dislike, re-run with
            // --video, and only that one is regenerated (the rest are reused as-is). Mirrors how
            // scene stills are reused on --from resume.
            let path = dir.join(format!("scene-{i:02}.mp4"));
            if path.exists() {
                println!("  scene {i}: reusing existing clip");
                return (i, Some(path));
            }
            // Size the clip up to the scene's window, within the model's accepted lengths.
            let duration = snap_duration(&or.video_model, durations[i]);
            let prompt = build_video_prompt(&scenes[i], characters, locations);
            // Use the already-generated still as the first frame (image-to-video) so the clip
            // animates the exact image the user previewed. A read failure just drops the frame.
            let frame = std::fs::read(&images[i])
                .ok()
                .map(|b| openrouter::data_url_from_image(&b));
            // The cast's saved reference portraits, as Veo "ingredients" for the text-to-video
            // fallback — if the judged first frame is rejected (e.g. Veo's person/face filter),
            // these keep the characters anchored instead of letting Veo reinvent them.
            let reference_urls = character_reference_urls(&scenes[i], dir);

            match or
                .generate_video(
                    &prompt,
                    frame.as_deref(),
                    &reference_urls,
                    duration,
                    resolution,
                    negative,
                )
                .await
            {
                Ok(bytes) => match std::fs::write(&path, &bytes) {
                    Ok(()) => (i, Some(path)),
                    Err(e) => {
                        eprintln!("  scene {i}: writing clip failed ({e}); using still");
                        (i, None)
                    }
                },
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

/// Assemble the motion prompt for one clip. Pure (no I/O) so the composition is unit-testable.
///
/// The scriptwriter's per-scene `motion_prompt` (written with full story context) leads when
/// present; the identity/no-text lock, the canonical entity descriptions, and the narration beat
/// follow so Veo has the same textual anchors the image model had — the first frame alone can be
/// ambiguous about who is who and what moment this is.
fn build_video_prompt(scene: &Scene, characters: &[Entity], locations: &[Entity]) -> String {
    let mut prompt = if scene.motion_prompt.trim().is_empty() {
        String::from(DEFAULT_MOTION)
    } else {
        format!("Animate this image. {}", scene.motion_prompt.trim())
    };

    prompt.push_str(
        " Keep every person's face, hair, build, and clothing and the entire setting \
         EXACTLY as in the first frame — do not change identities, wardrobe, props, or \
         background, and do not add or remove people. Do NOT add any text, words, letters, \
         captions, subtitles, watermarks, logos, or timestamps anywhere in the frame — keep \
         the footage completely clean of overlaid graphics.",
    );

    // Canonical text locks — the same anchors the image prompts carry, so identity holds even
    // where the first frame is ambiguous (or was rejected and replaced by text-to-video).
    let cast: Vec<&Entity> = scene
        .cast_ids
        .iter()
        .filter_map(|id| characters.iter().find(|c| &c.id == id))
        .filter(|c| !c.description.trim().is_empty())
        .collect();
    if !cast.is_empty() {
        prompt.push_str(" People in this shot, keep EXACTLY consistent: ");
        for c in &cast {
            prompt.push_str(&format!("[{}] {}; ", c.id, c.description));
        }
    }
    if let Some(loc) = locations
        .iter()
        .find(|l| !scene.location_id.trim().is_empty() && l.id == scene.location_id)
    {
        if !loc.description.trim().is_empty() {
            prompt.push_str(&format!(
                " Setting, keep EXACTLY consistent: {}.",
                loc.description
            ));
        }
    }

    // The spoken beat this clip plays under: mood/pacing context, plus an explicit no-lip-sync
    // rule since the clip carries no model audio (our narration is mixed over it).
    if !scene.line.trim().is_empty() {
        prompt.push_str(&format!(
            " This shot plays under the narration: \"{}\". Match its mood and pacing. The \
             characters do not speak or mouth words.",
            scene.line.trim()
        ));
    }

    prompt.push_str(&format!(" Scene: {}", scene.image_prompt));
    prompt
}

/// Data URLs of the saved reference portraits for this scene's cast (front view per character,
/// in cast order), for use as Veo "ingredients". Missing files are skipped silently — a
/// no-consistency run has none, and the fallback then simply goes unanchored as before.
fn character_reference_urls(scene: &Scene, dir: &Path) -> Vec<String> {
    scene
        .cast_ids
        .iter()
        .filter_map(|id| {
            std::fs::read(dir.join(format!("character-{}.jpg", images::slug(id)))).ok()
        })
        .map(|b| openrouter::data_url_from_image(&b))
        .collect()
}

/// The clip lengths a video model accepts: a discrete list (Veo: 4/6/8s only) or an integer
/// range (Wan/Seedance/Kling take flexible durations).
pub enum ClipLengths {
    Discrete(&'static [u32]),
    Range { min: u32, max: u32 },
}

/// Per-model clip-length capability (July 2026 OpenRouter catalog). Unknown models get Veo's
/// conservative discrete set — every provider in the catalog accepts those lengths.
pub fn clip_lengths(model: &str) -> ClipLengths {
    if model.contains("wan-2.6") || model.contains("seedance") {
        ClipLengths::Range { min: 2, max: 15 }
    } else if model.contains("kling") {
        ClipLengths::Range { min: 3, max: 15 }
    } else {
        // Veo (and anything unrecognized): 4/6/8s only.
        ClipLengths::Discrete(&[4, 6, 8])
    }
}

/// Seconds billed for a specific set of scene indices — those actually being generated after
/// existing clips are reused — so the cost estimate reflects only what will really be billed.
pub fn billed_seconds_for(model: &str, durations: &[f64], indices: &[usize]) -> u32 {
    indices
        .iter()
        .filter_map(|&i| durations.get(i))
        .map(|&d| snap_duration(model, d))
        .sum()
}

/// Round the scene's window *up* to the nearest clip length the model accepts, capped at its
/// maximum (a longer window is then covered by retiming in the render). Models with flexible
/// ranges get near-exact durations, so long-form scene windows aren't slow-motion-stretched.
fn snap_duration(model: &str, d: f64) -> u32 {
    // Round the (possibly fractional) window up to whole seconds, flooring at 0 to avoid a
    // negative-to-u32 wrap.
    let want = d.ceil().max(0.0) as u32;
    match clip_lengths(model) {
        ClipLengths::Discrete(supported) => supported
            .iter()
            .copied()
            .find(|&s| s >= want)
            .unwrap_or_else(|| *supported.last().unwrap()),
        ClipLengths::Range { min, max } => want.clamp(min, max),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_video_prompt, snap_duration};
    use crate::model::{Entity, Scene};

    fn scene(json: &str) -> Scene {
        serde_json::from_str(json).unwrap()
    }

    fn ent(id: &str, desc: &str) -> Entity {
        Entity {
            id: id.to_string(),
            description: desc.to_string(),
        }
    }

    #[test]
    fn snaps_to_supported_veo_durations() {
        const VEO: &str = "google/veo-3.1-lite";
        assert_eq!(snap_duration(VEO, 0.0), 4);
        assert_eq!(snap_duration(VEO, 3.2), 4);
        assert_eq!(snap_duration(VEO, 4.0), 4);
        assert_eq!(snap_duration(VEO, 4.1), 6); // 5s window -> 6, not the rejected 5
        assert_eq!(snap_duration(VEO, 6.0), 6);
        assert_eq!(snap_duration(VEO, 6.4), 8); // 7s window -> 8, not the rejected 7
        assert_eq!(snap_duration(VEO, 8.0), 8);
        assert_eq!(snap_duration(VEO, 20.0), 8); // capped at the model max
    }

    #[test]
    fn flexible_models_snap_to_near_exact_durations() {
        const WAN: &str = "alibaba/wan-2.6";
        assert_eq!(snap_duration(WAN, 9.3), 10); // near-exact, no 4/6/8 quantization
        assert_eq!(snap_duration(WAN, 20.0), 15); // clamped to Wan's 15s max
        assert_eq!(snap_duration(WAN, 1.0), 2); // floored at the model min
        assert_eq!(snap_duration("bytedance/seedance-2.0-fast", 12.2), 13);
        assert_eq!(snap_duration("kwaivgi/kling-v3.0-std", 1.0), 3);
        // Unknown models fall back to the conservative Veo set.
        assert_eq!(snap_duration("someone/other-video", 5.0), 6);
    }

    #[test]
    fn motion_prompt_leads_when_present_else_generic_default() {
        let with = scene(
            r#"{"line":"","image_prompt":"a dog on a path","motion_prompt":"slow push-in as the dog tilts its head"}"#,
        );
        let p = build_video_prompt(&with, &[], &[]);
        assert!(p.starts_with("Animate this image. slow push-in as the dog tilts its head"));
        assert!(!p.contains("subtle, natural motion")); // generic default replaced

        // Older script.json without motion_prompt → today's generic motion (resume compat).
        let without = scene(r#"{"line":"","image_prompt":"a dog on a path"}"#);
        let p = build_video_prompt(&without, &[], &[]);
        assert!(p.starts_with("Animate this image with subtle, natural motion"));
        // The identity/no-text lock survives in both.
        assert!(p.contains("EXACTLY as in the first frame"));
        assert!(p.contains("Do NOT add any text"));
    }

    #[test]
    fn prompt_carries_entity_locks_and_narration_beat() {
        let s = scene(
            r#"{"line":"Dexter sniffed the mitten.","image_prompt":"a sheltie on a snowy path",
                "cast_ids":["dexter","ghost"],"location_id":"garden"}"#,
        );
        let chars = vec![ent("dexter", "a SMALL Sheltie with a blue bandana")];
        let locs = vec![ent("garden", "a snowy garden path, cedar fence")];
        let p = build_video_prompt(&s, &chars, &locs);
        // Canonical locks for the entities present (unknown cast ids are skipped).
        assert!(p.contains("[dexter] a SMALL Sheltie with a blue bandana"));
        assert!(!p.contains("ghost"));
        assert!(p.contains("Setting, keep EXACTLY consistent: a snowy garden path, cedar fence"));
        // The narration beat, with the no-lip-sync rule.
        assert!(p.contains("narration: \"Dexter sniffed the mitten.\""));
        assert!(p.contains("do not speak or mouth words"));
        // The scene prompt lands last.
        assert!(p.trim_end().ends_with("Scene: a sheltie on a snowy path"));
    }

    #[test]
    fn prompt_omits_empty_context_blocks() {
        let s = scene(r#"{"line":"","image_prompt":"a city skyline"}"#);
        let p = build_video_prompt(&s, &[], &[]);
        assert!(!p.contains("People in this shot"));
        assert!(!p.contains("Setting, keep EXACTLY"));
        assert!(!p.contains("plays under the narration"));
    }
}
