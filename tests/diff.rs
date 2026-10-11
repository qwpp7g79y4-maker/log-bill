use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
}

fn manifest(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn write_tmp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn run_exit(args: &[&str]) -> (i32, String, String) {
    let output = bin().args(args).output().expect("start binary");
    let code = output.status.code().unwrap_or(9);
    let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    (code, out, err)
}

const ESTIMATE: &str =
    "estimate: extrapolation of one sample with your own prices; not an official rate";

/// alpha\n = 6, twice. beta\n = 5. Total 3 lines, 17 bytes.
/// gamma\n = 6. New total 2 lines, 12 bytes.
/// Tenths share: bytes × 1000 / total; halfway rounds away from zero.
/// Old alpha: 12000/17 = 705 rem 15, 2×15 ≥ 17 -> 706 = 70.6%.
/// beta 5000/17 = 294 rem 2 -> 29.4%. New, each 6/12 = 50.0%.
const BASIS: &str = "\
total: 3 lines, 17 bytes -> 2 lines, 12 bytes, change -1 line, -5 bytes
new P229176bd  0 -> 6 bytes, change +6 bytes, share 0.0% -> 50.0%, change +50.0 percentage points  gamma
gone P7627619b  5 -> 0 bytes, change -5 bytes, share 29.4% -> 0.0%, change -29.4 percentage points  beta
P8ac625bb  12 -> 6 bytes, change -6 bytes, share 70.6% -> 50.0%, change -20.6 percentage points  alpha
new patterns: 6 of 12 bytes, 50.0%
";

fn old_and_new() -> (PathBuf, PathBuf) {
    (
        manifest("tests/fixtures/diff-old.log"),
        manifest("tests/fixtures/diff-new.log"),
    )
}

#[test]
fn two_fixtures_one_pattern_added_one_gone() {
    let (old, new_sample) = old_and_new();
    let (code, out, err) = run_exit(&[
        "diff",
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert_eq!(out, BASIS);
    assert!(!out.contains('\r'));
}

#[test]
fn month_difference_only_with_prices_and_window() {
    // Difference -5 bytes. Window 1d, factor 1, gb_bytes 1000.
    // Month cents = bytes × days × price × 100 / 1000.
    // 30 days, price 1: 5 × 30 × 100 / 1000 = 15 cents -> -0.15.
    // Price 2: 30 cents -> -0.30.
    // 31 days, price 1: 5 × 31 × 100 / 1000 = 15.5 -> 16 cents away from zero -> -0.16.
    // Price 2: 31 cents exactly -> -0.31. Not -0.15 / 30 × 31.
    let (old, new_sample) = old_and_new();
    let price_set = write_tmp(
        "diff-prices.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"index_per_gb":2,"gb_bytes":1000}"#,
    );
    let old_path = old.to_str().expect("path");
    let new_path = new_sample.to_str().expect("path");
    let price_path = price_set.to_str().expect("path");

    let (code, without_window, err) =
        run_exit(&["diff", "--prices", price_path, old_path, new_path]);
    assert_eq!(code, 3);
    assert!(without_window.is_empty(), "{without_window}");
    assert_eq!(err, "--prices only works together with --window\n");

    let (code, day30, err) = run_exit(&[
        "diff", "--window", "1d", "--prices", price_path, old_path, new_path,
    ]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(day30.starts_with(BASIS), "{day30}");
    assert!(
        day30.contains("prices: EUR, source kladblok, date 1999-01-02\n"),
        "{day30}"
    );
    assert!(
        day30.contains("change per month ingest: -0.15 EUR\n"),
        "{day30}"
    );
    assert!(
        day30.contains("change per month index: -0.30 EUR\n"),
        "{day30}"
    );
    assert!(day30.ends_with(&format!("{ESTIMATE}\n")), "{day30}");

    let (code, day31, err) = run_exit(&[
        "diff",
        "--window",
        "1d",
        "--days-per-month",
        "31",
        "--prices",
        price_path,
        old_path,
        new_path,
    ]);
    let _ = std::fs::remove_file(&price_set);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(
        day31.contains("change per month ingest: -0.16 EUR\n"),
        "{day31}"
    );
    assert!(
        day31.contains("change per month index: -0.31 EUR\n"),
        "{day31}"
    );
}

#[test]
fn max_new_share_below_above_and_equal() {
    // gamma is 6 of 12 bytes = 50%. Above the limit fails; equal does not.
    let (old, new_sample) = old_and_new();
    let old_path = old.to_str().expect("path");
    let new_path = new_sample.to_str().expect("path");
    for pct in ["60", "100", "50", "50.0", "50.00"] {
        let (code, out, err) = run_exit(&["diff", "--max-new-share", pct, old_path, new_path]);
        assert_eq!(code, 0, "{pct}");
        assert!(err.is_empty(), "{pct}: {err}");
        assert_eq!(out, BASIS, "{pct}");
    }
    for pct in ["49", "49.9", "0", "0.0"] {
        let (code, out, err) = run_exit(&["diff", "--max-new-share", pct, old_path, new_path]);
        assert_eq!(code, 1, "{pct}");
        assert_eq!(err, "new patterns above the threshold\n", "{pct}");
        assert_eq!(out, BASIS, "{pct}");
    }
}

#[test]
fn threshold_uses_the_fraction_not_one_decimal() {
    // New: a\n a\n c\n. a\n = 2 bytes, together 4; c\n = 2. Total 6.
    // c is new: 2/6 = 1/3. Shown 33.3% (2000/6 = 333 rem 2, 2×2 < 6).
    // 1/3 × 100 = 33.333... is above 33.3 and below 33.4.
    let old = write_tmp("threshold-old.log", b"a\nb\n");
    let new_sample = write_tmp("threshold-new.log", b"a\na\nc\n");
    let old_path = old.to_str().expect("path");
    let new_path = new_sample.to_str().expect("path");
    let (above, out, err) = run_exit(&["diff", "--max-new-share", "33.3", old_path, new_path]);
    let (below, _, err_below) = run_exit(&["diff", "--max-new-share", "33.4", old_path, new_path]);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new_sample);
    assert_eq!(above, 1);
    assert_eq!(err, "new patterns above the threshold\n");
    assert!(out.contains("new patterns: 2 of 6 bytes, 33.3%\n"), "{out}");
    assert_eq!(below, 0);
    assert!(err_below.is_empty(), "{err_below}");
}

#[test]
fn invalid_percentage_exit_3() {
    // A bare `-1` is a clap flag. With `=` the text reaches the parser:
    // a minus, a plus, a comma, and an exponent are not a percentage.
    for arg in [
        "--max-new-share=10,5",
        "--max-new-share=-1",
        "--max-new-share=",
        "--max-new-share=5.",
        "--max-new-share=abc",
        "--max-new-share=+10",
        "--max-new-share=1e2",
    ] {
        let (code, out, err) = run_exit(&["diff", arg, "old.log", "new.log"]);
        assert_eq!(code, 3, "{arg}");
        assert!(out.is_empty(), "{arg}");
        assert_eq!(err, "invalid percentage\n", "{arg}");
    }
}

#[test]
fn scan_ignores_a_valid_threshold() {
    // A valid threshold does not apply to scan. Stop before reading.
    let message = "--max-new-share only works with diff\n";
    for args in [
        vec!["scan", "--max-new-share", "0", "missing.log"],
        vec!["scan", "--explain", "--max-new-share", "0", "missing.log"],
        vec!["scan", "--json", "--max-new-share", "0", "missing.log"],
        vec![
            "scan",
            "--json",
            "--explain",
            "--max-new-share",
            "0",
            "missing.log",
        ],
        vec!["scan", "--max-new-share=abc", "missing.log"],
    ] {
        let (code, out, err) = run_exit(&args);
        assert_eq!(code, 3, "{args:?}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert_eq!(err, message, "{args:?}");
    }
}

#[test]
fn message_field_without_jsonl_does_nothing() {
    // The field is used only for JSON lines. Plain text never reads it.
    let message = "--message-field only works with --format jsonl\n";
    for args in [
        vec!["scan", "--message-field", "note", "missing.log"],
        vec![
            "scan",
            "--explain",
            "--message-field",
            "note",
            "missing.log",
        ],
        vec!["scan", "--json", "--message-field", "note", "missing.log"],
        vec![
            "scan",
            "--json",
            "--explain",
            "--message-field",
            "note",
            "missing.log",
        ],
        vec![
            "scan",
            "--format",
            "text",
            "--message-field",
            "note",
            "missing.log",
        ],
        vec!["diff", "--message-field", "note", "OLD", "NEW"],
        vec!["diff", "--json", "--message-field", "note", "OLD", "NEW"],
    ] {
        let (code, out, err) = run_exit(&args);
        assert_eq!(code, 3, "{args:?}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert_eq!(err, message, "{args:?}");
    }
}

#[test]
fn days_per_month_without_window_does_nothing() {
    // Without --window, month length is unused. The default of 30 stays valid.
    let message = "--days-per-month only works together with --window\n";
    for args in [
        vec!["scan", "--days-per-month", "31", "missing.log"],
        vec!["scan", "--days-per-month", "30", "missing.log"],
        vec!["scan", "--explain", "--days-per-month", "31", "missing.log"],
        vec!["scan", "--json", "--days-per-month", "31", "missing.log"],
        vec![
            "scan",
            "--json",
            "--explain",
            "--days-per-month",
            "31",
            "missing.log",
        ],
        vec!["diff", "--days-per-month", "31", "OLD", "NEW"],
        vec!["diff", "--json", "--days-per-month", "31", "OLD", "NEW"],
    ] {
        let (code, out, err) = run_exit(&args);
        assert_eq!(code, 3, "{args:?}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert_eq!(err, message, "{args:?}");
    }
}

#[test]
fn diff_window_without_prices_or_json_does_nothing() {
    // Text shows no window and no monthly amount. --json or --prices does.
    let (code, out, err) = run_exit(&["diff", "--window", "1d", "OLD", "NEW"]);
    assert_eq!(code, 3);
    assert!(out.is_empty(), "{out}");
    assert_eq!(
        err,
        "--window with diff only works together with --prices or --json\n"
    );

    let (old, new_sample) = old_and_new();
    let (code, out, err) = run_exit(&[
        "diff",
        "--json",
        "--window",
        "1d",
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("\"seconds\": 86400"), "{out}");
}

#[test]
fn unreadable_file_exit_2() {
    let (old, _) = old_and_new();
    let file_path = old.to_str().expect("path");
    for args in [
        vec!["diff", "missing-dir/old.log", file_path],
        vec!["diff", file_path, "missing-dir/new.log"],
    ] {
        let (code, out, err) = run_exit(&args);
        assert_eq!(code, 2, "{args:?}");
        assert!(out.is_empty(), "{args:?}");
        assert_eq!(err, "file is not readable\n", "{args:?}");
    }
}

#[test]
fn crlf_rounds_the_byte_difference() {
    // abc\r\n = 5 bytes, abc\n = 4. Same pattern, difference -1 byte.
    // gb_bytes 3, price 1, window 1d, 1 day.
    // Cents = bytes × 100 / 3.
    // 5 -> 166.666... -> 1.67. 4 -> 133.333... -> 1.33. Those amounts differ by -0.34.
    // The difference itself is 1 byte: 33.333... -> 0.33, so -0.33.
    let old = write_tmp("crlf-old.log", b"abc\r\n");
    let new_sample = write_tmp("crlf-new.log", b"abc\n");
    let price_set = write_tmp(
        "crlf-prices.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":3}"#,
    );
    let (code, out, err) = run_exit(&[
        "diff",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--prices",
        price_set.to_str().expect("path"),
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new_sample);
    let _ = std::fs::remove_file(&price_set);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(
        out.contains("total: 1 line, 5 bytes -> 1 line, 4 bytes, change 0 lines, -1 byte\n"),
        "{out}"
    );
    assert!(
        out.contains("Pe71fa219  5 -> 4 bytes, change -1 byte"),
        "{out}"
    );
    assert!(
        out.contains("change per month ingest: -0.33 EUR\n"),
        "{out}"
    );
    assert!(!out.contains("-0.34"), "{out}");
    assert!(out.ends_with(&format!("{ESTIMATE}\n")), "{out}");
}

#[test]
fn more_bytes_gives_plus() {
    // aa\n = 3, bb\n = 3. Old 3 bytes, new 6, difference +3.
    // 3 × 30 × 100 / 1000 = 9 cents -> +0.09. bb is new: 3 of 6 = 50.0%.
    let old = write_tmp("plus-old.log", b"aa\n");
    let new_sample = write_tmp("plus-new.log", b"aa\nbb\n");
    let price_set = write_tmp(
        "plus-prices.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1000}"#,
    );
    let (code, out, err) = run_exit(&[
        "diff",
        "--window",
        "1d",
        "--prices",
        price_set.to_str().expect("path"),
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new_sample);
    let _ = std::fs::remove_file(&price_set);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(
        out.contains("total: 1 line, 3 bytes -> 2 lines, 6 bytes, change +1 line, +3 bytes\n"),
        "{out}"
    );
    assert!(out.contains("new P08a63607  0 -> 3 bytes"), "{out}");
    assert!(out.contains("new patterns: 3 of 6 bytes, 50.0%\n"), "{out}");
    assert!(
        out.contains("change per month ingest: +0.09 EUR\n"),
        "{out}"
    );
    assert!(!out.contains("index"), "{out}");
}

#[test]
fn jsonl_compares_the_message() {
    // Bytes are the whole line. The pattern is the message.
    // Old: 26 + 19 = 45. New: 26 + 20 = 46. Difference +1 byte, 0 lines.
    // alpha 26/45: 26000/45 = 577 rem 35 -> 578 = 57.8%. 26/46: 26000/46 = 565 rem 10 -> 56.5%.
    // beta: 19000/45 = 422 rem 10 -> 42.2%. gamma: 20000/46 = 434 rem 36 -> 435 = 43.5%.
    let old = write_tmp(
        "diff-old.jsonl",
        b"{\"message\":\"alpha\",\"n\":1}\n{\"message\":\"beta\"}\n",
    );
    let new_sample = write_tmp(
        "diff-new.jsonl",
        b"{\"message\":\"alpha\",\"n\":2}\n{\"message\":\"gamma\"}\n",
    );
    let (code, out, err) = run_exit(&[
        "diff",
        "--format",
        "jsonl",
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new_sample);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        out,
        "\
total: 2 lines, 45 bytes -> 2 lines, 46 bytes, change 0 lines, +1 byte
new P229176bd  0 -> 20 bytes, change +20 bytes, share 0.0% -> 43.5%, change +43.5 percentage points  gamma
gone P7627619b  19 -> 0 bytes, change -19 bytes, share 42.2% -> 0.0%, change -42.2 percentage points  beta
P8ac625bb  26 -> 26 bytes, change 0 bytes, share 57.8% -> 56.5%, change -1.3 percentage points  alpha
new patterns: 20 of 46 bytes, 43.5%
"
    );
    assert!(!out.contains("message"), "{out}");
    assert!(!out.contains('{'), "{out}");
}

#[test]
fn canary_does_not_appear_in_the_diff() {
    let old = write_tmp("canary-old.log", b"password=CANARY-SECRET-0001\n");
    let new_sample = write_tmp("canary-new.log", b"password=CANARY-SECRET-0002\n");
    let (code, out, err) = run_exit(&[
        "diff",
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new_sample);
    assert_eq!(code, 0);
    let combined = format!("{out}{err}");
    assert!(!combined.contains("CANARY-SECRET"), "{combined}");
    assert!(out.contains("password=<SECRET>"), "{out}");
    assert!(out.contains("new patterns: 0 of 28 bytes, 0.0%\n"), "{out}");
}

#[test]
fn diff_applies_drop_and_sample() {
    // Each line is 3 bytes. Old: aa aa bb cc cc cc cc -> 7 lines, 21 bytes.
    // New: aa cc cc dd -> 4 lines, 12 bytes.
    // `drop bb` removes bb. `sample 0.5 cc` keeps floor(n × 0.5) lines and bytes.
    // Old after rules: aa 2 lines / 6 bytes, cc 2 / 6. Together 4 lines, 12 bytes.
    // New after rules: aa 1 / 3, cc 1 / 3, dd 1 / 3. Together 3 lines, 9 bytes.
    // Share 6/12 = 50.0%. 3/9 = 333 rem 3 -> 33.3%. Change -16.7 percentage points.
    // dd is new: 3 of 9 = 33.3%. bb no longer appears on either side.
    // Month of the 3-byte difference, window 1d, gb 1000, price 1, 30 days:
    // 3 × 30 × 100 / 1000 = 9 cents -> -0.09. Without rules, 9 bytes -> 0.27.
    let old = write_tmp("rules-old.log", b"aa\naa\nbb\ncc\ncc\ncc\ncc\n");
    let new_sample = write_tmp("rules-new.log", b"aa\ncc\ncc\ndd\n");
    let line_list = write_tmp("rules-diff.txt", b"drop bb\nsample 0.5 cc\n");
    let price_set = write_tmp(
        "rules-diff.json",
        br#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1000}"#,
    );
    let file_path = |path: &std::path::Path| path.to_str().expect("path").to_owned();
    let (code, without, err) = run_exit(&["diff", &file_path(&old), &file_path(&new_sample)]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert!(
        without
            .contains("total: 7 lines, 21 bytes -> 4 lines, 12 bytes, change -3 lines, -9 bytes\n"),
        "{without}"
    );
    assert!(
        without.contains("gone P08a63607  3 -> 0 bytes, change -3 bytes, share 14.3% -> 0.0%, change -14.3 percentage points  bb\n"),
        "{without}"
    );
    assert!(!without.contains("rules applied"), "{without}");

    let (code, with_run, err) = run_exit(&[
        "diff",
        "--rules",
        &file_path(&line_list),
        "--window",
        "1d",
        "--prices",
        &file_path(&price_set),
        &file_path(&old),
        &file_path(&new_sample),
    ]);
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new_sample);
    let _ = std::fs::remove_file(&line_list);
    let _ = std::fs::remove_file(&price_set);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        with_run,
        "\
total: 4 lines, 12 bytes -> 3 lines, 9 bytes, change -1 line, -3 bytes
rules applied: 2
new P08915407  0 -> 3 bytes, change +3 bytes, share 0.0% -> 33.3%, change +33.3 percentage points  dd
P089c4307  6 -> 3 bytes, change -3 bytes, share 50.0% -> 33.3%, change -16.7 percentage points  aa
P08a24d07  6 -> 3 bytes, change -3 bytes, share 50.0% -> 33.3%, change -16.7 percentage points  cc
new patterns: 3 of 9 bytes, 33.3%
prices: EUR, source kladblok, date 1999-01-02
change per month ingest: -0.09 EUR
estimate: extrapolation of one sample with your own prices; not an official rate
"
    );
    assert!(!with_run.contains("bb"), "{with_run}");
    assert!(!with_run.contains("drop"), "{with_run}");
    assert!(
        !with_run.replace(ESTIMATE, "").contains("sample"),
        "{with_run}"
    );
    assert!(!with_run.contains("0.5"), "{with_run}");
    assert!(!with_run.contains("-0.27"), "{with_run}");
}

#[test]
fn overflow_diff_matches_scan() {
    // 10^19 seconds × 10^20 gb_bytes does not fit in u128.
    // Scan stops at exit 3. Diff must give the same error, and no rounded amount.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let log = dir.join("overflow-equal.log");
    let price_set = dir.join("overflow-equal.json");
    std::fs::write(&log, "ok\n").expect("write fixture");
    std::fs::write(
        &price_set,
        r#"{"currency":"EUR","source":"kladblok","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1e20}"#,
    )
    .expect("write fixture");
    let log_file = log.to_str().expect("path");
    let price_path = price_set.to_str().expect("path");
    let scan = run_exit(&[
        "scan",
        "--window",
        "10000000000000000000s",
        "--prices",
        price_path,
        log_file,
    ]);
    let diff = run_exit(&[
        "diff",
        "--window",
        "10000000000000000000s",
        "--prices",
        price_path,
        log_file,
        log_file,
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&price_set);
    for (name, (code, out, err)) in [("scan", scan), ("diff", diff)] {
        assert_eq!(code, 3, "{name}");
        assert!(out.is_empty(), "{name}: {out}");
        assert_eq!(err, "calculation overflow\n", "{name}");
        assert!(!err.contains("ok"), "{name}");
    }
}

#[test]
fn broken_rules_file_on_diff_exit_2() {
    // Line 1 is empty. Line 2 is a regex that does not compile.
    // Stderr names the line number, not the regex or the rest of the line.
    let line_list = write_tmp("diff-broken.rules", b"\ndrop INVALID-REGEX-0002[\n");
    let old = manifest("tests/fixtures/diff-old.log");
    let new_sample = manifest("tests/fixtures/diff-new.log");
    let (code, out, err) = run_exit(&[
        "diff",
        "--rules",
        line_list.to_str().expect("path"),
        old.to_str().expect("path"),
        new_sample.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&line_list);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "invalid rule on line 2 of the rules file\n");
    assert!(!err.contains("INVALID-REGEX"));
    assert!(!err.contains("drop"));
    assert!(!err.contains('['));
}
