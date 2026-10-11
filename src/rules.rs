use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;

use crate::mask;
use crate::pattern::Stats;
use crate::read::Line;

/// Compiled rule. The source pattern and fraction stay here, never in an error.
struct Compiled {
    line_no: u64,
    kind: Kind,
    regex: Regex,
}

enum Kind {
    Drop,
    /// Part that remains, `0 <= f < 1`, as a decimal fraction.
    Sample(Fraction),
}

/// Value = digits / 10^scale. Digits are little-endian, with no high zeros.
#[derive(Clone, Default)]
struct Fraction {
    digs: Vec<u8>,
    scale: usize,
}

pub struct RuleSet {
    rules: Vec<Compiled>,
}

impl std::fmt::Debug for RuleSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let line_list: Vec<u64> = self.rules.iter().map(|rule| rule.line_no).collect();
        f.debug_struct("RuleSet")
            .field("lines", &line_list)
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RulesError {
    Unreadable,
    /// Not a regular file. The path is not included.
    NotRegular,
    /// Line number in the rules file. The line itself is not included.
    Invalid {
        line: u64,
    },
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RulesError::Unreadable => f.write_str("rules file is not readable"),
            RulesError::NotRegular => f.write_str("rules file is not a regular file"),
            RulesError::Invalid { line } => {
                write!(f, "invalid rule on line {line} of the rules file")
            }
        }
    }
}

impl std::error::Error for RulesError {}

/// Reads a rules file. An unreadable path yields an error without that path.
pub fn load_rules(path: &str) -> Result<RuleSet, RulesError> {
    let data = match crate::read::read_regular(path) {
        Ok(data) => data,
        Err(crate::read::OpenError::NotRegular) => return Err(RulesError::NotRegular),
        Err(crate::read::OpenError::Unreadable) => return Err(RulesError::Unreadable),
    };
    parse_rules(&data)
}

/// `drop <regex>` or `sample <fraction> <regex>`, one per line. Blank lines
/// are ignored. An unknown command, a missing regex, a fraction outside
/// `0..1`, or a regex that does not compile, is invalid on that line.
/// The error text does not contain the line.
pub fn parse_rules(data: &[u8]) -> Result<RuleSet, RulesError> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let mut rules = Vec::new();
    let mut start = 0;
    let mut line_no = 0u64;
    for (index, &byte) in data.iter().enumerate() {
        if byte != b'\n' {
            continue;
        }
        line_no += 1;
        push_rule(&mut rules, &data[start..index], line_no)?;
        start = index + 1;
    }
    if start < data.len() {
        line_no += 1;
        push_rule(&mut rules, &data[start..], line_no)?;
    }
    Ok(RuleSet { rules })
}

fn push_rule(rules: &mut Vec<Compiled>, raw: &[u8], line_no: u64) -> Result<(), RulesError> {
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(());
    }
    let text = std::str::from_utf8(raw).map_err(|_| RulesError::Invalid { line: line_no })?;
    if let Some(rule) = parse_rule(text, line_no)? {
        rules.push(rule);
    }
    Ok(())
}

fn parse_rule(text: &str, line_no: u64) -> Result<Option<Compiled>, RulesError> {
    let text = text.trim_start();
    if text.is_empty() {
        return Ok(None);
    }
    let (verb, rest) = split_word(text);
    match verb {
        "drop" => parse_drop(rest, line_no),
        "sample" => parse_sample(rest, line_no),
        _ => Err(RulesError::Invalid { line: line_no }),
    }
}

fn split_word(text: &str) -> (&str, &str) {
    match text.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim_start()),
        None => (text, ""),
    }
}

fn parse_drop(rest: &str, line_no: u64) -> Result<Option<Compiled>, RulesError> {
    if rest.is_empty() {
        return Err(RulesError::Invalid { line: line_no });
    }
    let regex = Regex::new(rest).map_err(|_| RulesError::Invalid { line: line_no })?;
    Ok(Some(Compiled {
        line_no,
        kind: Kind::Drop,
        regex,
    }))
}

fn parse_sample(rest: &str, line_no: u64) -> Result<Option<Compiled>, RulesError> {
    let invalid = || RulesError::Invalid { line: line_no };
    let (token, regex_src) = match rest.split_once(char::is_whitespace) {
        Some((token, regex_src)) => (token, regex_src.trim_start()),
        None => return Err(invalid()),
    };
    if token.is_empty() || regex_src.is_empty() {
        return Err(invalid());
    }
    let fraction = parse_fraction(token).map_err(|_| invalid())?;
    let regex = Regex::new(regex_src).map_err(|_| invalid())?;
    Ok(Some(Compiled {
        line_no,
        kind: Kind::Sample(fraction),
        regex,
    }))
}

/// Decimal fraction in `0..1`: 0 counts, 1 does not. No comma, no exponent.
fn parse_fraction(token: &str) -> Result<Fraction, ()> {
    if token.starts_with('-') {
        return Err(());
    }
    let token = token.strip_prefix('+').unwrap_or(token);
    if token.is_empty() {
        return Err(());
    }
    let (whole, frac) = match token.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (token, ""),
    };
    if (whole.is_empty() && frac.is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
        || whole.bytes().any(|byte| byte != b'0')
    {
        return Err(());
    }
    let mut digs = Vec::with_capacity(frac.len());
    for byte in frac.bytes().rev() {
        digs.push(byte - b'0');
    }
    let mut fraction = Fraction {
        digs,
        scale: frac.len(),
    };
    fraction.normalize();
    Ok(fraction)
}

impl Fraction {
    fn is_zero(&self) -> bool {
        self.digs.is_empty()
    }

    /// Adds `bytes × fraction` to this sum. The sum stays an exact fraction.
    fn add_mul(&mut self, bytes: u64, fraction: &Fraction) {
        if bytes == 0 || fraction.is_zero() {
            return;
        }
        let mut digs = fraction.digs.clone();
        mul_digits(&mut digs, bytes);
        self.add_scaled(digs, fraction.scale);
    }

    fn add_scaled(&mut self, digs: Vec<u8>, scale: usize) {
        if digs.is_empty() {
            return;
        }
        let target = self.scale.max(scale);
        self.digs = align_scale(&self.digs, self.scale, target);
        let extra = align_scale(&digs, scale, target);
        add_digits(&mut self.digs, &extra);
        self.scale = target;
        self.normalize();
    }

    fn normalize(&mut self) {
        while self.scale > 0 && self.digs.first() == Some(&0) {
            self.digs.remove(0);
            self.scale -= 1;
        }
        while self.digs.last() == Some(&0) {
            self.digs.pop();
        }
        if self.digs.is_empty() {
            self.scale = 0;
        }
    }

    /// Integer part. A pattern's sum fits in u64: at most the raw bytes.
    fn floor_u64(&self) -> u64 {
        if self.digs.len() <= self.scale {
            return 0;
        }
        let mut value = 0u128;
        for &digit in self.digs[self.scale..].iter().rev() {
            value = value * 10 + u128::from(digit);
        }
        u64::try_from(value).expect("expected bytes fit in u64")
    }
}

fn align_scale(digs: &[u8], from: usize, to: usize) -> Vec<u8> {
    let mut out = vec![0; to - from];
    out.extend_from_slice(digs);
    out
}

fn mul_digits(digs: &mut Vec<u8>, factor: u64) {
    let mut carry = 0u128;
    for digit in digs.iter_mut() {
        let value = u128::from(*digit) * u128::from(factor) + carry;
        *digit = (value % 10) as u8;
        carry = value / 10;
    }
    while carry > 0 {
        digs.push((carry % 10) as u8);
        carry /= 10;
    }
    while digs.last() == Some(&0) {
        digs.pop();
    }
}

fn add_digits(left: &mut Vec<u8>, right: &[u8]) {
    let mut carry = 0u8;
    let len = left.len().max(right.len());
    left.resize(len, 0);
    for (index, digit) in left.iter_mut().enumerate() {
        let sum = u16::from(*digit)
            + u16::from(right.get(index).copied().unwrap_or(0))
            + u16::from(carry);
        *digit = (sum % 10) as u8;
        carry = (sum / 10) as u8;
    }
    if carry > 0 {
        left.push(carry);
    }
}

enum Hit<'a> {
    Drop(u64),
    Sample(u64, &'a Fraction),
}

impl RuleSet {
    /// Number of rules. Blank lines in the file do not count.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// First rule that matches the raw line. `None` when none matches.
    fn hit(&self, raw: &str) -> Option<Hit<'_>> {
        self.rules
            .iter()
            .find(|rule| rule.regex.is_match(raw))
            .map(|rule| match &rule.kind {
                Kind::Drop => Hit::Drop(rule.line_no),
                Kind::Sample(fraction) => Hit::Sample(rule.line_no, fraction),
            })
    }
}

/// Old and new per pattern after drop and sample. `stats` is from `pattern::group`
/// on the same `lines` and the same top-N.
#[derive(Debug, PartialEq, Eq)]
pub struct Effect {
    pub old_lines: u64,
    pub old_bytes: u64,
    pub new_lines: u64,
    pub new_bytes: u64,
    pub patterns: Vec<PatternEffect>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PatternEffect {
    pub id: String,
    pub text: String,
    pub old_lines: u64,
    pub old_bytes: u64,
    pub new_lines: u64,
    pub new_bytes: u64,
    /// Rules-file line numbers that matched this pattern, in ascending order.
    pub rule_lines: Vec<u64>,
}

#[derive(Default)]
struct Acc {
    old_lines: u64,
    old_bytes: u64,
    /// Lines where no rule matches.
    kept_lines: u64,
    /// Bytes of lines where no rule matches.
    kept_bytes: u64,
    /// Sum of 1 × fraction of the sampled lines, not yet rounded.
    expected_lines: Fraction,
    /// Sum of bytes × fraction of the sampled lines, not yet rounded.
    expected_bytes: Fraction,
    rules: BTreeSet<u64>,
}

/// Pattern after rounding. Safe to add: each pattern fraction was already floored.
#[derive(Default)]
struct Tallied {
    old_lines: u64,
    old_bytes: u64,
    new_lines: u64,
    new_bytes: u64,
    rules: BTreeSet<u64>,
}

impl Acc {
    /// Untouched lines stay whole. Sampled lines go, per pattern, to
    /// `floor(count × fraction)` and `floor(bytes × fraction)`, rounded once.
    fn finish(self) -> Tallied {
        Tallied {
            old_lines: self.old_lines,
            old_bytes: self.old_bytes,
            new_lines: self.kept_lines + self.expected_lines.floor_u64(),
            new_bytes: self.kept_bytes + self.expected_bytes.floor_u64(),
            rules: self.rules,
        }
    }
}

impl Tallied {
    fn add(&mut self, other: &Tallied) {
        self.old_lines += other.old_lines;
        self.old_bytes += other.old_bytes;
        self.new_lines += other.new_lines;
        self.new_bytes += other.new_bytes;
        self.rules.extend(&other.rules);
    }

    fn to_pattern(&self, id: &str, text: &str) -> PatternEffect {
        PatternEffect {
            id: id.to_owned(),
            text: text.to_owned(),
            old_lines: self.old_lines,
            old_bytes: self.old_bytes,
            new_lines: self.new_lines,
            new_bytes: self.new_bytes,
            rule_lines: self.rules.iter().copied().collect(),
        }
    }
}

pub fn effect(lines: &[Line], rules: &RuleSet, stats: &Stats) -> Effect {
    let mut groups: BTreeMap<String, Acc> = BTreeMap::new();
    for line in lines {
        let acc = groups.entry(mask::mask(&line.text)).or_default();
        acc.old_lines += 1;
        acc.old_bytes += line.bytes;
        match rules.hit(&line.raw) {
            Some(Hit::Drop(line_no)) => {
                acc.rules.insert(line_no);
            }
            Some(Hit::Sample(line_no, fraction)) => {
                acc.rules.insert(line_no);
                acc.expected_lines.add_mul(1, fraction);
                acc.expected_bytes.add_mul(line.bytes, fraction);
            }
            None => {
                acc.kept_lines += 1;
                acc.kept_bytes += line.bytes;
            }
        }
    }
    // Round each real pattern first, then add the other bucket.
    let groups: BTreeMap<String, Tallied> = groups
        .into_iter()
        .map(|(text, acc)| (text, acc.finish()))
        .collect();
    let shown: BTreeSet<&str> = stats
        .patterns
        .iter()
        .filter(|pattern| !pattern.rest)
        .map(|pattern| pattern.text.as_str())
        .collect();
    let patterns = stats
        .patterns
        .iter()
        .map(|pattern| {
            if pattern.rest {
                let mut acc = Tallied::default();
                for (text, group) in &groups {
                    if !shown.contains(text.as_str()) {
                        acc.add(group);
                    }
                }
                acc.to_pattern(&pattern.id, &pattern.text)
            } else {
                groups
                    .get(&pattern.text)
                    .map(|acc| acc.to_pattern(&pattern.id, &pattern.text))
                    .unwrap_or_else(|| Tallied::default().to_pattern(&pattern.id, &pattern.text))
            }
        })
        .collect::<Vec<_>>();
    debug_assert_eq!(
        patterns
            .iter()
            .map(|pattern| pattern.old_bytes)
            .sum::<u64>(),
        stats.total_bytes
    );
    Effect {
        old_lines: stats.total_lines,
        old_bytes: stats.total_bytes,
        new_lines: groups.values().map(|acc| acc.new_lines).sum(),
        new_bytes: groups.values().map(|acc| acc.new_bytes).sum(),
        patterns,
    }
}

#[cfg(test)]
mod tests {
    use super::{effect, parse_rules, RulesError};
    use crate::pattern::group;
    use crate::read::Line;

    fn sample_line(text: &str, bytes: u64) -> Line {
        Line {
            text: text.to_owned(),
            raw: text.to_owned(),
            bytes,
        }
    }

    #[test]
    fn ten_lines_four_dropped() {
        // 4 × the dropped line = 4 × 4 = 16 bytes, dropped by an anchored regex.
        // 6 × the kept line = 6 × 6 = 36 bytes, kept.
        // Total 10 lines, 52 bytes. New 6 lines, 36 bytes. Savings 16.
        let mut lines = Vec::new();
        for _ in 0..4 {
            lines.push(sample_line("weg", 4));
        }
        for _ in 0..6 {
            lines.push(sample_line("blijf", 6));
        }
        let rules = parse_rules(b"drop ^weg\x24\n").unwrap();
        let stats = group(&lines, None);
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.old_lines, 10);
        assert_eq!(effect.old_bytes, 52);
        assert_eq!(effect.new_lines, 6);
        assert_eq!(effect.new_bytes, 36);
        assert_eq!(effect.patterns.len(), 2);

        let kept = &effect.patterns[0];
        assert_eq!(kept.text, "blijf");
        assert_eq!(
            (
                kept.old_lines,
                kept.old_bytes,
                kept.new_lines,
                kept.new_bytes
            ),
            (6, 36, 6, 36)
        );
        assert!(kept.rule_lines.is_empty());

        let gone = &effect.patterns[1];
        assert_eq!(gone.text, "weg");
        assert_eq!(
            (
                gone.old_lines,
                gone.old_bytes,
                gone.new_lines,
                gone.new_bytes
            ),
            (4, 16, 0, 0)
        );
        assert_eq!(gone.rule_lines, vec![1]);
    }

    #[test]
    fn first_matching_rule_wins() {
        // `user 1` matches rules 1, 2, and 3. The first wins: rule 1, not 3.
        // `user 2` and `user 3` match 2 and 3. Rule 2 wins.
        // `vast` matches only rule 3 (`.`).
        // Each `user N\n` weighs 7, `vast\n` weighs 5. Everything is dropped: 26 bytes.
        let lines = vec![
            sample_line("user 1", 7),
            sample_line("user 2", 7),
            sample_line("user 3", 7),
            sample_line("vast", 5),
        ];
        let rules = parse_rules(b"drop user 1\ndrop user\ndrop .\n").unwrap();
        let stats = group(&lines, None);
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.old_lines, 4);
        assert_eq!(effect.old_bytes, 26);
        assert_eq!(effect.new_lines, 0);
        assert_eq!(effect.new_bytes, 0);

        let user = &effect.patterns[0];
        assert_eq!(user.text, "user <NUM>");
        assert_eq!(user.old_bytes, 21);
        assert_eq!(user.new_bytes, 0);
        assert_eq!(user.rule_lines, vec![1, 2]);

        let vast = &effect.patterns[1];
        assert_eq!(vast.text, "vast");
        assert_eq!(vast.old_bytes, 5);
        assert_eq!(vast.rule_lines, vec![3]);
    }

    #[test]
    fn regex_tests_the_raw_line_not_the_mask() {
        // The mask of `user 1` is `user <NUM>`. That tag is not in the raw line.
        let lines = vec![sample_line("user 1", 7)];
        let mask_set = parse_rules(b"drop <NUM>\n").unwrap();
        let stats = group(&lines, None);
        assert_eq!(effect(&lines, &mask_set, &stats).new_bytes, 7);

        let raw_text = parse_rules(b"drop user 1\n").unwrap();
        assert_eq!(effect(&lines, &raw_text, &stats).new_bytes, 0);

        // JSON: the message is `ok`, the marker is only in the raw line.
        let json = vec![Line {
            text: "ok".to_owned(),
            raw: "{\"message\":\"ok\",\"marker\":\"WEG\"}".to_owned(),
            bytes: 32,
        }];
        let stats = group(&json, None);
        let rules = parse_rules(b"drop WEG\n").unwrap();
        let gone = effect(&json, &rules, &stats);
        assert_eq!(gone.old_bytes, 32);
        assert_eq!(gone.new_bytes, 0);
        assert_eq!(gone.patterns[0].rule_lines, vec![1]);

        let rules_drop_ok = parse_rules(b"drop ^ok\x24\n").unwrap();
        assert_eq!(effect(&json, &rules_drop_ok, &stats).new_bytes, 32);
    }

    #[test]
    fn top_n_other_even_when_a_pattern_has_that_name() {
        // `other` is the largest pattern (10 bytes) and stays.
        // `bb` (3, dropped) and `cc` (2) become the rest bucket, also called `other`.
        // Rest: old 5, new 2, savings 3. Total old 15, new 12.
        let lines = vec![
            sample_line("other", 10),
            sample_line("bb", 3),
            sample_line("cc", 2),
        ];
        let rules = parse_rules(b"drop ^bb\x24\n").unwrap();
        let stats = group(&lines, Some(1));
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.old_bytes, 15);
        assert_eq!(effect.new_bytes, 12);
        assert_eq!(effect.patterns.len(), 2);

        let genoemd = &effect.patterns[0];
        assert_eq!(genoemd.text, "other");
        assert_eq!(genoemd.id, crate::pattern::pattern_id("other"));
        assert_eq!(genoemd.old_bytes, 10);
        assert_eq!(genoemd.new_bytes, 10);
        assert!(genoemd.rule_lines.is_empty());

        let rest = &effect.patterns[1];
        assert_eq!(rest.id, "other");
        assert_eq!(rest.old_lines, 2);
        assert_eq!(rest.old_bytes, 5);
        assert_eq!(rest.new_lines, 1);
        assert_eq!(rest.new_bytes, 2);
        assert_eq!(rest.rule_lines, vec![1]);
    }

    #[test]
    fn space_in_the_regex_stays() {
        // `hallo wereld\n` = 13 bytes, `hallowereld\n` = 12.
        let lines = vec![
            sample_line("hallo wereld", 13),
            sample_line("hallowereld", 12),
        ];
        let rules = parse_rules(b"drop hallo wereld\n").unwrap();
        let stats = group(&lines, None);
        let with_space = effect(&lines, &rules, &stats);
        let with = with_space
            .patterns
            .iter()
            .find(|pattern| pattern.text == "hallo wereld")
            .unwrap();
        let without = with_space
            .patterns
            .iter()
            .find(|pattern| pattern.text == "hallowereld")
            .unwrap();
        assert_eq!(with.old_bytes, 13);
        assert_eq!(with.new_bytes, 0);
        assert_eq!(without.old_bytes, 12);
        assert_eq!(without.new_bytes, 12);

        // The space after `einde` belongs to the regex. `einde \n` = 7, `einde\n` = 6.
        let end = parse_rules(b"drop einde \n").unwrap();
        let lines = vec![sample_line("einde ", 7), sample_line("einde", 6)];
        let stats = group(&lines, None);
        let with_end = effect(&lines, &end, &stats);
        assert_eq!(
            with_end
                .patterns
                .iter()
                .find(|pattern| pattern.text == "einde ")
                .unwrap()
                .new_bytes,
            0
        );
        assert_eq!(
            with_end
                .patterns
                .iter()
                .find(|pattern| pattern.text == "einde")
                .unwrap()
                .new_bytes,
            6
        );
    }

    #[test]
    fn bom_indent_and_crlf_count_the_line_number() {
        // BOM, blank line 1, the drop is on line 2. The 4-byte line is dropped.
        let data = b"\xef\xbb\xbf\r\n  drop ^weg\x24\r\n";
        let rules = parse_rules(data).unwrap();
        let lines = vec![sample_line("weg", 4), sample_line("blijf", 6)];
        let stats = group(&lines, None);
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.new_bytes, 6);
        let gone = effect
            .patterns
            .iter()
            .find(|pattern| pattern.text == "weg")
            .unwrap();
        assert_eq!(gone.rule_lines, vec![2]);
        assert_eq!(gone.new_bytes, 0);
    }

    #[test]
    fn error_shows_no_content() {
        let unknown = parse_rules(b"drop ^ok\x24\nkeep CANARY-REGEX-0001\n").unwrap_err();
        assert_eq!(unknown, RulesError::Invalid { line: 2 });
        assert_eq!(
            unknown.to_string(),
            "invalid rule on line 2 of the rules file"
        );
        assert!(!unknown.to_string().contains("CANARY"));
        assert!(!unknown.to_string().contains("keep"));

        let regex = parse_rules(b"\ndrop CANARY-REGEX-0002[\n").unwrap_err();
        assert_eq!(regex, RulesError::Invalid { line: 2 });
        assert!(!regex.to_string().contains("CANARY"));

        let empty = parse_rules(b"drop\n").unwrap_err();
        assert_eq!(empty, RulesError::Invalid { line: 1 });
        assert!(!empty.to_string().contains("drop"));

        let bytes = parse_rules(b"drop ^a\x24\n\xff\n").unwrap_err();
        assert_eq!(bytes, RulesError::Invalid { line: 2 });
        assert!(!bytes.to_string().contains('\u{FFFD}'));
    }

    #[test]
    fn sample_0_1_bytes_and_savings() {
        // One rule `sample 0.1`. The fraction is the part that remains.
        // `aa\n` = 3 bytes, 10 lines = 30. Bytes 30 × 1/10 = 3. Lines floor(10 × 1/10) = 1.
        // Flooring per line would give 0 bytes and 0 lines; it is per pattern.
        // Savings 9 lines, 27 bytes.
        // `abcdefghijklmn\n` = 15 bytes, 1 line. 15 × 1/10 = 1.5 → 1 byte. floor(0.1) = 0 lines.
        // `bb\n` = 3 bytes, 3 lines = 9. 9 × 1/10 = 0.9 → 0 bytes. floor(0.3) = 0 lines.
        // The unmatched line = 6 bytes, 2 lines = 12, no match, stays 2 lines and 12 bytes.
        // Old 16 lines, 66 bytes. New 1+0+0+2 = 3 lines, 3+1+0+12 = 16 bytes.
        // Savings 13 lines, 50 bytes.
        let mut lines = Vec::new();
        for _ in 0..10 {
            lines.push(sample_line("aa", 3));
        }
        lines.push(sample_line("abcdefghijklmn", 15));
        for _ in 0..3 {
            lines.push(sample_line("bb", 3));
        }
        for _ in 0..2 {
            lines.push(sample_line("blijf", 6));
        }
        let rules = parse_rules(b"sample 0.1 ^(aa|bb|abcdefghijklmn)\x24\n").unwrap();
        let stats = group(&lines, None);
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.old_lines, 16);
        assert_eq!(effect.old_bytes, 66);
        assert_eq!(effect.new_lines, 3);
        assert_eq!(effect.new_bytes, 16);
        assert_eq!(effect.old_lines - effect.new_lines, 13);
        assert_eq!(effect.old_bytes - effect.new_bytes, 50);

        let aa = effect.patterns.iter().find(|p| p.text == "aa").unwrap();
        assert_eq!(
            (aa.old_lines, aa.old_bytes, aa.new_lines, aa.new_bytes),
            (10, 30, 1, 3)
        );
        assert_eq!(aa.old_lines - aa.new_lines, 9);
        assert_eq!(aa.old_bytes - aa.new_bytes, 27);
        assert_eq!(aa.rule_lines, vec![1]);

        let half = effect
            .patterns
            .iter()
            .find(|p| p.text == "abcdefghijklmn")
            .unwrap();
        assert_eq!(
            (
                half.old_lines,
                half.new_lines,
                half.old_bytes,
                half.new_bytes
            ),
            (1, 0, 15, 1)
        );
        assert_eq!(half.rule_lines, vec![1]);

        let bb = effect.patterns.iter().find(|p| p.text == "bb").unwrap();
        assert_eq!(
            (bb.old_lines, bb.new_lines, bb.old_bytes, bb.new_bytes),
            (3, 0, 9, 0)
        );
        assert_eq!(bb.rule_lines, vec![1]);

        let kept = effect.patterns.iter().find(|p| p.text == "blijf").unwrap();
        assert_eq!((kept.old_lines, kept.new_lines, kept.new_bytes), (2, 2, 12));
        assert!(kept.rule_lines.is_empty());
    }

    #[test]
    fn sample_0_on_ten_aa_gives_zero_lines_and_zero_bytes() {
        // 10 × `aa` of 3 bytes = 30. sample 0: floor(10 × 0) = 0 lines, floor(30 × 0) = 0 bytes.
        // Not 10 lines and 0 bytes.
        let lines: Vec<_> = (0..10).map(|_| sample_line("aa", 3)).collect();
        let stats = group(&lines, None);
        let rules = parse_rules(b"sample 0 ^aa\x24\n").unwrap();
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.old_lines, 10);
        assert_eq!(effect.old_bytes, 30);
        assert_eq!(effect.new_lines, 0);
        assert_eq!(effect.new_bytes, 0);
        assert_eq!(effect.patterns.len(), 1);
        assert_eq!(
            (
                effect.patterns[0].old_lines,
                effect.patterns[0].new_lines,
                effect.patterns[0].old_bytes,
                effect.patterns[0].new_bytes
            ),
            (10, 0, 30, 0)
        );
        assert_eq!(effect.patterns[0].rule_lines, vec![1]);
    }

    #[test]
    fn drop_and_sample_first_match_wins() {
        // `user 1\n` = 7, 4 lines = 28. `user 2\n` = 7, 5 lines = 35.
        // Both mask to `user <NUM>`.
        // Rule 1 drop beats rule 2 sample: the 28 bytes become 0, not floor(2.8)=2.
        // `user 2` falls only under sample 0.1: 35 × 1/10 = 3.5 → 3 bytes.
        // Lines: floor(5 × 1/10) = 0, not the 5 whole lines.
        // Not 5 × floor(0.7) = 0 bytes and not floor((28+35)×1/10) = 6.
        // Pattern: old 9 lines, 63 bytes; new 0 lines, 3 bytes. Rules 1 and 2.
        // The dropped line = 4, 2 lines = 8, rule 3 drop → 0. The kept line = 6 stays 1 line, 6 bytes.
        // Old 12 lines, 77 bytes. New 1 line, 9 bytes. Savings 11 lines, 68 bytes.
        let mut lines = Vec::new();
        for _ in 0..4 {
            lines.push(sample_line("user 1", 7));
        }
        for _ in 0..5 {
            lines.push(sample_line("user 2", 7));
        }
        for _ in 0..2 {
            lines.push(sample_line("weg", 4));
        }
        lines.push(sample_line("blijf", 6));
        let eerst_drop =
            parse_rules(b"drop ^user 1\x24\nsample 0.1 ^user\ndrop ^weg\x24\n").unwrap();
        let stats = group(&lines, None);
        let outcome = effect(&lines, &eerst_drop, &stats);
        assert_eq!(outcome.old_lines, 12);
        assert_eq!(outcome.old_bytes, 77);
        assert_eq!(outcome.new_lines, 1);
        assert_eq!(outcome.new_bytes, 9);
        assert_eq!(outcome.old_lines - outcome.new_lines, 11);
        assert_eq!(outcome.old_bytes - outcome.new_bytes, 68);

        let user = outcome
            .patterns
            .iter()
            .find(|p| p.text == "user <NUM>")
            .unwrap();
        assert_eq!(
            (
                user.old_lines,
                user.old_bytes,
                user.new_lines,
                user.new_bytes
            ),
            (9, 63, 0, 3)
        );
        assert_eq!(user.rule_lines, vec![1, 2]);

        let gone = outcome.patterns.iter().find(|p| p.text == "weg").unwrap();
        assert_eq!((gone.new_bytes, gone.rule_lines.as_slice()), (0, &[3][..]));
        let kept = outcome.patterns.iter().find(|p| p.text == "blijf").unwrap();
        assert_eq!(kept.new_bytes, 6);
        assert!(kept.rule_lines.is_empty());

        // Same rules reversed: sample wins for `user 1`.
        // 28 × 1/10 = 2.8 → 2 bytes, not the drop to 0.
        // Lines: floor(4 × 1/10) = 0, not the 4 whole lines.
        // `user 2` now falls under drop: 0, not floor(3.5)=3.
        // New for the pattern: 0 lines, 2 bytes. Rules 1 and 2.
        let eerst_sample = parse_rules(b"sample 0.1 ^user 1\x24\ndrop ^user\n").unwrap();
        let reversed = effect(&lines, &eerst_sample, &stats);
        let user = reversed
            .patterns
            .iter()
            .find(|p| p.text == "user <NUM>")
            .unwrap();
        assert_eq!((user.old_bytes, user.new_bytes), (63, 2));
        assert_eq!(user.new_lines, 0);
        assert_eq!(user.rule_lines, vec![1, 2]);
    }

    #[test]
    fn two_fractions_on_one_pattern_one_rounding() {
        // `item 1`, `item 2`, and `item 3` become one pattern `item <NUM>`, 7 bytes each.
        // Rule 1: 7 × 1/2 = 3.5. Rule 2: 7 × 1/10 = 0.7. `item 3` stays 7.
        // Sum 11.2 → floored once per pattern: 11. Savings 10.
        // Rounding separately would be floor(3.5)+floor(0.7)+7 = 10.
        // Lines the same way: 1 × 1/2 + 1 × 1/10 = 0.6 → 0, plus the untouched line = 1.
        // Not 3, and not floor per line (here that is also 0 + 0 + 1).
        let lines = vec![
            sample_line("item 1", 7),
            sample_line("item 2", 7),
            sample_line("item 3", 7),
        ];
        let rules = parse_rules(b"sample 0.5 ^item 1\x24\nsample 0.1 ^item 2\x24\n").unwrap();
        let stats = group(&lines, None);
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.patterns.len(), 1);
        let item = &effect.patterns[0];
        assert_eq!(item.text, "item <NUM>");
        assert_eq!((item.old_bytes, item.new_bytes), (21, 11));
        assert_eq!(item.old_bytes - item.new_bytes, 10);
        assert_eq!(item.new_lines, 1);
        assert_eq!(item.rule_lines, vec![1, 2]);
        assert_eq!(effect.new_lines, 1);
        assert_eq!(effect.new_bytes, 11);
    }

    #[test]
    fn other_sums_per_pattern_rounded_bytes() {
        // top 1 keeps `zzzz` (1 line, 100 bytes). `aa` and `bb` are 1 line, 15 bytes, at 0.1.
        // Bytes: floor(1.5)+floor(1.5)=1+1=2. Rounding together would be floor(3.0)=3.
        // Lines: floor(0.1)+floor(0.1)=0. Not the 2 whole lines.
        // Old other: 2 lines, 30 bytes; new 0 lines, 2 bytes. Total old 3 lines, 130 bytes;
        // new 1 line, 102 bytes.
        let lines = vec![
            sample_line("zzzz", 100),
            sample_line("aa", 15),
            sample_line("bb", 15),
        ];
        let rules = parse_rules(b"sample 0.1 ^(aa|bb)\x24\n").unwrap();
        let stats = group(&lines, Some(1));
        let effect = effect(&lines, &rules, &stats);
        assert_eq!(effect.old_lines, 3);
        assert_eq!(effect.old_bytes, 130);
        assert_eq!(effect.new_lines, 1);
        assert_eq!(effect.new_bytes, 102);
        assert_eq!(effect.patterns.len(), 2);
        assert_eq!(effect.patterns[0].text, "zzzz");
        assert_eq!(effect.patterns[0].new_lines, 1);
        assert_eq!(effect.patterns[0].new_bytes, 100);
        let rest = &effect.patterns[1];
        assert_eq!(rest.id, "other");
        assert_eq!(
            (
                rest.old_lines,
                rest.new_lines,
                rest.old_bytes,
                rest.new_bytes
            ),
            (2, 0, 30, 2)
        );
        assert_eq!(rest.old_bytes - rest.new_bytes, 28);
        assert_eq!(rest.rule_lines, vec![1]);
    }

    #[test]
    fn fraction_inside_0_to_1_and_outside() {
        // Valid is 0 <= f < 1 (`0..1`): 0 leaves 0 lines and 0 bytes, 1 is outside.
        // One 10-byte line. floor(1 × 0) = 0 lines. 10 × 1/10 = 1 byte; floor(0.1) = 0 lines.
        // 10 × 999/1000 = 9 bytes and floor(0.999) = 0 lines.
        let lines = vec![sample_line("a", 10)];
        let stats = group(&lines, None);
        let zero = parse_rules(b"sample 0 ^a\x24\n").unwrap();
        let outcome = effect(&lines, &zero, &stats);
        assert_eq!(outcome.new_lines, 0);
        assert_eq!(outcome.new_bytes, 0);
        assert_eq!(outcome.old_lines - outcome.new_lines, 1);
        assert_eq!(outcome.old_bytes - outcome.new_bytes, 10);

        for source in [
            "sample 0.1 ^a\x24\n",
            "sample 0.100 ^a\x24\n",
            "sample .1 ^a\x24\n",
            "sample +0.1 ^a\x24\n",
            "sample 00.100 ^a\x24\n",
        ] {
            let rules = parse_rules(source.as_bytes()).unwrap();
            assert_eq!(effect(&lines, &rules, &stats).new_bytes, 1, "{source}");
        }
        let bijna = parse_rules(b"sample 0.999 ^a\x24\n").unwrap();
        assert_eq!(effect(&lines, &bijna, &stats).new_bytes, 9);

        // Rule 1 is valid, so a fraction that is too large on line 2 keeps that number.
        let too_large = parse_rules(b"sample 0.1 ^a\x24\nsample 1 ^b\x24\n").unwrap_err();
        assert_eq!(too_large, RulesError::Invalid { line: 2 });

        for source in [
            "sample 1 ^a\x24\n",
            "sample 1.0 ^a\x24\n",
            "sample 1.000 ^a\x24\n",
            "sample 2 ^a\x24\n",
            "sample -0.1 ^a\x24\n",
            "sample 1.5 CANARY-FRAC-0001\n",
            "sample 0.1\n",
            "sample 0.1 [\n",
        ] {
            let err = parse_rules(source.as_bytes()).unwrap_err();
            assert_eq!(err, RulesError::Invalid { line: 1 }, "{source}");
            let text_body = err.to_string();
            assert_eq!(text_body, "invalid rule on line 1 of the rules file");
            assert!(!text_body.contains("CANARY"));
            assert!(!text_body.contains("1.5"));
            assert!(!text_body.contains("sample"));
            assert!(!text_body.contains('['));
        }
    }

    #[test]
    fn sample_stays_exact_with_many_decimals_and_large_bytes() {
        // 0 with 40 nines = 1 - 10^-40.
        // 1000 × that fraction = 1000 - 10^-37 → 999. Savings 1.
        let mut source = b"sample 0.".to_vec();
        source.extend(std::iter::repeat_n(b'9', 40));
        source.extend(b" ^x\x24\n");
        let rules = parse_rules(&source).unwrap();
        let lines = vec![sample_line("x", 1000)];
        let stats = group(&lines, None);
        let outcome = effect(&lines, &rules, &stats);
        assert_eq!(outcome.new_bytes, 999);
        assert_eq!(outcome.old_bytes - outcome.new_bytes, 1);

        // u64::MAX = 18446744073709551615 = 10×q + 5, q = 1844674407370955161.
        // × 1/10 = q + 0.5 → q. Savings 9×q + 5 = 16602069666338596454.
        let large = parse_rules(b"sample 0.1 ^x\x24\n").unwrap();
        let lines = vec![sample_line("x", u64::MAX)];
        let stats = group(&lines, None);
        let outcome = effect(&lines, &large, &stats);
        assert_eq!(outcome.new_bytes, 1_844_674_407_370_955_161);
        assert_eq!(
            outcome.old_bytes - outcome.new_bytes,
            16_602_069_666_338_596_454
        );
    }
}
