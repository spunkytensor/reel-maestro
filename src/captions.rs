// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Word timings -> an ASS subtitle file with bottom-anchored "word-burst" captions
//! (1-3 words at a time), styled per output format via [`CaptionStyle`].

use crate::config::Format;
use crate::model::WordTiming;

const MAX_WORDS_PER_CARD: usize = 3; // keep cards short enough to read at a glance
const MAX_GAP_S: f64 = 0.2; // start a new card when the silence between words exceeds this
const FONT: &str = "DejaVu Sans"; // ships with the ffmpeg/libass image used to render

/// The format-dependent knobs of the caption look: the ASS PlayRes canvas the sizes are
/// expressed in, the font size, and the bottom/side margins.
pub struct CaptionStyle {
    pub play_res_x: u32,
    pub play_res_y: u32,
    pub font_size: u32,
    pub margin_v: u32,
    pub margin_lr: u32,
}

impl CaptionStyle {
    /// The caption look for an output format. Reel keeps the original thumb-readable style
    /// (big text lifted well above phone UI); youtube scales it for a 16:9 canvas — smaller
    /// text sitting lower-third, since there's no phone chrome to clear.
    pub fn for_format(f: Format) -> Self {
        match f {
            Format::Reel => CaptionStyle {
                play_res_x: 1080,
                play_res_y: 1920,
                font_size: 96,
                margin_v: 520,
                margin_lr: 80,
            },
            Format::Youtube => CaptionStyle {
                play_res_x: 1920,
                play_res_y: 1080,
                font_size: 64,
                margin_v: 100,
                margin_lr: 80,
            },
        }
    }
}

/// Build a complete `.ass` (Advanced SubStation Alpha) document for the given word timings.
///
/// The result is one `[Script Info]`/`[V4+ Styles]` header followed by one `Dialogue:` line per
/// caption card. ffmpeg's `subtitles`/libass filter burns it into the video. An empty `words`
/// slice yields just the header (a valid file with no captions).
pub fn build_ass(words: &[WordTiming], style: &CaptionStyle) -> String {
    let mut s = String::new();
    s.push_str(&header(style));
    for card in pack_cards(words) {
        s.push_str(&dialogue(&card));
        s.push('\n');
    }
    s
}

/// The words whose spoken window starts inside `[start_s, end_s)`, shifted to segment-local
/// time (clamped at 0). Used by chapter-chunked rendering: each segment burns its own ASS file,
/// so the full-reel timings must be re-based to that segment's clock. A word straddling a
/// boundary belongs to the segment containing its start.
pub fn rebase(words: &[WordTiming], start_s: f64, end_s: f64) -> Vec<WordTiming> {
    words
        .iter()
        .filter(|w| w.start_s >= start_s && w.start_s < end_s)
        .map(|w| WordTiming {
            word: w.word.clone(),
            start_s: (w.start_s - start_s).max(0.0),
            end_s: (w.end_s - start_s).max(0.0),
        })
        .collect()
}

/// One on-screen caption "burst": the text to show and the wall-clock window it's visible for.
/// `start_s`/`end_s` come straight from the first/last word's timings so captions stay locked to
/// the spoken audio.
struct Card {
    text: String,
    start_s: f64,
    end_s: f64,
}

/// Group consecutive words into short caption cards (1–3 words each), flushing the current run
/// whenever it hits the word cap, ends on clause punctuation, or is followed by a noticeable
/// pause. This produces the snappy "word burst" rhythm rather than long static subtitle lines.
fn pack_cards(words: &[WordTiming]) -> Vec<Card> {
    let mut cards = Vec::new();
    let mut cur: Vec<&WordTiming> = Vec::new();

    // Emit the accumulated words as one card (spanning their combined time window) and reset.
    // Text is upper-cased here so casing is consistent regardless of how whisper transcribed it.
    let flush = |cur: &mut Vec<&WordTiming>, cards: &mut Vec<Card>| {
        if cur.is_empty() {
            return;
        }
        let text = cur
            .iter()
            .map(|w| w.word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        cards.push(Card {
            text: text.to_uppercase(),
            start_s: cur.first().unwrap().start_s,
            end_s: cur.last().unwrap().end_s,
        });
        cur.clear();
    };

    for (i, w) in words.iter().enumerate() {
        cur.push(w);

        let at_cap = cur.len() >= MAX_WORDS_PER_CARD;
        let clause_end = w.word.ends_with([',', '.', '!', '?', ';', '—']);
        let gap_next = words
            .get(i + 1) // no next word on the last iteration → no gap-triggered flush
            .map(|n| n.start_s - w.end_s > MAX_GAP_S)
            .unwrap_or(false);

        // The `cur.len() >= 2` guard on clause_end avoids breaking after a single word that just
        // happens to end in punctuation (e.g. "Wait,") — those read better grouped with neighbours.
        if at_cap || (clause_end && cur.len() >= 2) || gap_next {
            flush(&mut cur, &mut cards);
        }
    }
    flush(&mut cur, &mut cards);
    cards
}

/// The ASS header: declares the style's PlayRes canvas and a single `Burst` style.
///
/// The `Style:` line encodes the caption look (libass field order, see the `Format:` line above
/// it): white fill + black outline (ASS colours are `&HAABBGGRR`), Bold (-1), 6px outline with no
/// shadow, Alignment 2 (bottom-centre), and the style's `MarginV` to lift the text off the very
/// bottom edge (on the vertical canvas, well above phone UI / safe areas).
fn header(style: &CaptionStyle) -> String {
    format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {}\n\
         PlayResY: {}\n\
         WrapStyle: 0\n\n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Burst,{FONT},{},&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,6,0,2,{},{},{},1\n\n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        style.play_res_x,
        style.play_res_y,
        style.font_size,
        style.margin_lr,
        style.margin_lr,
        style.margin_v,
    )
}

/// Render one card as an ASS `Dialogue:` event on layer 0 using the `Burst` style. The middle
/// zero fields are per-event margin overrides (0 = inherit the style's margins).
fn dialogue(card: &Card) -> String {
    format!(
        "Dialogue: 0,{},{},Burst,,0,0,0,,{}",
        ass_time(card.start_s),
        ass_time(card.end_s),
        escape_ass_text(&card.text)
    )
}

/// Escape user/transcriber text for ASS dialogue payloads. Braces delimit override tags and
/// backslashes introduce ASS escapes, so render them literally; other control characters are
/// normalized to spaces so a transcript cannot break the event line.
fn escape_ass_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Format seconds as ASS time `H:MM:SS.cc` (centiseconds).
fn ass_time(t: f64) -> String {
    let t = t.max(0.0);
    let total_cs = (t * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(word: &str, start: f64, end: f64) -> WordTiming {
        WordTiming {
            word: word.into(),
            start_s: start,
            end_s: end,
        }
    }

    #[test]
    fn ass_time_formats_centiseconds() {
        assert_eq!(ass_time(0.0), "0:00:00.00");
        assert_eq!(ass_time(75.5), "0:01:15.50");
        assert_eq!(ass_time(3661.23), "1:01:01.23");
    }

    #[test]
    fn caps_cards_at_three_words() {
        let words = vec![
            w("one", 0.0, 0.3),
            w("two", 0.3, 0.6),
            w("three", 0.6, 0.9),
            w("four", 0.9, 1.2),
        ];
        let cards = pack_cards(&words);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].text, "ONE TWO THREE");
        assert_eq!(cards[1].text, "FOUR");
    }

    #[test]
    fn splits_on_large_gap() {
        let words = vec![w("hello", 0.0, 0.3), w("world", 2.0, 2.4)];
        let cards = pack_cards(&words);
        assert_eq!(cards.len(), 2);
    }

    #[test]
    fn build_ass_has_header_and_events() {
        let style = CaptionStyle::for_format(crate::config::Format::Reel);
        let ass = build_ass(&[w("hi", 0.0, 0.5)], &style);
        // Reel keeps the original header values exactly.
        assert!(ass.contains("PlayResX: 1080"));
        assert!(ass.contains("PlayResY: 1920"));
        assert!(ass.contains("Style: Burst,DejaVu Sans,96,"));
        assert!(ass.contains(",80,80,520,1"));
        assert!(ass.contains("Dialogue: 0,0:00:00.00,0:00:00.50,Burst"));
        assert!(ass.contains("HI"));
    }

    #[test]
    fn dialogue_escapes_ass_control_syntax_and_control_chars() {
        let style = CaptionStyle::for_format(crate::config::Format::Reel);
        let ass = build_ass(&[w(r"{tag}\path", 0.0, 0.5), w("bad\nchar\t", 0.5, 1.0)], &style);
        assert!(ass.contains(r"\{TAG\}\\PATH"));
        assert!(ass.contains("BAD CHAR "));
        assert!(!ass.contains("BAD\nCHAR"));
    }

    #[test]
    fn youtube_style_is_landscape_lower_third() {
        let style = CaptionStyle::for_format(crate::config::Format::Youtube);
        let ass = build_ass(&[w("hi", 0.0, 0.5)], &style);
        assert!(ass.contains("PlayResX: 1920"));
        assert!(ass.contains("PlayResY: 1080"));
        assert!(ass.contains("Style: Burst,DejaVu Sans,64,"));
        assert!(ass.contains(",80,80,100,1"));
    }

    #[test]
    fn rebase_shifts_and_filters_to_window() {
        let words = vec![
            w("before", 1.0, 1.4),
            w("first", 10.0, 10.5),
            w("straddle", 19.8, 20.4), // starts inside → kept, even though it ends past the window
            w("after", 20.6, 21.0),
        ];
        let out = rebase(&words, 10.0, 20.0);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].word, "first");
        assert!((out[0].start_s - 0.0).abs() < 1e-9);
        assert!((out[0].end_s - 0.5).abs() < 1e-9);
        assert_eq!(out[1].word, "straddle");
        assert!((out[1].start_s - 9.8).abs() < 1e-9);
    }
}
