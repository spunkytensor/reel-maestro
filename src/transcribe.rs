// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! audio.mp3 -> word timings. Prefers real word-level timestamps from a local
//! whisper-timestamped run (OpenRouter's hosted STT only returns plain text — verified across
//! all of its transcription models). If that tool isn't installed or fails, estimates timings
//! from the audio duration and the spoken words so the pipeline still produces synced captions.
//!
//! Caption *text* always comes from the narration script (which is authoritative), while the
//! *timing* comes from whisper. Whisper can mishear the TTS audio ("calm as" -> "Kalm is"), so
//! we align its timed tokens to the narration words and emit the narration spelling carrying
//! whisper's timestamps — best of both: correct text, real timing.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::config::Config;
use crate::ffmpeg;
use crate::model::WordTiming;

pub fn word_timings(
    cfg: &Config,
    audio: &Path,
    narration: &str,
    debug_out: &Path,
) -> Result<Vec<WordTiming>> {
    // 1. Real timings from local whisper-timestamped, re-texted to the narration script so
    //    captions show the correct spelling rather than whisper's ASR guesses.
    let words = match local_word_timings(&cfg.whisper_cmd, &cfg.whisper_model, audio) {
        Ok(w) if !w.is_empty() => align_text_to_timings(narration, &w),
        Ok(_) => {
            eprintln!(
                "  note: {} produced no word timings; estimating instead",
                cfg.whisper_cmd
            );
            estimate_from_audio(audio, narration)?
        }
        Err(e) => {
            eprintln!(
                "  note: local word timing via `{}` unavailable ({e}); estimating instead",
                cfg.whisper_cmd
            );
            estimate_from_audio(audio, narration)?
        }
    };

    std::fs::write(debug_out, serde_json::to_vec_pretty(&words)?)?;
    Ok(words)
}

fn estimate_from_audio(audio: &Path, narration: &str) -> Result<Vec<WordTiming>> {
    let dur = ffmpeg::duration_s(audio)?;
    eprintln!("  note: estimating word timings from audio length ({dur:.1}s)");
    Ok(estimate(narration, dur))
}

/// Re-text whisper's timed tokens with the authoritative narration words. We align the two
/// token streams (whisper may mishear, split, merge, or drop words) and emit one timing per
/// narration word, carrying the timestamp of the whisper token it aligns to. Narration words
/// with no whisper match get their time interpolated from their neighbours.
fn align_text_to_timings(narration: &str, timed: &[WordTiming]) -> Vec<WordTiming> {
    let narr: Vec<&str> = narration.split_whitespace().collect();
    if narr.is_empty() {
        return timed.to_vec();
    }
    if timed.is_empty() {
        return Vec::new();
    }

    // Per narration word, the [start, end] of its aligned whisper token (None if unaligned).
    let aligned = needleman_wunsch(&narr, timed);

    // Fill unaligned narration words by interpolating across the gap between known anchors.
    let lo = 0.0_f64.min(timed[0].start_s);
    let hi = timed.iter().map(|w| w.end_s).fold(lo, f64::max);
    let n = narr.len();
    let mut spans: Vec<(f64, f64)> = vec![(0.0, 0.0); n];
    let mut i = 0;
    while i < n {
        if let Some(t) = aligned[i] {
            spans[i] = t;
            i += 1;
            continue;
        }
        // Run of unaligned words [i, j).
        let j = (i..n).find(|&k| aligned[k].is_some()).unwrap_or(n);
        let left = if i > 0 { spans[i - 1].1 } else { lo };
        let right = if j < n { aligned[j].unwrap().0 } else { hi };
        let span = (right - left).max(0.0);
        let cnt = (j - i) as f64;
        for (k, idx) in (i..j).enumerate() {
            let a = left + span * (k as f64) / cnt;
            let b = left + span * ((k + 1) as f64) / cnt;
            spans[idx] = (a, b);
        }
        i = j;
    }

    narr.iter()
        .zip(&spans)
        .map(|(w, &(start_s, end_s))| WordTiming {
            word: w.to_string(),
            start_s,
            end_s,
        })
        .collect()
}

/// Needleman–Wunsch global alignment of narration words against whisper's timed tokens
/// (compared on a normalized form: lowercased, alphanumerics only). Returns, per narration
/// word, the [start, end] of the whisper token it aligns to, or `None` if it aligns to a gap.
fn needleman_wunsch(narr: &[&str], timed: &[WordTiming]) -> Vec<Option<(f64, f64)>> {
    const MATCH: i32 = 2;
    const MISMATCH: i32 = -1;
    const GAP: i32 = -2;

    let a: Vec<String> = narr.iter().map(|w| normalize(w)).collect();
    let b: Vec<String> = timed.iter().map(|w| normalize(&w.word)).collect();
    let (n, m) = (a.len(), b.len());

    // Score matrix.
    let mut score = vec![vec![0i32; m + 1]; n + 1];
    for (i, row) in score.iter_mut().enumerate().take(n + 1) {
        row[0] = i as i32 * GAP;
    }
    for (j, cell) in score[0].iter_mut().enumerate().take(m + 1) {
        *cell = j as i32 * GAP;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub = if a[i - 1] == b[j - 1] {
                MATCH
            } else {
                MISMATCH
            };
            let diag = score[i - 1][j - 1] + sub;
            let up = score[i - 1][j] + GAP; // narration word unaligned
            let left = score[i][j - 1] + GAP; // whisper token unaligned
            score[i][j] = diag.max(up).max(left);
        }
    }

    // Traceback, recording each narration word's aligned whisper token.
    let mut out = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let sub = if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            MATCH
        } else {
            MISMATCH
        };
        if i > 0 && j > 0 && score[i][j] == score[i - 1][j - 1] + sub {
            out[i - 1] = Some((timed[j - 1].start_s, timed[j - 1].end_s));
            i -= 1;
            j -= 1;
        } else if i > 0 && score[i][j] == score[i - 1][j] + GAP {
            i -= 1; // narration word i-1 aligns to a gap
        } else {
            j -= 1; // whisper token j-1 dropped
        }
    }
    out
}

/// Normalize a token for matching: lowercase, alphanumerics only ("Kalm" -> "kalm",
/// "be." -> "be", "50" -> "50").
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Run whisper-timestamped (or a compatible CLI) on the audio and read back its word-level
/// timestamps. The tool writes `<out_dir>/<audio-stem>.json`; we point it at a dedicated
/// scratch dir and parse the single JSON file it produces.
fn local_word_timings(cmd: &str, model: &str, audio: &Path) -> Result<Vec<WordTiming>> {
    let out_dir = audio
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".whisper-ts");
    std::fs::create_dir_all(&out_dir)?;

    let status = Command::new(cmd)
        .arg(audio)
        .args(["--model", model])
        .arg("--output_dir")
        .arg(&out_dir)
        .args(["--output_format", "json"])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit()) // let progress/errors surface
        .status()
        .with_context(|| {
            format!("could not launch `{cmd}` (is whisper-timestamped installed and on PATH?)")
        })?;
    if !status.success() {
        bail!("`{cmd}` exited with {status}");
    }

    // `.whisper-ts` is shared across runs, so don't grab "any .json" — read the
    // exact file whisper writes for this audio: `<out_dir>/<audio-stem>.json`.
    // Append ".json" to the stem rather than `with_extension` so dotted names
    // (e.g. `my.audio.wav` -> `my.audio.json`) resolve correctly.
    let stem = audio.file_stem().context("audio path has no filename")?;
    let mut filename = stem.to_os_string();
    filename.push(".json");
    let json = out_dir.join(filename);
    if !json.exists() {
        bail!(
            "`{cmd}` produced no word-timing JSON at {}",
            json.display()
        );
    }
    let v: Value = serde_json::from_slice(&std::fs::read(&json)?)
        .with_context(|| format!("could not parse word-timing JSON from {}", json.display()))?;
    Ok(words_from_whisper_json(&v))
}

/// Pull word timings from whisper-timestamped JSON: `segments[].words[]` (each with `text`,
/// `start`, `end`), falling back to a top-level `words[]` array if present.
fn words_from_whisper_json(v: &Value) -> Vec<WordTiming> {
    let mut raw: Vec<&Value> = Vec::new();
    if let Some(segs) = v["segments"].as_array() {
        for s in segs {
            if let Some(arr) = s["words"].as_array() {
                raw.extend(arr);
            }
        }
    }
    if raw.is_empty() {
        if let Some(arr) = v["words"].as_array() {
            raw.extend(arr);
        }
    }
    raw.into_iter()
        .map(|w| WordTiming {
            // whisper-timestamped uses `text`; tolerate `word` from other tools.
            word: w["text"]
                .as_str()
                .or_else(|| w["word"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            start_s: w["start"].as_f64().unwrap_or(0.0),
            end_s: w["end"].as_f64().unwrap_or(0.0),
        })
        .filter(|w| !w.word.is_empty())
        .collect()
}

/// Distribute `total` seconds across the words of `text`, weighting each word by its
/// syllable count so longer words get proportionally more screen time. Deterministic.
fn estimate(text: &str, total: f64) -> Vec<WordTiming> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let weights: Vec<f64> = words.iter().map(|w| syllables(w) as f64).collect();
    let sum: f64 = weights.iter().sum();

    let mut out = Vec::with_capacity(words.len());
    let mut t = 0.0;
    for (w, weight) in words.iter().zip(&weights) {
        let dur = total * weight / sum;
        out.push(WordTiming {
            word: w.to_string(),
            start_s: t,
            end_s: t + dur,
        });
        t += dur;
    }
    out
}

/// Rough syllable count: number of vowel groups, at least 1.
fn syllables(word: &str) -> usize {
    let mut count = 0;
    let mut prev_vowel = false;
    for c in word.to_lowercase().chars() {
        let vowel = matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y');
        if vowel && !prev_vowel {
            count += 1;
        }
        prev_vowel = vowel;
    }
    count.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_covers_full_duration_and_is_monotonic() {
        let w = estimate("hello there friend", 6.0);
        assert_eq!(w.len(), 3);
        assert!((w[0].start_s - 0.0).abs() < 1e-9);
        assert!((w.last().unwrap().end_s - 6.0).abs() < 1e-6);
        for pair in w.windows(2) {
            assert!(pair[1].start_s >= pair[0].start_s);
            assert!((pair[0].end_s - pair[1].start_s).abs() < 1e-9); // contiguous
        }
    }

    #[test]
    fn syllables_basic() {
        assert_eq!(syllables("a"), 1);
        assert_eq!(syllables("beach"), 1);
        assert_eq!(syllables("summer"), 2);
        assert_eq!(syllables("rhythm"), 1); // no a/e/i/o/u, but min 1
    }

    #[test]
    fn parses_whisper_timestamped_words() {
        // Shape emitted by whisper-timestamped: segments[].words[] with `text`/`start`/`end`.
        let v = serde_json::json!({
            "text": "Tiny ants carry",
            "segments": [
                { "id": 0, "start": 0.0, "end": 1.2, "words": [
                    { "text": " Tiny",  "start": 0.04, "end": 0.30, "confidence": 0.99 },
                    { "text": " ants",  "start": 0.30, "end": 0.70, "confidence": 0.95 }
                ]},
                { "id": 1, "start": 1.2, "end": 1.9, "words": [
                    { "text": " carry", "start": 1.20, "end": 1.85, "confidence": 0.97 }
                ]}
            ]
        });
        let w = words_from_whisper_json(&v);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].word, "Tiny"); // leading space trimmed
        assert_eq!(w[2].word, "carry");
        assert!((w[0].start_s - 0.04).abs() < 1e-9);
        assert!((w[2].end_s - 1.85).abs() < 1e-9);
        // Real timings have gaps/pauses, unlike the contiguous estimator.
        assert!(w[1].end_s < w[2].start_s);
    }

    #[test]
    fn whisper_json_without_words_is_empty() {
        // OpenRouter-style text-only response → no words → caller falls back to estimation.
        let v = serde_json::json!({ "text": "hello world", "usage": { "seconds": 2.0 } });
        assert!(words_from_whisper_json(&v).is_empty());
    }

    fn t(word: &str, start: f64, end: f64) -> WordTiming {
        WordTiming {
            word: word.into(),
            start_s: start,
            end_s: end,
        }
    }

    #[test]
    fn align_keeps_narration_text_and_whisper_timing() {
        // Whisper misheard "calm as" -> "Kalm is"; captions must show the narration spelling
        // while keeping whisper's (gapped) timing.
        let timed = vec![
            t("Kalm", 0.10, 0.40),
            t("is", 0.45, 0.70),
            t("could", 0.80, 1.10),
            t("be.", 1.10, 1.50),
        ];
        let out = align_text_to_timings("calm as could be", &timed);
        assert_eq!(
            out.iter().map(|w| w.word.as_str()).collect::<Vec<_>>(),
            ["calm", "as", "could", "be"]
        );
        // Timing transferred verbatim from the positionally-aligned whisper tokens.
        assert!((out[0].start_s - 0.10).abs() < 1e-9);
        assert!((out[1].start_s - 0.45).abs() < 1e-9); // "as" gets "is"'s timing
        assert!((out[3].end_s - 1.50).abs() < 1e-9);
    }

    #[test]
    fn align_interpolates_when_whisper_drops_a_word() {
        // Narration has 3 words; whisper only timed 2 (dropped the middle). The unmatched
        // narration word's timing is interpolated between its neighbours, staying monotonic.
        let timed = vec![t("the", 0.0, 0.5), t("end", 2.0, 2.5)];
        let out = align_text_to_timings("the middle end", &timed);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].word, "middle");
        assert!(out[0].end_s <= out[1].start_s + 1e-9);
        assert!(out[1].end_s <= out[2].start_s + 1e-9);
        assert!(out[1].start_s >= 0.5 - 1e-9 && out[1].end_s <= 2.0 + 1e-9);
    }

    #[test]
    fn align_drops_extra_whisper_tokens() {
        // Whisper split/inserted a token ("um"); output still has exactly the narration words.
        let timed = vec![
            t("hello", 0.0, 0.4),
            t("um", 0.4, 0.6),
            t("world", 0.6, 1.0),
        ];
        let out = align_text_to_timings("hello world", &timed);
        assert_eq!(
            out.iter().map(|w| w.word.as_str()).collect::<Vec<_>>(),
            ["hello", "world"]
        );
        assert!((out[1].start_s - 0.6).abs() < 1e-9); // "world" keeps its own timing
    }

    #[test]
    fn align_handles_number_normalization_positionally() {
        // "fifty" (narration) vs "50" (whisper) is a mismatch but still aligns by position.
        let timed = vec![t("50", 1.0, 1.6), t("times", 1.6, 2.0)];
        let out = align_text_to_timings("fifty times", &timed);
        assert_eq!(out[0].word, "fifty");
        assert!((out[0].start_s - 1.0).abs() < 1e-9);
    }
}
