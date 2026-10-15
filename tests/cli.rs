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
        "--retention",
        "--billing",
        "--indexed-price",
        "--index-rules",
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
    for window in ["0m", "5x", "", "1é"] {
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

/// `scan m2.log > /dev/full`, and the same for `--version` and `--help`.
/// A full disk must be exit 3 with a message, not a panic (101) and not 0.
#[cfg(target_os = "linux")]
#[test]
fn full_stdout_is_exit_3() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let log = dir.join("m2.log");
    std::fs::write(&log, "disk full\n").expect("write fixture");
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("/dev/full");
    let cases = [
        vec!["scan".to_string(), log.display().to_string()],
        vec!["--version".to_string()],
        vec!["--help".to_string()],
    ];
    for args in cases {
        let output = bin()
            .args(&args)
            .stdout(full.try_clone().expect("clone /dev/full"))
            .output()
            .expect("start binary");
        let err = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(3), "{args:?} stderr: {err}");
        assert_eq!(err, "could not write output\n", "{args:?}");
        assert!(!err.contains("panicked"), "{args:?}");
    }
    let _ = std::fs::remove_file(&log);
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

/// A price or `gb_bytes` outside the f64 range is a price error, not invalid JSON.
/// `1` and 309 zeros, `1e400`, `-1e400` and `1E+400` are the reported inputs.
#[test]
fn price_number_outside_f64_exit_2() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let log = dir.join("huge-price.log");
    let prices = dir.join("huge-price.json");
    std::fs::write(&log, "disk full\n").expect("log");
    let log_path = log.to_str().expect("path");
    let prices_path = prices.to_str().expect("path");
    let huge = format!("1{}", "0".repeat(309));
    let negative_huge = format!("-{huge}");
    let not_number = "price file has a price that is not a number\n";
    let not_json = "price file is not JSON\n";
    let gb_bytes = "gb_bytes must be a whole number greater than 0\n";
    // `1e-400` still fits in f64 (it becomes zero) and does not fit the
    // accumulator, so every field says the price is not a number.
    let numbers = [
        huge.as_str(),
        negative_huge.as_str(),
        "1e400",
        "-1e400",
        "1E+400",
        "-1E+400",
        "1e-400",
        "-1e-400",
    ];
    let tiny = ["1e-400", "-1e-400"];
    for number in numbers {
        let message = |field: &str| {
            if field == "gb_bytes" && !tiny.contains(&number) {
                gb_bytes
            } else {
                not_number
            }
        };
        for field in ["ingest_per_gb", "index_per_gb", "gb_bytes"] {
            for ws in ["", "\n", "\t", "\n\t"] {
                for number_first in [false, true] {
                    let body = price_object(field, number, ws, number_first);
                    std::fs::write(&prices, &body).expect("prices");
                    for args in [
                        vec!["scan", "--window", "1d", "--prices", prices_path, log_path],
                        vec![
                            "diff",
                            "--window",
                            "1d",
                            "--prices",
                            prices_path,
                            log_path,
                            log_path,
                        ],
                    ] {
                        let output = bin().args(&args).output().expect("start binary");
                        let err = String::from_utf8(output.stderr).expect("stderr");
                        assert_eq!(
                            output.status.code(),
                            Some(2),
                            "{args:?} {body:?} stderr: {err}"
                        );
                        assert!(output.stdout.is_empty(), "{args:?} {body:?}");
                        assert_eq!(err, message(field), "{args:?} {body:?}");
                        assert!(!err.contains("not JSON"), "{args:?} {body:?}");
                    }
                }
            }
        }
    }
    // A real JSON error stays "not JSON", even when the text also has `1e400`.
    for broken in [
        "{",
        "{\"currency\":\"EUR\",\"source\":\"s\",\"date\":\"2026-10-08\",\"ingest_per_gb\":1e400",
        "{\"currency\":\"EUR\",\"source\":\"s\",\"date\":\"2026-10-08\",\"ingest_per_gb\":1e400,}",
    ] {
        std::fs::write(&prices, broken).expect("prices");
        let output = bin()
            .args(["scan", "--window", "1d", "--prices", prices_path, log_path])
            .output()
            .expect("start binary");
        let err = String::from_utf8(output.stderr).expect("stderr");
        assert_eq!(output.status.code(), Some(2), "{broken:?} {err}");
        assert_eq!(err, not_json, "{broken:?}");
    }
    // A repeated key is still rejected when one value is out of range.
    std::fs::write(
        &prices,
        r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1e400,"ingest_per_gb":1}"#,
    )
    .expect("prices");
    let output = bin()
        .args(["scan", "--window", "1d", "--prices", prices_path, log_path])
        .output()
        .expect("start binary");
    let err = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(err, "price file has a duplicate key\n");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(&prices);
}

fn price_object(field: &str, number: &str, ws: &str, number_first: bool) -> String {
    let numeric = format!(r#""{field}":{ws}{number}{ws}"#);
    let rest = if field == "ingest_per_gb" {
        r#""currency":"EUR","source":"s","date":"2026-10-08""#
    } else {
        r#""currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1"#
    };
    if number_first {
        format!("{{{numeric},{rest}}}")
    } else {
        format!("{{{rest},{numeric}}}")
    }
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

    // A symlink to the FIFO. Opening must not wait for a writer.
    let link = dir.join("link-to-fifo");
    std::os::unix::fs::symlink("CANARY-FIFO-0001", &link).expect("symlink");
    let link_path = link.to_str().expect("path");
    let (code, output_2, err) = within(&["scan", link_path]);
    assert_eq!(code, 2, "{err}");
    assert!(output_2.is_empty());
    assert_eq!(err, "file is not a regular file\n");
    assert!(!err.contains("CANARY"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sample_fraction_longer_than_80_characters_exits_2() {
    // `0.` plus 79 zeros is 81 characters. Shorter, the same zeros are a valid
    // fraction of 0 and the scan exits 0.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("long-fraction");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("directory");
    let log = dir.join("x.log");
    let rules = dir.join("rules.txt");
    std::fs::write(&log, "x\n").expect("log");
    let mut fraction = String::from("0.");
    fraction.extend(std::iter::repeat_n('0', 79));
    assert_eq!(fraction.len(), 81);
    std::fs::write(&rules, format!("sample {fraction} ^x$\n")).expect("rules");
    let output = bin()
        .args([
            "scan",
            "--rules",
            rules.to_str().expect("rules path"),
            log.to_str().expect("log path"),
        ])
        .output()
        .expect("start binary");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let err = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(err, "invalid rule on line 1 of the rules file\n");
    assert!(!err.contains(&fraction));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ev_brceg_and_ev_czbab_get_different_ids() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("id-collision.log");
    std::fs::write(&path, "ev brceg\nev czbab\n").expect("log");
    let output = bin().arg("scan").arg(&path).output().expect("start binary");
    assert_eq!(output.status.code(), Some(0));
    let out = String::from_utf8(output.stdout).expect("stdout");
    let id = |text: &str| {
        out.lines()
            .find(|line| line.contains(text))
            .expect("pattern line")
            .split_whitespace()
            .next()
            .expect("id")
            .to_owned()
    };
    let brceg = id("ev brceg");
    let czbab = id("ev czbab");
    assert_ne!(brceg, czbab);
    assert_eq!(brceg, "P6b31a22abd1e8f47");
    assert_eq!(czbab, "P6b31a22aba8b4370");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn extreme_json_fields_count_both_lines() {
    // `1e400` does not fit in a binary float. The second line nests 200 arrays.
    let mut deep = String::from("{\"msg\":\"ok\",\"x\":");
    deep.push_str(&"[".repeat(200));
    deep.push('1');
    deep.push_str(&"]".repeat(200));
    deep.push('}');
    let body = format!("{{\"msg\":\"ok\",\"x\":1e400}}\n{deep}\n");
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("extreme.jsonl");
    std::fs::write(&path, &body).expect("log");
    let output = bin()
        .args(["scan", "--format", "jsonl", path.to_str().expect("path")])
        .output()
        .expect("start binary");
    let err = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(output.status.code(), Some(0), "{err}");
    let out = String::from_utf8(output.stdout).expect("stdout");
    assert!(out.contains("total: 2 lines,"), "{out}");
    assert!(out.contains(" 2 lines "), "{out}");
    assert!(out.contains(" ok"), "{out}");
    assert!(!out.contains("1e400"), "{out}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn ci_runs_release_tests() {
    let yml = include_str!("../.github/workflows/ci.yml");
    assert!(
        yml.contains("cargo test --release"),
        "ci.yml does not run release tests"
    );
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
