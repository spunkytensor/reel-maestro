//! Word timings -> an ASS subtitle file with big, bottom-anchored "word-burst" captions
//! (1-3 words at a time), sized for a 1080x1920 vertical canvas.

use crate::model::WordTiming;

const MAX_WORDS_PER_CARD: usize = 3;
const MAX_GAP_S: f64 = 0.2; // start a new card when the silence between words exceeds this
const FONT: &str = "DejaVu Sans";
const FONT_SIZE: u32 = 96;

/// Build a complete `.ass` document for the given word timings.
pub fn build_ass(words: &[WordTiming]) -> String {
    let mut s = String::new();
    s.push_str(&header());
    for card in pack_cards(words) {
        s.push_str(&dialogue(&card));
        s.push('\n');
    }
    s
}

struct Card {
    text: String,
    start_s: f64,
    end_s: f64,
}

/// Group consecutive words into short caption cards.
fn pack_cards(words: &[WordTiming]) -> Vec<Card> {
    let mut cards = Vec::new();
    let mut cur: Vec<&WordTiming> = Vec::new();

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
            .get(i + 1)
            .map(|n| n.start_s - w.end_s > MAX_GAP_S)
            .unwrap_or(false);

        if at_cap || (clause_end && cur.len() >= 2) || gap_next {
            flush(&mut cur, &mut cards);
        }
    }
    flush(&mut cur, &mut cards);
    cards
}

fn header() -> String {
    format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: 1080\n\
         PlayResY: 1920\n\
         WrapStyle: 0\n\n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Burst,{FONT},{FONT_SIZE},&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,6,0,2,80,80,520,1\n\n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    )
}

fn dialogue(card: &Card) -> String {
    format!(
        "Dialogue: 0,{},{},Burst,,0,0,0,,{}",
        ass_time(card.start_s),
        ass_time(card.end_s),
        card.text
    )
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
        let ass = build_ass(&[w("hi", 0.0, 0.5)]);
        assert!(ass.contains("PlayResX: 1080"));
        assert!(ass.contains("Dialogue: 0,0:00:00.00,0:00:00.50,Burst"));
        assert!(ass.contains("HI"));
    }
}
