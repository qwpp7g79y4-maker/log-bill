//! Indexed-log cost on `scan`. Events stay a fraction until display.
//! Amounts round half away from zero. Without the new options the output
//! does not gain an indexed section.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const FIXTURE: &str = "tests/fixtures/indexed/app.log";
const CANARY: &str = "CANARY-INDEX-RULE-0001";

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-bill"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let output = bin().args(args).output().expect("start binary");
    let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    (output.status.code(), out, err)
}

fn scratch(name: &str, bytes: &str) -> String {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    fs::write(&path, bytes).expect("write scratch file");
    path.to_str().expect("utf-8 path").to_owned()
}

fn assert_usage(args: &[&str], message: &str) {
    let (code, out, err) = run(args);
    assert_eq!(code, Some(3), "{args:?}\nstdout: {out}\nstderr: {err}");
    assert!(out.is_empty(), "{args:?} stdout: {out}");
    assert_eq!(err, format!("{message}\n"), "{args:?}");
}

fn assert_indexed(
    out: &str,
    before: &str,
    after: &str,
    cost_before: &str,
    cost_after: &str,
    savings: &str,
) {
    assert!(
        out.contains(&format!(
            "indexed events per month: {before} before index rules, {after} after"
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "indexed cost per month: {cost_before} USD before index rules, {cost_after} USD after, savings {savings} USD"
        )),
        "{out}"
    );
}

/// `--window 1s --days-per-month 30 --retention 7 --billing annual` and `drop DEBUG`.
/// 2673/6000 × 15552M × 1.27 rounds to 8799.09.
#[test]
fn fixture_drop_debug_text_and_json() {
    let rules = scratch("drop-debug.txt", "drop DEBUG\n");
    let args = [
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "7",
        "--billing",
        "annual",
        "--index-rules",
        &rules,
        FIXTURE,
    ];
    let (code, out, err) = run(&args);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains(
        "indexed: list price, estimate, retention 7d, billing annual, 1.27 USD per 1M events"
    ));
    assert_indexed(
        &out,
        "15552.000M",
        "6928.416M",
        "19751.04",
        "8799.09",
        "10951.95",
    );

    let mut json_args = args.to_vec();
    json_args.insert(1, "--json");
    let (code, body, err) = run(&json_args);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    for needle in [
        "\"billing\": \"annual\"",
        "\"cost_after\": \"8799.09\"",
        "\"cost_before\": \"19751.04\"",
        "\"currency\": \"USD\"",
        "\"events_after\": \"6928.416M\"",
        "\"events_before\": \"15552.000M\"",
        "\"label\": \"list price, estimate\"",
        "\"retention_days\": 7",
        "\"savings\": \"10951.95\"",
        "\"usd_per_million\": \"1.27\"",
    ] {
        assert!(body.contains(needle), "missing {needle}\n{body}");
    }
}

#[test]
fn fixture_drop_debug_explain_shows_the_unrounded_cost() {
    let rules = scratch("drop-debug-explain.txt", "drop DEBUG\n");
    let (code, out, err) = run(&[
        "scan",
        "--explain",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "7",
        "--billing",
        "annual",
        "--index-rules",
        &rules,
        FIXTURE,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("6928.416"), "{out}");
    assert!(out.contains("8799.08832"), "{out}");
    assert!(out.contains("8799.09"), "{out}");
    assert!(out.contains("list price, estimate"), "{out}");
}

/// `drop DEBUG`, `drop GET /health`, `sample 0.1 INFO order`.
/// 378.9/6000 × 15552M × 1.27 rounds to 1247.28.
#[test]
fn fixture_three_index_rules_text_and_json() {
    let rules = scratch(
        "three-index-rules.txt",
        "drop DEBUG\ndrop GET /health\nsample 0.1 INFO order\n",
    );
    let args = [
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "7",
        "--billing",
        "annual",
        "--index-rules",
        &rules,
        FIXTURE,
    ];
    let (code, out, err) = run(&args);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert_indexed(
        &out,
        "15552.000M",
        "982.109M",
        "19751.04",
        "1247.28",
        "18503.76",
    );

    let mut json_args = args.to_vec();
    json_args.insert(1, "--json");
    let (code, body, err) = run(&json_args);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(body.contains("\"events_after\": \"982.109M\""), "{body}");
    assert!(body.contains("\"cost_after\": \"1247.28\""), "{body}");
    assert!(body.contains("\"events_before\": \"15552.000M\""), "{body}");
}

#[test]
fn every_retention_and_billing_cell() {
    let log = scratch("one-ok.log", "ok\n");
    // 1 record × 30 × 86400 / 1_000_000 = 2.592M. Cost = 2.592 × the cell.
    let cells = [
        ("3", "annual", "1.06", "2.75"),
        ("3", "monthly", "1.27", "3.29"),
        ("3", "ondemand", "1.59", "4.12"),
        ("7", "annual", "1.27", "3.29"),
        ("7", "monthly", "1.52", "3.94"),
        ("7", "ondemand", "1.91", "4.95"),
        ("15", "annual", "1.70", "4.41"),
        ("15", "monthly", "2.04", "5.29"),
        ("15", "ondemand", "2.55", "6.61"),
        ("30", "annual", "2.50", "6.48"),
        ("30", "monthly", "3.00", "7.78"),
        ("30", "ondemand", "3.75", "9.72"),
    ];
    for (retention, billing, rate, cost) in cells {
        let (code, out, err) = run(&[
            "scan",
            "--window",
            "1s",
            "--days-per-month",
            "30",
            "--retention",
            retention,
            "--billing",
            billing,
            &log,
        ]);
        assert_eq!(code, Some(0), "{retention} {billing} {err}");
        assert!(err.is_empty(), "{err}");
        assert!(
            out.contains(&format!(
                "indexed: list price, estimate, retention {retention}d, billing {billing}, {rate} USD per 1M events"
            )),
            "{retention} {billing}\n{out}"
        );
        assert_indexed(&out, "2.592M", "2.592M", cost, cost, "0.00");
    }
}

#[test]
fn indexed_price_overrides_the_table() {
    let log = scratch("override-ok.log", "ok\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--indexed-price",
        "1.5",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    // 2.592 × 1.5 = 3.888 → 3.89. The 15d ondemand cell (2.55) is not used.
    assert!(out.contains(
        "indexed: custom price, estimate, retention 15d, billing ondemand, 1.50 USD per 1M events"
    ));
    assert_indexed(&out, "2.592M", "2.592M", "3.89", "3.89", "0.00");
    assert!(!out.contains("2.55 USD"), "{out}");

    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "7",
        "--billing",
        "annual",
        "--indexed-price",
        "1.00",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    // 2.592 × 1.00 = 2.592 → 2.59, not the 7d annual cell 3.29.
    assert!(out.contains("retention 7d, billing annual, 1.00 USD per 1M events"));
    assert_indexed(&out, "2.592M", "2.592M", "2.59", "2.59", "0.00");
    assert!(!out.contains("3.29"), "{out}");
}

#[test]
fn indexed_price_keeps_digits_past_a_binary_float() {
    let log = scratch("long-price.log", "ok\n");
    let (code, out, err) = run(&[
        "scan",
        "--explain",
        "--window",
        "1s",
        "--indexed-price",
        "1000.0000000000000001",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("1000.0000000000000001"), "{out}");
}

#[test]
fn empty_file_is_zero() {
    let log = scratch("empty.log", "");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert_indexed(&out, "0.000M", "0.000M", "0.00", "0.00", "0.00");
}

#[test]
fn one_line_uses_the_fifteen_day_ondemand_default() {
    let log = scratch("one-line.log", "ok\n");
    // `--retention` alone defaults billing to ondemand: 2.55 → 6.61.
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(
        out.contains("billing ondemand, 2.55 USD per 1M events"),
        "{out}"
    );
    assert_indexed(&out, "2.592M", "2.592M", "6.61", "6.61", "0.00");

    // `--billing` alone defaults retention to 15 days.
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--billing",
        "ondemand",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        out.contains("retention 15d, billing ondemand, 2.55 USD"),
        "{out}"
    );
}

#[test]
fn sample_zero_keeps_nothing() {
    let log = scratch("sample-zero-lines.log", "ok\n");
    let rules = scratch("sample-zero.txt", "sample 0 ok\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        "--index-rules",
        &rules,
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert_indexed(&out, "2.592M", "0.000M", "6.61", "0.00", "6.61");
}

#[test]
fn sample_tenth_stays_a_fraction() {
    let mut body = String::new();
    for _ in 0..10 {
        body.push_str("INFO order 1\n");
    }
    let log = scratch("ten-orders.log", &body);
    let rules = scratch("sample-tenth.txt", "sample 0.1 INFO order\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        "--index-rules",
        &rules,
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    // 10 × 0.1 = 1 event, not floor(10 × 0.1) = 1 by accident of an integer.
    // 1 × 2.592 = 2.592M. Before the rule the 10 records are 25.920M.
    assert_indexed(&out, "25.920M", "2.592M", "66.10", "6.61", "59.49");
}

#[test]
fn sample_one_is_rejected_without_the_rule_line() {
    let log = scratch("sample-one.log", "ok\n");
    let rules = scratch("sample-one.txt", &format!("sample 1 {CANARY}\n"));
    let (code, out, err) = run(&["scan", "--window", "1s", "--index-rules", &rules, &log]);
    assert_eq!(code, Some(2), "{err}");
    assert!(out.is_empty(), "{out}");
    assert_eq!(err, "invalid rule on line 1 of the index rules file\n");
    assert!(!err.contains(CANARY), "{err}");
    assert!(!err.contains("sample 1"), "{err}");
}

#[test]
fn index_rule_that_hits_everything_and_one_that_hits_nothing() {
    let log = scratch("hit.log", "ok\n");
    let all = scratch("drop-all.txt", "drop .\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        "--index-rules",
        &all,
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert_indexed(&out, "2.592M", "0.000M", "6.61", "0.00", "6.61");

    let none = scratch("drop-none.txt", "drop NOPE\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        "--index-rules",
        &none,
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert_indexed(&out, "2.592M", "2.592M", "6.61", "6.61", "0.00");
}

#[test]
fn jsonl_newline_inside_a_message_is_one_record() {
    // A newline inside the JSON string, a blank line, then one more record.
    // Two ingested records: 5.184M, not three physical breaks at 7.776M.
    let log = scratch(
        "stacked.jsonl",
        "{\"message\":\"alpha\\nbeta\"}\n\n{\"message\":\"second\"}\n",
    );
    let (code, out, err) = run(&[
        "scan",
        "--format",
        "jsonl",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("total: 2 lines"), "{out}");
    assert_indexed(&out, "5.184M", "5.184M", "13.22", "13.22", "0.00");
    assert!(!out.contains("7.776M"), "{out}");
}

#[test]
fn rules_then_index_rules() {
    let log = scratch("both-rules.log", "keep\ndrop\nsample\n");
    let ingest = scratch("ingest-rules.txt", "drop ^drop\nsample 0.5 ^sample\n");
    let index = scratch("index-after.txt", "drop ^keep\nsample 0.1 ^sample\n");
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1s",
        "--days-per-month",
        "30",
        "--retention",
        "15",
        "--rules",
        &ingest,
        "--index-rules",
        &index,
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    // keep 1, drop 0, sample 0.5 → 1.5 before. Index drops keep and keeps
    // 0.1 of the 0.5 → 0.05 after. 1.5 × 2.592 = 3.888; 0.05 × 2.592 = 0.1296 → 0.130.
    assert_indexed(&out, "3.888M", "0.130M", "9.91", "0.33", "9.58");
}

#[test]
fn invalid_retention_billing_price_and_index_rules_without_window() {
    assert_usage(
        &["scan", "--retention", "5", "file.log"],
        "invalid retention",
    );
    assert_usage(
        &["diff", "--retention", "5", "OLD", "NEW"],
        "invalid retention",
    );
    assert_usage(
        &["scan", "--billing", "yearly", "file.log"],
        "invalid billing",
    );
    assert_usage(
        &["scan", "--indexed-price", "nope", "file.log"],
        "invalid indexed price",
    );
    assert_usage(
        &[
            "scan",
            "--indexed-price",
            "-1",
            "--window",
            "1s",
            "file.log",
        ],
        "invalid indexed price",
    );
    assert_usage(
        &["scan", "--indexed-price", "1", "file.log"],
        "--indexed-price only works together with --window",
    );
    let missing = format!("missing-{CANARY}/rules.txt");
    assert_usage(
        &["scan", "--index-rules", &missing, "file.log"],
        "--index-rules only works together with --window",
    );
    let (code, out, err) = run(&["scan", "--index-rules", &missing, "file.log"]);
    assert_eq!(code, Some(3));
    assert!(out.is_empty());
    assert!(!err.contains(CANARY), "{err}");
    assert!(!err.contains("file.log"), "{err}");
}

#[test]
fn half_cent_rounds_away_from_zero() {
    let mut body = String::new();
    for _ in 0..1000 {
        body.push_str("ok\n");
    }
    let log = scratch("half-cent.log", &body);
    // 1000 events × 5 USD / 1M = 0.005 → 0.01. 4 USD → 0.004 → 0.00.
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--indexed-price",
        "5",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert_indexed(&out, "0.001M", "0.001M", "0.01", "0.01", "0.00");

    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--indexed-price",
        "4",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert_indexed(&out, "0.001M", "0.001M", "0.00", "0.00", "0.00");

    let mut more = String::new();
    for _ in 0..5000 {
        more.push_str("ok\n");
    }
    let log = scratch("half-cent-dollar.log", &more);
    // 5000 × 201 / 1_000_000 = 1.005 → 1.01.
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--indexed-price",
        "201",
        &log,
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert_indexed(&out, "0.005M", "0.005M", "1.01", "1.01", "0.00");
}

#[test]
fn scan_without_the_new_flags_omits_indexing() {
    let (code, out, err) = run(&["scan", "--window", "1s", FIXTURE]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.is_empty(), "{err}");
    assert!(!out.contains("indexed:"), "{out}");
    assert!(!out.contains("list price"), "{out}");
    let (code, body, err) = run(&["scan", "--json", "--window", "1s", FIXTURE]);
    assert_eq!(code, Some(0), "{err}");
    assert!(!body.contains("\"indexed\""), "{body}");
}

#[test]
fn budget_ignores_the_indexed_estimate() {
    let log = scratch("budget-ok.log", "ok\n");
    let prices = scratch(
        "budget-prices.json",
        "{\"currency\":\"USD\",\"source\":\"scratch\",\"date\":\"2026-10-14\",\"ingest_per_gb\":0.10}\n",
    );
    // One record at 1_000_000 USD per 1M is far above a 1 USD budget.
    // The budget still looks only at the ingest bytes.
    let (code, out, err) = run(&[
        "scan",
        "--window",
        "1d",
        "--days-per-month",
        "1",
        "--prices",
        &prices,
        "--budget-month",
        "1",
        "--indexed-price",
        "1000000",
        &log,
    ]);
    assert_eq!(code, Some(0), "stderr: {err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert!(out.contains("1000000.00 USD"), "{out}");
    assert!(!out.contains("monthly cost above budget"), "{out}");
}

#[test]
fn diff_rejects_the_index_flags() {
    assert_usage(
        &["diff", "--explain", "--retention", "7", "OLD", "NEW"],
        "--explain only works with scan",
    );
    assert_usage(
        &[
            "diff",
            "--explain",
            "--index-rules",
            "rules.txt",
            "OLD",
            "NEW",
        ],
        "--explain only works with scan",
    );
    assert_usage(
        &["diff", "--retention", "7", "OLD", "NEW"],
        "--retention only works with scan",
    );
    assert_usage(
        &["diff", "--billing", "monthly", "OLD", "NEW"],
        "--billing only works with scan",
    );
    assert_usage(
        &["diff", "--indexed-price", "1", "OLD", "NEW"],
        "--indexed-price only works with scan",
    );
    assert_usage(
        &["diff", "--index-rules", "rules.txt", "OLD", "NEW"],
        "--index-rules only works with scan",
    );
    assert_usage(
        &["diff", "--window", "1m", "--retention", "7", "OLD", "NEW"],
        "--window with diff only works together with --prices or --json",
    );
}
