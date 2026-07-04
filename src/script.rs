// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Turns a topic, an article, or a finished narration into a `Script`
//! (title + narration + scene image prompts) via one structured LLM call.

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model::{Entity, Scene, Script};
use crate::openrouter::OpenRouter;

/// The reel-specific head of the scriptwriter prompt: short-form/vertical intro, hook, length
/// and scene-count budgets, and the vertical image framing rule. Joined with [`SHARED_RULES`]
/// and [`REEL_TAIL`] by [`reel_style`] — the pieces reproduce the original prompt byte-for-byte.
const REEL_HEAD: &str = "\
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
head off at the top edge.\n";

/// The aspect-neutral scene/entity consistency rules shared by every script-generation prompt
/// (reel and youtube alike): unified single frame, viewpoint coherence, music, the canonical
/// character/location lists, per-scene entity references, continuity/seating, transitions, and
/// motion direction. Kept as one block so the rules can never drift between formats.
const SHARED_RULES: &str = "\
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
- Do NOT write an `image_prompt` that depends on legible ON-SCREEN CONTENT: a software UI, form, \
dashboard, app window, web page, spreadsheet, chart, code, or any readable text shown on a screen. \
Image models render these as garbled gibberish and frequently put the picture on the WRONG face of \
the device (a glowing screen on the back or edge of a monitor). Convey software, technology, or \
digital frustration through the PERSON'S expression, posture, and the environment, not by showing \
what is on the screen. A screen MAY appear in frame, but describe its display only as a soft, \
out-of-focus glow or an abstract colour blur, never specific interface elements or words; keep the \
subject's ACTION about the person (typing, sighing, rubbing their temples), not about rendering the \
interface. Example: instead of \"typing into a rigid AI form that does not fit her screen\", write \
\"sitting stiffly, typing with a tense, defeated expression, the monitor casting a cool out-of-focus \
glow on her face\".\n\
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
- For each scene write a `motion_prompt`: one or two sentences of video direction for animating that \
scene's still — ONE gentle camera move (a slow push-in, drift, pan, or hold) plus the subject's small \
physical action for this beat (e.g. \"slow push-in as the dog tilts its head, ears lifting\"). It must \
stay consistent with the `image_prompt`'s viewpoint and describe MOTION ONLY: never introduce new \
people, props, text, or a location change, and no dialogue or lip-synced speech (the narration plays \
over the clip).\n";

/// The reel-specific tail: vertical poster concept and narrator-voice pick.
const REEL_TAIL: &str = "\
- Write a `poster_prompt`: a single striking cover/thumbnail image concept for the whole reel, \
designed to entice clicks — one clear expressive focal subject, high contrast, emotionally engaging, \
broad appeal, vertical 9:16, no text or logos in the image. Feature the recurring cast if there is one.\n\
- Set `narrator_gender` to the narrator voice that best fits the story: \"male\", \"female\", or \
\"neutral\". Base it on the protagonist or tone (a story centered on a boy or man → \"male\"; a girl \
or woman → \"female\"; otherwise \"neutral\").";

/// The reel scriptwriter system prompt (the original single `STYLE` constant, reassembled from
/// its pieces byte-for-byte).
fn reel_style() -> String {
    format!("{REEL_HEAD}{SHARED_RULES}{REEL_TAIL}")
}

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
            "transition": { "type": "string", "enum": ["cut", "dissolve"] },
            "motion_prompt": { "type": "string" }
        },
        "required": ["line", "image_prompt", "cast_ids", "location_id", "transition", "motion_prompt"]
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
    let script = or
        .chat_json(&reel_style(), &user, "script", full_schema())
        .await?;
    Ok(finalize(script))
}

/// Write a full script using a brief/notes file as the source material and direction.
pub async fn from_brief(or: &OpenRouter, brief: &str) -> Result<Script> {
    let user = format!(
        "Write a short vertical-video script based on the following notes/brief. Treat it as the \
         source material and creative direction:\n\n{brief}"
    );
    let script = or
        .chat_json(&reel_style(), &user, "script", full_schema())
        .await?;
    Ok(finalize(script))
}

/// Write a full script grounded in extracted article text.
pub async fn from_article(or: &OpenRouter, text: &str) -> Result<Script> {
    let user = format!(
        "Write a short vertical-video script that captures the most surprising idea in this article. \
         Stay faithful to its facts.\n\nARTICLE:\n{text}"
    );
    let script = or
        .chat_json(&reel_style(), &user, "script", full_schema())
        .await?;
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
///
/// The scene `line`s must reassemble to the narration (caption and scene timing silently drift
/// when the model paraphrases), so the result is verified; a mismatch gets one corrective retry,
/// then falls back to proportional chunking (keeping the visual plan) rather than failing the run.
pub async fn from_narration(or: &OpenRouter, narration: &str) -> Result<Script> {
    let system = format!(
        "{}\n\nThe narration is FIXED and given to you. Do NOT rewrite it. \
         Only produce a title and the scenes that cover it.",
        reel_style()
    );
    let user = format!(
        "NARRATION (use exactly as written, split into scene `line` chunks):\n\n{narration}"
    );
    let plan: ScenesOnly = or
        .chat_json(&system, &user, "scenes", scenes_schema())
        .await?;
    let mut script = finalize(assemble_script(plan, narration));
    if lines_reassemble(&script.narration, &script.scenes) {
        return Ok(script);
    }

    // Corrective retry: show the model its own bad chunking so it can fix it.
    eprintln!(
        "  note: scene lines did not reassemble to the narration; asking the model to re-chunk"
    );
    let got: Vec<&str> = script.scenes.iter().map(|s| s.line.as_str()).collect();
    let retry_user = format!(
        "{user}\n\nYour previous attempt produced scene `line`s that did NOT concatenate back to \
         the narration (you paraphrased, dropped, or reordered words): {got:?}. Each `line` MUST \
         be an exact, in-order, non-overlapping chunk of the narration so the chunks concatenated \
         equal the narration word for word. Re-chunk now."
    );
    match or
        .chat_json::<ScenesOnly>(&system, &retry_user, "scenes", scenes_schema())
        .await
    {
        Ok(plan) => {
            let retried = finalize(assemble_script(plan, narration));
            if lines_reassemble(&retried.narration, &retried.scenes) {
                return Ok(retried);
            }
            script = retried; // still wrong, but likely the fresher visual plan
        }
        Err(e) => eprintln!("  note: re-chunk attempt failed ({e:#})"),
    }

    // Last resort: keep the visual plan, replace the lines with even word-boundary chunks so
    // captions and cuts stay roughly aligned instead of silently drifting.
    eprintln!(
        "  warning: scene lines still don't match the narration; \
         falling back to proportional chunks (scene timing will be approximate)"
    );
    let chunks = proportional_chunks(&script.narration, script.scenes.len());
    for (scene, chunk) in script.scenes.iter_mut().zip(chunks) {
        scene.line = chunk;
    }
    Ok(script)
}

/// Reassemble a scenes-only plan plus the fixed narration into a full [`Script`].
fn assemble_script(plan: ScenesOnly, narration: &str) -> Script {
    Script {
        title: plan.title,
        narration: narration.to_string(),
        scenes: plan.scenes,
        music_prompt: plan.music_prompt,
        characters: plan.characters,
        locations: plan.locations,
        cast: String::new(),
        poster_prompt: plan.poster_prompt,
        narrator_gender: plan.narrator_gender,
        format: String::new(),
        chapters: Vec::new(),
        description: String::new(),
        tags: Vec::new(),
    }
}

/// Whether the scene `line`s concatenate back to the narration, comparing with whitespace
/// collapsed (chunk boundaries legitimately gain/lose spaces and newlines).
fn lines_reassemble(narration: &str, scenes: &[Scene]) -> bool {
    let joined: String = scenes
        .iter()
        .map(|s| s.line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    normalize_ws(&joined) == normalize_ws(narration)
}

/// Collapse every whitespace run to a single space and trim the ends.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split `narration` at word boundaries into `n` contiguous chunks of near-equal word counts.
/// With more chunks than words, the surplus chunks come back empty (their scenes get ~no time).
fn proportional_chunks(narration: &str, n: usize) -> Vec<String> {
    let words: Vec<&str> = narration.split_whitespace().collect();
    let n = n.max(1);
    let mut out = Vec::with_capacity(n);
    let mut start = 0;
    for i in 0..n {
        let end = (words.len() * (i + 1)) / n;
        out.push(words[start..end].join(" "));
        start = end;
    }
    out
}

// ---------------------------------------------------------------------------
// Long-form YouTube generation: one outline call, then one scenes call per chapter.
// ---------------------------------------------------------------------------

/// System prompt for the youtube outline call: long-form structure (chapters, description,
/// tags, landscape thumbnail) plus the shared entity-canon rules — the characters/locations
/// written here are the canon every chapter call receives verbatim.
fn youtube_outline_style(minutes: f64) -> String {
    let (w_lo, w_hi) = crate::config::word_budget(minutes);
    let n = crate::config::chapter_count(minutes);
    let per_ch = ((w_lo + w_hi) / 2) / n.max(1);
    format!(
        "You outline engaging long-form landscape (16:9) YouTube videos.\n\
         Rules:\n\
         - Structure the video as exactly {n} chapters, each with a short punchy `title` (it \
         becomes a YouTube chapter marker) and a 1-2 sentence `summary` of what that chapter \
         covers. Together they must form one coherent arc: the first chapter hooks and frames \
         the topic, the middle chapters each develop ONE distinct idea, and the last chapter \
         lands a satisfying payoff or conclusion.\n\
         - Plan for roughly {w_lo}-{w_hi} narration words total (~{per_ch} words per chapter), \
         conversational, no markdown, no stage directions. The narration itself is written \
         later, chapter by chapter, from your summaries.\n\
         - NEVER use em-dashes or en-dashes (— or –) anywhere. Use commas, periods, or rephrase.\n\
         {SHARED_RULES}\
         - Write a `poster_prompt`: a single striking landscape 16:9 YouTube thumbnail concept \
         — one clear expressive focal subject, high contrast, bold composition readable at small \
         sizes, no text or logos in the image. Feature the recurring cast if there is one.\n\
         - Set `narrator_gender` to the narrator voice that best fits the story: \"male\", \
         \"female\", or \"neutral\".\n\
         - Write a YouTube `description`: 2-3 short paragraphs, the hook first, no hashtags.\n\
         - Write 10-15 YouTube `tags` (single words or short phrases, no # signs).\n\
         NOTE: the scene-level rules above describe how scenes will later reference your \
         `characters`/`locations` canon — you produce only the outline fields here, no scenes."
    )
}

/// System prompt for one youtube chapter call: writes that chapter's narration and its scenes,
/// under the shared consistency rules and the outline's fixed entity canon.
fn youtube_chapter_style(
    per_chapter_words: (usize, usize),
    per_chapter_scenes: (usize, usize),
) -> String {
    let (w_lo, w_hi) = per_chapter_words;
    let (s_lo, s_hi) = per_chapter_scenes;
    format!(
        "You write one chapter of a long-form landscape (16:9) YouTube video: the chapter's \
         spoken narration plus the scenes that visualize it.\n\
         Rules:\n\
         - Write roughly {w_lo}-{w_hi} narration words for THIS chapter, conversational, no \
         markdown, no stage directions.\n\
         - NEVER use em-dashes or en-dashes (— or –). Use commas, periods, or rephrase instead. \
         Ordinary hyphens inside hyphenated words are fine.\n\
         - Break the chapter narration into {s_lo}-{s_hi} scenes at roughly 7-9 seconds of \
         speech each (about 17-22 words per scene). Each scene's `line` MUST be an exact, \
         in-order, non-overlapping substring chunk of the chapter narration so the chunks \
         concatenated equal it.\n\
         - For each scene write a vivid `image_prompt` for a photographic still: concrete \
         subject, landscape 16:9 framing, subject centered with a little headroom and room for \
         lower-third captions at the bottom, cinematic documentary lighting. No text or words in \
         the image. Every featured person MUST have their whole head and face within the frame.\n\
         {SHARED_RULES}\
         - The `characters` and `locations` canon is FIXED and given to you — reference it via \
         `cast_ids`/`location_id` and copy descriptions into `image_prompt`s VERBATIM, but do \
         NOT invent new recurring characters or locations, and do NOT produce a music prompt, \
         poster, or any outline field. Produce ONLY this chapter's `narration` and `scenes`."
    )
}

/// The outline call's structured-output schema. With `fixed_narration`, each chapter also
/// carries its exact narration chunk (the `--script` chapterization path).
fn outline_schema(fixed_narration: bool) -> Value {
    let chapter = if fixed_narration {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "title": { "type": "string" },
                "summary": { "type": "string" },
                "narration": { "type": "string" }
            },
            "required": ["title", "summary", "narration"]
        })
    } else {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "title": { "type": "string" },
                "summary": { "type": "string" }
            },
            "required": ["title", "summary"]
        })
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "chapters": { "type": "array", "items": chapter },
            "characters": entity_list_schema(),
            "locations": entity_list_schema(),
            "music_prompt": { "type": "string" },
            "poster_prompt": { "type": "string" },
            "narrator_gender": { "type": "string", "enum": ["male", "female", "neutral"] },
            "description": { "type": "string" },
            "tags": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["title", "chapters", "characters", "locations", "music_prompt",
                      "poster_prompt", "narrator_gender", "description", "tags"]
    })
}

/// One chapter call's structured-output schema: the chapter narration plus its scenes.
fn chapter_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "narration": { "type": "string" },
            "scenes": { "type": "array", "items": scene_schema() }
        },
        "required": ["narration", "scenes"]
    })
}

/// Deserialization target for [`outline_schema`].
#[derive(Deserialize)]
struct Outline {
    title: String,
    chapters: Vec<OutlineChapter>,
    characters: Vec<Entity>,
    locations: Vec<Entity>,
    music_prompt: String,
    poster_prompt: String,
    narrator_gender: String,
    description: String,
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct OutlineChapter {
    title: String,
    summary: String,
    /// Present only on the fixed-narration (chapterization) path.
    #[serde(default)]
    narration: String,
}

/// Deserialization target for [`chapter_schema`].
#[derive(Deserialize)]
struct ChapterScenes {
    narration: String,
    scenes: Vec<Scene>,
}

/// Write a long-form chaptered script from a short topic.
pub async fn youtube_from_topic(or: &OpenRouter, topic: &str, minutes: f64) -> Result<Script> {
    let user = format!("Outline a long-form YouTube video about this topic:\n\n{topic}");
    youtube_script(or, &user, minutes).await
}

/// Write a long-form chaptered script from a brief/notes file.
pub async fn youtube_from_brief(or: &OpenRouter, brief: &str, minutes: f64) -> Result<Script> {
    let user = format!(
        "Outline a long-form YouTube video based on the following notes/brief. Treat it as the \
         source material and creative direction:\n\n{brief}"
    );
    youtube_script(or, &user, minutes).await
}

/// Write a long-form chaptered script grounded in extracted article text.
pub async fn youtube_from_article(or: &OpenRouter, text: &str, minutes: f64) -> Result<Script> {
    let user = format!(
        "Outline a long-form YouTube video that develops the most interesting ideas in this \
         article. Stay faithful to its facts.\n\nARTICLE:\n{text}"
    );
    youtube_script(or, &user, minutes).await
}

/// The two-stage youtube flow: one outline call, then one scenes call per chapter (sequential —
/// each chapter sees the tail of the previously written narration for flow continuity).
async fn youtube_script(or: &OpenRouter, outline_user: &str, minutes: f64) -> Result<Script> {
    println!("  outlining ~{minutes:.0}-minute video ...");
    let outline: Outline = or
        .chat_json(
            &youtube_outline_style(minutes),
            outline_user,
            "outline",
            outline_schema(false),
        )
        .await?;
    if outline.chapters.is_empty() {
        anyhow::bail!("outline call returned no chapters");
    }

    let (w_lo, w_hi) = crate::config::word_budget(minutes);
    let (s_lo, s_hi) = crate::config::scene_budget(minutes);
    let n = outline.chapters.len();
    let per_chapter = (w_lo / n, (w_hi / n).max(w_lo / n + 10));
    let per_scenes = ((s_lo / n).max(2), (s_hi / n).max(s_lo / n + 1).max(3));
    let style = youtube_chapter_style(per_chapter, per_scenes);

    let mut chapters: Vec<crate::model::Chapter> = Vec::with_capacity(n);
    let mut scenes: Vec<Scene> = Vec::new();
    let mut prev_tail = String::new();
    for idx in 0..n {
        println!(
            "  writing chapter {}/{n}: {} ...",
            idx + 1,
            outline.chapters[idx].title
        );
        let user = chapter_user_message(&outline, idx, &prev_tail, None);
        let mut cs: ChapterScenes = or
            .chat_json(&style, &user, "chapter", chapter_schema())
            .await?;
        clean_chapter(&mut cs);
        verify_chapter_chunking(or, &style, &user, &mut cs, None).await;
        prev_tail = tail_words(&cs.narration, 40);
        chapters.push(crate::model::Chapter {
            title: outline.chapters[idx].title.clone(),
            summary: outline.chapters[idx].summary.clone(),
            narration: cs.narration.clone(),
            scene_start: scenes.len(),
            scene_count: cs.scenes.len(),
        });
        scenes.extend(cs.scenes);
    }

    Ok(assemble_youtube_script(outline, chapters, scenes))
}

/// Use the user's narration verbatim in youtube format: stage 1 chapterizes the fixed narration
/// (exact, in-order chunks — verified, with a proportional fallback), stage 2 plans scenes over
/// each fixed chunk.
pub async fn youtube_from_narration(
    or: &OpenRouter,
    narration: &str,
    minutes: f64,
) -> Result<Script> {
    let system = format!(
        "{}\n\nThe narration is FIXED and given to you. Do NOT rewrite it. Each chapter's \
         `narration` MUST be an exact, in-order, non-overlapping chunk of it, so the chapter \
         narrations concatenated equal the given narration word for word.",
        youtube_outline_style(minutes)
    );
    let user = format!(
        "NARRATION (use exactly as written, split into chapter `narration` chunks):\n\n{narration}"
    );
    println!("  chapterizing fixed narration ...");
    let mut outline: Outline = or
        .chat_json(&system, &user, "outline", outline_schema(true))
        .await?;
    if outline.chapters.is_empty() {
        anyhow::bail!("chapterization returned no chapters");
    }
    // The fixed narration must survive chapterization intact (dash cleanup applies to both
    // sides identically, so compare post-cleanup).
    let narration = remove_dashes(narration);
    for ch in &mut outline.chapters {
        ch.narration = remove_dashes(&ch.narration);
    }
    let chunks: Vec<&str> = outline
        .chapters
        .iter()
        .map(|c| c.narration.as_str())
        .collect();
    if !chunks_reassemble(&narration, &chunks) {
        eprintln!(
            "  note: chapter chunks did not reassemble to the narration; \
             splitting evenly instead (chapter boundaries will be approximate)"
        );
        let even = proportional_chunks(&narration, outline.chapters.len());
        for (ch, chunk) in outline.chapters.iter_mut().zip(even) {
            ch.narration = chunk;
        }
    }

    // Stage 2: scenes over each fixed chapter chunk.
    let n = outline.chapters.len();
    let words_per_ch = narration.split_whitespace().count() / n.max(1);
    let per_chapter = (words_per_ch, words_per_ch + 10);
    let per_scenes = ((words_per_ch / 22).max(2), (words_per_ch / 17).max(3));
    let style = youtube_chapter_style(per_chapter, per_scenes);
    let mut chapters: Vec<crate::model::Chapter> = Vec::with_capacity(n);
    let mut scenes: Vec<Scene> = Vec::new();
    for idx in 0..n {
        println!(
            "  planning scenes for chapter {}/{n}: {} ...",
            idx + 1,
            outline.chapters[idx].title
        );
        let fixed = outline.chapters[idx].narration.clone();
        let user = chapter_user_message(&outline, idx, "", Some(&fixed));
        let mut cs: ChapterScenes = or
            .chat_json(&style, &user, "chapter", chapter_schema())
            .await?;
        cs.narration = fixed.clone(); // the chunk is fixed regardless of what the model echoed
        clean_chapter(&mut cs);
        verify_chapter_chunking(or, &style, &user, &mut cs, Some(&fixed)).await;
        chapters.push(crate::model::Chapter {
            title: outline.chapters[idx].title.clone(),
            summary: outline.chapters[idx].summary.clone(),
            narration: cs.narration.clone(),
            scene_start: scenes.len(),
            scene_count: cs.scenes.len(),
        });
        scenes.extend(cs.scenes);
    }

    Ok(assemble_youtube_script(outline, chapters, scenes))
}

/// The user message for one chapter call: the full outline (this chapter marked), the fixed
/// entity canon, the previous chapter's narration tail, and position cues. `fixed_narration`
/// switches to the plan-scenes-only wording for the `--script` path.
fn chapter_user_message(
    outline: &Outline,
    idx: usize,
    prev_tail: &str,
    fixed_narration: Option<&str>,
) -> String {
    let mut msg = format!("VIDEO: {}\n\nCHAPTERS:\n", outline.title);
    for (i, ch) in outline.chapters.iter().enumerate() {
        let marker = if i == idx { " <-- WRITE THIS ONE" } else { "" };
        msg.push_str(&format!(
            "{}. {} — {}{}\n",
            i + 1,
            ch.title,
            ch.summary,
            marker
        ));
    }
    if !outline.characters.is_empty() {
        msg.push_str("\nCHARACTER CANON (fixed, copy descriptions verbatim):\n");
        for c in &outline.characters {
            msg.push_str(&format!("[{}] {}\n", c.id, c.description));
        }
    }
    if !outline.locations.is_empty() {
        msg.push_str("\nLOCATION CANON (fixed, copy descriptions verbatim):\n");
        for l in &outline.locations {
            msg.push_str(&format!("[{}] {}\n", l.id, l.description));
        }
    }
    if !prev_tail.is_empty() {
        msg.push_str(&format!(
            "\nThe previous chapter's narration ends with: \"...{prev_tail}\" — continue the \
             flow from there.\n"
        ));
    }
    if idx == 0 {
        msg.push_str("\nThis is the FIRST chapter: hook the viewer in the first 6-10 words.\n");
    } else if idx == outline.chapters.len() - 1 {
        msg.push_str(
            "\nThis is the LAST chapter: do not re-hook or re-introduce the topic; land the \
             conclusion and a satisfying final beat.\n",
        );
    } else {
        msg.push_str(
            "\nThis is a MIDDLE chapter: do not re-hook or re-introduce the topic; develop this \
             chapter's idea and hand off toward the next chapter.\n",
        );
    }
    match fixed_narration {
        Some(text) => msg.push_str(&format!(
            "\nTHIS CHAPTER'S NARRATION (FIXED — copy it into `narration` exactly as written and \
             split it into scene `line` chunks):\n\n{text}"
        )),
        None => msg.push_str(&format!(
            "\nWrite chapter {} now: its narration and its scenes.",
            idx + 1
        )),
    }
    msg
}

/// Per-chapter cleanup mirroring `finalize`: dash removal on narration + lines, phantom-scene
/// drop (a scene with an empty `line` AND empty `image_prompt`).
fn clean_chapter(cs: &mut ChapterScenes) {
    cs.narration = remove_dashes(&cs.narration);
    for s in &mut cs.scenes {
        s.line = remove_dashes(&s.line);
    }
    let before = cs.scenes.len();
    let kept: Vec<Scene> = cs
        .scenes
        .iter()
        .filter(|s| !(s.line.trim().is_empty() && s.image_prompt.trim().is_empty()))
        .cloned()
        .collect();
    if !kept.is_empty() && kept.len() < before {
        eprintln!(
            "  note: dropped {} empty scene(s) from the chapter",
            before - kept.len()
        );
        cs.scenes = kept;
    }
}

/// Verify a chapter's scene `line`s chunk its narration exactly; on mismatch retry once with a
/// corrective message, then fall back to proportional chunks. `fixed` pins the narration on the
/// verbatim-`--script` path (the model's echoed narration is ignored there).
async fn verify_chapter_chunking(
    or: &OpenRouter,
    system: &str,
    user: &str,
    cs: &mut ChapterScenes,
    fixed: Option<&str>,
) {
    if lines_reassemble(&cs.narration, &cs.scenes) {
        return;
    }
    eprintln!("  note: chapter scene lines did not reassemble to its narration; retrying");
    let got: Vec<&str> = cs.scenes.iter().map(|s| s.line.as_str()).collect();
    let retry_user = format!(
        "{user}\n\nYour previous attempt produced scene `line`s that did NOT concatenate back to \
         the chapter narration (you paraphrased, dropped, or reordered words): {got:?}. Each \
         `line` MUST be an exact, in-order, non-overlapping chunk of the chapter narration. \
         Rewrite the chapter now with correct chunks."
    );
    if let Ok(mut retried) = or
        .chat_json::<ChapterScenes>(system, &retry_user, "chapter", chapter_schema())
        .await
    {
        if let Some(f) = fixed {
            retried.narration = f.to_string();
        }
        clean_chapter(&mut retried);
        if lines_reassemble(&retried.narration, &retried.scenes) {
            *cs = retried;
            return;
        }
        *cs = retried; // still wrong, but the fresher plan
    }
    eprintln!(
        "  warning: chapter lines still don't match; falling back to proportional chunks \
         (scene timing in this chapter will be approximate)"
    );
    let chunks = proportional_chunks(&cs.narration, cs.scenes.len());
    for (scene, chunk) in cs.scenes.iter_mut().zip(chunks) {
        scene.line = chunk;
    }
}

/// Whether contiguous `chunks` concatenate back to `narration` (whitespace collapsed).
fn chunks_reassemble(narration: &str, chunks: &[&str]) -> bool {
    let joined = chunks
        .iter()
        .map(|c| c.trim())
        .collect::<Vec<_>>()
        .join(" ");
    normalize_ws(&joined) == normalize_ws(narration)
}

/// The last `n` words of `text` (for cross-chapter flow continuity in the next prompt).
fn tail_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let start = words.len().saturating_sub(n);
    words[start..].join(" ")
}

/// Assemble the outline + written chapters into the final chaptered [`Script`].
fn assemble_youtube_script(
    outline: Outline,
    chapters: Vec<crate::model::Chapter>,
    scenes: Vec<Scene>,
) -> Script {
    let narration = chapters
        .iter()
        .map(|c| c.narration.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut script = Script {
        title: outline.title,
        narration,
        scenes,
        music_prompt: outline.music_prompt,
        characters: outline.characters,
        locations: outline.locations,
        cast: String::new(),
        poster_prompt: outline.poster_prompt,
        narrator_gender: outline.narrator_gender,
        format: "youtube".to_string(),
        chapters,
        description: outline.description,
        tags: outline.tags,
    };
    script.normalize_entities();
    script
}

#[cfg(test)]
mod tests {
    use super::{
        chunks_reassemble, lines_reassemble, proportional_chunks, reel_style, remove_dashes,
        tail_words, youtube_chapter_style, youtube_outline_style,
    };
    use crate::model::{Scene, Script};

    #[test]
    fn reel_style_is_the_original_prompt() {
        // The prompt was split into REEL_HEAD/SHARED_RULES/REEL_TAIL; spot-check the joined
        // result still carries each region's rules in the original order.
        let s = reel_style();
        assert!(s.starts_with("You write punchy short-form vertical (9:16) video scripts"));
        let unified = s.find("SINGLE, unified photographic frame").unwrap();
        let chars = s.find("Write a `characters` list").unwrap();
        let motion = s.find("write a `motion_prompt`").unwrap();
        let poster = s.find("Write a `poster_prompt`").unwrap();
        assert!(unified < chars && chars < motion && motion < poster);
        assert!(s.ends_with("or woman → \"female\"; otherwise \"neutral\")."));
    }

    #[test]
    fn youtube_prompts_embed_shared_rules_and_budgets() {
        let outline = youtube_outline_style(3.0);
        // Budgets flow in: 3 min → 3 chapters, 370-500 words.
        assert!(outline.contains("exactly 3 chapters"));
        assert!(outline.contains("370-500 narration words"));
        // The shared entity rules are present, and the landscape poster replaces the vertical one.
        assert!(outline.contains("Write a `characters` list"));
        assert!(outline.contains("landscape 16:9 YouTube thumbnail"));
        assert!(!outline.contains("vertical 9:16, no text or logos"));

        let chapter = youtube_chapter_style((120, 160), (5, 8));
        assert!(chapter.contains("roughly 120-160 narration words"));
        assert!(chapter.contains("into 5-8 scenes"));
        assert!(chapter.contains("landscape 16:9 framing"));
        assert!(chapter.contains("SINGLE, unified photographic frame"));
        assert!(chapter.contains("NOT invent new recurring characters"));
    }

    #[test]
    fn old_script_json_defaults_new_longform_fields() {
        // A pre-longform script.json (no format/chapters/description/tags) must still resume.
        let s: Script =
            serde_json::from_str(r#"{"title":"t","narration":"n","scenes":[],"music_prompt":"m"}"#)
                .unwrap();
        assert_eq!(s.format, "");
        assert!(s.chapters.is_empty());
        assert_eq!(s.description, "");
        assert!(s.tags.is_empty());
        // Chapters round-trip through serialization.
        let ch: crate::model::Chapter = serde_json::from_str(
            r#"{"title":"Intro","narration":"hello there","scene_start":0,"scene_count":3}"#,
        )
        .unwrap();
        assert_eq!(ch.summary, ""); // summary is optional
        let json = serde_json::to_string(&ch).unwrap();
        assert!(json.contains("\"scene_count\":3"));
    }

    #[test]
    fn chunks_reassemble_matches_lines_semantics() {
        let narration = "one two three four";
        assert!(chunks_reassemble(narration, &["one two", "three four"]));
        assert!(!chunks_reassemble(narration, &["one two", "four three"]));
        assert!(!chunks_reassemble(narration, &["one two"]));
    }

    #[test]
    fn tail_words_takes_the_last_n() {
        assert_eq!(tail_words("a b c d e", 2), "d e");
        assert_eq!(tail_words("a b", 5), "a b");
        assert_eq!(tail_words("", 3), "");
    }

    fn scene_with_line(line: &str) -> Scene {
        serde_json::from_str(&format!(
            r#"{{"line":{},"image_prompt":"x"}}"#,
            serde_json::to_string(line).unwrap()
        ))
        .unwrap()
    }

    #[test]
    fn motion_prompt_defaults_empty_and_round_trips() {
        // Older script.json (no motion_prompt) still deserializes — resume compatibility.
        let s: Scene = serde_json::from_str(r#"{"line":"hi","image_prompt":"a city"}"#).unwrap();
        assert_eq!(s.motion_prompt, "");
        // When present it round-trips through serialization.
        let s: Scene = serde_json::from_str(
            r#"{"line":"hi","image_prompt":"a city","motion_prompt":"slow push-in"}"#,
        )
        .unwrap();
        assert_eq!(s.motion_prompt, "slow push-in");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("slow push-in"));
    }

    #[test]
    fn reassembly_tolerates_whitespace_but_rejects_paraphrase() {
        let narration = "He found a mitten.  He carried it\nhome.";
        // Exact chunks with different whitespace at the joins reassemble fine.
        let ok = vec![
            scene_with_line("He found a mitten."),
            scene_with_line("He carried it home."),
        ];
        assert!(lines_reassemble(narration, &ok));
        // A paraphrased chunk does not.
        let bad = vec![
            scene_with_line("He found a small mitten."),
            scene_with_line("He carried it home."),
        ];
        assert!(!lines_reassemble(narration, &bad));
        // A dropped chunk does not.
        assert!(!lines_reassemble(
            narration,
            &[scene_with_line("He found a mitten.")]
        ));
    }

    #[test]
    fn proportional_chunks_cover_narration_without_overlap() {
        let text = "one two three four five six seven";
        let chunks = proportional_chunks(text, 3);
        assert_eq!(chunks.len(), 3);
        // Contiguous, non-overlapping, and jointly equal to the narration.
        assert_eq!(chunks.join(" "), text);
        // More chunks than words: surplus chunks are empty, words never duplicated.
        let chunks = proportional_chunks("a b", 4);
        assert_eq!(chunks.len(), 4);
        let joined: Vec<&str> = chunks
            .iter()
            .filter(|c| !c.is_empty())
            .map(|s| s.as_str())
            .collect();
        assert_eq!(joined.join(" "), "a b");
        // n = 0 is clamped to one chunk.
        assert_eq!(proportional_chunks("a b", 0), vec!["a b".to_string()]);
    }

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
