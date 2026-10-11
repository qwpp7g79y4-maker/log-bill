use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
}

fn scan(name: &str, bytes: &[u8]) -> (i32, String, String) {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    let output = bin()
        .args(["scan", path.to_str().expect("path")])
        .output()
        .expect("start binary");
    let _ = std::fs::remove_file(&path);
    (
        output.status.code().unwrap_or(101),
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn scan_invalid_utf8_and_multibyte() {
    let mut raw = Vec::new();
    raw.extend_from_slice(b"\xff\xfe foo\n");
    raw.extend_from_slice("\u{FFFD}password=abc\n".as_bytes());
    for key in [
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "bearer",
    ] {
        raw.extend(format!("😀{key}=abc\n").into_bytes());
    }

    let (code, out, err) = scan("utf8-invalid.log", &raw);
    assert_eq!(code, 0, "stderr: {err}");
    assert!(err.is_empty(), "{err}");
    assert!(!err.contains("panicked"));
    assert!(out.contains("total: 10 lines, "));
    assert!(out.contains("\u{FFFD}\u{FFFD} foo"), "{out}");
    assert!(out.contains("\u{FFFD}password=<SECRET>"), "{out}");
    for key in [
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "bearer",
    ] {
        assert!(out.contains(&format!("😀{key}=<SECRET>")), "{key}\n{out}");
    }
    assert!(!out.contains("=abc"), "{out}");
}
