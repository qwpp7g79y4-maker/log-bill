//! A line longer than 1 MiB must not be held in memory in full.
//! Text lines are cut at 1 MiB and still count every byte. JSON lines that
//! long are refused with exit 2. Regression for a 2 GB line using 6 GB of RAM.

use std::path::PathBuf;
use std::process::Command;

const MIB: usize = 1 << 20;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("log-bill-long-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temporary directory");
    TempDir(dir)
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(args)
        .output()
        .expect("start binary");
    (
        output.status.code().unwrap_or(101),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn totals(path: &str) -> (u64, u64, String) {
    let (code, out, err) = run(&["scan", "--json", path]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("json");
    (
        value["totals"]["bytes"].as_u64().expect("bytes"),
        value["totals"]["lines"].as_u64().expect("lines"),
        out,
    )
}

fn write(dir: &TempDir, name: &str, data: &[u8]) -> String {
    let path = dir.0.join(name);
    std::fs::write(&path, data).expect("write");
    path.to_str().expect("path").to_string()
}

/// Exact byte counts at and around the cut, with and without `\n`, with `\r\n`,
/// and a short line after a long one, which must stay its own line.
#[test]
fn byte_counts_around_the_cut_are_exact() {
    let dir = temp("counts");
    for len in [MIB - 1, MIB, MIB + 1, 3 * MIB + 7] {
        for (ending, extra) in [("\n", 1u64), ("\r\n", 2), ("", 1)] {
            let mut data = vec![b'a'; len];
            data.extend_from_slice(ending.as_bytes());
            data.extend_from_slice(b"disk full\n");
            let path = write(&dir, "c.log", &data);
            let (bytes, lines, out) = totals(&path);
            let first = len as u64 + extra;
            if ending.is_empty() {
                // Without a newline the short text joins the long line.
                assert_eq!(lines, 1, "len {len} no newline");
                assert_eq!(bytes, len as u64 + 10, "len {len} no newline");
            } else {
                assert_eq!(lines, 2, "len {len} {ending:?}");
                assert_eq!(bytes, first + 10, "len {len} {ending:?}");
                assert!(out.contains("disk full"), "len {len} {ending:?}");
            }
        }
    }
}

/// Secrets in a long line stay masked: at the start, just before the cut with
/// the value running over it, and with the key itself cut in half.
#[test]
fn secrets_in_a_long_line_stay_masked() {
    let dir = temp("secrets");
    let canary = "CANARYLONGLINEVALUE";
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("start", {
            let mut d = format!("password={canary} ").into_bytes();
            d.extend(vec![b'x'; 2 * MIB]);
            d
        }),
        ("value-over-cut", {
            let key = "password=";
            let mut d = vec![b' '; MIB - key.len() - 5];
            d.extend_from_slice(key.as_bytes());
            d.extend_from_slice(canary.as_bytes());
            d.extend(vec![b' '; MIB]);
            d
        }),
        ("key-over-cut", {
            let mut d = vec![b' '; MIB - 5];
            d.extend_from_slice(format!("password={canary} more").as_bytes());
            d
        }),
        ("after-cut", {
            let mut d = vec![b' '; MIB + 10];
            d.extend_from_slice(format!("token={canary}").as_bytes());
            d
        }),
    ];
    for (name, mut data) in cases {
        data.push(b'\n');
        let path = write(&dir, "s.log", &data);
        for mode in [&["scan"][..], &["scan", "--json"], &["scan", "--explain"]] {
            let mut args = mode.to_vec();
            args.push(&path);
            let (code, out, err) = run(&args);
            assert_eq!(code, 0, "{name} {mode:?} {err}");
            let piece = &canary[..8];
            assert!(!out.contains(piece), "{name} {mode:?} leaks {piece}");
            assert!(!out.contains(canary), "{name} {mode:?} leaks");
        }
    }
}

/// A line cut at 1 MiB keeps a `\r` that ends up last in the kept part: the
/// cut is not a line end, so that `\r` is content and must not be trimmed.
/// A `$` rule sees it only if it is kept.
#[test]
fn cut_line_keeps_its_trailing_cr() {
    let dir = temp("cut-cr");
    let rules = write(&dir, "cr.rules", b"drop ^sample .*\\r$\n");
    let mut data = b"sample ".to_vec();
    data.resize(MIB - 1, b' ');
    data.extend_from_slice(b"\rtail\n");
    let path = write(&dir, "cr.log", &data);
    let (code, out, err) = run(&["scan", "--rules", &rules, &path]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("-> 0 lines, 0 bytes"), "{out}");
}

/// A JSON line over 1 MiB is refused with exit 2, the line number and no path
/// or content; at exactly 1 MiB it is still read.
#[test]
fn json_line_over_the_cap_is_refused() {
    let dir = temp("json");
    let wrap = |n: usize| format!("{{\"message\":\"{}\"}}", "a".repeat(n));
    let fits = wrap(MIB - 14);
    assert_eq!(fits.len(), MIB);
    let ok = write(&dir, "ok.jsonl", format!("{fits}\n").as_bytes());
    let (code, _, err) = run(&["scan", "--format", "jsonl", &ok]);
    assert_eq!(code, 0, "{err}");
    for (name, data) in [
        ("over", format!("{{\"message\":\"x\"}}\n{}\n", wrap(MIB))),
        (
            "over-no-newline",
            format!("{{\"message\":\"x\"}}\n{}", wrap(2 * MIB)),
        ),
        (
            "over-crlf",
            format!("{{\"message\":\"x\"}}\r\n{}\r\n", wrap(MIB)),
        ),
    ] {
        let path = write(&dir, "big.jsonl", data.as_bytes());
        let (code, out, err) = run(&["scan", "--format", "jsonl", &path]);
        assert_eq!(code, 2, "{name}");
        assert!(out.is_empty(), "{name}");
        assert!(err.contains("line 2 is longer than 1 MiB"), "{name}: {err}");
        assert!(!err.contains("big.jsonl"), "{name}: path in {err}");
        assert!(!err.contains("aaaa"), "{name}: content in {err}");
    }
}

/// A 512 MiB line under a 256 MiB address-space limit. Before the cut the
/// whole line was held several times and this ran out of memory.
#[cfg(target_os = "linux")]
#[test]
fn huge_line_runs_in_little_memory() {
    let dir = temp("huge");
    let path = dir.0.join("huge.log");
    let file = std::fs::File::create(&path).expect("create");
    file.set_len(512 * MIB as u64).expect("sparse file");
    drop(file);
    let script = format!(
        "ulimit -v 262144 && exec '{}' scan --json '{}'",
        env!("CARGO_BIN_EXE_log-bill"),
        path.display()
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("sh");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(
        value["totals"]["bytes"].as_u64(),
        Some(512 * MIB as u64 + 1)
    );
}
