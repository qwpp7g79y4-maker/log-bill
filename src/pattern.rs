use std::collections::BTreeMap;

use crate::mask;
use crate::read::Line;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const OVERIG: &str = "other";

/// FNV-1a 64-bit over the bytes. The empty input is `cbf29ce484222325`.
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// `P` plus the first 8 hex digits of FNV-1a 64-bit over the pattern text.
pub fn pattern_id(text: &str) -> String {
    let high = (fnv1a64(text.as_bytes()) >> 32) as u32;
    format!("P{high:08x}")
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

/// Group lines by the masked message. Bytes stay the raw bytes.
/// Sort: bytes descending, then pattern text ascending. `top` keeps
/// the first N and folds the rest into `other`.
pub fn group(lines: &[Line], top: Option<u32>) -> Stats {
    let mut groups: BTreeMap<String, Acc> = BTreeMap::new();
    for line in lines {
        let (text, counts) = mask::mask_detail(&line.text);
        let acc = groups.entry(text).or_default();
        acc.lines += 1;
        acc.bytes += line.bytes;
        acc.masks.add(counts);
    }
    let total_lines = groups.values().map(|acc| acc.lines).sum();
    let total_bytes = groups.values().map(|acc| acc.bytes).sum();
    let mut patterns: Vec<Pattern> = groups
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
                id: OVERIG.to_owned(),
                text: OVERIG.to_owned(),
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

/// Tenths of a percent. A half step rounds away from zero, as with amounts.
pub(crate) fn share_tenths(bytes: u64, total: u64) -> u64 {
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
    use super::{fnv1a64, format_share, group, pattern_id};
    use crate::read::Line;

    fn sample_line(text: &str, bytes: u64) -> Line {
        Line {
            text: text.to_owned(),
            raw: text.to_owned(),
            bytes,
        }
    }

    #[test]
    fn fnv1a_empty_text() {
        assert_eq!(format!("{:016x}", fnv1a64(b"")), "cbf29ce484222325");
        assert_eq!(pattern_id(""), "Pcbf29ce4");
        assert_eq!(format!("{:016x}", fnv1a64(b"a")), "af63dc4c8601ec8c");
        assert_eq!(pattern_id("a"), "Paf63dc4c");
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
        assert_eq!(high_pattern.id, "P4033e9cc");
        assert!(!high_pattern.rest);
        assert_eq!(high_pattern.lines, 2);
        assert_eq!(high_pattern.bytes, 2000);
        assert_eq!(high_pattern.share_tenths, 667);
        assert_eq!(format_share(high_pattern.share_tenths), "66.7");

        let low = &stats.patterns[1];
        assert_eq!(low.text, "laag");
        assert_eq!(low.id, "P0493cfad");
        assert_eq!(low.lines, 1);
        assert_eq!(low.bytes, 500);
        assert_eq!(low.share_tenths, 167);
        assert_eq!(format_share(low.share_tenths), "16.7");

        let middle = &stats.patterns[2];
        assert_eq!(middle.text, "midden");
        assert_eq!(middle.id, "P5be0d38d");
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

        let letterlijk = group(
            &[sample_line("user 7", 7), sample_line("user <NUM>", 11)],
            None,
        );
        assert_eq!(letterlijk.patterns.len(), 1);
        assert_eq!(letterlijk.patterns[0].text, "user <NUM>");
        assert_eq!(letterlijk.patterns[0].lines, 2);
        assert_eq!(letterlijk.patterns[0].masks.num, 1);
        assert_eq!(letterlijk.patterns[0].masks.var, 0);
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
