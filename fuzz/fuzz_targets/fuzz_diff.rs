#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use log_bill::diff;
use log_bill::pattern::{Grouper, Stats};
use log_bill::read::Line;

#[derive(Debug, Arbitrary)]
struct Case {
    old_lines: Vec<Vec<u8>>,
    new_lines: Vec<Vec<u8>>,
    percent_src: Vec<u8>,
    part: u64,
    total: u64,
}

fn build_stats(raw_lines: &[Vec<u8>]) -> Stats {
    let mut grouper = Grouper::default();
    for raw in raw_lines.iter().take(32) {
        let n = raw.len().min(256);
        let text = String::from_utf8_lossy(&raw[..n]).into_owned();
        if text.is_empty() {
            continue;
        }
        let line = Line {
            text: text.clone(),
            raw: text,
            bytes: n as u64 + 1,
        };
        grouper.push(&line);
    }
    grouper.finish(None)
}

// Diff: comparing two arbitrary samples never panics, the reported totals
// match the inputs, and no row's bytes exceed its sample's total.
fuzz_target!(|data: &[u8]| {
    let Ok(case) = Case::arbitrary_take_rest(Unstructured::new(data)) else {
        return;
    };
    let old = build_stats(&case.old_lines);
    let new = build_stats(&case.new_lines);
    let report = diff::compare(&old, &new);

    assert_eq!(report.old_bytes, old.total_bytes);
    assert_eq!(report.new_bytes, new.total_bytes);
    assert!(report.added_bytes <= report.new_bytes);
    for row in &report.rows {
        assert!(row.old_bytes <= report.old_bytes);
        assert!(row.new_bytes <= report.new_bytes);
    }

    let n = case.percent_src.len().min(16);
    let percent_text = String::from_utf8_lossy(&case.percent_src[..n]).into_owned();
    if let Some(percent) = diff::parse_percent(&percent_text) {
        let _ = percent.exceeded_by(case.part, case.total);
    }
});
