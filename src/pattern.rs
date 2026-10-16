use std::collections::BTreeMap;

use crate::mask;
use crate::read::Line;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const OTHER: &str = "other";

/// FNV-1a 64-bit over the bytes. The empty input is `cbf29ce484222325`.
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Control characters and bidi controls are shown as `\uXXXX`, the same
/// spelling as a JSON escape. A log line can then not move the cursor, clear
/// the screen, or reverse the text that follows (U+202E and the other bidi
/// marks). Other characters are left as they are. Applying this twice does
/// not change the text: the escape itself has none of those characters.
pub fn show_controls(text: &str) -> String {
    if !text.chars().any(is_shown_escaped) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if is_shown_escaped(ch) {
            out.push_str(&format!("\\u{:04x}", u32::from(ch)));
        } else {
            out.push(ch);
        }
    }
    out
}

/// Controls, plus the bidi marks that can reorder text in a terminal.
fn is_shown_escaped(ch: char) -> bool {
    ch.is_control() || is_bidi_control(ch)
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
    )
}

/// `P` plus all 16 hex digits of FNV-1a 64-bit over the pattern text.
/// The high 32 bits alone collide: `ev brceg` and `ev czbab` share them.
pub fn pattern_id(text: &str) -> String {
    format!("P{:016x}", fnv1a64(text.as_bytes()))
}

/// Share as a percent with 1 decimal, period as separator. 667 becomes `66.7`.
pub fn format_share(tenths: u64) -> String {
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// One pattern, or the rest bucket `other` when `rest` is set.
#[derive(Debug, PartialEq, Eq)]
pub struct Pattern {
    pub id: String,
    pub text: String,
    pub lines: u64,
    pub bytes: u64,
    /// Tenths of a percent: 667 is 66.7%.
    pub share_tenths: u64,
    pub rest: bool,
    /// Sum of the masks applied to the lines of this pattern.
    pub masks: mask::MaskCounts,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stats {
    pub total_lines: u64,
    pub total_bytes: u64,
    pub patterns: Vec<Pattern>,
}

#[derive(Default)]
struct Acc {
    lines: u64,
    bytes: u64,
    masks: mask::MaskCounts,
}

/// Counts masked lines. The lines themselves are not kept.
#[derive(Default)]
pub struct Grouper {
    groups: BTreeMap<String, Acc>,
}

impl Grouper {
    pub fn push(&mut self, line: &Line) {
        let (masked, counts) = mask::mask_detail(&line.text);
        let acc = self.groups.entry(masked).or_default();
        acc.lines += 1;
        acc.bytes += line.bytes;
        acc.masks.add(counts);
    }

    /// Sort: bytes descending, then pattern text ascending. `top` keeps
    /// the first N and folds the rest into `other`.
    pub fn finish(self, top: Option<u32>) -> Stats {
        let total_lines = self.groups.values().map(|acc| acc.lines).sum();
        let total_bytes = self.groups.values().map(|acc| acc.bytes).sum();
        let mut patterns: Vec<Pattern> = self
            .groups
            .into_iter()
            .map(|(text, acc)| Pattern {
                id: pattern_id(&text),
                share_tenths: share_tenths(acc.bytes, total_bytes),
                text,
                lines: acc.lines,
                bytes: acc.bytes,
                rest: false,
                masks: acc.masks,
            })
            .collect();
        patterns.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.text.cmp(&b.text)));
        if let Some(n) = top {
            let n = n as usize;
            if patterns.len() > n {
                let rest = patterns.split_off(n);
                let lines = rest.iter().map(|pattern| pattern.lines).sum();
                let bytes = rest.iter().map(|pattern| pattern.bytes).sum();
                let masks = rest
                    .iter()
                    .fold(mask::MaskCounts::default(), |mut acc, pattern| {
                        acc.add(pattern.masks);
                        acc
                    });
                patterns.push(Pattern {
                    id: OTHER.to_owned(),
                    text: OTHER.to_owned(),
                    lines,
                    bytes,
                    share_tenths: share_tenths(bytes, total_bytes),
                    rest: true,
                    masks,
                });
            }
        }
        Stats {
            total_lines,
            total_bytes,
            patterns,
        }
    }
}

/// Group lines by the masked message. Bytes stay the raw bytes.
/// Control characters and bidi marks stay in the pattern text. Text output
/// shows them as `\uXXXX`; JSON keeps the character. The pattern id is the
/// hash of the masked text.
#[cfg(test)]
pub fn group(lines: &[Line], top: Option<u32>) -> Stats {
    let mut grouped = Grouper::default();
    for line in lines {
        grouped.push(line);
    }
    grouped.finish(top)
}

/// Tenths of a percent. A half step rounds away from zero, as with amounts.
pub fn share_tenths(bytes: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let numer = u128::from(bytes) * 1000;
    let den = u128::from(total);
    let mut tenths = numer / den;
    if (numer % den) * 2 >= den {
        tenths += 1;
    }
    tenths as u64
}

#[cfg(test)]
mod tests {
    use super::{fnv1a64, format_share, group, pattern_id, show_controls};
    use crate::read::Line;

    fn sample_line(text: &str, bytes: u64) -> Line {
        Line {
            text: text.to_owned(),
            raw: text.to_owned(),
            bytes,
        }
    }

    #[test]
    fn control_in_the_message_stays_in_the_pattern() {
        // `boom \033[H here`: the pattern keeps the ESC. The id is the hash
        // of that masked text. Showing it writes `\u001b`, not the raw byte.
        let raw = "boom \u{1b}[H here";
        let stats = group(&[sample_line(raw, 16)], None);
        assert_eq!(stats.patterns.len(), 1);
        assert_eq!(stats.patterns[0].text, raw);
        assert_eq!(stats.patterns[0].id, pattern_id(raw));
        assert_ne!(stats.patterns[0].id, pattern_id("boom \\u001b[H here"));
        assert_eq!(show_controls(raw), "boom \\u001b[H here");
        assert!(!show_controls(raw).chars().any(char::is_control));
        assert_eq!(show_controls("E\u{1b}[2JUR"), "E\\u001b[2JUR");
        assert_eq!(show_controls("disk full"), "disk full");
    }

    #[test]
    fn bidi_override_is_escaped_in_text() {
        let raw = "disk \u{202E}full";
        let shown = show_controls(raw);
        assert_eq!(shown, "disk \\u202efull");
        assert!(!shown.chars().any(|ch| ch == '\u{202E}'));
        assert_eq!(show_controls(&shown), shown);
        let stats = group(&[sample_line(raw, 10)], None);
        assert_eq!(stats.patterns[0].text, raw);
        assert_eq!(stats.patterns[0].id, pattern_id(raw));
        for ch in [
            '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}', '\u{200E}', '\u{200F}', '\u{061C}',
        ] {
            let text = format!("ev {ch}x");
            let shown = show_controls(&text);
            assert!(!shown.contains(ch), "{shown}");
            assert!(
                shown.contains(&format!("\\u{:04x}", u32::from(ch))),
                "{shown}"
            );
        }
    }

    #[test]
    fn fnv1a_empty_text() {
        assert_eq!(format!("{:016x}", fnv1a64(b"")), "cbf29ce484222325");
        assert_eq!(pattern_id(""), "Pcbf29ce484222325");
        assert_eq!(format!("{:016x}", fnv1a64(b"a")), "af63dc4c8601ec8c");
        assert_eq!(pattern_id("a"), "Paf63dc4c8601ec8c");
        // The high 32 bits match. The full hash does not.
        assert_eq!(fnv1a64(b"ev brceg") >> 32, fnv1a64(b"ev czbab") >> 32);
        assert_eq!(pattern_id("ev brceg"), "P6b31a22abd1e8f47");
        assert_eq!(pattern_id("ev czbab"), "P6b31a22aba8b4370");
        assert_ne!(pattern_id("ev brceg"), pattern_id("ev czbab"));
    }

    #[test]
    fn three_patterns_hand_calculated() {
        // The 1000-byte line: 2 lines, 1000 + 1000 = 2000 bytes.
        // Each 500-byte line: 1 line.
        // Total: 4 lines, 3000 bytes.
        // Tenths = bytes * 1000 / 3000. Remainder 2000, and 2 * 2000 >= 3000, so +1.
        // The 1000-byte line: 666 + 1 = 667 → 66.7. Each 500-byte line: 166 + 1 = 167 → 16.7.
        // On equal bytes the pattern text wins, ascending: the low token before the middle token.
        let stats = group(
            &[
                sample_line("midden", 500),
                sample_line("hoog", 1000),
                sample_line("laag", 500),
                sample_line("hoog", 1000),
            ],
            None,
        );
        assert_eq!(stats.total_lines, 4);
        assert_eq!(stats.total_bytes, 3000);
        assert_eq!(stats.patterns.len(), 3);

        let high_pattern = &stats.patterns[0];
        assert_eq!(high_pattern.text, "hoog");
        assert_eq!(high_pattern.id, "P4033e9cc7e756726");
        assert!(!high_pattern.rest);
        assert_eq!(high_pattern.lines, 2);
        assert_eq!(high_pattern.bytes, 2000);
        assert_eq!(high_pattern.share_tenths, 667);
        assert_eq!(format_share(high_pattern.share_tenths), "66.7");

        let low = &stats.patterns[1];
        assert_eq!(low.text, "laag");
        assert_eq!(low.id, "P0493cfad908bf0de");
        assert_eq!(low.lines, 1);
        assert_eq!(low.bytes, 500);
        assert_eq!(low.share_tenths, 167);
        assert_eq!(format_share(low.share_tenths), "16.7");

        let middle = &stats.patterns[2];
        assert_eq!(middle.text, "midden");
        assert_eq!(middle.id, "P5be0d38d7d307304");
        assert_eq!(middle.lines, 1);
        assert_eq!(middle.bytes, 500);
        assert_eq!(middle.share_tenths, 167);
        assert_eq!(format_share(middle.share_tenths), "16.7");
    }

    #[test]
    fn top_n_folds_the_rest_into_other() {
        // 400 + 300 + 200 + 100 = 1000. Top 2 keeps the 400-byte line and the 300-byte line;
        // the 200-byte line (2 lines, 200) and the 100-byte line (100) become other: 3 lines, 300 bytes, 30.0%.
        let lines = [
            sample_line("vier", 100),
            sample_line("drie", 100),
            sample_line("een", 400),
            sample_line("drie", 100),
            sample_line("twee", 300),
        ];
        let stats = group(&lines, Some(2));
        assert_eq!(stats.total_lines, 5);
        assert_eq!(stats.total_bytes, 1000);
        assert_eq!(stats.patterns.len(), 3);

        assert_eq!(stats.patterns[0].text, "een");
        assert_eq!(stats.patterns[0].lines, 1);
        assert_eq!(stats.patterns[0].bytes, 400);
        assert_eq!(format_share(stats.patterns[0].share_tenths), "40.0");
        assert!(!stats.patterns[0].rest);

        assert_eq!(stats.patterns[1].text, "twee");
        assert_eq!(stats.patterns[1].bytes, 300);
        assert_eq!(format_share(stats.patterns[1].share_tenths), "30.0");

        let other = &stats.patterns[2];
        assert!(other.rest);
        assert_eq!(other.id, "other");
        assert_eq!(other.text, "other");
        assert_eq!(other.lines, 3);
        assert_eq!(other.bytes, 300);
        assert_eq!(other.share_tenths, 300);
        assert_eq!(format_share(other.share_tenths), "30.0");

        assert_eq!(group(&lines, None).patterns.len(), 4);
        assert_eq!(group(&lines, Some(9)).patterns.len(), 4);
    }

    #[test]
    fn masks_add_up_per_line() {
        // `pod-7` and `pod-8` both become `<VAR>`. Each set one mask, 2 together.
        // `user <NUM>` already has the tag and sets no mask. `user 7` sets one.
        // The pattern counts 1, not the tags in the text times the lines.
        let combined = group(&[sample_line("pod-7", 10), sample_line("pod-8", 20)], None);
        assert_eq!(combined.patterns.len(), 1);
        assert_eq!(combined.patterns[0].text, "<VAR>");
        assert_eq!(combined.patterns[0].masks.var, 2);
        assert_eq!(combined.patterns[0].masks.num, 0);

        let literal = group(
            &[sample_line("user 7", 7), sample_line("user <NUM>", 11)],
            None,
        );
        assert_eq!(literal.patterns.len(), 1);
        assert_eq!(literal.patterns[0].text, "user <NUM>");
        assert_eq!(literal.patterns[0].lines, 2);
        assert_eq!(literal.patterns[0].masks.num, 1);
        assert_eq!(literal.patterns[0].masks.var, 0);
    }

    #[test]
    fn other_adds_up_masks_from_the_rest() {
        // `aaaa` (100 bytes) stays at top 1 and has no mask.
        // `pod-7` sets one `<VAR>`, `xx 3` one `<NUM>`. Those two become other.
        let stats = group(
            &[
                sample_line("aaaa", 100),
                sample_line("pod-7", 10),
                sample_line("xx 3", 10),
            ],
            Some(1),
        );
        assert_eq!(stats.patterns.len(), 2);
        assert_eq!(stats.patterns[0].masks, crate::mask::MaskCounts::default());
        assert_eq!(stats.patterns[0].text, "aaaa");
        let other = &stats.patterns[1];
        assert!(other.rest);
        assert_eq!(other.lines, 2);
        assert_eq!(other.bytes, 20);
        assert_eq!(other.masks.var, 1);
        assert_eq!(other.masks.num, 1);
        assert_eq!(other.masks.ts, 0);
    }

    #[test]
    fn same_mask_is_one_pattern() {
        let stats = group(&[sample_line("pod-7", 10), sample_line("pod-8", 20)], None);
        assert_eq!(stats.patterns.len(), 1);
        assert_eq!(stats.patterns[0].text, "<VAR>");
        assert_eq!(stats.patterns[0].id, pattern_id("<VAR>"));
        assert_eq!(stats.patterns[0].lines, 2);
        assert_eq!(stats.patterns[0].bytes, 30);
        assert_eq!(format_share(stats.patterns[0].share_tenths), "100.0");
    }
}
