//! Turns a topic, an article, or a finished narration into a `Script`
//! (title + narration + scene image prompts) via one structured LLM call.

use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::model::{Scene, Script};
use crate::openrouter::OpenRouter;

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
cinematic documentary lighting. No text or words in the image.\n\
- Write a `music_prompt`: a short instrumental soundtrack description matching the mood — genre, \
tempo/BPM, key instruments, energy. Always instrumental, explicitly NO vocals (it plays under narration).\n\
- Write a `cast`: if a specific person or animal recurs through the story, describe them and their \
FIXED visual traits for consistency (e.g. \"a woman ~30, curly red hair, freckles, olive-green jacket\" \
or \"a golden retriever puppy, fluffy, red collar\"); feature them across scenes. If nothing specific \
recurs (abstract topic, landscapes, crowds), set `cast` to an empty string.\n\
- Write a `poster_prompt`: a single striking cover/thumbnail image concept for the whole reel, \
designed to entice clicks — one clear expressive focal subject, high contrast, emotionally engaging, \
broad appeal, vertical 9:16, no text or logos in the image. Feature the recurring cast if there is one.\n\
- Set `narrator_gender` to the narrator voice that best fits the story: \"male\", \"female\", or \
\"neutral\". Base it on the protagonist or tone (a story centered on a boy or man → \"male\"; a girl \
or woman → \"female\"; otherwise \"neutral\").";

fn full_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "narration": { "type": "string" },
            "scenes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "line": { "type": "string" },
                        "image_prompt": { "type": "string" }
                    },
                    "required": ["line", "image_prompt"]
                }
            },
            "music_prompt": { "type": "string" },
            "cast": { "type": "string" },
            "poster_prompt": { "type": "string" },
            "narrator_gender": { "type": "string", "enum": ["male", "female", "neutral"] }
        },
        "required": ["title", "narration", "scenes", "music_prompt", "cast", "poster_prompt", "narrator_gender"]
    })
}

/// Scenes-only schema, used when the narration is fixed (user-supplied).
fn scenes_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "scenes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "line": { "type": "string" },
                        "image_prompt": { "type": "string" }
                    },
                    "required": ["line", "image_prompt"]
                }
            },
            "music_prompt": { "type": "string" },
            "cast": { "type": "string" },
            "poster_prompt": { "type": "string" },
            "narrator_gender": { "type": "string", "enum": ["male", "female", "neutral"] }
        },
        "required": ["title", "scenes", "music_prompt", "cast", "poster_prompt", "narrator_gender"]
    })
}

#[derive(Deserialize)]
struct ScenesOnly {
    title: String,
    scenes: Vec<Scene>,
    music_prompt: String,
    cast: String,
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
        cast: plan.cast,
        poster_prompt: plan.poster_prompt,
        narrator_gender: plan.narrator_gender,
    }))
}

#[cfg(test)]
mod tests {
    use super::remove_dashes;

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
