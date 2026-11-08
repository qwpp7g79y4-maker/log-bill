//! `--group-by`: costs per source from a JSON field. Synthetic input only.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(args)
        .output()
        .expect("start binary")
}

fn out(args: &[&str]) -> String {
    let output = run(args);
    assert_eq!(output.status.code(), Some(0), "{args:?}");
    String::from_utf8(output.stdout).expect("utf-8")
}

fn sample(dir: &tempfile::TempDir, body: &str) -> String {
    let path = dir.path().join("in.jsonl");
    std::fs::write(&path, body).expect("write");
    path.to_str().expect("utf-8 path").to_owned()
}

const BASE: [&str; 3] = ["scan", "--format", "jsonl"];

#[test]
fn groups_lines_and_bytes_per_value() {
    let dir = tempfile::tempdir().unwrap();
    let file = sample(
        &dir,
        "{\"s\":\"a\",\"message\":\"x\"}\n{\"s\":\"b\",\"message\":\"x\"}\n{\"s\":\"a\",\"message\":\"x\"}\n",
    );
    let mut args = BASE.to_vec();
    args.extend(["--group-by", "s", &file]);
    let text = out(&args);
    assert!(text.contains("by s:\n"), "{text}");
    // 2 lines of 24 bytes (23 + newline) and 1 line: a = 48 of 72 bytes.
    assert!(text.contains("a  2 lines  48 bytes  66.7%\n"), "{text}");
    assert!(text.contains("b  1 line  24 bytes  33.3%\n"), "{text}");
}

#[test]
fn missing_and_non_text_values_are_no_source() {
    let dir = tempfile::tempdir().unwrap();
    let file = sample(&dir, "{\"message\":\"x\"}\n{\"s\":3}\n{\"s\":null}\n");
    let mut args = BASE.to_vec();
    args.extend(["--group-by", "s", &file]);
    let text = out(&args);
    assert!(text.contains("<NO-SOURCE>  3 lines"), "{text}");
}

#[test]
fn without_the_option_there_is_no_block() {
    let text = out(&["scan", "--format", "jsonl", "examples/services.jsonl"]);
    assert!(!text.contains("by service:"), "{text}");
    assert!(!text.contains("NO-SOURCE"), "{text}");
}

#[test]
fn costs_per_source_need_window_and_prices() {
    let plain = out(&[
        "scan",
        "--format",
        "jsonl",
        "--group-by",
        "service",
        "examples/services.jsonl",
    ]);
    assert!(!plain.contains("USD"), "{plain}");
    let priced = out(&[
        "scan",
        "--format",
        "jsonl",
        "--group-by",
        "service",
        "--window",
        "1s",
        "--prices",
        "tests/fixtures/quickstart-prices.json",
        "examples/services.jsonl",
    ]);
    assert!(
        priced.contains(
            "checkout  3 lines  286 bytes  49.4%\n  ingest: 0.05 USD per day, 1.48 USD per month\n"
        ),
        "{priced}"
    );
}

#[test]
fn json_has_sources_and_input() {
    let json = out(&[
        "scan",
        "--format",
        "jsonl",
        "--group-by",
        "service",
        "--json",
        "examples/services.jsonl",
    ]);
    let value: serde_json::Value = serde_json::from_str(&json).expect("json");
    assert_eq!(value["inputs"]["group_by"], "service");
    let sources = value["sources"].as_array().expect("sources");
    assert_eq!(sources[0]["name"], "checkout");
    assert_eq!(sources[0]["lines"], 3);
    assert_eq!(sources[0]["bytes"], 286);
    assert_eq!(sources[0]["share"], "49.4");
    assert_eq!(sources.last().unwrap()["name"], "<NO-SOURCE>");
    // Without the option both keys are absent.
    let plain = out(&[
        "scan",
        "--format",
        "jsonl",
        "--json",
        "examples/services.jsonl",
    ]);
    let plain: serde_json::Value = serde_json::from_str(&plain).unwrap();
    assert!(plain.get("sources").is_none());
    assert!(plain["inputs"].get("group_by").is_none());
}

#[test]
fn control_characters_in_a_value_are_not_written_raw() {
    let dir = tempfile::tempdir().unwrap();
    let file = sample(&dir, "{\"s\":\"a\\u001b[31mb\",\"message\":\"x\"}\n");
    let mut args = BASE.to_vec();
    args.extend(["--group-by", "s", &file]);
    let text = out(&args);
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert!(text.contains("a\\u001b[31mb"), "{text}");
}

#[test]
fn rejects_text_format_and_diff() {
    for (args, message) in [
        (
            vec!["scan", "--group-by", "s", "examples/services.jsonl"],
            "--group-by only works with --format jsonl\n",
        ),
        (
            vec![
                "diff",
                "--format",
                "jsonl",
                "--group-by",
                "s",
                "examples/services.jsonl",
                "examples/services.jsonl",
            ],
            "--group-by only works with scan\n",
        ),
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(3), "{args:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(String::from_utf8(output.stderr).unwrap(), message);
    }
}

#[test]
fn reads_standard_input() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(["scan", "--format", "jsonl", "--group-by", "s", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"s\":\"q\",\"message\":\"x\"}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("q  1 line"));
}
