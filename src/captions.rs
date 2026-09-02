// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0

//! Word timings -> an ASS subtitle file with bottom-anchored "word-burst" captions
//! (1-3 words at a time), styled per output format via [`CaptionStyle`].

use crate::config::Format;
use crate::model::WordTiming;
use clap::ValueEnum;

const MAX_WORDS_PER_CARD: usize = 3; // keep cards short enough to read at a glance
const MAX_GAP_S: f64 = 0.2; // start a new card when the silence between words exceeds this

/// The visual treatment applied to word-timed captions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum CaptionPreset {
    /// The original large, white word-burst captions.
    Burst,
    /// White captions that sweep yellow across each word as it is spoken.
    Karaoke,
    /// White captions on an opaque dark box.
    Boxed,
    /// A smaller, cleaner lower-third caption treatment.
    Minimal,
}

/// The format-dependent knobs of the caption look: the ASS PlayRes canvas the sizes are
/// expressed in, the font size, and the bottom/side margins.
pub struct CaptionStyle {
    pub play_res_x: u32,
    pub play_res_y: u32,
    pub font_size: u32,
    pub margin_v: u32,
    pub margin_lr: u32,
    pub preset: CaptionPreset,
    pub font: String,
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
                preset: CaptionPreset::Burst,
                font: "DejaVu Sans".to_string(),
            },
            Format::Youtube => CaptionStyle {
                play_res_x: 1920,
                play_res_y: 1080,
                font_size: 64,
                margin_v: 100,
                margin_lr: 80,
                preset: CaptionPreset::Burst,
                font: "DejaVu Sans".to_string(),
            },
        }
    }

    /// The caption look for an output format, preset, and optional installed font name.
    pub fn for_format_and_preset(f: Format, preset: CaptionPreset, font: Option<&str>) -> Self {
        let mut style = Self::for_format(f);
        style.preset = preset;
        if let Some(font) = font {
            style.font = font.to_string();
        }
        if preset == CaptionPreset::Minimal {
            match f {
                Format::Reel => {
                    style.font_size = 72;
                    style.margin_v = 240;
                }
                Format::Youtube => {
                    style.font_size = 48;
                    style.margin_v = 55;
                }
            }
        }
        style
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
        s.push_str(&dialogue(&card, style));
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

/// One on-screen caption "burst": the text and word timings within its wall-clock window.
/// `start_s`/`end_s` come straight from the first/last word's timings so captions stay locked to
/// the spoken audio.
struct Card {
    text: String,
    words: Vec<WordTiming>,
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
            words: cur.iter().map(|w| (*w).clone()).collect(),
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
/// The burst branch deliberately preserves the original header byte-for-byte. Other presets use
/// the same layout while changing only their visual style fields.
fn header(style: &CaptionStyle) -> String {
    let (format, style_line) = match style.preset {
        CaptionPreset::Burst => (
            "Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            format!(
                "Style: Burst,{},{},&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,6,0,2,{},{},{},1",
                style.font, style.font_size, style.margin_lr, style.margin_lr, style.margin_v
            ),
        ),
        CaptionPreset::Karaoke => (
            "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            format!(
                "Style: Burst,{},{},&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,6,0,2,{},{},{},1",
                style.font, style.font_size, style.margin_lr, style.margin_lr, style.margin_v
            ),
        ),
        CaptionPreset::Boxed => (
            "Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            format!(
                "Style: Burst,{},{},&H00FFFFFF,&H00181818,&H00181818,-1,0,0,0,100,100,0,0,3,0,0,2,{},{},{},1",
                style.font, style.font_size, style.margin_lr, style.margin_lr, style.margin_v
            ),
        ),
        CaptionPreset::Minimal => (
            "Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
            format!(
                "Style: Burst,{},{},&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,{},{},{},1",
                style.font, style.font_size, style.margin_lr, style.margin_lr, style.margin_v
            ),
        ),
    };
    format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {}\n\
         PlayResY: {}\n\
         WrapStyle: 0\n\n\
         [V4+ Styles]\n\
         {format}\n\
         {style_line}\n\n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        style.play_res_x, style.play_res_y,
    )
}

/// Render one card as an ASS `Dialogue:` event on layer 0 using the `Burst` style. The middle
/// zero fields are per-event margin overrides (0 = inherit the style's margins).
fn dialogue(card: &Card, style: &CaptionStyle) -> String {
    let text = if style.preset == CaptionPreset::Karaoke {
        karaoke_text(card)
    } else {
        card.text.clone()
    };
    format!(
        "Dialogue: 0,{},{},Burst,,0,0,0,,{}",
        ass_time(card.start_s),
        ass_time(card.end_s),
        text
    )
}

/// ASS karaoke text with one `\k` duration tag per word. `\k` is measured in centiseconds;
/// each duration comes from its word's own spoken timing and is at least one centisecond.
fn karaoke_text(card: &Card) -> String {
    card.words
        .iter()
        .map(|word| {
            let duration_cs = ((word.end_s - word.start_s) * 100.0).round().max(1.0) as u64;
            format!("{{\\k{duration_cs}}}{}", word.word.to_uppercase())
        })
        .collect::<Vec<_>>()
        .join(" ")
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

    #[test]
    fn default_preset_keeps_original_style_line() {
        let style = CaptionStyle::for_format(Format::Reel);
        let ass = build_ass(&[], &style);
        assert!(ass.contains("Style: Burst,DejaVu Sans,96,"));
    }

    #[test]
    fn karaoke_tags_sum_to_card_duration() {
        let cards = pack_cards(&[
            w("one", 0.0, 0.33),
            w("two", 0.33, 0.67),
            w("three", 0.67, 1.0),
        ]);
        let text = karaoke_text(&cards[0]);
        assert_eq!(text, "{\\k33}ONE {\\k34}TWO {\\k33}THREE");
        let duration_sum: u64 = [33, 34, 33].into_iter().sum();
        let card_duration = ((cards[0].end_s - cards[0].start_s) * 100.0).round() as i64;
        assert!((duration_sum as i64 - card_duration).abs() <= 1);
    }

    #[test]
    fn boxed_preset_uses_opaque_box_border_style() {
        let style = CaptionStyle::for_format_and_preset(Format::Reel, CaptionPreset::Boxed, None);
        let ass = build_ass(&[], &style);
        assert!(ass.contains("&H00181818,&H00181818,-1,0,0,0,100,100,0,0,3,0,0,2,"));
    }

    #[test]
    fn font_override_appears_in_style_line() {
        let style =
            CaptionStyle::for_format_and_preset(Format::Reel, CaptionPreset::Burst, Some("Impact"));
        let ass = build_ass(&[], &style);
        assert!(ass.contains("Style: Burst,Impact,96,"));
    }
}
