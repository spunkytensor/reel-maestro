// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! `youtube.md` — upload-ready YouTube metadata for a long-form run: title, description, tags,
//! and chapter timestamps computed from the final scene durations.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::assemble;
use crate::model::Script;

/// Format seconds as a YouTube chapter timestamp: `M:SS` under an hour, `H:MM:SS` above.
fn yt_timestamp(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The `youtube.md` document body. `windows` are the chapter `[start, end)` windows on the
/// final timeline (from [`assemble::chapter_windows`]).
fn youtube_markdown(script: &Script, windows: &[(f64, f64)]) -> String {
    let mut s = format!("# {}\n\n", script.title);
    if !script.description.trim().is_empty() {
        s.push_str(script.description.trim());
        s.push_str("\n\n");
    }
    if !script.chapters.is_empty() {
        s.push_str("## Chapters\n\n");
        for (ch, &(start, _)) in script.chapters.iter().zip(windows) {
            s.push_str(&format!("{} {}\n", yt_timestamp(start), ch.title));
        }
        s.push('\n');
    }
    if !script.tags.is_empty() {
        s.push_str(&format!("Tags: {}\n", script.tags.join(", ")));
    }
    s
}

/// Write `youtube.md` into the run folder. Non-fatal by design — the caller treats a failure
/// as a note, never a run abort.
pub fn write_youtube_md(dir: &Path, script: &Script, durations: &[f64]) -> Result<PathBuf> {
    let windows = assemble::chapter_windows(&script.chapters, durations);
    let path = dir.join("youtube.md");
    std::fs::write(&path, youtube_markdown(script, &windows))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{youtube_markdown, yt_timestamp};
    use crate::model::{Chapter, Script};

    #[test]
    fn timestamps_follow_youtube_format() {
        assert_eq!(yt_timestamp(0.0), "0:00");
        assert_eq!(yt_timestamp(75.5), "1:16"); // rounded
        assert_eq!(yt_timestamp(600.0), "10:00");
        assert_eq!(yt_timestamp(3661.0), "1:01:01");
    }

    #[test]
    fn markdown_lists_chapters_at_their_windows() {
        let chapter = |title: &str, start: usize, count: usize| Chapter {
            title: title.into(),
            summary: String::new(),
            narration: String::new(),
            scene_start: start,
            scene_count: count,
        };
        let script: Script = serde_json::from_str(
            r#"{"title":"T","narration":"n","scenes":[],"music_prompt":"m",
                "description":"A story.","tags":["dogs","winter"]}"#,
        )
        .unwrap();
        let mut script = script;
        script.chapters = vec![chapter("Intro", 0, 2), chapter("Payoff", 2, 2)];
        let md = youtube_markdown(&script, &[(0.0, 65.0), (65.0, 130.0)]);
        assert!(md.starts_with("# T\n\nA story.\n\n"));
        assert!(md.contains("0:00 Intro\n"));
        assert!(md.contains("1:05 Payoff\n"));
        assert!(md.contains("Tags: dogs, winter"));
    }
}
