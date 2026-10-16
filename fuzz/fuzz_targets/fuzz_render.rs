#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use log_bill::diff;
use log_bill::pattern::{Grouper, Stats};
use log_bill::read::Line;
use log_bill::render::{self, JsonMeta};

#[derive(Debug, Arbitrary)]
struct Case {
    old_lines: Vec<Vec<u8>>,
    new_lines: Vec<Vec<u8>>,
    secret: Vec<u8>,
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

fn meta() -> JsonMeta<'static> {
    JsonMeta {
        command: "scan",
        format: "text",
        message_field: None,
        days_per_month: 30,
        top: None,
        paths: &[],
        window: None,
        explain: true,
    }
}

fn assert_valid_json(label: &str, body: &str) {
    if let Err(err) = serde_json::from_str::<serde_json::Value>(body) {
        panic!("{label} produced invalid JSON: {err}\n{body}");
    }
}

// Render to human text and JSON: never panics, every JSON body it produces
// parses back, and a secret masked out of a pattern never resurfaces in
// either the text or the JSON rendering.
fuzz_target!(|data: &[u8]| {
    let Ok(case) = Case::arbitrary_take_rest(Unstructured::new(data)) else {
        return;
    };
    let old = build_stats(&case.old_lines);
    let new = build_stats(&case.new_lines);

    let _ = render::scan_text(&old);
    if let Ok(body) = render::scan_json(&meta(), &old, None, None, None) {
        assert_valid_json("scan_json", &body);
    }

    let report = diff::compare(&old, &new);
    let _ = render::diff_text(&report, None);
    if let Ok(body) = render::diff_json(&meta(), &report, None, None) {
        assert_valid_json("diff_json", &body);
    }

    let secret: String = case
        .secret
        .iter()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .take(40)
        .map(|&byte| byte as char)
        .collect();
    if secret.len() < 6 {
        return;
    }
    let line_text = format!("password={secret}");
    let mut grouper = Grouper::default();
    grouper.push(&Line {
        text: line_text.clone(),
        raw: line_text,
        bytes: 64,
    });
    let stats = grouper.finish(None);
    let text_out = render::scan_text(&stats);
    assert!(
        !text_out.contains(&secret),
        "secret leaked in scan_text: {text_out:?}"
    );
    if let Ok(json_out) = render::scan_json(&meta(), &stats, None, None, None) {
        assert!(
            !json_out.contains(&secret),
            "secret leaked in scan_json: {json_out:?}"
        );
    }
});
