// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Turns a topic, an article, or a finished narration into a `Script`
//! (title + narration + scene image prompts) via one structured LLM call.

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model::{Entity, Scene, Script};
use crate::openrouter::OpenRouter;

/// System prompt shared by every script-generation entry point. It pins the format (length,
/// scene count, the requirement that each scene `line` be an exact in-order substring of the
/// narration so chunks reconcile against the audio), and asks for the auxiliary fields the rest of
/// the pipeline consumes: `image_prompt` (images.rs), `music_prompt` (music.rs), `cast`
/// (character consistency), `poster_prompt` (thumbnail), and `narrator_gender` (voice pick).
const STYLE: &str = "\
You write punchy short-form vertical (9:16) video scripts for TikTok/Reels.\n\
Rules:\n\
- Hook the viewer in the first 6-10 spoken words.\n\
- Keep the whole narration tight: roughly 50-110 words, conversational, no markdown, no stage directions.\n\
- NEVER use em-dashes or en-dashes (— or –). Use commas, periods, or rephrase instead. Ordinary \
hyphens inside hyphenated words are fine.\n\
- Break the narration into 3-6 scenes. Each scene's `line` MUST be an exact, in-order, \
non-overlapping substring chunk of the narration so the chunks concatenated equal the narration.\n\
- For each scene write a vivid `image_prompt` for a photographic still: concrete subject, \
vertical 9:16 framing, subject in the upper-middle two-thirds leaving room for captions at the bottom, \
cinematic documentary lighting. No text or words in the image. Every featured person MUST have their \
whole head and face within the frame — when people of different heights share a shot (e.g. an adult \
and a child), pull the camera back or frame to fit ALL their heads; never crop a featured person's \
head off at the top edge.\n\
- Each `image_prompt` MUST describe a SINGLE, unified photographic frame of REAL, SOLID subjects. \
NEVER request a split-screen, diptych, side-by-side, before/after, collage, triptych, or multi-panel \
image, and NEVER describe a ghostly, translucent, see-through, faded, overlaid, superimposed, \
duplicated, cloned, or doppelganger figure (e.g. an \"imagined\" or \"dream\" or \"perfect\" version \
of someone standing in the same frame as their real self) — those render as broken ghost/duplicate \
people. To contrast two ideas (e.g. imagination vs reality), use TWO SEPARATE scenes, one per idea, \
never a split frame and never two versions of the same person in one frame.\n\
- Keep each `image_prompt`'s camera viewpoint CONSISTENT with the details it asks to show: do not \
request a feature the chosen angle cannot see. If the shot is from BEHIND or the subject is walking \
AWAY, do not also describe front-only details (the face, eyes, or an item on the front/chest); if a \
front detail matters, put the camera in front. Contradictory viewpoints make the image model produce \
malformed, headless, or two-faced subjects.\n\
- Write a `music_prompt`: a short instrumental soundtrack description matching the mood — genre, \
tempo/BPM, key instruments, energy. Always instrumental, explicitly NO vocals (it plays under narration).\n\
- Write a `characters` list: one entry per person/animal that RECURS across two or more scenes. Give \
each a short stable `id` slug (e.g. \"man\", \"date\", \"puppy\") and a FULLY-SPECIFIED, canonical \
`description` that fixes EVERY visual detail so it can't drift: age, hair (colour, length, AND whether \
worn up or down), eyes, build, complexion, AND complete outfit. For sleeves, pin BOTH the sleeve length \
AND exactly how they are worn — pick ONE unambiguous state and state it (e.g. \"long sleeves worn down, \
buttoned at the wrist\" OR \"long sleeves rolled to the elbow\" OR \"short sleeves\") — never just \
\"long sleeves\", which the image model renders inconsistently (sometimes rolled, sometimes not). Do \
the same for any other adjustable garment detail (collar open/buttoned, jacket on/off). Example: \"woman \
~27, sleek black hair worn DOWN to the shoulders, warm tan complexion, sage-green wrap dress with \
three-quarter sleeves\" or \"man ~29, navy button-up shirt with long sleeves rolled to the elbow, slim \
dark-grey chinos\". The description fixes only STABLE identity and wardrobe — NEVER bake in \
transient state: no pose, no body or hand/arm/leg position, nothing the person is holding or doing, \
no gaze direction, and no facial expression (do NOT write things like \"one bare hand at her side, \
the other in her pocket\" or \"smiling\"). Those change every scene and belong in that scene's \
`image_prompt`; pinning them in the description makes every other scene read as \"wrong\". For an \
animal (or any subject) easily confused with a larger or different LOOKALIKE, pin the distinction \
with an explicit negative AND its size, e.g. \"a SMALL Shetland Sheepdog (Sheltie), compact build, \
NOT a larger Rough Collie / Lassie-type\" — the image model otherwise drifts toward the more common \
lookalike. If nothing specific recurs (abstract topic, landscapes, crowds), use an empty \
list. One-off people who appear in a single scene do NOT go here.\n\
- Write a `locations` list: one entry per place that RECURS across scenes (e.g. the restaurant). Give \
each a short stable `id` and a FULLY-SPECIFIED `description` fixing ONLY the FIXED setting: decor, \
architecture, furniture, materials, colour palette, and lighting (e.g. \"a warm bistro: exposed brick, \
brass pendant lights, bare dark-wood tables, matte-black chairs, candlelit, amber palette\"). Be \
UNAMBIGUOUS and NON-CONTRADICTORY about focal surfaces: state the table/seating surface exactly ONE way \
(e.g. \"bare dark-wood tables, NO tablecloths\" OR \"tables with white tablecloths\", never wording \
that implies both). Do NOT put TRANSIENT or movable tabletop items in the location description — no \
specific glasses, water levels, menus, plates, cutlery, food, or counts of them; those naturally \
change scene to scene, so listing them only makes every later scene look \"wrong\". Put any such \
per-scene prop in that scene's `image_prompt` instead. Reuse ONE location across scenes when the story \
stays in one place rather than inventing a new setting each beat. Empty list if there is no recurring place.\n\
- For each scene set `cast_ids`: the ids of the `characters` that actually appear in THAT scene's \
image (a subset, possibly empty). Set `location_id`: the id of the `locations` entry the scene is set \
in, or \"\" if none. When a scene includes a character, write that character's canonical traits into \
its `image_prompt` VERBATIM (do not paraphrase or change any detail). Other, non-recurring people in a \
scene are DIFFERENT individuals: give them their own distinct appearance in the `image_prompt`, clearly \
different from any recurring character, and never describe them as looking like one.\n\
- A recurring location's distinctive STRUCTURE (a specific bridge, building, or landmark) must only \
appear in scenes set in THAT location. If such a structure is visible in a scene, set that scene's \
`location_id` to the location that contains it (so it stays anchored to its reference and renders the \
same) — never show another recurring location's structure as unanchored BACKGROUND in a scene set \
elsewhere, or it will be reinvented differently. For a \"leaving X\" beat, either keep `location_id` = \
X, or frame the shot so X's structure is out of view and do not mention it in the `image_prompt`.\n\
- Keep recurring characters' presence CONTINUOUS within a location: once two characters are together \
in a setting (e.g. seated at the same table), include BOTH in `cast_ids` for EVERY scene set in that \
location. Do not drop a character in one beat and reintroduce them the next, and do not have someone \
appear or vanish mid-conversation.\n\
- Keep SEATING/POSITIONING consistent within a location: decide ONE fixed arrangement for the \
recurring characters there (e.g. \"Jake seated on the LEFT, Maya on the RIGHT\") and write that exact \
placement into the `image_prompt` of EVERY scene set in that location, so they never swap sides of the \
table between scenes.\n\
- For each scene set `transition` (how it enters from the PREVIOUS scene): \"dissolve\" for soft, \
continuous beats where a gentle cross-fade fits (time passing, dream/imagination, a mood shift, or \
staying in the same place), or \"cut\" for a sharp contrast or a new location. The FIRST scene must be \
\"cut\". Note: dissolves only render between two consecutive image stills, so use them for feel, not \
for pacing.\n\
- Write a `poster_prompt`: a single striking cover/thumbnail image concept for the whole reel, \
designed to entice clicks — one clear expressive focal subject, high contrast, emotionally engaging, \
broad appeal, vertical 9:16, no text or logos in the image. Feature the recurring cast if there is one.\n\
- Set `narrator_gender` to the narrator voice that best fits the story: \"male\", \"female\", or \
\"neutral\". Base it on the protagonist or tone (a story centered on a boy or man → \"male\"; a girl \
or woman → \"female\"; otherwise \"neutral\").";

/// JSON Schema for a from-scratch script (topic/brief/article): the model writes the `narration`
/// too. Passed to the LLM as a structured-output constraint so the reply deserializes straight
/// into [`Script`]. `additionalProperties: false` keeps the model from inventing extra fields.
fn full_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "narration": { "type": "string" },
            "scenes": {
                "type": "array",
                "items": scene_schema()
            },
            "music_prompt": { "type": "string" },
            "characters": entity_list_schema(),
            "locations": entity_list_schema(),
            "poster_prompt": { "type": "string" },
            "narrator_gender": { "type": "string", "enum": ["male", "female", "neutral"] }
        },
        "required": ["title", "narration", "scenes", "music_prompt", "characters", "locations", "poster_prompt", "narrator_gender"]
    })
}

/// Shared schema for one scene object (used by both the full and scenes-only schemas).
fn scene_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "line": { "type": "string" },
            "image_prompt": { "type": "string" },
            "cast_ids": { "type": "array", "items": { "type": "string" } },
            "location_id": { "type": "string" },
            "transition": { "type": "string", "enum": ["cut", "dissolve"] }
        },
        "required": ["line", "image_prompt", "cast_ids", "location_id", "transition"]
    })
}

/// Shared schema for a list of recurring entities (`characters` or `locations`).
fn entity_list_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "id": { "type": "string" },
                "description": { "type": "string" }
            },
            "required": ["id", "description"]
        }
    })
}

/// Scenes-only schema, used when the narration is fixed (user-supplied). Identical to
/// [`full_schema`] minus the `narration` field — the model only plans a title and scenes over
/// text it must not rewrite. Deserializes into [`ScenesOnly`].
fn scenes_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "scenes": {
                "type": "array",
                "items": scene_schema()
            },
            "music_prompt": { "type": "string" },
            "characters": entity_list_schema(),
            "locations": entity_list_schema(),
            "poster_prompt": { "type": "string" },
            "narrator_gender": { "type": "string", "enum": ["male", "female", "neutral"] }
        },
        "required": ["title", "scenes", "music_prompt", "characters", "locations", "poster_prompt", "narrator_gender"]
    })
}

/// Deserialization target for [`scenes_schema`] — every `Script` field except `narration`, which
/// the caller supplies verbatim. Reassembled into a full [`Script`] in [`from_narration`].
#[derive(Deserialize)]
struct ScenesOnly {
    title: String,
    scenes: Vec<Scene>,
    music_prompt: String,
    characters: Vec<Entity>,
    locations: Vec<Entity>,
    poster_prompt: String,
    narrator_gender: String,
}

/// Write a full script from a short topic.
pub async fn from_topic(or: &OpenRouter, topic: &str) -> Result<Script> {
    let user = format!("Write a short vertical-video script about this topic:\n\n{topic}");
    let script = or.chat_json(STYLE, &user, "script", full_schema()).await?;
    Ok(finalize(script))
}

/// Write a full script using a brief/notes file as the source material and direction.
pub async fn from_brief(or: &OpenRouter, brief: &str) -> Result<Script> {
    let user = format!(
        "Write a short vertical-video script based on the following notes/brief. Treat it as the \
         source material and creative direction:\n\n{brief}"
    );
    let script = or.chat_json(STYLE, &user, "script", full_schema()).await?;
    Ok(finalize(script))
}

/// Write a full script grounded in extracted article text.
pub async fn from_article(or: &OpenRouter, text: &str) -> Result<Script> {
    let user = format!(
        "Write a short vertical-video script that captures the most surprising idea in this article. \
         Stay faithful to its facts.\n\nARTICLE:\n{text}"
    );
    let script = or.chat_json(STYLE, &user, "script", full_schema()).await?;
    Ok(finalize(script))
}

/// Post-process a generated script so the narration and captions never contain dashes the AI
/// likes to over-use. (Belt-and-suspenders with the prompt rule above.)
fn finalize(mut script: Script) -> Script {
    script.narration = remove_dashes(&script.narration);
    for scene in &mut script.scenes {
        scene.line = remove_dashes(&scene.line);
    }
    // Drop phantom scenes: the model sometimes appends an extra scene with an empty `line` AND empty
    // `image_prompt`. It covers no narration (so it gets ~no time window) and, having no subject or
    // references, makes the image model hallucinate an unrelated frame that ALSO skips validation
    // (no references to judge against). Remove any such scene — but never empty the list outright
    // (all-blank output is a catastrophic model failure better surfaced downstream than masked).
    let before = script.scenes.len();
    let kept: Vec<Scene> = script
        .scenes
        .iter()
        .filter(|s| !(s.line.trim().is_empty() && s.image_prompt.trim().is_empty()))
        .cloned()
        .collect();
    if !kept.is_empty() && kept.len() < before {
        eprintln!(
            "  note: dropped {} empty scene(s) the scriptwriter appended",
            before - kept.len()
        );
        script.scenes = kept;
    }
    // Fold any legacy single-cast string into `characters` (no-op for fresh multi-character runs).
    script.normalize_entities();
    script
}

/// Replace em/en dashes (and the horizontal bar) with a comma break, then tidy spacing and
/// punctuation. Leaves ordinary hyphens and existing commas (e.g. "1,000") untouched.
fn remove_dashes(text: &str) -> String {
    let mut s = text.to_string();
    for d in ["—", "–", "―"] {
        s = s.replace(&format!(" {d} "), ", "); // spaced dash → comma break
        s = s.replace(d, ", "); // any remaining (unspaced or one-sided)
    }
    // Tidy artifacts from the substitution only.
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s = s.replace(" ,", ",");
    while s.contains(", ,") {
        s = s.replace(", ,", ", ");
    }
    while s.contains(",,") {
        s = s.replace(",,", ",");
    }
    for (bad, good) in [(", .", ". "), (", !", "! "), (", ?", "? ")] {
        s = s.replace(bad, good);
    }
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim().trim_end_matches(',').trim().to_string()
}

/// Use the user's narration verbatim; only plan a title and scene image prompts for it.
pub async fn from_narration(or: &OpenRouter, narration: &str) -> Result<Script> {
    let system = format!(
        "{STYLE}\n\nThe narration is FIXED and given to you. Do NOT rewrite it. \
         Only produce a title and the scenes that cover it."
    );
    let user = format!(
        "NARRATION (use exactly as written, split into scene `line` chunks):\n\n{narration}"
    );
    let plan: ScenesOnly = or
        .chat_json(&system, &user, "scenes", scenes_schema())
        .await?;
    Ok(finalize(Script {
        title: plan.title,
        narration: narration.to_string(),
        scenes: plan.scenes,
        music_prompt: plan.music_prompt,
        characters: plan.characters,
        locations: plan.locations,
        cast: String::new(),
        poster_prompt: plan.poster_prompt,
        narrator_gender: plan.narrator_gender,
    }))
}

#[cfg(test)]
mod tests {
    use super::remove_dashes;
    use crate::model::{Scene, Script};

    #[test]
    fn old_scene_json_deserializes_with_empty_entity_refs() {
        // Back-compat: a `script.json` predating multi-character support carries `features_cast`
        // (now ignored) and no `cast_ids`/`location_id`. It must still deserialize — resume reuses
        // existing images, so empty per-scene refs are harmless.
        let s: Scene =
            serde_json::from_str(r#"{"line":"hi","image_prompt":"a city","features_cast":true}"#)
                .unwrap();
        assert!(s.cast_ids.is_empty());
        assert_eq!(s.location_id, "");
        // New-format scenes round-trip their entity references.
        let s: Scene = serde_json::from_str(
            r#"{"line":"hi","image_prompt":"a city","cast_ids":["man","date"],"location_id":"bistro"}"#,
        )
        .unwrap();
        assert_eq!(s.cast_ids, ["man", "date"]);
        assert_eq!(s.location_id, "bistro");
    }

    #[test]
    fn legacy_cast_string_folds_into_characters() {
        // A legacy `cast` string is migrated into a single character so old runs keep one anchor.
        let mut script: Script = serde_json::from_str(
            r#"{"title":"t","narration":"n","scenes":[],"music_prompt":"m","cast":"a woman ~30"}"#,
        )
        .unwrap();
        assert!(script.characters.is_empty());
        script.normalize_entities();
        assert_eq!(script.characters.len(), 1);
        assert_eq!(script.characters[0].id, "main");
        assert_eq!(script.characters[0].description, "a woman ~30");
    }

    #[test]
    fn finalize_drops_phantom_scenes() {
        // A scene with an empty `line` AND empty `image_prompt` is a phantom the model sometimes
        // appends; it covers no narration and renders an unrelated frame, so it must be dropped.
        let script: Script = serde_json::from_str(
            r#"{"title":"t","narration":"hello world","music_prompt":"m","scenes":[
                {"line":"hello world","image_prompt":"a vivid frame"},
                {"line":"","image_prompt":""}
            ]}"#,
        )
        .unwrap();
        let out = super::finalize(script);
        assert_eq!(out.scenes.len(), 1, "phantom scene should be dropped");
        assert_eq!(out.scenes[0].line, "hello world");

        // A scene with content in EITHER field is kept (don't drop real scenes).
        let keep: Script = serde_json::from_str(
            r#"{"title":"t","narration":"n","music_prompt":"m","scenes":[{"line":"","image_prompt":"a city skyline"}]}"#,
        )
        .unwrap();
        assert_eq!(super::finalize(keep).scenes.len(), 1);

        // Never empty the list, even if every scene is blank (catastrophic output, surfaced later).
        let all_blank: Script = serde_json::from_str(
            r#"{"title":"t","narration":"","music_prompt":"m","scenes":[{"line":"","image_prompt":""}]}"#,
        )
        .unwrap();
        assert_eq!(
            super::finalize(all_blank).scenes.len(),
            1,
            "must not empty the scene list"
        );
    }

    #[test]
    fn strips_em_and_en_dashes() {
        assert_eq!(remove_dashes("a — b"), "a, b"); // spaced em dash
        assert_eq!(remove_dashes("wait—what"), "wait, what"); // unspaced
        assert_eq!(remove_dashes("range 5–10 wide"), "range 5, 10 wide"); // en dash
        assert_eq!(remove_dashes("end—. Next"), "end. Next"); // dash before period
        assert_eq!(remove_dashes("trailing—"), "trailing"); // trailing dash
                                                            // ordinary hyphens and numeric commas are left alone
        assert_eq!(
            remove_dashes("a well-known 1,000 ft drop"),
            "a well-known 1,000 ft drop"
        );
        assert!(!remove_dashes("one—two—three").contains('—'));
    }
}
