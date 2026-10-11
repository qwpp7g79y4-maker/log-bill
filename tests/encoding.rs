//! Input encodings. UTF-16 and UTF-32 put NUL bytes between the letters of a
//! key such as `password`, so masking would not recognise it. Such input is
//! refused with exit 2 before anything of it reaches the output. NUL bytes
//! inside otherwise UTF-8 text are removed before masking. A UTF-8 byte order
//! mark is skipped for JSON lines and price files and still counts in the bytes.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Synthetic canaries. None of them may appear in any output, not even after
/// the NUL bytes are removed from that output.
const CANARIES: [&str; 3] = ["CANARYPW7741", "CANARYTK7742", "CANARYAU7743"];

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-bill"))
}

fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn run(args: &[&std::ffi::OsStr]) -> (i32, Vec<u8>, Vec<u8>) {
    let output = bin().args(args).output().expect("start binary");
    (
        output.status.code().unwrap_or(101),
        output.stdout,
        output.stderr,
    )
}

fn secret_text() -> String {
    format!(
        "password={}\r\ntoken={}\r\nAuthorization: Bearer {}\r\n",
        CANARIES[0], CANARIES[1], CANARIES[2]
    )
}

fn secret_jsonl() -> String {
    format!(
        "{{\"message\":\"password={}\"}}\r\n{{\"message\":\"token={}\"}}\r\n{{\"message\":\"Authorization: Bearer {}\"}}\r\n",
        CANARIES[0], CANARIES[1], CANARIES[2]
    )
}

#[derive(Clone, Copy)]
enum Wide {
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl Wide {
    fn name(self) -> &'static str {
        match self {
            Wide::Utf16Le => "UTF-16LE",
            Wide::Utf16Be => "UTF-16BE",
            Wide::Utf32Le => "UTF-32LE",
            Wide::Utf32Be => "UTF-32BE",
        }
    }
}

fn encode(text: &str, wide: Wide, bom: bool) -> Vec<u8> {
    let mut chars: Vec<char> = Vec::new();
    if bom {
        chars.push('\u{FEFF}');
    }
    chars.extend(text.chars());
    let mut out = Vec::new();
    for c in chars {
        match wide {
            Wide::Utf16Le | Wide::Utf16Be => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    match wide {
                        Wide::Utf16Le => out.extend_from_slice(&unit.to_le_bytes()),
                        _ => out.extend_from_slice(&unit.to_be_bytes()),
                    }
                }
            }
            Wide::Utf32Le => out.extend_from_slice(&u32::from(c).to_le_bytes()),
            Wide::Utf32Be => out.extend_from_slice(&u32::from(c).to_be_bytes()),
        }
    }
    out
}

/// No canary in the bytes, also not once NUL bytes are removed and the rest
/// is read lossily as UTF-8.
fn assert_no_canary(label: &str, bytes: &[u8]) {
    let stripped: Vec<u8> = bytes.iter().copied().filter(|&b| b != 0).collect();
    let text = String::from_utf8_lossy(&stripped);
    for canary in CANARIES {
        assert!(!text.contains(canary), "{label}: {canary} leaked:\n{text}");
        // Also not as wide text in the raw output.
        for wide in [Wide::Utf16Le, Wide::Utf16Be, Wide::Utf32Le, Wide::Utf32Be] {
            let needle = encode(canary, wide, false);
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_slice()),
                "{label}: {canary} leaked as {}",
                wide.name()
            );
        }
    }
}

fn message(wide: Wide) -> String {
    let name = wide.name();
    format!(
        "file is {name}, not UTF-8; convert it first, for example with iconv -f {name} -t UTF-8, or in PowerShell with Out-File -Encoding utf8\n"
    )
}

#[test]
fn utf16_and_utf32_are_refused_without_leaking() {
    let prices = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prices.json");
    let plain = tmp("encoding-plain.log", b"hello world\n");
    for wide in [Wide::Utf16Le, Wide::Utf16Be, Wide::Utf32Le, Wide::Utf32Be] {
        for bom in [true, false] {
            for jsonl in [false, true] {
                let body = if jsonl { secret_jsonl() } else { secret_text() };
                let name = format!(
                    "encoding-{}-{}.{}",
                    wide.name(),
                    if bom { "bom" } else { "nobom" },
                    if jsonl { "jsonl" } else { "log" }
                );
                let file = tmp(&name, &encode(&body, wide, bom));
                let format: &[&str] = if jsonl { &["--format", "jsonl"] } else { &[] };
                let mut cases: Vec<Vec<std::ffi::OsString>> = Vec::new();
                let os = |items: &[&str]| -> Vec<std::ffi::OsString> {
                    items.iter().map(Into::into).collect()
                };
                for extra in [
                    &[][..],
                    &["--json"][..],
                    &["--explain"][..],
                    &["--json", "--explain"][..],
                ] {
                    let mut args = os(&["scan"]);
                    args.extend(os(format));
                    args.extend(os(extra));
                    args.push(file.clone().into());
                    cases.push(args);
                }
                let mut priced = os(&["scan", "--window", "1m", "--prices"]);
                priced.push(prices.clone().into());
                priced.extend(os(format));
                priced.push(file.clone().into());
                cases.push(priced);
                if !jsonl {
                    for (old, new) in [(&plain, &file), (&file, &plain)] {
                        let mut args = os(&["diff"]);
                        args.push(old.clone().into());
                        args.push(new.clone().into());
                        cases.push(args.clone());
                        args.insert(1, "--json".into());
                        cases.push(args);
                    }
                }
                for args in cases {
                    let refs: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_os_str()).collect();
                    let label = format!("{name} {args:?}");
                    let (code, out, err) = run(&refs);
                    assert_eq!(code, 2, "{label}");
                    assert!(out.is_empty(), "{label}: stdout not empty");
                    assert_eq!(String::from_utf8_lossy(&err), message(wide), "{label}");
                    assert_no_canary(&label, &out);
                    assert_no_canary(&label, &err);
                }
            }
        }
    }
}

#[test]
fn nul_bytes_inside_utf8_text_do_not_split_a_key() {
    // Too few NUL bytes for a wide encoding, but enough to split every key.
    let mut raw = Vec::new();
    raw.extend_from_slice(b"pa\0ss\0word=CANARYPW7741\n");
    raw.extend_from_slice(b"to\0ken=CANARYTK7742\n");
    raw.extend_from_slice(b"Author\0ization: Bea\0rer CANARYAU7743\n");
    // UTF-16LE after 4096 bytes of UTF-8 escapes the detection; NUL removal still masks it.
    raw.extend(std::iter::repeat_n(b"plain line\n".as_slice(), 400).flatten());
    raw.extend_from_slice(&encode("password=CANARYPW7741\n", Wide::Utf16Le, false));
    let file = tmp("encoding-nul-in-utf8.log", &raw);
    for extra in [&[][..], &["--json"][..], &["--explain"][..]] {
        let mut args: Vec<&std::ffi::OsStr> = vec!["scan".as_ref()];
        args.extend(extra.iter().map(|a| std::ffi::OsStr::new(*a)));
        args.push(file.as_os_str());
        let (code, out, err) = run(&args);
        assert_eq!(code, 0, "{extra:?}: {}", String::from_utf8_lossy(&err));
        assert_no_canary(&format!("{extra:?}"), &out);
        assert_no_canary(&format!("{extra:?}"), &err);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("<SECRET>"), "{text}");
    }
    let (_, out, _) = run(&["scan".as_ref(), file.as_os_str()]);
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("password=<SECRET>"), "{text}");
    assert!(text.contains("token=<SECRET>"), "{text}");
    assert!(text.contains("Authorization: <SECRET>"), "{text}");
    // Bytes are the raw bytes, NUL bytes included: 24 + 20 + 37 + 400 × 11 + 44,
    // plus 1 because the UTF-16LE `\n` leaves a last line of one NUL byte
    // without a newline. That line is the 405th.
    assert!(text.starts_with("total: 405 lines, 4526 bytes\n"), "{text}");
}

#[test]
fn escaped_nul_in_a_json_message_does_not_split_a_key() {
    let raw = b"{\"message\":\"pass\\u0000word=CANARYPW7741\"}\n";
    let file = tmp("encoding-nul.jsonl", raw);
    let (code, out, err) = run(&[
        "scan".as_ref(),
        "--format".as_ref(),
        "jsonl".as_ref(),
        file.as_os_str(),
    ]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_no_canary("jsonl", &out);
    assert!(String::from_utf8_lossy(&out).contains("password=<SECRET>"));
}

#[test]
fn zero_filled_start_is_not_a_wide_encoding() {
    // A truncated log can start with a run of NUL bytes on every position.
    let mut raw = vec![0u8; 2000];
    raw.extend_from_slice(b"hello\n");
    let file = tmp("encoding-zero-run.log", &raw);
    let (code, out, _) = run(&["scan".as_ref(), file.as_os_str()]);
    assert_eq!(code, 0);
    let text = String::from_utf8(out).expect("utf-8");
    assert!(text.starts_with("total: 1 line, 2006 bytes\n"), "{text}");
    assert!(text.contains("100.0%  hello\n"), "{text}");
}

#[test]
fn utf8_bom_in_jsonl_is_skipped_and_counted() {
    // BOM (3) + {"message":"hello"} (19) + \n = 23 bytes.
    let file = tmp(
        "encoding-bom.jsonl",
        b"\xEF\xBB\xBF{\"message\":\"hello\"}\n{\"message\":\"hello\"}\n",
    );
    let (code, out, err) = run(&[
        "scan".as_ref(),
        "--format".as_ref(),
        "jsonl".as_ref(),
        file.as_os_str(),
    ]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let text = String::from_utf8(out).expect("utf-8");
    assert!(text.starts_with("total: 2 lines, 43 bytes\n"), "{text}");
    assert!(
        text.contains("2 lines  43 bytes  100.0%  hello\n"),
        "{text}"
    );
}

#[test]
fn utf8_bom_in_price_file_is_skipped() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prices.json");
    let mut raw = b"\xEF\xBB\xBF".to_vec();
    raw.extend(std::fs::read(&fixture).expect("fixture"));
    let with_bom = tmp("encoding-bom-prices.json", &raw);
    let log = tmp("encoding-bom-prices.log", b"hello\n");
    let priced = |prices: &Path| {
        run(&[
            "scan".as_ref(),
            "--window".as_ref(),
            "1m".as_ref(),
            "--prices".as_ref(),
            prices.as_os_str(),
            log.as_os_str(),
        ])
    };
    let (code, out, err) = priced(&with_bom);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert_eq!(out, priced(&fixture).1);
}

#[cfg(unix)]
#[test]
fn non_utf8_file_name_is_read() {
    use std::os::unix::ffi::OsStrExt;
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let file = dir.join(std::ffi::OsStr::from_bytes(b"encoding-\xff.log"));
    std::fs::write(&file, b"hello\n").expect("write fixture");
    let (code, out, err) = run(&["scan".as_ref(), file.as_os_str()]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    assert!(String::from_utf8(out).expect("utf-8").contains("hello"));

    let (code, out, err) = run(&["scan".as_ref(), "--json".as_ref(), file.as_os_str()]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let body = String::from_utf8(out).expect("JSON is UTF-8");
    let value: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    let shown = value["inputs"]["paths"][0].as_str().expect("path");
    assert!(shown.ends_with("encoding-\u{FFFD}.log"), "{shown}");

    let (code, _, err) = run(&["diff".as_ref(), file.as_os_str(), file.as_os_str()]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
}
