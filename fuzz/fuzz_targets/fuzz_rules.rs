#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use log_bill::pattern::Grouper;
use log_bill::read::Line;
use log_bill::rules::{self, Folder};

#[derive(Debug, Arbitrary)]
struct Case {
    rules_src: Vec<u8>,
    line: Vec<u8>,
}

// Rule parsing and application: a rules file of arbitrary bytes never
// panics to parse, and applying it never grows a pattern's lines or bytes.
fuzz_target!(|data: &[u8]| {
    let _ = rules::parse_rules(data);

    let Ok(case) = Case::arbitrary_take_rest(Unstructured::new(data)) else {
        return;
    };
    let Ok(rule_set) = rules::parse_rules(&case.rules_src) else {
        return;
    };
    let n = case.line.len().min(4096);
    let text = String::from_utf8_lossy(&case.line[..n]).into_owned();
    let line = Line {
        text: text.clone(),
        raw: text,
        bytes: n as u64 + 1,
    };

    let mut grouper = Grouper::default();
    grouper.push(&line);
    let stats = grouper.finish(None);

    let mut folder = Folder::default();
    folder.push(&line, &rule_set);
    let effect = folder.finish(&stats);

    assert_eq!(effect.old_lines, stats.total_lines);
    assert_eq!(effect.old_bytes, stats.total_bytes);
    assert!(effect.new_lines <= effect.old_lines);
    assert!(effect.new_bytes <= effect.old_bytes);
    for pattern in &effect.patterns {
        assert!(pattern.new_lines <= pattern.old_lines);
        assert!(pattern.new_bytes <= pattern.old_bytes);
    }
});
