//! A `.gz` or `.zst` log has the same totals as that log unpacked.

use std::process::Command;

use serde_json::Value;

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_log-bill"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("start binary");
    (
        output.status.code().unwrap_or(101),
        String::from_utf8(output.stdout).expect("stdout"),
        String::from_utf8(output.stderr).expect("stderr"),
    )
}

fn scan_totals(path: &str) -> Value {
    let (code, out, err) = run(&["scan", "--json", path]);
    assert_eq!(code, 0, "{path}\n{err}");
    assert!(err.is_empty(), "{path}\n{err}");
    let value: Value = serde_json::from_str(&out).expect("json");
    value["totals"].clone()
}

#[test]
fn compressed_log_totals_match_the_unpacked_file() {
    let plain = "tests/fixtures/packed.log";
    let gz = "tests/fixtures/packed.log.gz";
    let zst = "tests/fixtures/packed.log.zst";
    let totals = scan_totals(plain);
    assert!(totals["lines"].as_u64().expect("lines") > 0);
    assert!(totals["bytes"].as_u64().expect("bytes") > 0);
    assert_eq!(scan_totals(gz), totals);
    assert_eq!(scan_totals(zst), totals);

    let (code, text, err) = run(&["scan", plain]);
    assert_eq!(code, 0, "{err}");
    let total_line = text
        .lines()
        .find(|line| line.starts_with("total:"))
        .expect("total line")
        .to_owned();
    for path in [gz, zst] {
        let (code, out, err) = run(&["scan", path]);
        assert_eq!(code, 0, "{path} {err}");
        let line = out
            .lines()
            .find(|line| line.starts_with("total:"))
            .expect("total line");
        assert_eq!(line, total_line, "{path}");
    }

    for packed in [gz, zst] {
        let (code, out, err) = run(&["diff", "--json", plain, packed]);
        assert_eq!(code, 0, "{packed}\n{err}");
        let value: Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["totals"]["old"], totals, "{packed}");
        assert_eq!(value["totals"]["new"], totals, "{packed}");
        assert_eq!(value["totals_delta"]["lines"], 0, "{packed}");
        assert_eq!(value["totals_delta"]["bytes"], 0, "{packed}");
    }
}

#[test]
fn a_broken_gz_or_zst_is_unreadable_and_hides_the_bytes() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("broken-compressed");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let gz = dir.join("sample.gz");
    let zst = dir.join("sample.zst");
    std::fs::write(&gz, b"not gzip\n").expect("gz");
    std::fs::write(&zst, b"not zstd\n").expect("zst");
    for path in [gz, zst] {
        let text = path.to_str().expect("utf-8");
        let (code, out, err) = run(&["scan", text]);
        assert_eq!(code, 2, "{text}\n{err}");
        assert!(out.is_empty(), "{text}\n{out}");
        assert!(err.contains("not readable"), "{err}");
        assert!(!err.contains("not gzip"), "{err}");
        assert!(!err.contains("not zstd"), "{err}");
        assert!(!err.contains(text), "{err}");
    }
}

#[test]
fn a_plain_name_is_not_unpacked() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("named-plain");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let bytes = std::fs::read("tests/fixtures/packed.log.gz").expect("gz");
    let path = dir.join("packed.log");
    std::fs::write(&path, bytes).expect("write");
    let text = path.to_str().expect("utf-8");
    let (code, _out, err) = run(&["scan", "--json", text]);
    assert_eq!(code, 0, "{err}");
    let packed = scan_totals("tests/fixtures/packed.log.gz");
    let as_text = scan_totals(text);
    assert_ne!(as_text, packed);
}
