//! Costs per source: lines grouped by the value of one JSON field
//! (`--group-by`). Only the counts are kept, never the lines.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::pattern::share_tenths;
use crate::read::Line;

/// Shown for a line without a text value in the field, and for a line whose
/// JSON cannot be parsed again here (for example nested over 128 levels).
pub const NO_SOURCE: &str = "<NO-SOURCE>";

/// One source: its lines and bytes, and its share of the bytes in tenths of a percent.
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub lines: u64,
    pub bytes: u64,
    pub share_tenths: u64,
}

/// Counts lines per value of `field`. Bytes are the raw bytes of the line.
pub struct SourceTally {
    field: String,
    groups: BTreeMap<String, (u64, u64)>,
}

impl SourceTally {
    pub fn new(field: &str) -> Self {
        SourceTally {
            field: field.to_owned(),
            groups: BTreeMap::new(),
        }
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    /// `line.raw` is the whole JSON line. A field that appears twice counts
    /// with its later value, as with `--message-field`.
    pub fn push(&mut self, line: &Line) {
        let name = match serde_json::from_str::<Value>(&line.raw) {
            Ok(Value::Object(map)) => match map.get(&self.field) {
                Some(Value::String(text)) => text.clone(),
                _ => NO_SOURCE.to_owned(),
            },
            _ => NO_SOURCE.to_owned(),
        };
        let entry = self.groups.entry(name).or_default();
        entry.0 += 1;
        entry.1 += line.bytes;
    }

    /// Sort: bytes descending, then name ascending.
    pub fn finish(self) -> Vec<Source> {
        let total: u64 = self.groups.values().map(|entry| entry.1).sum();
        let mut sources: Vec<Source> = self
            .groups
            .into_iter()
            .map(|(name, (lines, bytes))| Source {
                name,
                lines,
                bytes,
                share_tenths: share_tenths(bytes, total),
            })
            .collect();
        sources.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(raw: &str) -> Line {
        Line {
            text: String::new(),
            raw: raw.to_owned(),
            bytes: raw.len() as u64 + 1,
        }
    }

    #[test]
    fn groups_by_value_and_sorts_by_bytes() {
        let mut tally = SourceTally::new("service");
        tally.push(&line(r#"{"service":"b","message":"x"}"#));
        tally.push(&line(r#"{"service":"a","message":"x"}"#));
        tally.push(&line(r#"{"service":"b","message":"xx"}"#));
        let sources = tally.finish();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "b");
        assert_eq!(sources[0].lines, 2);
        assert_eq!(sources[1].name, "a");
        assert_eq!(sources[0].share_tenths + sources[1].share_tenths, 1000);
    }

    #[test]
    fn missing_or_non_text_value_is_no_source() {
        let mut tally = SourceTally::new("service");
        tally.push(&line(r#"{"message":"x"}"#));
        tally.push(&line(r#"{"service":5}"#));
        tally.push(&line(r#"{"service":null}"#));
        tally.push(&line("[1]"));
        let sources = tally.finish();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, NO_SOURCE);
        assert_eq!(sources[0].lines, 4);
    }

    #[test]
    fn later_duplicate_wins() {
        let mut tally = SourceTally::new("s");
        tally.push(&line(r#"{"s":"a","s":"b"}"#));
        assert_eq!(tally.finish()[0].name, "b");
    }

    #[test]
    fn empty_tally_has_no_sources() {
        assert!(SourceTally::new("s").finish().is_empty());
    }
}
