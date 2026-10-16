#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;

const KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "authorization",
    "bearer",
];

/// Separators `mask::mask` itself treats as introducing a value: plain,
/// spaced, and Unicode whitespace around `=` or `:` (tab, LF, CR, NBSP,
/// ideographic space, NEL). Fullwidth `＝`/`：` are folded to ASCII by
/// `read::without_nul` before a line ever reaches the masker, so they are
/// not this function's contract and are not included here.
const SEPS: &[&str] = &[
    "=",
    ":",
    " = ",
    " : ",
    "\t=",
    "\n=",
    "\r:",
    "\u{00A0}=\u{00A0}",
    "\u{3000}=",
    "\u{0085}=",
];

/// Mask tags, so a secret that happens to equal one is not mistaken for a leak.
const TAGS: &[&str] = &["TS", "UUID", "IP", "EMAIL", "SECRET", "HEX", "NUM", "VAR"];

#[derive(Debug, Arbitrary)]
struct Case {
    key_index: u8,
    sep_index: u8,
    escape_key: bool,
    secret: Vec<u8>,
}

// Masking: no input may panic, and a secret value placed after a key and a
// separator must never appear in the masked output.
fuzz_target!(|data: &[u8]| {
    let lossy = String::from_utf8_lossy(data);
    let _ = log_bill::mask::mask(&lossy);

    let Ok(case) = Case::arbitrary_take_rest(Unstructured::new(data)) else {
        return;
    };
    let secret: String = case
        .secret
        .iter()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .take(40)
        .map(|&byte| byte as char)
        .collect();
    if secret.len() < 6 {
        return;
    }
    let upper = secret.to_ascii_uppercase();
    if TAGS.contains(&upper.as_str()) {
        return;
    }

    let mut key = KEYS[case.key_index as usize % KEYS.len()].to_owned();
    if case.escape_key {
        if let Some(first) = key.chars().next() {
            let escaped = format!("\\u{:04x}", first as u32);
            key = format!("{escaped}{}", &key[first.len_utf8()..]);
        }
    }
    let sep = SEPS[case.sep_index as usize % SEPS.len()];
    let line = format!("{key}{sep}{secret}");
    let masked = log_bill::mask::mask(&line);
    assert!(
        !masked.contains(&secret),
        "secret leaked: input={line:?} output={masked:?}"
    );
});
