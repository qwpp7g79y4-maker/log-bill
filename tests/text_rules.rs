use std::path::PathBuf;
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

fn write_line(name: &str, line: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("log-bill-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temporary directory");
    let path = dir.join("line.log");
    std::fs::write(&path, format!("{line}\n")).expect("line");
    path
}

#[test]
fn bidi_override_is_escaped_in_text_output() {
    let marks = [
        '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
        '\u{2068}', '\u{2069}', '\u{200E}', '\u{200F}', '\u{061C}',
    ];
    let mut line = String::from("disk ");
    for mark in marks {
        line.push(mark);
    }
    line.push_str("full");
    let path = write_line("bidi", &line);
    let arg = path.to_str().expect("path");
    for (mode, args) in [
        ("scan", vec!["scan", arg]),
        ("scan --explain", vec!["scan", "--explain", arg]),
        ("diff", vec!["diff", arg, arg]),
    ] {
        let (code, out, err) = run(&args);
        assert_eq!(code, 0, "{mode}\n{out}\n{err}");
        assert!(err.is_empty(), "{mode}: {err}");
        for mark in marks {
            let escaped = format!("\\u{:04x}", u32::from(mark));
            assert!(out.contains(&escaped), "{mode} missing {escaped}\n{out}");
            assert!(!out.contains(mark), "{mode} shows raw {escaped}\n{out}");
            assert!(!err.contains(mark), "{mode} stderr");
        }
    }
    let (code, out, err) = run(&["scan", "--json", arg]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(err.is_empty(), "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("json");
    let text = value["patterns"][0]["text"].as_str().expect("pattern text");
    assert!(text.contains('\u{202E}'), "{text}");
    assert!(!text.contains("\\u202e"), "{text}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn quoted_email_with_a_space_is_masked() {
    let path = write_line("quoted-email", r#"mail "a b"@example.com einde"#);
    let arg = path.to_str().expect("path");
    for args in [
        vec!["scan", arg],
        vec!["scan", "--explain", arg],
        vec!["scan", "--json", arg],
        vec!["diff", arg, arg],
    ] {
        let (code, out, err) = run(&args);
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(out.contains("<EMAIL>"), "{out}");
        assert!(!out.contains("a b"), "{out}");
        assert!(!out.contains("example.com"), "{out}");
        assert!(err.is_empty(), "{err}");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn number_with_trailing_mark_is_num() {
    let path = write_line("trailing-num", "code 7!");
    let arg = path.to_str().expect("path");
    let (code, out, err) = run(&["scan", arg]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("<NUM>"), "{out}");
    assert!(!out.contains("7!"), "{out}");
    assert!(err.is_empty(), "{err}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn later_duplicate_message_is_kept() {
    let path = write_line("dup-message", r#"{"message":"alpha","message":"beta"}"#);
    let arg = path.to_str().expect("path");
    let (code, out, err) = run(&["scan", "--format", "jsonl", arg]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("beta"), "{out}");
    assert!(!out.contains("alpha"), "{out}");
    assert!(err.is_empty(), "{err}");

    let between = write_line(
        "dup-between",
        r#"{"msg":"alpha","message":"beta","message":"gamma"}"#,
    );
    let (code, out, err) = run(&["scan", "--format", "jsonl", between.to_str().expect("path")]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("gamma"), "{out}");
    assert!(!out.contains("alpha"), "{out}");
    assert!(!out.contains("beta"), "{out}");

    let named = write_line("dup-named", r#"{"note":"alpha","note":"beta"}"#);
    let (code, out, err) = run(&[
        "scan",
        "--format",
        "jsonl",
        "--message-field",
        "note",
        named.to_str().expect("path"),
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("beta"), "{out}");
    assert!(!out.contains("alpha"), "{out}");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&between);
    let _ = std::fs::remove_file(&named);
}
