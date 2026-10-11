use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
}

#[test]
fn help_shows_every_option() {
    let output = bin().arg("--help").output().expect("start binary");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help is utf-8");
    for needle in [
        "scan",
        "diff",
        "--format",
        "text",
        "jsonl",
        "--message-field",
        "--window",
        "--days-per-month",
        "--prices",
        "--rules",
        "--top",
        "--explain",
        "--json",
        "--budget-month",
        "--max-new-share",
    ] {
        assert!(help.contains(needle), "help is missing {needle}");
    }
}

#[test]
fn unknown_option_exit_3() {
    let output = bin().arg("--unknown").output().expect("start binary");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn invalid_window_exit_3() {
    for window in ["0m", "5x", ""] {
        let output = bin()
            .args(["scan", "--window", window, "missing-file.log"])
            .output()
            .expect("start binary");
        assert_eq!(output.status.code(), Some(3), "{window:?}");
        assert!(output.stdout.is_empty(), "{window:?}");
        let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
        assert_eq!(err, "invalid window\n", "{window:?}");
    }
}

#[test]
fn diff_with_one_file_exit_3() {
    let output = bin()
        .args(["diff", "alone"])
        .output()
        .expect("start binary");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn missing_file_exit_2() {
    let output = bin()
        .args(["scan", "missing-dir/log.txt"])
        .output()
        .expect("start binary");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(err, "file is not readable\n");
}

#[test]
fn broken_json_line_exit_2() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("broken-line.jsonl");
    std::fs::write(&path, "{\"message\":\"ok\"}\n\n{DO-NOT-SHOW-0001\n").expect("write fixture");
    let output = bin()
        .args(["scan", "--format", "jsonl"])
        .arg(&path)
        .output()
        .expect("start binary");
    let _ = std::fs::remove_file(&path);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(err, "invalid JSON on line 3\n");
    assert!(!err.contains("DO-NOT-SHOW"));
}

#[test]
fn invalid_price_file_exit_2_without_content() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("prices-broken.json");
    std::fs::write(&path, "{CANARY-PRICE-0001").expect("write fixture");
    let file_path = path.to_str().expect("path");
    // With --window the price file applies. Without a window, that is exit 3.
    for args in [
        vec![
            "scan",
            "--window",
            "1m",
            "--prices",
            file_path,
            "missing.log",
        ],
        vec![
            "diff", "--window", "1d", "--prices", file_path, "old.log", "new.log",
        ],
    ] {
        let output = bin().args(&args).output().expect("start binary");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
        assert_eq!(err, "price file is not JSON\n", "{args:?}");
        assert!(!err.contains("CANARY"), "{args:?}");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn large_window_and_gb_bytes_exit_3() {
    // 10^19 seconds × 10^20 gb_bytes does not fit in u128.
    // No silent 0 and no panic: exit 3 with an error, and no output.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let log = dir.join("overflow.log");
    let prices = dir.join("overflow.json");
    std::fs::write(&log, "ok\n").expect("write fixture");
    std::fs::write(
        &prices,
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":1e20}"#,
    )
    .expect("write fixture");
    let output = bin()
        .args([
            "scan",
            "--window",
            "10000000000000000000s",
            "--prices",
            prices.to_str().expect("path"),
            log.to_str().expect("path"),
        ])
        .output()
        .expect("start binary");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(err, "calculation overflow\n");
}

#[test]
fn price_file_without_window_leaves_scan_unchanged() {
    // --prices without --window stops before reading, also with --explain or --json.
    let message = "--prices only works together with --window\n";
    for args in [
        vec!["scan", "--prices", "missing-prices.json", "missing.log"],
        vec![
            "scan",
            "--explain",
            "--prices",
            "missing-prices.json",
            "missing.log",
        ],
        vec![
            "scan",
            "--json",
            "--prices",
            "missing-prices.json",
            "missing.log",
        ],
        vec![
            "scan",
            "--json",
            "--explain",
            "--prices",
            "missing-prices.json",
            "missing.log",
        ],
        vec!["diff", "--prices", "missing-prices.json", "OLD", "NEW"],
        vec![
            "diff",
            "--explain",
            "--prices",
            "missing-prices.json",
            "OLD",
            "NEW",
        ],
        vec![
            "diff",
            "--json",
            "--prices",
            "missing-prices.json",
            "OLD",
            "NEW",
        ],
    ] {
        let output = bin().args(&args).output().expect("start binary");
        assert_eq!(output.status.code(), Some(3), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
        assert_eq!(err, message, "{args:?}");
    }
}

#[test]
fn gb_bytes_fraction_exit_2() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let log = dir.join("gb-fraction.log");
    let prices = dir.join("gb-fraction.json");
    std::fs::write(&log, "ok\n").expect("log");
    std::fs::write(
        &prices,
        r#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":0.5}"#,
    )
    .expect("prices");
    let output = bin()
        .args([
            "scan",
            "--window",
            "1d",
            "--prices",
            prices.to_str().expect("path"),
            log.to_str().expect("path"),
        ])
        .output()
        .expect("start binary");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let err = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(err, "gb_bytes must be a whole number greater than 0\n");
}

/// Metadata before opening. A FIFO with no writer must not make the tool wait.
#[cfg(unix)]
#[test]
fn fifo_exit_2_within_two_seconds() {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn within(args: &[&str]) -> (i32, String, String) {
        let mut child = bin()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start");
        let pid = child.id();
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let _ = stdout.read_to_end(&mut out);
            let _ = stderr.read_to_end(&mut err);
            let code = child.wait().ok().and_then(|status| status.code());
            let _ = tx.send((code, out, err));
        });
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok((code, out, err)) => (
                code.expect("exit code"),
                String::from_utf8(out).expect("stdout"),
                String::from_utf8(err).expect("stderr"),
            ),
            Err(_) => {
                let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
                panic!("still running after 2 seconds");
            }
        }
    }

    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("fifo-cli");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("directory");
    let fifo = dir.join("CANARY-FIFO-0001");
    let log = dir.join("plain.log");
    std::fs::write(&log, "ok\n").expect("log");
    let created = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(created.success(), "mkfifo");
    let fifo_path = fifo.to_str().expect("path");
    let log_path = log.to_str().expect("path");

    let (code, output_2, err) = within(&["scan", fifo_path]);
    assert_eq!(code, 2, "{err}");
    assert!(output_2.is_empty());
    assert_eq!(err, "file is not a regular file\n");
    assert!(!err.contains("CANARY"));

    let (code, output_2, err) = within(&["diff", fifo_path, log_path]);
    assert_eq!(code, 2, "{err}");
    assert!(output_2.is_empty());
    assert_eq!(err, "file is not a regular file\n");

    let (code, output_2, err) =
        within(&["scan", "--window", "1d", "--prices", fifo_path, log_path]);
    assert_eq!(code, 2, "{err}");
    assert!(output_2.is_empty());
    assert_eq!(err, "price file is not a regular file\n");
    assert!(!err.contains("CANARY"));

    let (code, output_2, err) = within(&["scan", "--rules", fifo_path, log_path]);
    assert_eq!(code, 2, "{err}");
    assert!(output_2.is_empty());
    assert_eq!(err, "rules file is not a regular file\n");

    let map = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir_path = map.to_str().expect("path");
    let (code, output_2, err) = within(&["scan", dir_path]);
    assert_eq!(code, 2, "{err}");
    assert!(output_2.is_empty());
    assert_eq!(err, "file is not a regular file\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_names_the_binary_log_bill() {
    // Also on Windows, where the file is `log-bill.exe`.
    let output = bin().args(["scan"]).output().expect("start binary");
    assert_eq!(output.status.code(), Some(3));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("Usage: log-bill scan "), "{err}");
    assert!(!err.contains("log-bill.exe"), "{err}");
}
