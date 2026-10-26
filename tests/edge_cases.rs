//! Regression tests for edge cases the input formats allow but that earlier
//! tests did not exercise directly: an empty file, a file of only newlines,
//! a lone `\r` with no `\n`, a byte order mark with nothing else, a raw NUL
//! byte inside a JSON string, a JSON Unicode escape for NUL splitting a key, a
//! duplicate key nested inside a price-file object, and a negative-zero
//! price.

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(args)
        .output()
        .expect("start binary");
    let code = output.status.code().expect("exit code");
    let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    (code, out, err)
}

fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn file_path(path: &Path) -> &str {
    path.to_str().expect("path")
}

fn scan_json(path: &Path, extra: &[&str]) -> (i32, serde_json::Value, String) {
    let mut args = vec!["scan", "--json"];
    args.extend_from_slice(extra);
    args.push(file_path(path));
    let (code, out, err) = run(&args);
    let value = if out.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&out).unwrap_or_else(|error| panic!("{error}: {out}"))
    };
    (code, value, err)
}

#[test]
fn empty_file_is_zero_lines_and_bytes() {
    let log = tmp("edge-empty.log", b"");
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 0);
    assert_eq!(value["totals"]["bytes"], 0);
    assert_eq!(value["patterns"].as_array().expect("patterns").len(), 0);
}

#[test]
fn only_newlines_is_zero_lines_but_counts_the_bytes_read() {
    // `\n`, `\r\n`, `\n`: every line is empty and dropped, so no pattern and
    // no bytes are attributed, even though the file itself is not empty.
    let log = tmp("edge-only-newlines.log", b"\n\r\n\n");
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 0);
    assert_eq!(value["totals"]["bytes"], 0);
    assert_eq!(value["patterns"].as_array().expect("patterns").len(), 0);
}

#[test]
fn bom_only_file_is_zero_in_both_formats() {
    let text = tmp("edge-bom-only.log", b"\xEF\xBB\xBF");
    let (code, value, err) = scan_json(&text, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 0);
    assert_eq!(value["totals"]["bytes"], 0);

    let jsonl = tmp("edge-bom-only.jsonl", b"\xEF\xBB\xBF");
    let (code, value, err) = scan_json(&jsonl, &["--format", "jsonl"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 0);
    assert_eq!(value["totals"]["bytes"], 0);
}

#[test]
fn lone_cr_with_no_newline_is_literal_content_not_a_line_break() {
    // No `\n` anywhere: the `\r` is ordinary content, and the whole file is
    // one line of 8 raw bytes (`a`, `b`, `c`, `\r`, `d`, `e`, `f`, +1).
    let log = tmp("edge-lone-cr.log", b"abc\rdef");
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 1);
    assert_eq!(value["totals"]["bytes"], 8);
    assert_eq!(value["patterns"][0]["text"], "abc\rdef");
}

#[test]
fn bom_then_crlf_lines_split_and_count_correctly() {
    // BOM (3) + `abc` (3) + `\r\n` (2) = 8 for the first line; `def` (3) + `\r\n` (2) = 5.
    let log = tmp("edge-bom-crlf.log", b"\xEF\xBB\xBFabc\r\ndef\r\n");
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 2);
    assert_eq!(value["totals"]["bytes"], 13);
    let texts: Vec<&str> = value["patterns"]
        .as_array()
        .expect("patterns")
        .iter()
        .map(|pattern| pattern["text"].as_str().expect("text"))
        .collect();
    assert!(texts.contains(&"abc"), "{texts:?}");
    assert!(texts.contains(&"def"), "{texts:?}");
}

#[test]
fn raw_nul_byte_inside_a_json_string_is_invalid_json() {
    // JSON forbids an unescaped control byte inside a string, NUL included.
    // That line is refused with its number, not read lossily.
    let mut raw = br#"{"message":"a"#.to_vec();
    raw.push(0);
    raw.extend_from_slice(br#"b"}"#);
    raw.push(b'\n');
    let log = tmp("edge-raw-nul.jsonl", &raw);
    let (code, _out, err) = run(&["scan", "--format", "jsonl", "--json", file_path(&log)]);
    assert_eq!(code, 2, "{err}");
    assert_eq!(err, "invalid JSON on line 1\n");
}

#[test]
fn escaped_unicode_nul_in_a_json_message_splits_the_key_for_masking() {
    // A JSON Unicode escape for NUL decodes to a real NUL inside the
    // message text, which `without_nul` then removes before masking, so
    // the key reads as `password` and the value still hides.
    let nul_escape = format!("{}u0000", char::from(0x5c_u8));
    let body = format!(r#"{{"message":"pass{nul_escape}word=secret"}}"#);
    let log = tmp("edge-escaped-nul.jsonl", body.as_bytes());
    let (code, value, err) = scan_json(&log, &["--format", "jsonl"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["patterns"][0]["text"], "password=<SECRET>");
    assert!(!err.contains("secret"));
}

#[test]
fn duplicate_key_nested_inside_a_price_object_is_rejected() {
    let log = tmp("edge-dup-nested.log", b"");
    let prices = tmp(
        "edge-dup-nested-prices.json",
        br#"{"currency":"USD","source":"x","date":"2026-01-01","ingest_per_gb":1,"gb_bytes":{"a":{"b":1,"b":2}}}"#,
    );
    let (code, _out, err) = run(&[
        "scan",
        "--json",
        "--window",
        "1m",
        "--prices",
        file_path(&prices),
        file_path(&log),
    ]);
    assert_eq!(code, 2, "{err}");
    assert_eq!(err, "price file has a duplicate key\n");
}

#[test]
fn negative_zero_price_is_rejected_not_treated_as_zero() {
    let log = tmp("edge-neg-zero.log", b"");
    let prices = tmp(
        "edge-neg-zero-prices.json",
        br#"{"currency":"USD","source":"x","date":"2026-01-01","ingest_per_gb":-0}"#,
    );
    let (code, _out, err) = run(&[
        "scan",
        "--json",
        "--window",
        "1m",
        "--prices",
        file_path(&prices),
        file_path(&log),
    ]);
    assert_eq!(code, 2, "{err}");
    assert_eq!(err, "price file has a negative price\n");
}

#[test]
fn budget_past_the_f64_range_is_rejected_cleanly() {
    let log = tmp("edge-budget-huge.log", b"");
    let prices = tmp(
        "edge-budget-huge-prices.json",
        br#"{"currency":"USD","source":"x","date":"2026-01-01","ingest_per_gb":1}"#,
    );
    let (code, _out, err) = run(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "1e309",
        file_path(&log),
    ]);
    assert_eq!(code, 3, "{err}");
    assert_eq!(err, "invalid budget\n");
}

fn pattern_texts(value: &serde_json::Value) -> Vec<String> {
    value["patterns"]
        .as_array()
        .expect("patterns")
        .iter()
        .map(|pattern| pattern["text"].as_str().expect("text").to_owned())
        .collect()
}

fn pattern_ids(value: &serde_json::Value) -> Vec<String> {
    value["patterns"]
        .as_array()
        .expect("patterns")
        .iter()
        .map(|pattern| pattern["id"].as_str().expect("id").to_owned())
        .collect()
}

#[test]
fn last_line_ending_in_a_lone_cr_masks_the_number() {
    // No `\n`: the trailing `\r` is not part of the text, but still weighs 1.
    // 8 (`id 12345`) + 1 (`\r`) + 1 = 10 bytes.
    let log = tmp("edge-trailing-lone-cr.log", b"id 12345\r");
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 1);
    assert_eq!(value["totals"]["bytes"], 10);
    assert_eq!(pattern_texts(&value), ["id <NUM>"]);
}

#[test]
fn double_cr_before_lf_masks_the_number() {
    // `id 12345` (8) + `\r\r\n` (3) = 11 bytes.
    let log = tmp("edge-double-cr.log", b"id 12345\r\r\n");
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 1);
    assert_eq!(value["totals"]["bytes"], 11);
    assert_eq!(pattern_texts(&value), ["id <NUM>"]);
}

#[test]
fn lf_crlf_and_double_cr_lines_share_one_pattern_id() {
    let ids = [
        ("edge-same-lf.log", &b"took 250 ms\n"[..]),
        ("edge-same-crlf.log", &b"took 250 ms\r\n"[..]),
        ("edge-same-crcrlf.log", &b"took 250 ms\r\r\n"[..]),
        ("edge-same-lone-cr.log", &b"took 250 ms\r"[..]),
    ]
    .map(|(name, bytes)| {
        let (code, value, err) = scan_json(&tmp(name, bytes), &[]);
        assert_eq!(code, 0, "{err}");
        pattern_ids(&value)
    });
    assert_eq!(ids[0].len(), 1);
    for other in &ids[1..] {
        assert_eq!(&ids[0], other, "{ids:?}");
    }
}

#[test]
fn a_thousand_double_cr_lines_are_one_pattern() {
    let mut raw = Vec::new();
    for n in 0..1000 {
        raw.extend_from_slice(format!("req done {n}\r\r\n").as_bytes());
    }
    let log = tmp("edge-thousand-crcrlf.log", &raw);
    let (code, value, err) = scan_json(&log, &[]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(value["totals"]["lines"], 1000);
    assert_eq!(pattern_texts(&value), ["req done <NUM>"]);
}

#[test]
fn end_anchored_rule_matches_with_any_line_ending() {
    let rules = tmp("edge-anchor.rules", b"drop ^sample this \\d$\n");
    for (name, bytes) in [
        ("edge-anchor-lf.log", &b"sample this 2\n"[..]),
        ("edge-anchor-crlf.log", &b"sample this 2\r\n"[..]),
        ("edge-anchor-lone-cr.log", &b"sample this 2\r"[..]),
        ("edge-anchor-crcrlf.log", &b"sample this 2\r\r\n"[..]),
    ] {
        let log = tmp(name, bytes);
        let (code, out, err) = run(&["scan", "--rules", file_path(&rules), file_path(&log)]);
        assert_eq!(code, 0, "{name}: {out}\n{err}");
        assert!(out.contains("-> 0 lines, 0 bytes"), "{name}: {out}");
    }
}
