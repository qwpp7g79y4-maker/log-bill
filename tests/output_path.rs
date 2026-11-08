//! `--output PATH` writes the report to a file. An existing file is replaced.
//! A missing parent directory is exit 3, the same code as a failed write to
//! stdout, and that directory is not created. Exit 1 stays the budget or
//! the new-pattern threshold.

use std::fs;
use std::process::Command;

fn run(args: &[&str]) -> (Option<i32>, Vec<u8>, Vec<u8>) {
    let output = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("log-bill binary");
    (output.status.code(), output.stdout, output.stderr)
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn output_flag_writes_a_new_file() {
    let help = run(&["--help"]);
    assert_eq!(help.0, Some(0));
    let help_text = String::from_utf8(help.1).expect("help is utf-8");
    assert!(help_text.contains("--output"), "{help_text}");
    assert!(
        !help_text.contains("--out "),
        "no second flag next to --output"
    );

    let log = fixture("scan-text.log");
    let (plain_code, expected, plain_err) = run(&["scan", &log]);
    let plain_err = String::from_utf8_lossy(&plain_err);
    assert_eq!(plain_code, Some(0), "{plain_err}");
    assert!(!expected.is_empty());

    let root = tempfile::tempdir().expect("scratch dir");
    let report = root.path().join("report.txt");
    assert!(!report.exists());
    let report_path = report.to_str().expect("utf-8 path");
    let (code, stdout, stderr) = run(&["scan", "--output", report_path, &log]);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, Some(0), "{err}");
    assert!(stdout.is_empty(), "stdout must stay empty: {stdout:?}");
    assert!(stderr.is_empty(), "{err}");
    assert_eq!(fs::read(&report).expect("read report"), expected);

    let old = fixture("diff-old.log");
    let new = fixture("diff-new.log");
    let (diff_code, diff_expected, diff_err) = run(&["diff", &old, &new]);
    let diff_err = String::from_utf8_lossy(&diff_err);
    assert_eq!(diff_code, Some(0), "{diff_err}");
    let diff_report = root.path().join("diff.txt");
    let diff_path = diff_report.to_str().expect("utf-8 path");
    let (code, stdout, stderr) = run(&["diff", "--output", diff_path, &old, &new]);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, Some(0), "{err}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert_eq!(
        fs::read(&diff_report).expect("read diff report"),
        diff_expected
    );

    let prices = fixture("prices.json");
    let (budget_code, budget_expected, budget_err) = run(&[
        "scan",
        "--window",
        "1m",
        "--prices",
        &prices,
        "--budget-month",
        "0",
        &log,
    ]);
    let budget_err = String::from_utf8_lossy(&budget_err);
    assert_eq!(
        budget_code,
        Some(1),
        "budget without --output: {budget_err}"
    );
    let limited = root.path().join("limited.txt");
    let limited_path = limited.to_str().expect("utf-8 path");
    let (code, stdout, stderr) = run(&[
        "scan",
        "--output",
        limited_path,
        "--window",
        "1m",
        "--prices",
        &prices,
        "--budget-month",
        "0",
        &log,
    ]);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, Some(1), "budget stays exit 1: {err}");
    assert_ne!(code, Some(4), "no new exit code");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(
        err.contains("monthly cost above budget"),
        "threshold message stays: {err}"
    );
    assert_eq!(
        fs::read(&limited).expect("read limited report"),
        budget_expected
    );

    let verbose_report = root.path().join("verbose.txt");
    let verbose_path = verbose_report.to_str().expect("utf-8 path");
    let (vcode, vout, verr) = run(&["scan", "--verbose", "--output", verbose_path, &log]);
    let verr = String::from_utf8_lossy(&verr);
    assert_eq!(vcode, Some(0), "{verr}");
    assert!(vout.is_empty(), "{vout:?}");
    assert!(verr.starts_with("context: "), "{verr}");
    assert_eq!(
        fs::read(&verbose_report).expect("read verbose report"),
        expected
    );
}

#[test]
fn output_flag_overwrites_an_existing_file() {
    let root = tempfile::tempdir().expect("scratch dir");
    let report = root.path().join("report.txt");
    let prior = b"PRIOR-BYTES-NOT-THE-REPORT\n";
    fs::write(&report, prior).unwrap();
    let log = fixture("scan-text.log");
    let report_path = report.to_str().expect("utf-8 path");
    let (code, stdout, stderr) = run(&["scan", "--output", report_path, &log]);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, Some(0), "{err}");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.is_empty(), "{err}");
    let (plain_code, expected, plain_err) = run(&["scan", &log]);
    let plain_err = String::from_utf8_lossy(&plain_err);
    assert_eq!(plain_code, Some(0), "{plain_err}");
    let body = fs::read(&report).expect("read report");
    assert_eq!(body, expected);
    assert_ne!(body, prior);
}

#[test]
fn output_flag_missing_directory_exits_3() {
    let root = tempfile::tempdir().expect("scratch dir");
    let missing_dir = root.path().join("no-such-dir");
    let report = missing_dir.join("report.txt");
    let log = fixture("scan-text.log");
    let report_path = report.to_str().expect("utf-8 path");
    let (code, stdout, stderr) = run(&["scan", "--output", report_path, &log]);
    let err = String::from_utf8_lossy(&stderr);
    assert_eq!(code, Some(3), "{err}");
    assert_ne!(code, Some(1), "a missing directory is not a threshold");
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(err.contains("could not write the report to "), "{err}");
    assert!(err.contains("does not create it"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
    assert!(!report.exists());
    assert!(
        !missing_dir.exists(),
        "a missing parent directory must not be created"
    );

    let prices = fixture("prices.json");
    let (budget_code, budget_out, budget_err) = run(&[
        "scan",
        "--output",
        report_path,
        "--window",
        "1m",
        "--prices",
        &prices,
        "--budget-month",
        "0",
        &log,
    ]);
    let budget_err = String::from_utf8_lossy(&budget_err);
    assert_eq!(
        budget_code,
        Some(3),
        "a missing directory wins over the budget: {budget_err}"
    );
    assert_ne!(budget_code, Some(1));
    assert!(budget_out.is_empty(), "{budget_out:?}");
    assert!(!missing_dir.exists());
}
