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
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn file_path(path: &Path) -> &str {
    path.to_str().expect("path")
}

const ESTIMATE: &str =
    "estimate: extrapolation of one sample with your own prices; not an official rate";

/// 999_999 times 'q' plus `\n` = 1_000_000 bytes.
fn million(name: &str) -> PathBuf {
    let mut raw = vec![b'q'; 999_999];
    raw.push(b'\n');
    tmp(name, &raw)
}

fn prices(name: &str, json: &str) -> PathBuf {
    tmp(name, json.as_bytes())
}

/// 1_000_000 bytes, window 1m, 30 days, ingest 1/10.
/// Exact month: 1_000_000 × 1440 / 1_000_000_000 × 1/10 × 30 = 4.32.
#[test]
fn example_4_32_equal_is_exit_0_above_is_exit_1() {
    let log = million("budget-voorbeeld.log");
    let prices = prices(
        "budget-ingest.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let log_path = file_path(&log);
    let prices_file = file_path(&prices);
    // Rare but equal spellings of 4.32: leading zeros, trailing zeros, a plus.
    for amount in ["4.32", "4.320", "+4.32", "04.32", "4.3200"] {
        let (code, out, err) = run(&[
            "scan",
            "--window",
            "1m",
            "--prices",
            prices_file,
            "--budget-month",
            amount,
            log_path,
        ]);
        assert_eq!(code, 0, "{amount} stderr: {err} stdout: {out}");
        assert!(err.is_empty(), "{amount}: {err}");
        assert!(
            out.contains("total ingest: 0.14 EUR per day, 4.32 EUR per month\n"),
            "{out}"
        );
        assert!(out.ends_with(&format!("{ESTIMATE}\n")), "{out}");
        assert!(!out.contains('q'), "{out}");
    }
    // 4.32 is above 4.319 and above 0. 4.325 is still above the cost: do not round to cents.
    for amount in ["4.319", "0", "4.31"] {
        let (code, out, err) = run(&[
            "scan",
            "--window",
            "1m",
            "--prices",
            prices_file,
            "--budget-month",
            amount,
            log_path,
        ]);
        assert_eq!(code, 1, "{amount}");
        assert_eq!(err, "monthly cost above budget\n", "{amount}");
        assert!(out.contains("4.32 EUR per month\n"), "{out}");
        assert!(out.contains(ESTIMATE), "{out}");
        assert!(!out.contains('q'), "{out}");
        assert!(!err.contains('q'), "{err}");
    }
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        prices_file,
        "--budget-month",
        "4.325",
        log_path,
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("4.32 EUR per month\n"), "{out}");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
}

/// Same sample, index 1/4: 1.440 × 1/4 × 30 = 10.80. Together 4.32 + 10.80 = 15.12.
#[test]
fn ingest_plus_index_15_12() {
    let log = million("budget-index.log");
    let prices = prices(
        "budget-index.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10,"index_per_gb":0.25}"#,
    );
    let basis = [
        "scan",
        "--window",
        "1m",
        "--prices",
        file_path(&prices),
        "--budget-month",
    ];
    let (code, out, err) = run(&[&basis[..], &["15.12", file_path(&log)]].concat());
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(
        out.contains("total ingest: 0.14 EUR per day, 4.32 EUR per month\n"),
        "{out}"
    );
    assert!(
        out.contains("total index: 0.36 EUR per day, 10.80 EUR per month\n"),
        "{out}"
    );
    let (code, out, err) = run(&[&basis[..], &["15.11", file_path(&log)]].concat());
    assert_eq!(code, 1, "{out}");
    assert_eq!(err, "monthly cost above budget\n");
    assert!(out.contains("10.80 EUR per month\n"), "{out}");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
}

/// `keep` = 5 bytes, `drop-me` = 8, together 13. After `drop`, 5 remain.
/// Window 1d, gb 1, price 1: month per stream = bytes × 30.
/// After rules, 5 × 30 = 150 per stream; ingest + index = 300.
/// Without rules, 13 × 30 × 2 = 780. Shown total stays the pre-rules total.
#[test]
fn after_rules_counts_ingest_plus_index() {
    let log = tmp("budget-rules.log", b"keep\ndrop-me\n");
    let lines = tmp("budget-rules.txt", b"drop drop-me\n");
    let prices = prices(
        "budget-rules.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"index_per_gb":1,"gb_bytes":1}"#,
    );
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--rules",
        file_path(&lines),
        "--budget-month",
        "300",
        file_path(&log),
    ]);
    assert_eq!(code, 0, "stderr: {err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("total: 2 lines, 13 bytes\n"), "{out}");
    assert!(
        out.contains(
            "after rules: 2 lines, 13 bytes -> 1 line, 5 bytes, savings 1 line, 8 bytes\n"
        ),
        "{out}"
    );
    assert!(
        out.contains("total ingest: 13.00 EUR per day, 390.00 EUR per month\n"),
        "{out}"
    );
    assert!(
        out.contains("total index: 13.00 EUR per day, 390.00 EUR per month\n"),
        "{out}"
    );
    assert!(!out.contains("CANARY"), "{out}");

    let (code, _, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--rules",
        file_path(&lines),
        "--budget-month",
        "299",
        file_path(&log),
    ]);
    assert_eq!(code, 1);
    assert_eq!(err, "monthly cost above budget\n");

    // Without rules, 780 is above 300.
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "300",
        file_path(&log),
    ]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(err, "monthly cost above budget\n");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&lines);
    let _ = std::fs::remove_file(&prices);
}

/// 9 bytes, 1d, gb 2000, price 1, 30 days.
/// Exact month 9 × 30 / 2000 = 0.135. 13.5 cents round to 0.14.
/// The budget compares 0.135, not the displayed 0.14.
#[test]
fn budget_compares_the_fraction_not_the_cent() {
    let log = tmp("budget-fraction.log", b"abcdefgh\n");
    let prices = prices(
        "budget-fraction.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":2000}"#,
    );
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "0.135",
        file_path(&log),
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert!(
        out.contains("total ingest: 0.00 EUR per day, 0.14 EUR per month\n"),
        "{out}"
    );

    let (code, _, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "0.134",
        file_path(&log),
    ]);
    assert_eq!(code, 1);
    assert_eq!(err, "monthly cost above budget\n");

    let (code, _, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "0.14",
        file_path(&log),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty(), "{err}");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
}

/// 26 bytes, 1 day, gb 250, ingest 1 and index 1.
/// Each is 26/250 = 0.104, sum 0.208. Each shows 0.10 (10.4 cents -> 10).
/// 0.10 + 0.10 = 0.20 is below the real sum.
#[test]
fn sum_of_the_fractions_not_of_the_cents() {
    let log = tmp("budget-sum.log", b"abcdefghijklmnopqrstuvwxy\n");
    let prices = prices(
        "budget-sum.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"index_per_gb":1,"gb_bytes":250}"#,
    );
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "0.208",
        file_path(&log),
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert!(
        out.contains("total ingest: 0.10 EUR per day, 0.10 EUR per month\n"),
        "{out}"
    );
    assert!(
        out.contains("total index: 0.10 EUR per day, 0.10 EUR per month\n"),
        "{out}"
    );

    let (code, _, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "0.20",
        file_path(&log),
    ]);
    assert_eq!(code, 1);
    assert_eq!(err, "monthly cost above budget\n");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
}

/// Empty input is 0 bytes. 0 is not above 0.
#[test]
fn zero_bytes_budget_zero_is_exit_0() {
    let log = tmp("budget-empty.log", b"\n");
    let prices = prices(
        "budget-empty.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1}"#,
    );
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "0",
        file_path(&log),
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("total: 0 lines, 0 bytes\n"), "{out}");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
}

#[test]
fn without_prices_or_window_exit_3() {
    let missing = "missing-dir/budget.log";
    let (code, out, err) = run(&["scan", "--budget-month", "1", missing]);
    assert_eq!(code, 3);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "budget requires a window\n");

    let (code, out, err) = run(&["scan", "--budget-month", "1", "--window", "1m", missing]);
    assert_eq!(code, 3);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "budget requires a price file\n");

    let prices = prices(
        "budget-without-window.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let (code, out, err) = run(&[
        "scan",
        "--budget-month",
        "1",
        "--prices",
        file_path(&prices),
        missing,
    ]);
    assert_eq!(code, 3, "{err}");
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "budget requires a window\n");
    let _ = std::fs::remove_file(&prices);
}

#[test]
fn invalid_budget_exit_3_without_the_text() {
    let missing = "missing-dir/budget.log";
    for amount in [
        "4,32",
        "5.",
        "1e2",
        "",
        "CANARY-SECRET-0001",
        "++1",
        ".",
        "+",
    ] {
        let (code, out, err) = run(&["scan", "--budget-month", amount, "--window", "1m", missing]);
        assert_eq!(code, 3, "{amount:?} stderr: {err}");
        assert!(out.is_empty(), "{amount:?}: {out}");
        assert_eq!(err, "invalid budget\n", "{amount:?}");
        assert!(!err.contains("CANARY"), "{amount:?}: {err}");
        assert!(
            amount.is_empty() || !err.contains(amount),
            "{amount:?}: {err}"
        );
    }
    // A bare `-1` is a clap flag. Attached to the option, it reaches the parser.
    let (code, out, err) = run(&["scan", "--budget-month=-1", "--window", "1m", missing]);
    assert_eq!(code, 3);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "invalid budget\n");
    assert!(!err.contains("-1"), "{err}");
    let (code, out, err) = run(&["diff", "--budget-month", concat!("4,", "32"), "a", "b"]);
    assert_eq!(code, 3);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "invalid budget\n");
}

#[test]
fn missing_file_and_broken_rules_stay_exit_2() {
    let prices = prices(
        "budget-exit2.json",
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":0.10}"#,
    );
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        file_path(&prices),
        "--budget-month",
        "1",
        "missing-dir/budget.log",
    ]);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "file is not readable\n");

    let log = tmp("budget-exit2.log", b"keep\n");
    let lines = tmp("budget-exit2.txt", b"CANARY-SECRET-0001\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        file_path(&prices),
        "--rules",
        file_path(&lines),
        "--budget-month",
        "1",
        file_path(&log),
    ]);
    assert_eq!(code, 2);
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "invalid rule on line 1 of the rules file\n");
    assert!(!err.contains("CANARY"), "{err}");
    let _ = std::fs::remove_file(&prices);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&lines);
}
