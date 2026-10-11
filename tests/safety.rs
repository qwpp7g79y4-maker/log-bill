use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Fixed canary text. Every canary fixture contains it; no output may contain it.
const CANARY: &str = "CANARY-SECRET";

const TEXT_PATH: &str = "tests/fixtures/canary-text.log";
const JSONL: &str = "tests/fixtures/canary-jsonl.jsonl";
const NESTED: &str = "tests/fixtures/canary-nested.jsonl";
const BROKEN: &str = "tests/fixtures/canary-broken.jsonl";
const NEW_PATH: &str = "tests/fixtures/canary-new.log";
const RULES: &str = "tests/fixtures/canary-rules.txt";
const RULES_BAD: &str = "tests/fixtures/canary-rules-invalid.txt";
const KEY_LOG: &str = "tests/fixtures/canary-keys.log";
const AUTH_LOG: &str = "tests/fixtures/canary-authorization.log";
const AUTH_JSONL: &str = "tests/fixtures/canary-authorization.jsonl";
const BARE: &str = "CANARYGEHEIMZONDERCIJFER";
const B64: &str = "dXNlcjpwYXNz";

fn manifest(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("start binary");
    (
        output.status.code().unwrap_or(101),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn refuse(label: &str, out: &str, err: &str) {
    assert!(
        !out.contains(CANARY),
        "{label} stdout contains {CANARY}:\n{out}"
    );
    assert!(
        !err.contains(CANARY),
        "{label} stderr contains {CANARY}:\n{err}"
    );
}

fn has_text(label: &str, output_2: &str, pieces: &[&str]) {
    for piece in pieces {
        assert!(
            output_2.contains(piece),
            "{label} is missing {piece:?}\n{output_2}"
        );
    }
}

fn assert_count(label: &str, output_2: &str, piece: &str, n: usize) {
    assert_eq!(
        output_2.matches(piece).count(),
        n,
        "{label} counts {piece:?}, not {n} times\n{output_2}"
    );
}

fn ok(label: &str, args: &[&str]) -> String {
    let (code, out, err) = run(args);
    assert_eq!(code, 0, "{label}\nstdout:\n{out}\nstderr:\n{err}");
    assert!(err.is_empty(), "{label} stderr:\n{err}");
    refuse(label, &out, &err);
    out
}

fn error(label: &str, args: &[&str], stderr: &str) {
    let (code, out, err) = run(args);
    assert_eq!(code, 2, "{label}\nstdout:\n{out}\nstderr:\n{err}");
    assert!(out.is_empty(), "{label} stdout must be empty:\n{out}");
    assert_eq!(err, stderr, "{label}");
    refuse(label, &out, &err);
}

fn json_tool(label: &str, body: &str) -> String {
    let mut child = Command::new("python3")
        // Python on Windows reads stdin as the ANSI code page unless told otherwise.
        .env("PYTHONIOENCODING", "utf-8")
        .args(["-m", "json.tool"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("python3");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write");
    let done = child.wait_with_output().expect("python3");
    assert!(
        done.status.success(),
        "{label} json.tool: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    let again = String::from_utf8_lossy(&done.stdout).into_owned();
    refuse(&format!("{label} after json.tool"), &again, "");
    again
}

fn ok_json(label: &str, args: &[&str]) -> String {
    let out = ok(label, args);
    json_tool(label, &out);
    out
}

fn fixture_has_canary(rel: &str) {
    let bytes = std::fs::read(manifest(rel)).unwrap_or_else(|err| panic!("read {rel}: {err}"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains(CANARY), "{rel} is not a canary fixture");
}

/// Text, JSON, `--explain`, diff, and stderr. The canary is in the fixture;
/// output shows the masked pattern, a line number, or nothing.
///
/// Bytes are raw bytes plus 1 for the newline. The share is
/// `bytes * 1000 / total`, up when twice the remainder is at least the total.
#[test]
fn canary_does_not_leak() {
    for rel in [TEXT_PATH, JSONL, NESTED, BROKEN, NEW_PATH, RULES, RULES_BAD] {
        fixture_has_canary(rel);
    }
    // `\u0043` is the letter C. The message source line does not contain the canary;
    // the `user` field next to it does. Decoded, the message is `<VAR>`.
    let jsonl = std::fs::read_to_string(manifest(JSONL)).expect("jsonl");
    assert!(jsonl.contains(r"\u0043ANARY-SECRET-0014"), "{jsonl}");
    assert!(jsonl.contains("CANARY-SECRET-0022"));

    text();
    jsonl_fields();
    nested();
    stderr_paths();
    compared();
}

fn text() {
    // 37 + 28 + 26 + 41 = 132. Shares 31.1 + 28.0 + 21.2 + 19.7 = 100.0.
    let basis = [
        "total: 4 lines, 132 bytes",
        "1 line  41 bytes  31.1%  Authorization: <SECRET>",
        "1 line  37 bytes  28.0%  wachtwoord gelekt <VAR>",
        "1 line  28 bytes  21.2%  password=<SECRET>",
        "1 line  26 bytes  19.7%  token: <SECRET>",
    ];
    // BOM and CRLF. `drop` removes 0001 (37 bytes). `sample 0.5` on 0002:
    // floor(28 × 0.5) = 14 bytes and floor(1 × 0.5) = 0 lines.
    // New: 81 bytes and 2 lines. Savings: 51 bytes and 2 lines.
    let with_rules = ok(
        "scan text with rules and explain",
        &["scan", "--rules", RULES, "--explain", TEXT_PATH],
    );
    has_text("scan text", &with_rules, &basis);
    has_text(
        "scan text rules",
        &with_rules,
        &[
            "after rules: 4 lines, 132 bytes -> 2 lines, 81 bytes, savings 2 lines, 51 bytes",
            "1 line, 41 bytes -> 1 line, 41 bytes, savings 0 lines, 0 bytes",
            "1 line, 37 bytes -> 0 lines, 0 bytes, savings 1 line, 37 bytes, rule 1",
            "1 line, 28 bytes -> 0 lines, 14 bytes, savings 1 line, 14 bytes, rule 2",
            "1 line, 26 bytes -> 1 line, 26 bytes, savings 0 lines, 0 bytes",
        ],
    );
    assert_count("masks secret", &with_rules, "<SECRET> 1", 3);
    assert_count("masks var", &with_rules, "<VAR> 1", 1);
    assert_count("rule line none", &with_rules, ": none\n", 2);
    assert!(with_rules.contains(": 1\n"), "{with_rules}");
    assert!(with_rules.contains(": 2\n"), "{with_rules}");

    let json = ok_json(
        "scan text json explain",
        &["scan", "--json", "--explain", TEXT_PATH],
    );
    has_text(
        "scan text json",
        &json,
        &[
            "\"schema_version\": 1",
            "\"bytes\": 132",
            "\"lines\": 4",
            "\"share\": \"31.1\"",
            "\"text\": \"Authorization: <SECRET>\"",
            "\"text\": \"wachtwoord gelekt <VAR>\"",
            "\"text\": \"password=<SECRET>\"",
            "\"text\": \"token: <SECRET>\"",
            "\"secret\": 1",
            "\"var\": 1",
        ],
    );
    assert!(!json.contains("\"rules\""), "{json}");

    let with_json = ok_json(
        "scan text json with rules",
        &["scan", "--json", "--rules", RULES, TEXT_PATH],
    );
    has_text(
        "scan text json rules",
        &with_json,
        &[
            "\"applied\": 2",
            "\"bytes\": 81",
            "\"bytes\": 51",
            "\"new_bytes\": 14",
            "\"saved_bytes\": 37",
            "\"saved_bytes\": 14",
            "\"text\": \"password=<SECRET>\"",
            "\"text\": \"wachtwoord gelekt <VAR>\"",
        ],
    );
}

fn jsonl_fields() {
    // The whole line counts, newline included. Pattern is only the message field.
    // 67 + 68 + 80 + 30 + 66 = 311.
    // 80 -> 25.7; 68 -> 21.9; 67 -> 21.5; 66 -> 21.2; 30 -> 9.6. Sum 99.9.
    let text = ok(
        "scan jsonl",
        &["scan", "--format", "jsonl", "--explain", JSONL],
    );
    has_text(
        "scan jsonl",
        &text,
        &[
            "total: 5 lines, 311 bytes",
            "1 line  80 bytes  25.7%  Authorization: <SECRET>",
            "1 line  68 bytes  21.9%  token: <SECRET>",
            "1 line  67 bytes  21.5%  login <VAR>",
            "1 line  66 bytes  21.2%  <VAR>",
            "1 line  30 bytes  9.6%  <NO-MESSAGE>",
        ],
    );
    assert!(!text.contains("password"), "{text}");
    assert_count("jsonl secret", &text, "<SECRET> 1", 2);
    assert_count("jsonl var", &text, "<VAR> 1", 2);
    assert_count("jsonl without mask", &text, ": none\n", 1);

    let json = ok_json(
        "scan jsonl json",
        &["scan", "--format", "jsonl", "--json", "--explain", JSONL],
    );
    has_text(
        "scan jsonl json",
        &json,
        &[
            "\"bytes\": 311",
            "\"lines\": 5",
            "\"text\": \"login <VAR>\"",
            "\"text\": \"<NO-MESSAGE>\"",
            "\"share\": \"9.6\"",
        ],
    );
    assert_count("jsonl json secret", &json, "\"secret\": 1", 2);
    assert_count("jsonl json var", &json, "\"var\": 1", 2);

    // `--message-field note`: only line 4 (30 bytes) is `<VAR>`.
    // The other four are 281 bytes `<NO-MESSAGE>`: 281 * 1000 / 311 = 90.4.
    let note = ok(
        "scan jsonl message-field",
        &[
            "scan",
            "--format",
            "jsonl",
            "--message-field",
            "note",
            JSONL,
        ],
    );
    has_text(
        "message-field note",
        &note,
        &[
            "total: 5 lines, 311 bytes",
            "4 lines  281 bytes  90.4%  <NO-MESSAGE>",
            "1 line  30 bytes  9.6%  <VAR>",
        ],
    );
    assert!(!note.contains("login"), "{note}");
    assert!(!note.contains("Authorization"), "{note}");

    let note_json = ok_json(
        "scan jsonl message-field json",
        &[
            "scan",
            "--format",
            "jsonl",
            "--message-field",
            "note",
            "--json",
            JSONL,
        ],
    );
    has_text(
        "message-field note json",
        &note_json,
        &[
            "\"message_field\": \"note\"",
            "\"bytes\": 281",
            "\"bytes\": 30",
            "\"text\": \"<NO-MESSAGE>\"",
            "\"text\": \"<VAR>\"",
        ],
    );
    assert!(!note_json.contains("login"), "{note_json}");
}

fn nested() {
    // Object message and a JSON array: no message text, 69 + 23 = 92 bytes.
    let text = ok("scan nested", &["scan", "--format", "jsonl", NESTED]);
    has_text(
        "scan nested",
        &text,
        &[
            "total: 2 lines, 92 bytes",
            "2 lines  92 bytes  100.0%  <NO-MESSAGE>",
        ],
    );
    let json = ok_json(
        "scan nested json",
        &["scan", "--format", "jsonl", "--json", NESTED],
    );
    has_text(
        "scan nested json",
        &json,
        &[
            "\"bytes\": 92",
            "\"lines\": 2",
            "\"share\": \"100.0\"",
            "\"text\": \"<NO-MESSAGE>\"",
        ],
    );
}

fn stderr_paths() {
    error(
        "text as jsonl",
        &["scan", "--format", "jsonl", TEXT_PATH],
        "invalid JSON on line 1\n",
    );
    error(
        "broken json",
        &["scan", "--format", "jsonl", BROKEN],
        "invalid JSON on line 2\n",
    );
    error(
        "broken json with json and explain",
        &["scan", "--format", "jsonl", "--json", "--explain", BROKEN],
        "invalid JSON on line 2\n",
    );
    error(
        "invalid rules file",
        &["scan", "--rules", RULES_BAD, TEXT_PATH],
        "invalid rule on line 2 of the rules file\n",
    );
    error(
        "invalid rules file with json and explain",
        &[
            "scan",
            "--json",
            "--explain",
            "--rules",
            RULES_BAD,
            TEXT_PATH,
        ],
        "invalid rule on line 2 of the rules file\n",
    );
}

fn compared() {
    // New lacks the 37-byte line and has `sample <VAR>` of 26 bytes.
    // 132 - 37 + 26 = 121. New patterns: 26 / 121 = 21.5%.
    let diff = ok("diff text", &["diff", TEXT_PATH, NEW_PATH]);
    has_text(
        "diff text",
        &diff,
        &[
            "total: 4 lines, 132 bytes -> 4 lines, 121 bytes, change 0 lines, -11 bytes",
            "0 -> 26 bytes, change +26 bytes, share 0.0% -> 21.5%, change +21.5 percentage points  sample <VAR>",
            "37 -> 0 bytes, change -37 bytes, share 28.0% -> 0.0%, change -28.0 percentage points  wachtwoord gelekt <VAR>",
            "41 -> 41 bytes, change 0 bytes, share 31.1% -> 33.9%, change +2.8 percentage points  Authorization: <SECRET>",
            "28 -> 28 bytes, change 0 bytes, share 21.2% -> 23.1%, change +1.9 percentage points  password=<SECRET>",
            "26 -> 26 bytes, change 0 bytes, share 19.7% -> 21.5%, change +1.8 percentage points  token: <SECRET>",
            "new patterns: 26 of 121 bytes, 21.5%",
        ],
    );

    let diff_json = ok_json("diff text json", &["diff", "--json", TEXT_PATH, NEW_PATH]);
    has_text(
        "diff text json",
        &diff_json,
        &[
            "\"schema_version\": 1",
            "\"added_bytes\": 26",
            "\"added_share\": \"21.5\"",
            "\"bytes\": 132",
            "\"bytes\": 121",
            "\"bytes\": -11",
            "\"text\": \"sample <VAR>\"",
            "\"text\": \"wachtwoord gelekt <VAR>\"",
        ],
    );

    // After rules: old 2 lines / 81 bytes, new 3 lines / 107 bytes.
    // Old shares 50.6 / 17.3 / 32.1. New 38.3 / 13.1 / 24.3 / 24.3.
    let diff_with_rules = ok(
        "diff with rules",
        &["diff", "--rules", RULES, TEXT_PATH, NEW_PATH],
    );
    has_text(
        "diff with rules",
        &diff_with_rules,
        &[
            "total: 2 lines, 81 bytes -> 3 lines, 107 bytes, change +1 line, +26 bytes",
            "rules applied: 2",
            "0 -> 26 bytes, change +26 bytes, share 0.0% -> 24.3%, change +24.3 percentage points  sample <VAR>",
            "41 -> 41 bytes, change 0 bytes, share 50.6% -> 38.3%, change -12.3 percentage points  Authorization: <SECRET>",
            "14 -> 14 bytes, change 0 bytes, share 17.3% -> 13.1%, change -4.2 percentage points  password=<SECRET>",
            "26 -> 26 bytes, change 0 bytes, share 32.1% -> 24.3%, change -7.8 percentage points  token: <SECRET>",
            "new patterns: 26 of 107 bytes, 24.3%",
        ],
    );
    assert!(!diff_with_rules.contains("gone "), "{diff_with_rules}");

    let same = ok("diff jsonl", &["diff", "--format", "jsonl", JSONL, JSONL]);
    has_text(
        "diff jsonl",
        &same,
        &[
            "total: 5 lines, 311 bytes -> 5 lines, 311 bytes, change 0 lines, 0 bytes",
            "80 -> 80 bytes, change 0 bytes, share 25.7% -> 25.7%, change 0.0 percentage points  Authorization: <SECRET>",
            "68 -> 68 bytes, change 0 bytes, share 21.9% -> 21.9%, change 0.0 percentage points  token: <SECRET>",
            "67 -> 67 bytes, change 0 bytes, share 21.5% -> 21.5%, change 0.0 percentage points  login <VAR>",
            "66 -> 66 bytes, change 0 bytes, share 21.2% -> 21.2%, change 0.0 percentage points  <VAR>",
            "30 -> 30 bytes, change 0 bytes, share 9.6% -> 9.6%, change 0.0 percentage points  <NO-MESSAGE>",
            "new patterns: 0 of 311 bytes, 0.0%",
        ],
    );
    assert!(
        !same.lines().any(|line| line.starts_with("new P")),
        "{same}"
    );
    assert!(!same.contains("gone "), "{same}");

    let nested = ok_json(
        "diff nested json",
        &["diff", "--format", "jsonl", "--json", NESTED, NESTED],
    );
    has_text(
        "diff nested json",
        &nested,
        &[
            "\"added_bytes\": 0",
            "\"bytes\": 92",
            "\"text\": \"<NO-MESSAGE>\"",
        ],
    );
}

#[test]
fn canary_key_with_prefix_or_suffix() {
    // No digit and shorter than 32: with KEYS empty, `is_var` makes no `<VAR>` here.
    assert!(
        !BARE.chars().any(|ch| ch.is_ascii_digit()),
        "{BARE} contains a digit"
    );
    assert!(BARE.chars().count() < 32, "{BARE} is not shorter than 32");
    let raw = std::fs::read_to_string(manifest(KEY_LOG)).expect("fixture");
    assert!(raw.contains(BARE), "{raw}");
    for key in [
        "db_password: ",
        "SECRET_KEY: ",
        "x_token: ",
        "my_secret_value: ",
        "x-api-key: ",
        "X-Api-Key: ",
        "Db-Password: ",
        "API_KEY: ",
    ] {
        assert!(
            raw.contains(&format!("{key}{BARE}")),
            "fixture is missing {key}"
        );
    }

    let text = ok("scan keys", &["scan", KEY_LOG]);
    assert!(!text.contains(BARE), "{text}");
    assert_eq!(text.matches("<SECRET>").count(), 8, "{text}");

    let explanation = ok("explain sleutels", &["scan", "--explain", KEY_LOG]);
    assert!(!explanation.contains(BARE), "{explanation}");
    assert_eq!(
        explanation.matches("<SECRET> 1").count(),
        8,
        "{explanation}"
    );

    let json = ok_json("json sleutels", &["scan", "--json", "--explain", KEY_LOG]);
    assert!(!json.contains(BARE), "{json}");
    assert_eq!(json.matches("\"secret\": 1").count(), 8, "{json}");

    let diff = ok("diff sleutels", &["diff", KEY_LOG, KEY_LOG]);
    assert!(!diff.contains(BARE), "{diff}");
    assert!(diff.contains("<SECRET>"), "{diff}");

    let diff_json = ok_json("diff json sleutels", &["diff", "--json", KEY_LOG, KEY_LOG]);
    assert!(!diff_json.contains(BARE), "{diff_json}");

    error(
        "keys as jsonl",
        &["scan", "--format", "jsonl", KEY_LOG],
        "invalid JSON on line 1\n",
    );

    // Basic/Foo keep the token when only the first word becomes `<SECRET>`.
    // `dXNlcjpwYXNz` has no digit and is under 32, so `is_var` does not catch it.
    assert!(!B64.chars().any(|ch| ch.is_ascii_digit()), "{B64}");
    assert!(B64.chars().count() < 32, "{B64}");
    let auth = std::fs::read_to_string(manifest(AUTH_LOG)).expect("auth-fixture");
    let auth_jsonl = std::fs::read_to_string(manifest(AUTH_JSONL)).expect("auth-jsonl");
    assert!(auth.contains(BARE) && auth.contains(B64), "{auth}");
    assert!(
        auth_jsonl.contains(BARE) && auth_jsonl.contains(B64),
        "{auth_jsonl}"
    );
    for key in [
        "Authorization: Basic ",
        "Authorization: Bearer ",
        "Authorization: NTLM ",
        "Authorization: Negotiate ",
        "Authorization: Foo ",
        "Proxy-Authorization: Basic ",
        "x-authorization: Basic ",
        "\"authorization\": \"Basic ",
    ] {
        assert!(auth.contains(&format!("{key}{BARE}")), "missing {key}");
    }

    for (label, args) in [
        ("scan auth", vec!["scan", AUTH_LOG]),
        ("explain auth", vec!["scan", "--explain", AUTH_LOG]),
        ("json auth", vec!["scan", "--json", "--explain", AUTH_LOG]),
        ("diff auth", vec!["diff", AUTH_LOG, AUTH_LOG]),
        ("diff json auth", vec!["diff", "--json", AUTH_LOG, AUTH_LOG]),
        (
            "scan auth jsonl",
            vec!["scan", "--format", "jsonl", "--explain", AUTH_JSONL],
        ),
        (
            "json auth jsonl",
            vec![
                "scan",
                "--format",
                "jsonl",
                "--json",
                "--explain",
                AUTH_JSONL,
            ],
        ),
        (
            "diff auth jsonl",
            vec!["diff", "--format", "jsonl", AUTH_JSONL, AUTH_JSONL],
        ),
        (
            "diff json auth jsonl",
            vec![
                "diff", "--format", "jsonl", "--json", AUTH_JSONL, AUTH_JSONL,
            ],
        ),
    ] {
        let output_2 = ok(label, &args);
        assert!(!output_2.contains(BARE), "{label}\n{output_2}");
        assert!(!output_2.contains(B64), "{label}\n{output_2}");
        assert!(!output_2.contains("YWJjZGVm"), "{label}\n{output_2}");
        assert!(output_2.contains("<SECRET>"), "{label}\n{output_2}");
    }
    error(
        "auth text as jsonl",
        &["scan", "--format", "jsonl", AUTH_LOG],
        "invalid JSON on line 1\n",
    );
}

#[test]
fn no_network_in_lock_or_source() {
    let lock = std::fs::read_to_string(manifest("Cargo.lock")).expect("Cargo.lock");
    for crate_name in ["clap", "regex", "serde_json"] {
        assert!(
            lock.contains(&format!("name = \"{crate_name}\"")),
            "Cargo.lock is missing {crate_name}"
        );
    }
    for crate_name in ["reqwest", "hyper", "ureq", "tokio"] {
        assert!(
            !lock.contains(crate_name),
            "Cargo.lock contains network crate {crate_name}"
        );
    }

    let mut files = Vec::new();
    visit(&manifest("src"), &mut files);
    assert!(files.len() >= 8, "{files:?}");
    let mut source = String::new();
    for file_path in &files {
        let text = std::fs::read_to_string(file_path)
            .unwrap_or_else(|err| panic!("{}: {err}", file_path.display()));
        source.push_str(&text);
    }
    assert!(source.contains("fn main"));
    assert!(source.contains("fn mask"));
    assert!(!source.contains("std::net"), "src contains std::net");
}

fn visit(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|err| panic!("{}: {err}", dir.display())) {
        let entry = entry.expect("src");
        let file_path = entry.path();
        if file_path.is_dir() {
            visit(&file_path, files);
        } else {
            files.push(file_path);
        }
    }
}
