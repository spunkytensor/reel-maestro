// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Scene prompts -> 1080x1920 JPEG stills. Runs generations concurrently (bounded) and
//! falls back to a solid placeholder frame so one bad generation never kills the run.

use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::stream::{self, StreamExt};
use image::{imageops, Rgb, RgbImage};

use crate::model::Scene;
use crate::openrouter::{self, OpenRouter};

const W: u32 = 1080;
const H: u32 = 1920;
const MAX_CONCURRENT: usize = 4;

const MAX_ATTEMPTS: usize = 3;

/// Generate one image per scene into `dir`, returning their paths in scene order.
///
/// When consistency is enabled and there's a recurring cast (or a `--character-ref` photo),
/// a single shared reference image is built first and every scene is conditioned on it so the
/// same person/animal carries through the reel.
pub async fn generate(
    or: &OpenRouter,
    scenes: &[Scene],
    cast: &str,
    character_ref: Option<&Path>,
    consistency: bool,
    dir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut refs: Vec<String> = Vec::new();
    if consistency_enabled(consistency, cast, character_ref.is_some()) {
        match build_reference(or, cast, character_ref, dir).await {
            Some(data_url) => refs.push(data_url),
            None => eprintln!(
                "  note: no character reference available; scenes generated independently"
            ),
        }
    }
    let refs = &refs;

    let paths: Vec<PathBuf> = stream::iter(scenes.iter().enumerate())
        .map(|(i, scene)| async move {
            let path = dir.join(format!("scene-{i:02}.jpg"));
            let img = match generate_one(or, &scene.image_prompt, cast, refs, &format!("scene {i}")).await {
                Some(img) => img,
                None => {
                    eprintln!("  scene {i}: image generation failed after {MAX_ATTEMPTS} tries; using placeholder");
                    placeholder(i)
                }
            };
            // Always leave a usable file on disk — fall back to a placeholder if the save fails,
            // so a missing scene never breaks assembly downstream.
            if let Err(e) = img.save(&path) {
                eprintln!("  scene {i}: saving image failed ({e}); writing placeholder");
                let _ = placeholder(i).save(&path);
            }
            path
        })
        .buffered(MAX_CONCURRENT)
        .collect()
        .await;
    Ok(paths)
}

/// Generate a custom cover/thumbnail image from `prompt` (conditioned on `references` so a
/// recurring character matches the reel) and save it as `poster.jpg`. Returns its path, or
/// `None` if generation fails.
pub async fn generate_poster(
    or: &OpenRouter,
    prompt: &str,
    cast: &str,
    references: &[String],
    dir: &Path,
) -> Option<PathBuf> {
    let img = generate_one(or, prompt, cast, references, "poster").await?;
    let path = dir.join("poster.jpg");
    img.save(&path).ok()?;
    Some(path)
}

/// Consistency conditioning applies only when enabled AND there's something to anchor to.
fn consistency_enabled(flag: bool, cast: &str, has_ref: bool) -> bool {
    flag && (has_ref || !cast.trim().is_empty())
}

/// Produce the shared reference image as a data URL: a user-supplied photo if given, else a
/// generated character portrait from the cast description. `None` if neither is available.
async fn build_reference(
    or: &OpenRouter,
    cast: &str,
    character_ref: Option<&Path>,
    dir: &Path,
) -> Option<String> {
    if let Some(p) = character_ref {
        return match std::fs::read(p) {
            Ok(bytes) => Some(openrouter::data_url_from_image(&bytes)),
            Err(e) => {
                eprintln!(
                    "  note: could not read --character-ref {}: {e}",
                    p.display()
                );
                None
            }
        };
    }

    println!("  building character reference for: {cast}");
    let prompt = format!(
        "A clear, well-lit reference photograph of {cast}. Plain neutral background, \
         sharp focus, subject centered and fully visible."
    );
    let img = generate_one(or, &prompt, cast, &[], "character reference").await?;
    let path = dir.join("character-ref.jpg");
    img.save(&path).ok()?;
    std::fs::read(&path)
        .ok()
        .map(|b| openrouter::data_url_from_image(&b))
}

/// Try to generate and crop one image, retrying on soft refusals / decode errors. `cast` and
/// `references` (data URLs of the anchor) steer the model toward a consistent recurring subject.
async fn generate_one(
    or: &OpenRouter,
    image_prompt: &str,
    cast: &str,
    references: &[String],
    label: &str,
) -> Option<RgbImage> {
    // An explicit instruction makes image-output models far less likely to reply with text.
    let mut prompt = String::from(
        "Generate one photorealistic vertical 9:16 photograph. Do not include any text, words, \
         captions, or watermarks in the image.",
    );
    if !references.is_empty() {
        // The reference is an identity lock, NOT a second subject. Without this, the model
        // tends to render the reference subject AND a fresh one, duplicating anatomy
        // (two heads / two tails), especially in compact poses.
        prompt.push_str(
            " The attached reference image defines ONLY the identity and appearance of the recurring \
             character — it is not an additional subject and must not be copied into the frame as a \
             second animal or person. Depict EXACTLY ONE of them, matching the reference (face, \
             fur/hair, markings, collar/clothing, age). Never duplicate the subject: no twins, no \
             extra or merged heads, tails, or limbs. Keep the identity fixed and change only the \
             setting, pose, and action. If the recurring character does not belong in this scene, \
             ignore the reference entirely.",
        );
    } else if !cast.trim().is_empty() {
        prompt.push_str(&format!(" Keep the recurring subject consistent: {cast}."));
    }
    prompt.push_str(&format!(" Scene: {image_prompt}"));

    for attempt in 1..=MAX_ATTEMPTS {
        match or.generate_image(&prompt, references).await {
            Ok(bytes) => match crop_to_vertical(&bytes) {
                Ok(img) => return Some(img),
                Err(e) => {
                    eprintln!("  {label}: decode/crop failed (try {attempt}/{MAX_ATTEMPTS}): {e}")
                }
            },
            Err(e) => eprintln!("  {label}: {e} (try {attempt}/{MAX_ATTEMPTS})"),
        }
    }
    None
}

/// Center-crop to 9:16 then resize to the final canvas.
fn crop_to_vertical(bytes: &[u8]) -> Result<RgbImage> {
    let img = image::load_from_memory(bytes)?.to_rgb8();
    let (w, h) = img.dimensions();
    let target = W as f64 / H as f64;
    let current = w as f64 / h as f64;

    let (cw, ch) = if current > target {
        ((h as f64 * target).round() as u32, h) // too wide -> trim sides
    } else {
        (w, (w as f64 / target).round() as u32) // too tall -> trim top/bottom
    };
    let x = (w - cw) / 2;
    let y = (h - ch) / 2;
    let cropped = imageops::crop_imm(&img, x, y, cw, ch).to_image();
    Ok(imageops::resize(
        &cropped,
        W,
        H,
        imageops::FilterType::Lanczos3,
    ))
}

/// A simple slate so a failed scene still renders something on-screen.
fn placeholder(idx: usize) -> RgbImage {
    let shades = [
        Rgb([30, 30, 40]),
        Rgb([40, 30, 45]),
        Rgb([30, 40, 45]),
        Rgb([45, 40, 30]),
    ];
    RgbImage::from_pixel(W, H, shades[idx % shades.len()])
}

#[cfg(test)]
mod tests {
    use super::consistency_enabled;

    #[test]
    fn consistency_decision() {
        assert!(!consistency_enabled(true, "", false)); // nothing to anchor to
        assert!(!consistency_enabled(true, "   ", false)); // whitespace cast counts as empty
        assert!(consistency_enabled(true, "a red-haired woman", false)); // cast present
        assert!(consistency_enabled(true, "", true)); // user-supplied reference
        assert!(!consistency_enabled(false, "a red-haired woman", true)); // disabled by flag
    }
}
