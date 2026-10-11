use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
}

fn manifest(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn scan(args: &[&str]) -> String {
    let output = bin().args(args).output().expect("start binary");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).expect("stdout is utf-8")
}

fn assert_snapshot(name: &str, actual: &str) {
    let path = manifest(&format!("tests/snapshots/{name}"));
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read snapshot {} ({err})", path.display()));
    assert_eq!(actual, expected, "snapshot {}", path.display());
}

#[test]
fn snapshot_text() {
    let path = manifest("tests/fixtures/scan-text.log");
    let actual = scan(&["scan", path.to_str().expect("path")]);
    assert!(!actual.contains("192.0.2.10"));
    assert!(!actual.contains("2026-10-08T11:07:44Z"));
    assert!(!actual.contains("user 7"));
    assert!(!actual.contains("user 8"));
    assert_snapshot("scan-text.txt", &actual);
}

#[test]
fn window_1m_and_31_days() {
    // 999_999 times '!' plus `\n` = 1_000_000 bytes. '!' masks to `<VAR>`.
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("window-1m.log");
    let mut raw = vec![b'!'; 999_999];
    raw.push(b'\n');
    std::fs::write(&path, raw).expect("write fixture");
    let file_path = path.to_str().expect("path");

    let without = scan(&["scan", file_path]);
    let day30 = scan(&["scan", "--window", "1m", file_path]);
    let day31 = scan(&[
        "scan",
        "--window",
        "1m",
        "--days-per-month",
        "31",
        file_path,
    ]);
    let _ = std::fs::remove_file(&path);

    assert!(!without.contains("GB"), "{without}");
    assert!(!without.contains("factor:"), "{without}");
    assert!(day30.contains("total: 1 line, 1000000 bytes\n"), "{day30}");
    assert!(day30.contains("factor: 1440\n"), "{day30}");
    assert!(
        day30.contains("per day: 1440000000 bytes, 1.440 GB\n"),
        "{day30}"
    );
    assert!(
        day30.contains("per month: 43200000000 bytes, 43.200 GB\n"),
        "{day30}"
    );
    assert!(!day30.contains('!'), "{day30}");
    assert!(
        day31.contains("per day: 1440000000 bytes, 1.440 GB\n"),
        "{day31}"
    );
    assert!(
        day31.contains("per month: 44640000000 bytes, 44.640 GB\n"),
        "{day31}"
    );
    assert!(!day31.contains('!'), "{day31}");
}

fn write_tmp(item_name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(item_name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn pattern_id(text: &str, fragment: &str) -> String {
    text.lines()
        .find(|sample_line| {
            sample_line.contains(fragment)
                && !sample_line.contains("ingest")
                && !sample_line.contains("index")
        })
        .expect("pattern line")
        .split_whitespace()
        .next()
        .expect("pattern id")
        .to_owned()
}

const ESTIMATE: &str =
    "estimate: extrapolation of one sample with your own prices; not an official rate";

#[test]
fn cost_example_ingest() {
    // 999_999 times 'q' plus `\n` = 1_000_000 bytes.
    // Window 1m, factor 1440, gb_bytes default 1_000_000_000, ingest 1/10 per GB.
    // Per day: 1_000_000 × 1440 / 1_000_000_000 × 1/10 = 144/1000.
    // Shown to 2 decimals: 0.14. Month = that fraction × 30 = 432/100 = 4.32.
    let mut raw = vec![b'q'; 999_999];
    raw.push(b'\n');
    let log = write_tmp("cost-1m.log", &raw);
    let prices = write_tmp(
        "cost-ingest.json",
        br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let text = scan(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        prices.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);

    assert!(!text.contains('q'), "{text}");
    assert!(
        text.contains("prices: EUR, source scratchpad, date 1999-01-02\n"),
        "{text}"
    );
    assert!(
        text.contains("total ingest: 0.14 EUR per day, 4.32 EUR per month\n"),
        "{text}"
    );
    let id = pattern_id(&text, "<VAR>");
    assert!(
        text.contains(&format!(
            "{id} ingest: 0.14 EUR per day, 4.32 EUR per month\n"
        )),
        "{text}"
    );
    assert!(!text.contains("index"), "{text}");
    assert!(text.ends_with(&format!("{ESTIMATE}\n")), "{text}");
    assert_eq!(text.matches("estimate:").count(), 1);
}

#[test]
fn cost_example_with_index() {
    // Same 1_000_000 bytes and ingest 1/10: 0.14 per day and 4.32 per month.
    // Index 1/4 per GB: 1.440 × 1/4 = 0.36 per day, × 30 = 10.80 per month.
    let mut raw = vec![b'q'; 999_999];
    raw.push(b'\n');
    let log = write_tmp("cost-index.log", &raw);
    let prices = write_tmp(
        "cost-index.json",
        br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10,"index_per_gb":0.25}"#,
    );
    let text = scan(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        prices.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);

    assert!(!text.contains('q'), "{text}");
    assert!(
        text.contains("total ingest: 0.14 EUR per day, 4.32 EUR per month\n"),
        "{text}"
    );
    assert!(
        text.contains("total index: 0.36 EUR per day, 10.80 EUR per month\n"),
        "{text}"
    );
    let id = pattern_id(&text, "<VAR>");
    assert!(
        text.contains(&format!(
            "{id} index: 0.36 EUR per day, 10.80 EUR per month\n"
        )),
        "{text}"
    );
    assert!(text.ends_with(&format!("{ESTIMATE}\n")), "{text}");
}

#[test]
fn cost_rounding_half_cent() {
    // Price 1 per GB, gb_bytes 2000, window 1d (factor 1), 30 days.
    // 10 bytes: 10/2000 = 0.005 -> half a cent, away from zero -> 0.01.
    // Month exactly 0.15.
    // 9 bytes: 9/2000 = 0.0045 -> 0.00 per day.
    // Month 9/2000×30 = 0.135 -> also half a cent -> 0.14. Not 0.00×30.
    // 1005 bytes, gb_bytes 1000: 1.005 -> 1.01. Month exactly 30.15.
    let prices = write_tmp(
        "cost-half.json",
        br#"{"currency":"GULDEN","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":2000}"#,
    );
    let half = write_tmp("cost-half.log", b"abcdefghi\n");
    let below = write_tmp("cost-below.log", b"abcdefgh\n");
    let half_text = scan(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        half.to_str().expect("path"),
    ]);
    let below_text = scan(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        below.to_str().expect("path"),
    ]);

    let thousand = write_tmp(
        "cost-1005.json",
        br#"{"currency":"GULDEN","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1000}"#,
    );
    let mut large = vec![b'q'; 1004];
    large.push(b'\n');
    let large_log = write_tmp("cost-1005.log", &large);
    let large_text = scan(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        thousand.to_str().expect("path"),
        large_log.to_str().expect("path"),
    ]);
    for file_path in [&prices, &half, &below, &thousand, &large_log] {
        let _ = std::fs::remove_file(file_path);
    }

    assert!(
        half_text.contains("total ingest: 0.01 GULDEN per day, 0.15 GULDEN per month\n"),
        "{half_text}"
    );
    assert!(
        below_text.contains("total ingest: 0.00 GULDEN per day, 0.14 GULDEN per month\n"),
        "{below_text}"
    );
    assert!(!large_text.contains('q'), "{large_text}");
    assert!(
        large_text.contains("total ingest: 1.01 GULDEN per day, 30.15 GULDEN per month\n"),
        "{large_text}"
    );
}

#[test]
fn cost_per_pattern() {
    // The 5-byte line and the 4-byte line, total 9. Window 1d, price 1, gb_bytes 9.
    // GB: 9/9 = 1.000 per day. Month 270 bytes, 270/9 = 30.000 GB.
    // Total: 9/9 = 1.00 per day, × 30 = 30.00.
    // 5-byte line: 500/9 cents = 55 + 5/9 -> 56 cents = 0.56. Month 15000/9 = 1666 + 2/3 -> 16.67.
    // 4-byte line: 400/9 cents = 44 + 4/9 -> 44 cents = 0.44. Month 12000/9 = 1333 + 1/3 -> 13.33.
    let log = write_tmp("cost-patterns.log", b"hond\nkat\n");
    let prices = write_tmp(
        "cost-patterns.json",
        br#"{"currency":"MARK","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":9}"#,
    );
    let text = scan(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);

    assert!(text.contains("per day: 9 bytes, 1.000 GB\n"), "{text}");
    assert!(text.contains("per month: 270 bytes, 30.000 GB\n"), "{text}");
    assert!(
        text.contains("total ingest: 1.00 MARK per day, 30.00 MARK per month\n"),
        "{text}"
    );
    let dog = pattern_id(&text, "hond");
    let cat = pattern_id(&text, "kat");
    assert_ne!(dog, cat);
    assert!(
        text.contains(&format!(
            "{dog} ingest: 0.56 MARK per day, 16.67 MARK per month\n"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "{cat} ingest: 0.44 MARK per day, 13.33 MARK per month\n"
        )),
        "{text}"
    );
    assert!(text.ends_with(&format!("{ESTIMATE}\n")), "{text}");
}

#[test]
fn without_prices_no_currency() {
    let log = manifest("tests/fixtures/scan-text.log");
    let prices = write_tmp(
        "cost-unused.json",
        br#"{"currency":"KRONER","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1}"#,
    );
    let with_window = scan(&["scan", "--window", "1m", log.to_str().expect("path")]);
    // Prices without a window are no longer a silent scan.
    let with_prices = bin()
        .args([
            "scan",
            "--prices",
            prices.to_str().expect("path"),
            "missing.log",
        ])
        .output()
        .expect("start binary");
    let _ = std::fs::remove_file(&prices);

    assert!(with_window.contains("factor: 1440\n"), "{with_window}");
    assert!(!with_window.contains("KRONER"), "{with_window}");
    assert!(!with_window.contains("EUR"), "{with_window}");
    assert!(!with_window.contains("estimate:"), "{with_window}");
    assert!(!with_window.contains("ingest:"), "{with_window}");
    assert_eq!(with_prices.status.code(), Some(3));
    assert!(with_prices.stdout.is_empty());
    assert_eq!(
        String::from_utf8(with_prices.stderr).expect("stderr is utf-8"),
        "--prices only works together with --window\n"
    );
}

#[test]
fn drop_ten_lines_four_gone() {
    // 4 × the dropped line = 16 bytes, dropped. 6 × the kept line = 36 bytes, kept.
    // Total 10 lines, 52 bytes. New 6 lines, 36 bytes. Savings 4 lines, 16 bytes.
    // Window 1d, gb_bytes 16, ingest 1, index 1/2.
    // Savings per day: 16/16 × 1 = 1.00 and 16/16 × 1/2 = 0.50.
    // Month 30: 30.00 and 15.00. Month 31: 31.00 and 15.50.
    // The volume block stays the sample: 52/16 = 3.250 GB, not the 36 bytes after drop.
    let mut raw = Vec::new();
    for _ in 0..4 {
        raw.extend_from_slice(b"weg\n");
    }
    for _ in 0..6 {
        raw.extend_from_slice(b"blijf\n");
    }
    let log = write_tmp("drop-ten.log", &raw);
    let line_list = write_tmp("drop-ten.rules", b"drop ^weg$\n");
    let prices = write_tmp(
        "drop-ten.json",
        br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"index_per_gb":0.5,"gb_bytes":16}"#,
    );
    let args = [
        "scan",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ];
    let text = scan(&args);
    let mut day31 = args.to_vec();
    day31.insert(3, "--days-per-month");
    day31.insert(4, "31");
    let day31 = scan(&day31);
    let without_window = bin()
        .args([
            "scan",
            "--prices",
            prices.to_str().expect("path"),
            "--rules",
            line_list.to_str().expect("path"),
            "missing.log",
        ])
        .output()
        .expect("start binary");
    let without_prices = scan(&[
        "scan",
        "--window",
        "1d",
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    for file_path in [&log, &line_list, &prices] {
        let _ = std::fs::remove_file(file_path);
    }

    let kept = pattern_id(&text, "blijf");
    let gone = pattern_id(&text, "weg");
    let block = format!(
        "after rules: 10 lines, 52 bytes -> 6 lines, 36 bytes, savings 4 lines, 16 bytes\n\
         {kept}  6 lines, 36 bytes -> 6 lines, 36 bytes, savings 0 lines, 0 bytes\n\
         {gone}  4 lines, 16 bytes -> 0 lines, 0 bytes, savings 4 lines, 16 bytes, rule 1\n"
    );
    assert!(text.contains(&block), "{text}");
    assert!(text.contains("per day: 52 bytes, 3.250 GB\n"), "{text}");
    assert!(
        text.contains("savings total ingest: 1.00 EUR per day, 30.00 EUR per month\n"),
        "{text}"
    );
    assert!(
        text.contains("savings total index: 0.50 EUR per day, 15.00 EUR per month\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "savings {gone} ingest: 1.00 EUR per day, 30.00 EUR per month\n"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "savings {gone} index: 0.50 EUR per day, 15.00 EUR per month\n"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "savings {kept} ingest: 0.00 EUR per day, 0.00 EUR per month\n"
        )),
        "{text}"
    );
    assert!(!text.contains("^weg$"), "{text}");
    assert!(!text.contains("drop"), "{text}");
    assert!(text.ends_with(&format!("{ESTIMATE}\n")), "{text}");

    assert!(
        day31.contains("savings total ingest: 1.00 EUR per day, 31.00 EUR per month\n"),
        "{day31}"
    );
    assert!(
        day31.contains("savings total index: 0.50 EUR per day, 15.50 EUR per month\n"),
        "{day31}"
    );

    assert_eq!(without_window.status.code(), Some(3));
    assert!(without_window.stdout.is_empty());
    assert_eq!(
        String::from_utf8(without_window.stderr).expect("stderr is utf-8"),
        "--prices only works together with --window\n"
    );

    assert!(without_prices.contains(&block), "{without_prices}");
    assert!(without_prices.contains("factor: 1\n"), "{without_prices}");
    assert!(!without_prices.contains("EUR"), "{without_prices}");
    assert!(!without_prices.contains("ingest:"), "{without_prices}");
    assert!(!without_prices.contains("estimate:"), "{without_prices}");
}

#[test]
fn drop_tests_the_raw_json_line() {
    // Line 1 is 32 bytes and contains the marker outside the message. Line 2 is 17 bytes.
    // The marker rule removes only line 1: 1 line, 32 bytes.
    // `drop ^ok$` matches the message, not the raw JSON line, and removes nothing.
    let log = write_tmp(
        "drop-json.jsonl",
        b"{\"message\":\"ok\",\"marker\":\"WEG\"}\n{\"message\":\"ok\"}\n",
    );
    let gone = write_tmp("drop-json-gone.rules", b"drop WEG\n");
    let ok = write_tmp("drop-json-ok.rules", b"drop ^ok$\n");
    let file_path = log.to_str().expect("path");
    let with = scan(&[
        "scan",
        "--format",
        "jsonl",
        "--rules",
        gone.to_str().expect("path"),
        file_path,
    ]);
    let without = scan(&[
        "scan",
        "--format",
        "jsonl",
        "--rules",
        ok.to_str().expect("path"),
        file_path,
    ]);
    for file in [&log, &gone, &ok] {
        let _ = std::fs::remove_file(file);
    }

    assert!(
        with.contains(
            "after rules: 2 lines, 49 bytes -> 1 line, 17 bytes, savings 1 line, 32 bytes\n"
        ),
        "{with}"
    );
    assert!(!with.contains("WEG"), "{with}");
    assert!(!with.contains("marker"), "{with}");
    assert!(
        without.contains(
            "after rules: 2 lines, 49 bytes -> 2 lines, 49 bytes, savings 0 lines, 0 bytes\n"
        ),
        "{without}"
    );
    assert!(!without.contains("^ok$"), "{without}");
    assert!(!without.contains("WEG"), "{without}");
}

#[test]
fn drop_crlf_counts_the_return() {
    // The dropped line with a carriage return = 5 bytes, including the return. The whole line is dropped, not 4.
    let log = write_tmp("drop-crlf.log", b"weg\r\n");
    let line_list = write_tmp("drop-crlf.rules", b"drop ^weg$\r\n");
    let text = scan(&[
        "scan",
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&line_list);
    assert!(
        text.contains(
            "after rules: 1 line, 5 bytes -> 0 lines, 0 bytes, savings 1 line, 5 bytes\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(", savings 1 line, 5 bytes, rule 1\n"),
        "{text}"
    );
    assert!(!text.contains('\r'), "{text}");
}

#[test]
fn sample_0_1_bytes_and_savings() {
    // 10 × `aa\n` = 30 bytes. `sample 0.1` leaves floor(30 × 1/10) = 3 bytes
    // and floor(10 × 1/10) = 1 line. Per line, rounding down would be 0.
    // Savings 9 lines, 27 bytes.
    // Window 1d, gb_bytes 27, ingest 1: savings 1.00 per day and 30.00 per month.
    let log = write_tmp(
        "sample-dot1.log",
        b"aa\naa\naa\naa\naa\naa\naa\naa\naa\naa\n",
    );
    let line_list = write_tmp("sample-dot1.rules", b"sample 0.1 ^aa$\n");
    let prices = write_tmp(
        "sample-dot1.json",
        br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":27}"#,
    );
    let text = scan(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    for file_path in [&log, &line_list, &prices] {
        let _ = std::fs::remove_file(file_path);
    }

    // `aa` is inside the word total; the pattern is on the line with the share.
    let id = pattern_id(&text, "%  aa");
    let block = format!(
        "after rules: 10 lines, 30 bytes -> 1 line, 3 bytes, savings 9 lines, 27 bytes\n\
         {id}  10 lines, 30 bytes -> 1 line, 3 bytes, savings 9 lines, 27 bytes, rule 1\n"
    );
    assert!(text.contains(&block), "{text}");
    assert!(
        text.contains("savings total ingest: 1.00 EUR per day, 30.00 EUR per month\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "savings {id} ingest: 1.00 EUR per day, 30.00 EUR per month\n"
        )),
        "{text}"
    );
    assert!(!text.replace(ESTIMATE, "").contains("sample"));
    assert!(!text.contains("^aa$"));
    assert!(text.ends_with(&format!("{ESTIMATE}\n")), "{text}");
}

#[test]
fn sample_0_gives_zero_lines_and_zero_bytes() {
    // 10 × `aa\n` = 30 bytes. sample 0 leaves floor(10 × 0) lines and floor(30 × 0) bytes.
    let log = write_tmp(
        "sample-zero.log",
        b"aa\naa\naa\naa\naa\naa\naa\naa\naa\naa\n",
    );
    let line_list = write_tmp("sample-zero.rules", b"sample 0 ^aa$\n");
    let text = scan(&[
        "scan",
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&line_list);
    let id = pattern_id(&text, "%  aa");
    let block = format!(
        "after rules: 10 lines, 30 bytes -> 0 lines, 0 bytes, savings 10 lines, 30 bytes\n\
         {id}  10 lines, 30 bytes -> 0 lines, 0 bytes, savings 10 lines, 30 bytes, rule 1\n"
    );
    assert!(text.contains(&block), "{text}");
    assert!(!text.replace(ESTIMATE, "").contains("sample"), "{text}");
    assert!(!text.contains("^aa$"), "{text}");
}

#[test]
fn sample_fraction_outside_0_to_1_exit_2() {
    // 1, 1.0, 2, and -0.1 lie outside 0..1. A valid sample on line 1
    // followed by 1.5 keeps line number 2, without the fraction or the regex.
    for (item_name, source, sample_line) in [
        ("sample-one.rules", &b"sample 1 ^a$\n"[..], "1"),
        ("sample-one-dot.rules", &b"sample 1.0 ^a$\n"[..], "1"),
        ("sample-twee.rules", &b"sample 2 ^a$\n"[..], "1"),
        ("sample-negatief.rules", &b"sample -0.1 ^a$\n"[..], "1"),
        (
            "sample-anderhalf.rules",
            &b"sample 0.1 ^ok$\nsample 1.5 CANARY-FRAC-0001\n"[..],
            "2",
        ),
    ] {
        let file_path = write_tmp(item_name, source);
        let (code, out, err) = run_exit(&[
            "scan",
            "--rules",
            file_path.to_str().expect("path"),
            "missing-log.txt",
        ]);
        assert_eq!(code, 2, "{item_name}");
        assert!(out.is_empty(), "{item_name}");
        assert_eq!(
            err,
            format!("invalid rule on line {sample_line} of the rules file\n")
        );
        assert!(!err.contains("CANARY"), "{err}");
        assert!(!err.contains("1.5"), "{err}");
        assert!(!err.contains("sample"), "{err}");
        assert!(!err.contains("^a$"), "{err}");
        let _ = std::fs::remove_file(&file_path);
    }
}

fn run_exit(args: &[&str]) -> (i32, String, String) {
    let output = bin().args(args).output().expect("start binary");
    let code = output.status.code().expect("exit code");
    (
        code,
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        String::from_utf8(output.stderr).expect("stderr is utf-8"),
    )
}

#[test]
fn rules_exit_2_without_content() {
    let unknown = write_tmp("drop-unknown.rules", b"drop ^ok$\nkeep CANARY-REGEX-0001\n");
    let (code, out, err) = run_exit(&[
        "scan",
        "--rules",
        unknown.to_str().expect("path"),
        "missing-log.txt",
    ]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err, "invalid rule on line 2 of the rules file\n");
    assert!(!err.contains("CANARY"));
    assert!(!err.contains("keep"));
    let _ = std::fs::remove_file(&unknown);

    let regex = write_tmp("drop-regex.rules", b"\ndrop CANARY-REGEX-0002[\n");
    let (code, out, err) = run_exit(&[
        "scan",
        "--rules",
        regex.to_str().expect("path"),
        "missing-log.txt",
    ]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err, "invalid rule on line 2 of the rules file\n");
    assert!(!err.contains("CANARY"));
    let _ = std::fs::remove_file(&regex);

    let empty = write_tmp("drop-empty.rules", b"drop\n");
    let (code, out, err) = run_exit(&[
        "scan",
        "--rules",
        empty.to_str().expect("path"),
        "missing-log.txt",
    ]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err, "invalid rule on line 1 of the rules file\n");
    assert!(!err.contains("drop"));
    let _ = std::fs::remove_file(&empty);

    let (code, out, err) = run_exit(&[
        "scan",
        "--rules",
        "CANARY-SECRET-0001.rules",
        "missing-log.txt",
    ]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(err, "rules file is not readable\n");
    assert!(!err.contains("CANARY"));
}

#[test]
fn explain_example_1m() {
    // 999_999 times 'q' plus `\n` = 1_000_000 bytes.
    // One token of 32 or more characters becomes one `<VAR>`.
    // FNV-1a 64 of `<VAR>` is 5a76f64fc19b374a, pattern id P5a76f64f.
    // Window 1m, factor 1440, gb_bytes 1_000_000_000, ingest 1/10.
    // 1_000_000 × 1440 / 1_000_000_000 × 1/10 = 0.144 -> shown 0.14 per day.
    // 0.144 × 30 = 4.32 -> shown 4.32 per month.
    let mut raw = vec![b'q'; 999_999];
    raw.push(b'\n');
    let log = write_tmp("explain-1m.log", &raw);
    let prices = write_tmp(
        "explain-1m.json",
        br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let file_path = log.to_str().expect("path");
    let prices_file = prices.to_str().expect("path");
    let actual = scan(&[
        "scan",
        "--explain",
        "--window",
        "1m",
        "--prices",
        prices_file,
        file_path,
    ]);
    let without = scan(&["scan", "--window", "1m", "--prices", prices_file, file_path]);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);

    assert!(!actual.contains('q'), "{actual}");
    assert!(!actual.contains("rule line"), "{actual}");
    assert_snapshot("explain-1m.txt", &actual);
    assert!(!without.contains("calculation"), "{without}");
    assert!(!without.contains("masks"), "{without}");
    assert!(!without.contains("0.144"), "{without}");
    assert!(
        without.contains("total ingest: 0.14 EUR per day, 4.32 EUR per month\n"),
        "{without}"
    );
}

#[test]
fn explain_masks_and_line_number() {
    // `pod-7\n` = 6 bytes, one `<VAR>`. The other line = 6 bytes, no mask.
    // On equal bytes the pattern text wins: `<VAR>` before the unmatched line.
    // `drop pod` is on line 1 and matches only the first line.
    // Window 1d, factor 1, gb_bytes 6, price 1.
    // Total 12 bytes: 12/6 = 2 -> 2.00 per day, × 30 = 60 -> 60.00 per month.
    // `<VAR>` and the savings are 6 bytes: 6/6 = 1 -> 1.00, × 30 = 30 -> 30.00.
    // The unmatched line stays: savings 0 -> 0.00 per day and 0.00 per month.
    let log = write_tmp("explain-line.log", b"pod-7\nblijf\n");
    let line_list = write_tmp("explain-line.rules", b"drop pod\n");
    let prices = write_tmp(
        "explain-line.json",
        br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":6}"#,
    );
    let text = scan(&[
        "scan",
        "--explain",
        "--window",
        "1d",
        "--prices",
        prices.to_str().expect("path"),
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    let without_price = scan(&[
        "scan",
        "--explain",
        "--rules",
        line_list.to_str().expect("path"),
        log.to_str().expect("path"),
    ]);
    for file_path in [&log, &line_list, &prices] {
        let _ = std::fs::remove_file(file_path);
    }

    let block = "\
masks P5a76f64f: <VAR> 1
rule line P5a76f64f: 1
masks P062a7bab: none
rule line P062a7bab: none
";
    assert!(text.contains(block), "{text}");
    assert!(
        text.contains(
            "calculation total ingest: 12 × 1 / 6 × 1 = 2 → 2.00 per day, × 30 = 60 → 60.00 per month\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "calculation P5a76f64f ingest: 6 × 1 / 6 × 1 = 1 → 1.00 per day, × 30 = 30 → 30.00 per month\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "calculation savings total ingest: 6 × 1 / 6 × 1 = 1 → 1.00 per day, × 30 = 30 → 30.00 per month\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "calculation savings P062a7bab ingest: 0 × 1 / 6 × 1 = 0 → 0.00 per day, × 30 = 0 → 0.00 per month\n"
        ),
        "{text}"
    );
    assert!(!text.contains("pod"), "{text}");
    assert!(!text.contains("drop"), "{text}");
    assert!(text.ends_with(&format!("{ESTIMATE}\n")), "{text}");
    assert!(without_price.contains(block), "{without_price}");
    assert!(!without_price.contains("calculation"), "{without_price}");
    assert!(!without_price.contains("EUR"), "{without_price}");
}

#[test]
fn scan_ipv6_fixture_masks_and_counts_ip() {
    let file_path = manifest("tests/fixtures/scan-ipv6.log");
    let arg = file_path.to_str().expect("path");
    let text = scan(&["scan", arg]);
    assert!(text.contains("client <IP> connected\n"), "{text}");
    assert!(text.contains("connect from <IP> ok\n"), "{text}");
    assert!(
        text.lines()
            .any(|sample_line| sample_line.ends_with("  <IP>")),
        "{text}"
    );
    for raw_text in [
        "[::1]", "2001:db8", ":8080", ":443", "::ffff", "fe80::", "%eth0",
    ] {
        assert!(
            !text.contains(raw_text),
            "{raw_text} stays visible in {text}"
        );
    }

    let explanation = scan(&["scan", "--explain", arg]);
    assert!(explanation.contains("<IP> 1"), "{explanation}");
    assert!(explanation.contains("<IP> 2"), "{explanation}");
    assert!(!explanation.contains("<VAR>"), "{explanation}");
    assert!(!explanation.contains("::ffff"), "{explanation}");

    let json = scan(&["scan", "--json", "--explain", arg]);
    assert!(
        json.contains("\"text\": \"client <IP> connected\""),
        "{json}"
    );
    assert!(
        json.contains("\"text\": \"connect from <IP> ok\""),
        "{json}"
    );
    assert!(json.contains("\"text\": \"<IP>\""), "{json}");
    assert!(json.contains("\"ip\": 1"), "{json}");
    assert!(json.contains("\"ip\": 2"), "{json}");
    assert!(!json.contains("\"ip\": 0"), "{json}");
    for raw_text in ["[::1]", "2001:db8", "::ffff", "fe80::", ":8080"] {
        assert!(!json.contains(raw_text), "{raw_text} in {json}");
    }
}

#[test]
fn scan_key_with_prefix_or_suffix() {
    let file_path = manifest("tests/fixtures/scan-keys.log");
    let arg = file_path.to_str().expect("path");
    let text = scan(&["scan", arg]);
    for piece in [
        "db_password=<SECRET>",
        "SECRET_KEY=<SECRET>",
        "x_token=<SECRET>",
        "my_secret_value=<SECRET>",
        "x-api-key: <SECRET>",
        "X-Api-Key: <SECRET>",
        "Db-Password: <SECRET>",
        "API_KEY=<SECRET>",
        "\"db_password\": \"<SECRET>\"",
        "voor <TS> na",
    ] {
        assert!(text.contains(piece), "missing {piece:?}\n{text}");
    }
    for raw_text in [
        "hunterSecretWord",
        "abcSuperSecretValue",
        "thisIsNotHexValueAtAll",
        "=abcdef",
        "=geheim",
        ": geheim",
        "=letters",
        "2026-10-08T13:55:36,123Z",
    ] {
        assert!(!text.contains(raw_text), "{raw_text} in {text}");
    }

    let explanation = scan(&["scan", "--explain", arg]);
    assert_eq!(
        explanation.matches("<SECRET> 1").count(),
        9,
        "{explanation}"
    );
    assert!(explanation.contains("<TS> 1"), "{explanation}");
    assert!(!explanation.contains("hunterSecretWord"), "{explanation}");
    assert!(
        !explanation.contains("thisIsNotHexValueAtAll"),
        "{explanation}"
    );

    let json = scan(&["scan", "--json", "--explain", arg]);
    assert_eq!(json.matches("\"secret\": 1").count(), 9, "{json}");
    assert!(json.contains("\"ts\": 1"), "{json}");
    assert!(
        json.contains("\"text\": \"db_password=<SECRET>\""),
        "{json}"
    );
    assert!(json.contains("\"text\": \"x-api-key: <SECRET>\""), "{json}");
    assert!(json.contains("\"text\": \"API_KEY=<SECRET>\""), "{json}");
    assert!(!json.contains("hunterSecretWord"), "{json}");
    assert!(!json.contains("thisIsNotHexValueAtAll"), "{json}");
    assert!(!json.contains("abcSuperSecretValue"), "{json}");
}

#[test]
fn scan_authorization_masks_the_whole_value() {
    let text_path = manifest("tests/fixtures/scan-authorization.log");
    let jsonl_path = manifest("tests/fixtures/scan-authorization.jsonl");
    let text_arg = text_path.to_str().expect("path");
    let jsonl_arg = jsonl_path.to_str().expect("path");
    let secret_text = [
        "dXNlcjpwYXNz",
        "hunterSecretWord",
        "abcSuperSecretValue",
        "YWJjZGVm",
        "Mufasa",
        "realmWordValue",
        "nonceWordValue",
        "responseWordValue",
        "ntlmLetterToken",
        "negotiateLetter",
        "fooLetterToken",
        "Basic",
        "Bearer",
        "Digest",
        "NTLM",
        "Negotiate",
        "Foo",
    ];
    let patterns = [
        "Authorization: <SECRET>",
        "Proxy-Authorization: <SECRET>",
        "x-authorization: <SECRET>",
        "my_proxy-authorization: <SECRET>",
        "\"authorization\": \"<SECRET>\"",
        "\"proxy-authorization\": \"<SECRET>\"",
        "{\"authorization\": \"<SECRET>\", \"user\": \"ada\"}",
    ];

    let text = scan(&["scan", text_arg]);
    for piece in patterns {
        assert!(text.contains(piece), "missing {piece:?}\n{text}");
    }
    for raw_text in secret_text {
        assert!(!text.contains(raw_text), "{raw_text} in {text}");
    }

    let explanation = scan(&["scan", "--explain", text_arg]);
    assert!(explanation.contains("<SECRET> 12"), "{explanation}");
    assert!(explanation.contains("\"user\": \"ada\""), "{explanation}");
    for raw_text in secret_text {
        assert!(
            !explanation.contains(raw_text),
            "{raw_text} in {explanation}"
        );
    }

    let json = scan(&["scan", "--json", "--explain", text_arg]);
    assert!(json.contains("\"secret\": 12"), "{json}");
    assert!(
        json.contains("\"text\": \"Authorization: <SECRET>\""),
        "{json}"
    );
    assert!(
        json.contains("\"text\": \"Proxy-Authorization: <SECRET>\""),
        "{json}"
    );
    for raw_text in secret_text {
        assert!(!json.contains(raw_text), "{raw_text} in {json}");
    }

    let diff = scan(&["diff", text_arg, text_arg]);
    assert!(diff.contains("Authorization: <SECRET>"), "{diff}");
    assert!(diff.contains("Proxy-Authorization: <SECRET>"), "{diff}");
    for raw_text in secret_text {
        assert!(!diff.contains(raw_text), "{raw_text} in {diff}");
    }

    let diff_json = scan(&["diff", "--json", text_arg, text_arg]);
    assert!(
        diff_json.contains("\"text\": \"Authorization: <SECRET>\""),
        "{diff_json}"
    );
    for raw_text in secret_text {
        assert!(!diff_json.contains(raw_text), "{raw_text} in {diff_json}");
    }

    let jsonl = scan(&["scan", "--format", "jsonl", "--explain", jsonl_arg]);
    assert!(jsonl.contains("Authorization: <SECRET>"), "{jsonl}");
    assert!(jsonl.contains("Proxy-Authorization: <SECRET>"), "{jsonl}");
    assert!(jsonl.contains("\"authorization\": \"<SECRET>\""), "{jsonl}");
    for raw_text in secret_text {
        assert!(!jsonl.contains(raw_text), "{raw_text} in {jsonl}");
    }

    let jsonl_json = scan(&[
        "scan",
        "--format",
        "jsonl",
        "--json",
        "--explain",
        jsonl_arg,
    ]);
    assert!(jsonl_json.contains("\"secret\":"), "{jsonl_json}");
    for raw_text in secret_text {
        assert!(!jsonl_json.contains(raw_text), "{raw_text} in {jsonl_json}");
    }

    let jsonl_diff = scan(&["diff", "--format", "jsonl", jsonl_arg, jsonl_arg]);
    for raw_text in secret_text {
        assert!(!jsonl_diff.contains(raw_text), "{raw_text} in {jsonl_diff}");
    }

    let jsonl_diff_json = scan(&["diff", "--format", "jsonl", "--json", jsonl_arg, jsonl_arg]);
    for raw_text in secret_text {
        assert!(
            !jsonl_diff_json.contains(raw_text),
            "{raw_text} in {jsonl_diff_json}"
        );
    }

    let error = bin()
        .args(["scan", "--format", "jsonl", text_arg])
        .output()
        .expect("binary");
    let error_stdout = String::from_utf8_lossy(&error.stdout);
    let error_stderr = String::from_utf8_lossy(&error.stderr);
    assert_eq!(error.status.code(), Some(2), "{error_stderr}");
    for raw_text in secret_text {
        assert!(
            !error_stdout.contains(raw_text),
            "{raw_text} in stdout {error_stdout}"
        );
        assert!(
            !error_stderr.contains(raw_text),
            "{raw_text} in stderr {error_stderr}"
        );
    }
}

#[test]
fn scan_quoted_examples_are_fully_masked() {
    let file_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("scan-quoted.log");
    let body = concat!(
        r#"{"password": "hunter two words", "user": "ada"}"#,
        "\n",
        r#"{"token":"eyJhbGciOiJIUzI1NiJ9"}"#,
        "\n",
    );
    std::fs::write(&file_path, body).expect("fixture");
    let arg = file_path.to_str().expect("path");
    let patterns = [
        r#"{"password": "<SECRET>", "user": "ada"}"#,
        r#"{"token":"<SECRET>"}"#,
    ];
    let json_patterns = [
        r#"{\"password\": \"<SECRET>\", \"user\": \"ada\"}"#,
        r#"{\"token\":\"<SECRET>\"}"#,
    ];
    let secret_text = [
        "hunter two words",
        "hunter",
        "two words",
        "eyJhbGciOiJIUzI1NiJ9",
    ];

    let text = scan(&["scan", arg]);
    for piece in patterns {
        assert!(text.contains(piece), "missing {piece:?}\n{text}");
    }
    for raw_text in secret_text {
        assert!(!text.contains(raw_text), "{raw_text} in {text}");
    }

    let explanation = scan(&["scan", "--explain", arg]);
    for piece in patterns {
        assert!(
            explanation.contains(piece),
            "missing {piece:?}\n{explanation}"
        );
    }
    assert_eq!(
        explanation.matches("<SECRET> 1").count(),
        2,
        "{explanation}"
    );
    for raw_text in secret_text {
        assert!(
            !explanation.contains(raw_text),
            "{raw_text} in {explanation}"
        );
    }

    let json = scan(&["scan", "--json", "--explain", arg]);
    for piece in json_patterns {
        assert!(json.contains(piece), "missing {piece:?}\n{json}");
    }
    assert_eq!(json.matches("\"secret\": 1").count(), 2, "{json}");
    for raw_text in secret_text {
        assert!(!json.contains(raw_text), "{raw_text} in {json}");
    }

    let diff = scan(&["diff", arg, arg]);
    for piece in patterns {
        assert!(diff.contains(piece), "missing {piece:?}\n{diff}");
    }
    for raw_text in secret_text {
        assert!(!diff.contains(raw_text), "{raw_text} in {diff}");
    }

    let diff_json = scan(&["diff", "--json", arg, arg]);
    for piece in json_patterns {
        assert!(diff_json.contains(piece), "missing {piece:?}\n{diff_json}");
    }
    for raw_text in secret_text {
        assert!(!diff_json.contains(raw_text), "{raw_text} in {diff_json}");
    }

    let _ = std::fs::remove_file(&file_path);
}

#[test]
fn plain_sentence_hex_and_digit_secret() {
    // Seven lines, each with a newline (bytes = length + 1).
    // 31+1, 10+1, 12+1, 10+1, 10+1, 12+1, 16+1 = 32+11+13+11+11+13+17 = 108.
    // `1699482000` and `123456789012` both become `<NUM>`: 11+13 = 24 bytes, 2 lines.
    // `deadbeef01` and `0a1b2c3d4e` both become `<HEX>`: 11+11 = 22 bytes, 2 lines.
    // Tenths = bytes × 1000 / 108, half step away from zero.
    // 32 -> 296 (remainder 32, 64 < 108) = 29.6
    // 24 -> 222 (remainder 24, 48 < 108) = 22.2
    // 22 -> 204 (remainder 76, 152 >= 108) = 20.4
    // 17 -> 157 (remainder 44, 88 < 108) = 15.7
    // 13 -> 120 (remainder 40, 80 < 108) = 12.0
    let body = "\
This is a secret mission report
1699482000
123456789012
deadbeef01
0a1b2c3d4e
deadbeefcafe
token=1699482000
";
    let log = write_tmp("scan-sentence.log", body.as_bytes());
    let file_path = log.to_str().expect("path");
    let pattern = "This is a secret <SECRET> report";
    let forbidden = [
        "mission",
        "1699482000",
        "123456789012",
        "deadbeef01",
        "0a1b2c3d4e",
    ];

    let text = scan(&["scan", file_path]);
    assert!(text.contains("total: 7 lines, 108 bytes\n"), "{text}");
    assert!(
        text.contains(&format!("1 line  32 bytes  29.6%  {pattern}")),
        "{text}"
    );
    assert!(text.contains("2 lines  24 bytes  22.2%  <NUM>"), "{text}");
    assert!(text.contains("2 lines  22 bytes  20.4%  <HEX>"), "{text}");
    assert!(
        text.contains("1 line  17 bytes  15.7%  token=<SECRET>"),
        "{text}"
    );
    assert!(
        text.contains("1 line  13 bytes  12.0%  deadbeefcafe"),
        "{text}"
    );
    assert_eq!(text.matches("<HEX>").count(), 1, "{text}");
    for raw_text in forbidden {
        assert!(!text.contains(raw_text), "{raw_text} in {text}");
    }

    let explanation = scan(&["scan", "--explain", file_path]);
    assert!(explanation.contains(pattern), "{explanation}");
    assert!(explanation.contains("token=<SECRET>"), "{explanation}");
    assert_eq!(
        explanation.matches("<SECRET> 1 (possible secret)").count(),
        2,
        "{explanation}"
    );
    assert!(explanation.contains("<NUM> 2"), "{explanation}");
    assert!(explanation.contains("<HEX> 2"), "{explanation}");
    assert!(explanation.contains("deadbeefcafe"), "{explanation}");
    for raw_text in forbidden {
        assert!(
            !explanation.contains(raw_text),
            "{raw_text} in {explanation}"
        );
    }

    let json = scan(&["scan", "--json", file_path]);
    assert!(json.contains(&format!("\"text\": \"{pattern}\"")), "{json}");
    assert!(json.contains("\"text\": \"<NUM>\""), "{json}");
    assert!(json.contains("\"text\": \"<HEX>\""), "{json}");
    assert!(json.contains("\"text\": \"token=<SECRET>\""), "{json}");
    assert!(json.contains("\"text\": \"deadbeefcafe\""), "{json}");
    assert_eq!(
        json.matches("\"note\": \"possible secret\"").count(),
        2,
        "{json}"
    );
    assert!(!json.contains("\"masks\""), "{json}");
    for raw_text in forbidden {
        assert!(!json.contains(raw_text), "{raw_text} in {json}");
    }

    let json_explain = scan(&["scan", "--json", "--explain", file_path]);
    assert!(
        json_explain.contains(&format!("\"text\": \"{pattern}\"")),
        "{json_explain}"
    );
    assert_eq!(
        json_explain
            .matches("\"note\": \"possible secret\"")
            .count(),
        2,
        "{json_explain}"
    );
    assert_eq!(
        json_explain.matches("\"secret\": 1").count(),
        2,
        "{json_explain}"
    );
    for key in ["ts", "uuid", "ip", "email", "secret", "hex", "num", "var"] {
        assert!(
            json_explain.contains(&format!("\"{key}\":")),
            "missing masks.{key}\n{json_explain}"
        );
    }
    assert!(json_explain.contains("\"hex\": 2"), "{json_explain}");
    assert!(json_explain.contains("\"num\": 2"), "{json_explain}");
    for raw_text in forbidden {
        assert!(
            !json_explain.contains(raw_text),
            "{raw_text} in {json_explain}"
        );
    }

    let _ = std::fs::remove_file(&log);
}

#[test]
fn snapshot_jsonl() {
    let path = manifest("tests/fixtures/scan-jsonl.jsonl");
    let actual = scan(&["scan", "--format", "jsonl", path.to_str().expect("path")]);
    assert!(!actual.contains("RAW-TRACE-0001"));
    assert!(!actual.contains("192.0.2.10"));
    assert!(!actual.contains("2026-10-08T11:07:44Z"));
    assert_snapshot("scan-jsonl.txt", &actual);
}
