use std::collections::{BTreeMap, BTreeSet};

use crate::pattern::{Pattern, Stats};

/// New, gone, or in both samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    New,
    Gone,
    Both,
}

/// One pattern in the comparison. Bytes and share are 0 on the side that lacks it.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub text: String,
    pub status: Status,
    pub old_lines: u64,
    pub new_lines: u64,
    pub old_bytes: u64,
    pub new_bytes: u64,
    /// Tenths of a percent, as in `pattern::share_tenths`.
    pub old_share: u64,
    pub new_share: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub old_lines: u64,
    pub old_bytes: u64,
    pub new_lines: u64,
    pub new_bytes: u64,
    pub rows: Vec<Row>,
    /// Bytes of patterns that appear only in the new sample.
    pub added_bytes: u64,
}

/// Percentage as a reduced fraction. `50` is 50%, `.5` is 0.5%.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Percent {
    numer: u128,
    denom: u128,
}

/// Match on the pattern text. The id abbreviates it: two different
/// texts with the same id stay two rows.
/// Order: new, then gone, then both. In a group, largest bytes first
/// (for both, the absolute difference), and the text when bytes are equal.
pub fn compare(old: &Stats, new: &Stats) -> Report {
    let old_map = by_text(old);
    let new_map = by_text(new);
    let mut texts = BTreeSet::new();
    texts.extend(old_map.keys().copied());
    texts.extend(new_map.keys().copied());

    let mut rows = Vec::new();
    let mut added_bytes = 0u64;
    for text in texts {
        let old_pattern = old_map.get(text).copied();
        let new_pattern = new_map.get(text).copied();
        let (status, source, new_bytes) = match (old_pattern, new_pattern) {
            (None, Some(pattern)) => (Status::New, pattern, pattern.bytes),
            (Some(pattern), None) => (Status::Gone, pattern, 0),
            (Some(pattern), Some(added)) => (Status::Both, pattern, added.bytes),
            (None, None) => continue,
        };
        if status == Status::New {
            added_bytes += new_bytes;
        }
        rows.push(Row {
            id: source.id.clone(),
            text: text.to_owned(),
            status,
            old_lines: old_pattern.map(|pattern| pattern.lines).unwrap_or(0),
            new_lines: new_pattern.map(|pattern| pattern.lines).unwrap_or(0),
            old_bytes: old_pattern.map(|pattern| pattern.bytes).unwrap_or(0),
            new_bytes,
            old_share: old_pattern.map(|pattern| pattern.share_tenths).unwrap_or(0),
            new_share: new_pattern.map(|pattern| pattern.share_tenths).unwrap_or(0),
        });
    }
    rows.sort_by(|a, b| {
        rank(a.status).cmp(&rank(b.status)).then_with(|| {
            let by_bytes = match a.status {
                Status::New => b.new_bytes.cmp(&a.new_bytes),
                Status::Gone => b.old_bytes.cmp(&a.old_bytes),
                Status::Both => abs_delta(b).cmp(&abs_delta(a)),
            };
            by_bytes.then_with(|| a.text.cmp(&b.text))
        })
    });
    Report {
        old_lines: old.total_lines,
        old_bytes: old.total_bytes,
        new_lines: new.total_lines,
        new_bytes: new.total_bytes,
        rows,
        added_bytes,
    }
}

fn by_text(stats: &Stats) -> BTreeMap<&str, &Pattern> {
    stats
        .patterns
        .iter()
        .map(|pattern| (pattern.text.as_str(), pattern))
        .collect()
}

fn rank(status: Status) -> u8 {
    match status {
        Status::New => 0,
        Status::Gone => 1,
        Status::Both => 2,
    }
}

fn abs_delta(row: &Row) -> u128 {
    (i128::from(row.new_bytes) - i128::from(row.old_bytes)).unsigned_abs()
}

/// A non-negative decimal with a decimal point. `10,5`, `5.`, and `1e2` are invalid.
/// More digits than fit in a u128 is invalid, not truncation.
pub fn parse_percent(text: &str) -> Option<Percent> {
    if text.is_empty() || text.len() > 80 {
        return None;
    }
    let (whole, frac) = if let Some((whole, frac)) = text.split_once('.') {
        if frac.is_empty() || frac.contains('.') {
            return None;
        }
        (whole, frac)
    } else {
        (text, "")
    };
    if !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut numer = digits(whole)?;
    for byte in frac.bytes() {
        numer = numer
            .checked_mul(10)?
            .checked_add(u128::from(byte - b'0'))?;
    }
    let scale = u32::try_from(frac.len()).ok()?;
    let denom = 10u128.checked_pow(scale)?;
    Some(reduce(numer, denom))
}

fn digits(text: &str) -> Option<u128> {
    let mut value = 0u128;
    for byte in text.bytes() {
        value = value
            .checked_mul(10)?
            .checked_add(u128::from(byte - b'0'))?;
    }
    Some(value)
}

fn reduce(mut numer: u128, mut denom: u128) -> Percent {
    let divisor = gcd(numer, denom);
    numer /= divisor;
    denom /= divisor;
    Percent { numer, denom }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let rest = a % b;
        a = b;
        b = rest;
    }
    a
}

impl Percent {
    /// True when the value is above 100%. `100` and `100.0` are not.
    pub fn above_hundred(self) -> bool {
        let whole = self.numer / self.denom;
        whole > 100 || (whole == 100 && self.numer % self.denom != 0)
    }

    /// `part / total × 100` strictly greater than this percentage.
    /// Equal is not greater. A total of 0 is not greater.
    /// The comparison is the fraction, not the shown 1-decimal share.
    pub fn exceeded_by(self, part: u64, total: u64) -> bool {
        if part == 0 || total == 0 {
            return false;
        }
        let left = mul_wide(u128::from(part) * 100, self.denom);
        let right = mul_wide(u128::from(total), self.numer);
        left > right
    }
}

/// Product as `(high, low)`, even when it does not fit in one u128.
fn mul_wide(a: u128, b: u128) -> (u128, u128) {
    let mask = (1u128 << 64) - 1;
    let a_lo = a & mask;
    let a_hi = a >> 64;
    let b_lo = b & mask;
    let b_hi = b >> 64;
    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;
    let (sum, carry_sum) = lh.overflowing_add(hl);
    let (mid, carry_mid) = sum.overflowing_add(ll >> 64);
    let carry = u128::from(carry_sum) + u128::from(carry_mid);
    let hi = hh + (mid >> 64) + (carry << 64);
    let lo = (ll & mask) | ((mid & mask) << 64);
    (hi, lo)
}

#[cfg(test)]
mod tests {
    use super::{compare, mul_wide, parse_percent, Status};
    use crate::pattern::{group, pattern_id};
    use crate::read::Line;

    fn line(text: &str, bytes: u64) -> Line {
        Line {
            text: text.to_owned(),
            raw: text.to_owned(),
            bytes,
        }
    }

    #[test]
    fn alpha_beta_gamma_hand_calculated() {
        // alpha 12 of 17: 12000/17 = 705 remainder 15 → 706. beta 5000/17 = 294 remainder 2.
        // New, each 6 of 12 = 500. gamma is in the new bytes, beta is not.
        let old_sample = group(&[line("alpha", 6), line("alpha", 6), line("beta", 5)], None);
        let new_sample = group(&[line("alpha", 6), line("gamma", 6)], None);
        let report = compare(&old_sample, &new_sample);
        assert_eq!(report.old_lines, 3);
        assert_eq!(report.old_bytes, 17);
        assert_eq!(report.new_lines, 2);
        assert_eq!(report.new_bytes, 12);
        assert_eq!(report.added_bytes, 6);
        assert_eq!(report.rows.len(), 3);

        let gamma = &report.rows[0];
        assert_eq!(gamma.status, Status::New);
        assert_eq!(gamma.id, pattern_id("gamma"));
        assert_eq!(gamma.old_bytes, 0);
        assert_eq!(gamma.new_bytes, 6);
        assert_eq!(gamma.old_share, 0);
        assert_eq!(gamma.new_share, 500);

        let beta = &report.rows[1];
        assert_eq!(beta.status, Status::Gone);
        assert_eq!(beta.id, pattern_id("beta"));
        assert_eq!(beta.old_bytes, 5);
        assert_eq!(beta.new_bytes, 0);
        assert_eq!(beta.old_share, 294);
        assert_eq!(beta.new_share, 0);

        let alpha = &report.rows[2];
        assert_eq!(alpha.status, Status::Both);
        assert_eq!(alpha.old_lines, 2);
        assert_eq!(alpha.new_lines, 1);
        assert_eq!(alpha.old_bytes, 12);
        assert_eq!(alpha.new_bytes, 6);
        assert_eq!(alpha.old_share, 706);
        assert_eq!(alpha.new_share, 500);
    }

    #[test]
    fn same_mask_is_not_a_new_pattern() {
        // `user 7` and `user 9` both become `user <NUM>`. That is not a new pattern.
        // Old, 18 bytes: 10/18 → 555 remainder 10, 2×10 ≥ 18 → 556. 8/18 → 444 remainder 8.
        let old_sample = group(&[line("user 7", 10), line("disk full", 8)], None);
        let new_sample = group(&[line("user 9", 25)], None);
        let report = compare(&old_sample, &new_sample);
        assert_eq!(report.added_bytes, 0);
        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.rows[0].status, Status::Gone);
        assert_eq!(report.rows[0].text, "disk full");
        assert_eq!(report.rows[0].old_bytes, 8);
        assert_eq!(report.rows[0].old_share, 444);
        assert_eq!(report.rows[1].status, Status::Both);
        assert_eq!(report.rows[1].text, "user <NUM>");
        assert_eq!(report.rows[1].old_bytes, 10);
        assert_eq!(report.rows[1].new_bytes, 25);
        assert_eq!(report.rows[1].old_share, 556);
        assert_eq!(report.rows[1].new_share, 1000);
        assert!(report.rows.iter().all(|row| !row.text.contains("user 7")));
        assert!(report.rows.iter().all(|row| !row.text.contains("user 9")));
    }

    #[test]
    fn order_on_equal_bytes_is_the_text() {
        // New: newa and newb each 70, then newc 10. Gone: gonea before goneb, each 80.
        // Both: stay differs by 20, same differs by 0.
        let old_sample = group(
            &[
                line("stay", 10),
                line("gonea", 80),
                line("goneb", 80),
                line("same", 5),
            ],
            None,
        );
        let new_sample = group(
            &[
                line("stay", 30),
                line("same", 5),
                line("newa", 70),
                line("newb", 70),
                line("newc", 10),
            ],
            None,
        );
        let report = compare(&old_sample, &new_sample);
        let texts_2: Vec<&str> = report.rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(
            texts_2,
            vec!["newa", "newb", "newc", "gonea", "goneb", "stay", "same"]
        );
        assert_eq!(report.added_bytes, 150);
        assert_eq!(report.old_bytes, 175);
        assert_eq!(report.new_bytes, 185);
    }

    #[test]
    fn empty_is_not_an_overrun() {
        let report = compare(&group(&[], None), &group(&[], None));
        assert_eq!(report.added_bytes, 0);
        assert!(report.rows.is_empty());
        let zero = parse_percent("0").expect("0");
        assert!(!zero.exceeded_by(0, 0));
        assert!(!zero.exceeded_by(0, 10));
        assert!(zero.exceeded_by(1, 10));
    }

    #[test]
    fn percentage_is_the_fraction() {
        // 6/12 = 50, not more than 50. 2/6 = 1/3 is above 33.3 and below 33.4.
        let half = parse_percent("50").expect("50");
        assert!(!half.exceeded_by(6, 12));
        assert!(parse_percent("49.9").expect("49.9").exceeded_by(6, 12));
        assert!(parse_percent("33.3").expect("33.3").exceeded_by(2, 6));
        assert!(!parse_percent("33.4").expect("33.4").exceeded_by(2, 6));
        assert!(!parse_percent("100")
            .expect("100")
            .exceeded_by(u64::MAX, u64::MAX));
        assert!(parse_percent("99.9").expect("99.9").exceeded_by(5, 5));
        assert_eq!(parse_percent("00.5"), parse_percent(".5"));
        assert!(parse_percent(".5").expect(".5").exceeded_by(1, 100));
        assert!(!parse_percent(".5").expect(".5").exceeded_by(1, 1000));
        // 10^-30. The product part × 100 × 10^30 does not fit in one u128.
        let mini = format!("0.{}1", "0".repeat(29));
        assert!(parse_percent(&mini)
            .expect("mini")
            .exceeded_by(u64::MAX, u64::MAX));
        for text in ["0", "99.9", "100", "100.0", "100.000"] {
            assert!(!parse_percent(text).expect(text).above_hundred(), "{text}");
        }
        for text in ["100.001", "100.1", "101", "1000"] {
            assert!(parse_percent(text).expect(text).above_hundred(), "{text}");
        }
        for text in [
            "", ".", "5.", "10,5", "-1", "+10", "1e2", " 1", "1 ", "1.2.3",
        ] {
            assert!(parse_percent(text).is_none(), "{text}");
        }
    }

    #[test]
    fn wide_product() {
        assert_eq!(mul_wide(3, 5), (0, 15));
        assert_eq!(mul_wide(1 << 64, 1 << 64), (1, 0));
        assert_eq!(mul_wide(1 << 64, (1 << 64) + 1), (1, 1 << 64));
        assert_eq!(mul_wide(1 << 100, 1 << 100), (1 << 72, 0));
        assert_eq!(mul_wide(u128::MAX, 2), (1, u128::MAX - 1));
        assert_eq!(mul_wide(u128::MAX, u128::MAX), (u128::MAX - 1, 1));
    }
}
