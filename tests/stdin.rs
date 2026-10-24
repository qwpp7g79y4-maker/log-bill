//! The input path `-` reads standard input instead of a file. For the same
//! bytes, the output and exit code must be identical to passing a real file
//! path, for `scan`, `diff`, and the error paths (a FIFO is still refused;
//! stdin is not).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
}

fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_with_file(args: &[&str], file: &std::ffi::OsStr) -> Run {
    let output = bin().args(args).arg(file).output().expect("start binary");
    Run {
        code: output.status.code().unwrap_or(101),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

/// Runs with `-` in place of the given positional slot, piping `input` on stdin.
fn run_with_stdin(args: &[&str], input: &[u8]) -> Run {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start binary");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    Run {
        code: output.status.code().unwrap_or(101),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn assert_same(file: &Run, stdin: &Run, label: &str) {
    assert_eq!(file.code, stdin.code, "{label}: exit code differs");
    assert_eq!(file.stdout, stdin.stdout, "{label}: stdout differs");
    assert_eq!(file.stderr, stdin.stderr, "{label}: stderr differs");
}

#[test]
fn scan_text_from_stdin_matches_file() {
    let data = b"user 1 failed at 12:00:00 from 1.2.3.4\ndisk full\n";
    let file = tmp("stdin-scan.log", data);
    let via_file = run_with_file(&["scan"], file.as_os_str());
    let via_stdin = run_with_stdin(&["scan", "-"], data);
    assert!(String::from_utf8_lossy(&via_stdin.stdout).contains("disk full"));
    assert_same(&via_file, &via_stdin, "scan text");
}

#[test]
fn scan_jsonl_from_stdin_matches_file() {
    let data = b"{\"message\":\"disk full\"}\n{\"message\":\"disk full\"}\n";
    let file = tmp("stdin-scan.jsonl", data);
    let via_file = run_with_file(&["scan", "--format", "jsonl"], file.as_os_str());
    let via_stdin = run_with_stdin(&["scan", "--format", "jsonl", "-"], data);
    assert_same(&via_file, &via_stdin, "scan jsonl");
}

#[test]
fn scan_with_window_and_prices_from_stdin_matches_file() {
    let prices = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prices.json");
    let data = b"user 1 failed at 12:00:00 from 1.2.3.4\ndisk full\n";
    let file = tmp("stdin-scan-priced.log", data);
    let args = [
        "scan",
        "--window",
        "1m",
        "--prices",
        prices.to_str().unwrap(),
    ];
    let via_file = run_with_file(&args, file.as_os_str());
    let mut stdin_args: Vec<&str> = args.to_vec();
    stdin_args.push("-");
    let via_stdin = run_with_stdin(&stdin_args, data);
    assert_same(&via_file, &via_stdin, "scan with window and prices");
    assert!(String::from_utf8_lossy(&via_stdin.stdout).contains("EUR"));
}

fn run_diff_file(old: &Path, new: &Path) -> Run {
    let output = bin()
        .args(["diff"])
        .arg(old)
        .arg(new)
        .output()
        .expect("start binary");
    Run {
        code: output.status.code().unwrap_or(101),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

/// Runs `diff ARG1 ARG2` where one of them is `-`, piping `input` on stdin.
fn run_diff_stdin(args: [&std::ffi::OsStr; 2], input: &[u8]) -> Run {
    let mut child = bin()
        .arg("diff")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start binary");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    Run {
        code: output.status.code().unwrap_or(101),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

#[test]
fn diff_old_from_stdin_matches_file() {
    let old_data = b"alpha\nalpha\nbeta\n";
    let new_data = b"alpha\ngamma\n";
    let old = tmp("stdin-diff-old.log", old_data);
    let new = tmp("stdin-diff-new.log", new_data);
    let via_file = run_diff_file(&old, &new);
    let via_stdin = run_diff_stdin(["-".as_ref(), new.as_os_str()], old_data);
    assert_same(&via_file, &via_stdin, "diff old from stdin");
}

#[test]
fn diff_new_from_stdin_matches_file() {
    let old_data = b"alpha\nalpha\nbeta\n";
    let new_data = b"alpha\ngamma\n";
    let old = tmp("stdin-diff-old-2.log", old_data);
    let new = tmp("stdin-diff-new-2.log", new_data);
    let via_file = run_diff_file(&old, &new);
    let via_stdin = run_diff_stdin([old.as_os_str(), "-".as_ref()], new_data);
    assert_same(&via_file, &via_stdin, "diff new from stdin");
}

#[test]
fn invalid_json_from_stdin_matches_file_exit_2() {
    let data = b"{\"message\":\"ok\"}\n\n{DO-NOT-SHOW-0002\n";
    let file = tmp("stdin-broken.jsonl", data);
    let via_file = run_with_file(&["scan", "--format", "jsonl"], file.as_os_str());
    let via_stdin = run_with_stdin(&["scan", "--format", "jsonl", "-"], data);
    assert_same(&via_file, &via_stdin, "invalid json from stdin");
    assert_eq!(via_stdin.code, 2);
    assert!(via_stdin.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&via_stdin.stderr).contains("DO-NOT-SHOW"));
}

#[test]
fn wide_encoding_from_stdin_matches_file_exit_2() {
    let mut data = vec![0xFF, 0xFE]; // UTF-16LE BOM
    for ch in "hello".encode_utf16() {
        data.extend_from_slice(&ch.to_le_bytes());
    }
    let file = tmp("stdin-wide.log", &data);
    let via_file = run_with_file(&["scan"], file.as_os_str());
    let via_stdin = run_with_stdin(&["scan", "-"], &data);
    assert_same(&via_file, &via_stdin, "wide encoding from stdin");
    assert_eq!(via_stdin.code, 2);
    assert!(String::from_utf8_lossy(&via_stdin.stderr).contains("UTF-16LE"));
}

#[test]
fn empty_stdin_matches_empty_file() {
    let file = tmp("stdin-empty.log", b"");
    let via_file = run_with_file(&["scan"], file.as_os_str());
    let via_stdin = run_with_stdin(&["scan", "-"], b"");
    assert_same(&via_file, &via_stdin, "empty input");
    assert_eq!(
        String::from_utf8(via_stdin.stdout).unwrap(),
        "total: 0 lines, 0 bytes\n"
    );
}

/// A literal file named `-` is not reachable through the CLI once `-` means
/// stdin; a FIFO (an unrelated non-regular-file path) still gets the
/// existing "not a regular file" rejection, unaffected by the stdin feature.
#[cfg(unix)]
#[test]
fn fifo_is_still_rejected_unlike_stdin() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stdin-fifo-dir");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let fifo = dir.join("fifo");
    let status = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(status.success());
    let output = bin()
        .args(["scan"])
        .arg(&fifo)
        .stdin(Stdio::null())
        .output()
        .expect("start binary");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "file is not a regular file\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
