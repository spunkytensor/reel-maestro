// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Scene prompts -> 1080x1920 JPEG stills. Runs generations concurrently (bounded) and
//! falls back to a solid placeholder frame so one bad generation never kills the run.
//!
//! Consistency works by building a small set of shared **reference images** up front — one
//! portrait per recurring character and one establishing still per recurring location — then
//! conditioning each scene on the references for the entities it actually contains. The same
//! canonical text description is also injected into every scene featuring an entity, so even a
//! missing reference image still pins the unspecified details (hairstyle, sleeve length, decor).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::stream::{self, StreamExt};
use image::{imageops, Rgb, RgbImage};

use crate::model::{Entity, Scene};
use crate::openrouter::{self, OpenRouter};

const W: u32 = 1080; // final canvas width  (9:16 vertical)
const H: u32 = 1920; // final canvas height (9:16 vertical)
const MAX_CONCURRENT: usize = 4; // in-flight image generations — caps load on the API/our memory
const MAX_ATTEMPTS: usize = 3; // per-image retries before falling back to a placeholder

/// One attached reference image plus a human label of what it anchors. References are listed in
/// the prompt in the SAME order they're attached so the model can map each image to its identity.
#[derive(Clone)]
struct Reference {
    label: String,
    data_url: String,
}

/// Shared, read-only context for rendering one scene, bundled so the per-scene helper and the
/// two-phase fan-out don't pass a dozen positional args around.
struct SceneCtx<'a> {
    or: &'a OpenRouter,
    chars: &'a [Entity],
    char_urls: &'a HashMap<String, String>,
    loc_urls: &'a HashMap<String, String>,
    locations: &'a [Entity],
    forced_all: bool,
    dir: &'a Path,
}

/// Generate one image per scene into `dir`, returning their paths in scene order.
///
/// When consistency is enabled, a shared reference image is built first for every recurring
/// character (a portrait) and location (an establishing still), and each scene is conditioned on
/// the references for the entities it lists (`cast_ids` / `location_id`) so people and places stay
/// the same across the reel.
pub async fn generate(
    or: &OpenRouter,
    scenes: &[Scene],
    characters: &[Entity],
    locations: &[Entity],
    character_ref: Option<&Path>,
    consistency: bool,
    dir: &Path,
) -> Result<Vec<PathBuf>> {
    // Build the shared, per-entity reference images once.
    let (effective_chars, char_urls, loc_urls, forced_all) = if consistency {
        build_references(or, characters, locations, character_ref, dir).await
    } else {
        (Vec::new(), HashMap::new(), HashMap::new(), false)
    };
    let ctx = SceneCtx {
        or,
        chars: &effective_chars,
        char_urls: &char_urls,
        loc_urls: &loc_urls,
        locations,
        forced_all,
        dir,
    };
    let ctx = &ctx;

    // Sequential same-location chaining. The first scene using each location is its "anchor",
    // rendered first; the location's other scenes are then conditioned on the anchor's ACTUAL
    // image so fine props (table surface, place settings, decor) propagate forward instead of
    // being reinvented — the establishing reference fixes the room, the anchor fixes the details.
    let anchor_of = location_anchors(scenes);
    let anchor_of = &anchor_of;
    // Phase 1 = scenes with no chain dependency: location-less scenes and each location's anchor.
    let is_phase1 = |i: usize| {
        let lid = scenes[i].location_id.trim();
        lid.is_empty() || anchor_of.get(lid) == Some(&i)
    };

    let n = scenes.len();
    let mut out: Vec<Option<PathBuf>> = (0..n).map(|_| None).collect();

    // Phase 1: anchors + location-less scenes, fully concurrent.
    let done1: Vec<(usize, PathBuf)> = stream::iter((0..n).filter(|&i| is_phase1(i)))
        .map(|i| async move { (i, render_scene(ctx, i, &scenes[i], &[]).await) })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;
    for (i, p) in done1 {
        out[i] = Some(p);
    }

    // Read each location's rendered anchor back as a data URL to chain onto its remaining scenes.
    let mut anchor_url: HashMap<String, String> = HashMap::new();
    for (lid, &ai) in anchor_of {
        if let Some(Some(path)) = out.get(ai) {
            if let Ok(bytes) = std::fs::read(path) {
                anchor_url.insert(lid.clone(), openrouter::data_url_from_image(&bytes));
            }
        }
    }
    let anchor_url = &anchor_url;

    // Phase 2: each location's remaining scenes, conditioned on that location's anchor image.
    let done2: Vec<(usize, PathBuf)> = stream::iter((0..n).filter(|&i| !is_phase1(i)))
        .map(|i| async move {
            let chained: Vec<Reference> = anchor_url
                .get(scenes[i].location_id.trim())
                .map(|url| {
                    vec![Reference {
                        label: "PRIOR PHOTO of this exact location — copy ONLY its room, table \
                                surface, table settings, props, furniture, lighting, and background \
                                patrons. IGNORE which people sit at the table in it and their count \
                                and positions; the people present are defined solely by the PERSON \
                                references above"
                            .to_string(),
                        data_url: url.clone(),
                    }]
                })
                .unwrap_or_default();
            (i, render_scene(ctx, i, &scenes[i], &chained).await)
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;
    for (i, p) in done2 {
        out[i] = Some(p);
    }

    Ok(out.into_iter().map(|p| p.expect("every scene rendered")).collect())
}

/// Render one scene to `scene-NN.jpg` and return its path. Conditions on the recurring entities it
/// contains (people + location references), plus any `chained` references (a same-location anchor).
/// Always leaves a usable file on disk — a placeholder if generation or saving fails.
async fn render_scene(ctx: &SceneCtx<'_>, i: usize, scene: &Scene, chained: &[Reference]) -> PathBuf {
    let path = ctx.dir.join(format!("scene-{i:02}.jpg"));

    // Which recurring characters appear here: the scene's own ids, or — when the user pinned a
    // --character-ref but the script declared no characters — the forced protagonist in every scene.
    let ids: Vec<&str> = if ctx.forced_all {
        ctx.chars.iter().map(|c| c.id.as_str()).collect()
    } else {
        scene.cast_ids.iter().map(|s| s.as_str()).collect()
    };
    let people: Vec<&Entity> = ids
        .iter()
        .filter_map(|id| ctx.chars.iter().find(|c| c.id == *id))
        .collect();
    let location: Option<&Entity> = if scene.location_id.trim().is_empty() {
        None
    } else {
        ctx.locations.iter().find(|l| l.id == scene.location_id)
    };

    // Attach references in a stable order: each present person, the location, then any chained anchor.
    let mut references: Vec<Reference> = Vec::new();
    for p in &people {
        if let Some(url) = ctx.char_urls.get(&p.id) {
            references.push(Reference {
                label: person_label(p),
                data_url: url.clone(),
            });
        }
    }
    if let Some(loc) = location {
        if let Some(url) = ctx.loc_urls.get(&loc.id) {
            references.push(Reference {
                label: format!("LOCATION ({})", loc.description),
                data_url: url.clone(),
            });
        }
    }
    references.extend(chained.iter().cloned());

    let img = match generate_one(
        ctx.or,
        &scene.image_prompt,
        &people,
        location,
        &references,
        &format!("scene {i}"),
    )
    .await
    {
        Some(img) => img,
        None => {
            eprintln!("  scene {i}: image generation failed after {MAX_ATTEMPTS} tries; using placeholder");
            placeholder(i)
        }
    };
    if let Err(e) = img.save(&path) {
        eprintln!("  scene {i}: saving image failed ({e}); writing placeholder");
        let _ = placeholder(i).save(&path);
    }
    path
}

/// Generate a custom cover/thumbnail image from `prompt`, conditioned on `references` (the
/// protagonist's portrait) and steered by the protagonist's canonical `description` so the
/// poster's cast matches the reel. Returns its path, or `None` if generation fails.
pub async fn generate_poster(
    or: &OpenRouter,
    prompt: &str,
    protagonist: &str,
    references: &[String],
    dir: &Path,
) -> Option<PathBuf> {
    let person = Entity {
        id: "protagonist".to_string(),
        description: protagonist.to_string(),
    };
    let people: Vec<&Entity> = if protagonist.trim().is_empty() {
        Vec::new()
    } else {
        vec![&person]
    };
    let refs: Vec<Reference> = references
        .iter()
        .map(|url| Reference {
            label: person_label(&person),
            data_url: url.clone(),
        })
        .collect();
    let img = generate_one(or, prompt, &people, None, &refs, "poster").await?;
    let path = dir.join("poster.jpg");
    img.save(&path).ok()?;
    Some(path)
}

/// Label for a person reference, including its canonical description when present.
fn person_label(p: &Entity) -> String {
    if p.description.trim().is_empty() {
        format!("PERSON \"{}\"", p.id)
    } else {
        format!("PERSON \"{}\" ({})", p.id, p.description)
    }
}

/// Build the shared reference images for every recurring character and location. Returns the
/// effective character list (which may include a synthesized protagonist when a `--character-ref`
/// photo is given without any declared characters), the id→data-URL maps for characters and
/// locations, and whether the protagonist is forced into every scene.
async fn build_references(
    or: &OpenRouter,
    characters: &[Entity],
    locations: &[Entity],
    character_ref: Option<&Path>,
    dir: &Path,
) -> (
    Vec<Entity>,
    HashMap<String, String>,
    HashMap<String, String>,
    bool,
) {
    let mut effective: Vec<Entity> = characters.to_vec();
    let mut forced_all = false;
    // A user-pinned photo with no declared characters → one forced protagonist across all scenes.
    if character_ref.is_some() && effective.is_empty() {
        effective.push(Entity {
            id: "main".to_string(),
            description: String::new(),
        });
        forced_all = true;
    }

    let mut char_urls: HashMap<String, String> = HashMap::new();
    for (i, c) in effective.iter().enumerate() {
        // The user photo overrides the FIRST character's reference; the rest are generated.
        let photo = if i == 0 { character_ref } else { None };
        match build_character_ref(or, c, photo, dir).await {
            Some(url) => {
                // Mirror the primary character to the legacy `character-ref.jpg` the poster reads.
                if i == 0 {
                    mirror_primary(dir, c, photo);
                }
                char_urls.insert(c.id.clone(), url);
            }
            None => eprintln!(
                "  note: no reference for character \"{}\"; scenes will rely on its text description",
                c.id
            ),
        }
    }

    let mut loc_urls: HashMap<String, String> = HashMap::new();
    for l in locations {
        match build_location_ref(or, l, dir).await {
            Some(url) => {
                loc_urls.insert(l.id.clone(), url);
            }
            None => eprintln!(
                "  note: no reference for location \"{}\"; scenes will rely on its text description",
                l.id
            ),
        }
    }

    (effective, char_urls, loc_urls, forced_all)
}

/// Copy the primary character's reference to `character-ref.jpg` (the name the poster step reads),
/// so the poster matches the reel's protagonist on both generated-portrait and `--character-ref` runs.
fn mirror_primary(dir: &Path, primary: &Entity, photo: Option<&Path>) {
    let legacy = dir.join("character-ref.jpg");
    let portrait = dir.join(format!("character-{}.jpg", slug(&primary.id)));
    if portrait.exists() {
        let _ = std::fs::copy(&portrait, &legacy);
    } else if let Some(p) = photo {
        let _ = std::fs::copy(p, &legacy);
    }
}

/// Produce a recurring character's reference portrait as a data URL: a user-supplied photo if
/// given, else a generated portrait from its canonical description. Saved as `character-<id>.jpg`.
async fn build_character_ref(
    or: &OpenRouter,
    entity: &Entity,
    photo: Option<&Path>,
    dir: &Path,
) -> Option<String> {
    if let Some(p) = photo {
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

    println!(
        "  building character reference \"{}\": {}",
        entity.id, entity.description
    );
    let prompt = format!(
        "A clear, well-lit reference photograph of {}. Plain neutral background, \
         sharp focus, subject centered and fully visible.",
        entity.description
    );
    let img = generate_one(
        or,
        &prompt,
        &[],
        None,
        &[],
        &format!("character \"{}\"", entity.id),
    )
    .await?;
    let path = dir.join(format!("character-{}.jpg", slug(&entity.id)));
    img.save(&path).ok()?;
    std::fs::read(&path)
        .ok()
        .map(|b| openrouter::data_url_from_image(&b))
}

/// Produce a recurring location's establishing reference (no people) as a data URL, generated from
/// its canonical description. Saved as `location-<id>.jpg`.
async fn build_location_ref(or: &OpenRouter, entity: &Entity, dir: &Path) -> Option<String> {
    println!(
        "  building location reference \"{}\": {}",
        entity.id, entity.description
    );
    let prompt = format!(
        "A clear establishing photograph of this location with NO people in frame, with the \
         location's main repeated furniture/setting (e.g. a representative two-person table) shown \
         clearly in the foreground. Render every repeated element identically and EXACTLY as \
         described — same surfaces, same settings, same props — so the image is an internally \
         consistent, unambiguous reference: {}. Vertical 9:16, sharp focus, cinematic lighting.",
        entity.description
    );
    let img = generate_one(
        or,
        &prompt,
        &[],
        None,
        &[],
        &format!("location \"{}\"", entity.id),
    )
    .await?;
    let path = dir.join(format!("location-{}.jpg", slug(&entity.id)));
    img.save(&path).ok()?;
    std::fs::read(&path)
        .ok()
        .map(|b| openrouter::data_url_from_image(&b))
}

/// Assemble the text prompt for one image generation. Pure (no I/O) so the branch logic is
/// unit-testable. `people`/`location` are the recurring entities in this scene (for the canonical
/// text lock); `references` are the attached anchor images, listed in attachment order.
fn build_image_prompt(
    image_prompt: &str,
    people: &[&Entity],
    location: Option<&Entity>,
    references: &[Reference],
) -> String {
    // An explicit instruction makes image-output models far less likely to reply with text.
    let mut prompt = String::from(
        "Generate one photorealistic vertical 9:16 photograph. Do not include any text, words, \
         captions, or watermarks in the image. Render a SINGLE, unified, full-frame photograph — \
         never a split-screen, diptych, side-by-side, before/after, collage, grid, triptych, or \
         multi-panel composition. Every person must be fully and solidly rendered: no translucent, \
         ghosted, faded, doubled, or partially-formed figures, and no posters, mirrors, or \
         reflections that read as extra people.",
    );

    if !references.is_empty() {
        // List each attached reference in order, then lock identities/setting and forbid the
        // classic failure modes (cloning the anchor as a second subject, merging/duplicating
        // people, or applying a recurring identity to a one-off stranger).
        prompt.push_str(" Attached reference images, in order: ");
        for (i, r) in references.iter().enumerate() {
            prompt.push_str(&format!("{}) {}; ", i + 1, r.label));
        }
        prompt.push_str(
            "Match each attached reference EXACTLY — for every PERSON keep the same face, hair, \
             build, age, and complete outfit (including sleeve length); for the LOCATION keep the \
             same decor, materials, colour palette, and lighting. Depict each listed person exactly \
             once: never duplicate or merge them (no twins, no extra or merged heads, limbs, or \
             tails), and do not copy a reference in as an additional subject. Anyone NOT in the \
             reference list is a DIFFERENT individual — give them a clearly distinct face, hair, \
             build, and clothing, and do not apply a reference identity to them.",
        );
    }

    // Canonical text lock — pins the fixed traits even when a reference image is missing.
    let described: Vec<&&Entity> = people
        .iter()
        .filter(|p| !p.description.trim().is_empty())
        .collect();
    if !described.is_empty() {
        prompt.push_str(" Recurring people in this scene, keep EXACTLY consistent: ");
        for p in described {
            prompt.push_str(&format!("[{}] {}; ", p.id, p.description));
        }
    }
    if let Some(loc) = location {
        if !loc.description.trim().is_empty() {
            prompt.push_str(&format!(
                " Setting, keep EXACTLY consistent: {}.",
                loc.description
            ));
        }
    }

    prompt.push_str(&format!(" Scene: {image_prompt}"));
    prompt
}

/// Try to generate and crop one image, retrying on soft refusals / decode errors. `people`,
/// `location`, and `references` steer the model toward consistent recurring entities.
async fn generate_one(
    or: &OpenRouter,
    image_prompt: &str,
    people: &[&Entity],
    location: Option<&Entity>,
    references: &[Reference],
    label: &str,
) -> Option<RgbImage> {
    let prompt = build_image_prompt(image_prompt, people, location, references);
    let ref_urls: Vec<String> = references.iter().map(|r| r.data_url.clone()).collect();

    for attempt in 1..=MAX_ATTEMPTS {
        match or.generate_image(&prompt, &ref_urls).await {
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

/// Map each recurring location id to its "anchor" scene — the first (lowest-index) scene set there.
/// The anchor renders first; the location's other scenes are chained onto its actual image so fine
/// props propagate. Scenes with no `location_id` are excluded (they have no chain dependency).
fn location_anchors(scenes: &[Scene]) -> HashMap<String, usize> {
    let mut anchor_of: HashMap<String, usize> = HashMap::new();
    for (i, scene) in scenes.iter().enumerate() {
        let lid = scene.location_id.trim();
        if !lid.is_empty() {
            anchor_of.entry(lid.to_string()).or_insert(i);
        }
    }
    anchor_of
}

/// Sanitize a model-generated entity id into a filesystem-safe filename stem.
fn slug(id: &str) -> String {
    let s: String = id
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "x".to_string()
    } else {
        s
    }
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
    use super::{build_image_prompt, location_anchors, slug, Reference};
    use crate::model::{Entity, Scene};

    fn ent(id: &str, desc: &str) -> Entity {
        Entity {
            id: id.to_string(),
            description: desc.to_string(),
        }
    }

    #[test]
    fn prompt_lists_references_in_order_and_locks_each() {
        let man = ent("man", "a man ~28, dark wavy hair");
        let date = ent("date", "a woman ~27, black hair in a low bun");
        let people = vec![&man, &date];
        let refs = vec![
            Reference {
                label: "PERSON \"man\" (a man ~28)".to_string(),
                data_url: "x".to_string(),
            },
            Reference {
                label: "PERSON \"date\" (a woman ~27)".to_string(),
                data_url: "y".to_string(),
            },
        ];
        let p = build_image_prompt("they laugh at a table", &people, None, &refs);
        assert!(p.contains("Attached reference images, in order"));
        assert!(p.contains("1) PERSON \"man\""));
        assert!(p.contains("2) PERSON \"date\""));
        assert!(p.contains("DIFFERENT individual"));
        // Canonical text lock repeats the fixed traits.
        assert!(p.contains("keep EXACTLY consistent"));
        assert!(p.contains("black hair in a low bun"));
        assert!(p.contains("Scene: they laugh at a table"));
    }

    #[test]
    fn prompt_injects_location_text_lock() {
        let loc = ent("bistro", "exposed brick, brass lights, amber palette");
        let p = build_image_prompt("a candlelit table", &[], Some(&loc), &[]);
        assert!(p.contains(
            "Setting, keep EXACTLY consistent: exposed brick, brass lights, amber palette"
        ));
        // No references attached -> no reference block.
        assert!(!p.contains("Attached reference images"));
    }

    #[test]
    fn prompt_without_entities_is_independent() {
        // No recurring people/location and no references: a plain, independent generation.
        let p = build_image_prompt("a bustling crowd", &[], None, &[]);
        assert!(!p.contains("Attached reference images"));
        assert!(!p.contains("keep EXACTLY consistent"));
        assert!(p.contains("Scene: a bustling crowd"));
    }

    #[test]
    fn location_anchor_is_first_scene_per_location() {
        let sc = |loc: &str| Scene {
            line: String::new(),
            image_prompt: String::new(),
            cast_ids: Vec::new(),
            location_id: loc.to_string(),
        };
        // scenes: [none, none, bistro, bistro, bistro] → bistro's anchor is index 2.
        let scenes = vec![sc(""), sc(""), sc("bistro"), sc("bistro"), sc("bistro")];
        let anchors = location_anchors(&scenes);
        assert_eq!(anchors.get("bistro"), Some(&2));
        assert_eq!(anchors.len(), 1); // location-less scenes don't create anchors
    }

    #[test]
    fn slug_is_filesystem_safe() {
        assert_eq!(slug("man"), "man");
        assert_eq!(slug("the date"), "the-date");
        assert_eq!(slug("  weird/id!  "), "weird-id");
        assert_eq!(slug("///"), "x");
    }
}
