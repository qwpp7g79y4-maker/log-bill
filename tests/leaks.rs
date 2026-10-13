use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// Letter-only canary, shorter than 32, so `is_var` does not hide a leak.
const CANARY_LEN: usize = 6;

#[derive(Clone, Copy)]
enum Quoting {
    Unquoted,
    Single,
    Double,
    Escape,
}

#[derive(Clone, Copy)]
enum ValueKind {
    Letters,
    Spaces,
    Base64,
    Digits,
    /// Same combination, with digits and base64 (`=`, `+`, `/`) in the value.
    DigitsBase64,
    /// Unquoted `4242` stuck to the canary, so `<VAR>` does not hide a leak.
    DigitBefore,
    /// Unquoted canary with `4242` right after it.
    DigitAfter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Alone,
    Fields,
    JsonCompact,
    JsonSpaces,
    Jsonl,
}

/// Unescaped quote in the value, with the canary right after it.
/// Unquoted is the old dimension. The other three belong only with an
/// outer quote of the same character, otherwise the old code does not close too early.
#[derive(Clone, Copy)]
enum InnerQuote {
    Unquoted,
    OneApostrophe,
    MoreApostrophes,
    DoubleQuote,
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn keys_from_source() -> Vec<&'static str> {
    let source = include_str!("../src/mask.rs");
    let start = source
        .find("const KEYS: &[&str] = &[")
        .expect("KEYS in src/mask.rs");
    let rest = &source[start..];
    let end = rest.find("];").expect("end of KEYS");
    rest[..end]
        .split('"')
        .enumerate()
        .filter(|(i, part)| i % 2 == 1 && !part.is_empty())
        .map(|(_, part)| part)
        .collect()
}

fn mixed_case(form: &str) -> String {
    let mut stdout_text = String::new();
    let mut new_sample = true;
    for ch in form.chars() {
        if ch == '_' || ch == '-' {
            stdout_text.push(ch);
            new_sample = true;
        } else if new_sample {
            stdout_text.push(ch.to_ascii_uppercase());
            new_sample = false;
        } else {
            stdout_text.push(ch.to_ascii_lowercase());
        }
    }
    stdout_text
}

fn add_new(stdout_text: &mut Vec<String>, seen: &mut BTreeSet<String>, s: String) {
    if !seen.contains(&s) {
        seen.insert(s.clone());
        stdout_text.push(s);
    }
}

fn keys() -> Vec<String> {
    let mut stdout_text = Vec::new();
    let mut seen = BTreeSet::new();
    for key in keys_from_source() {
        for form in [
            key.to_string(),
            format!("_{key}"),
            format!("-{key}"),
            format!("{key}_"),
            format!("{key}-"),
        ] {
            add_new(&mut stdout_text, &mut seen, form.to_ascii_lowercase());
            add_new(&mut stdout_text, &mut seen, form.to_ascii_uppercase());
            add_new(&mut stdout_text, &mut seen, mixed_case(&form));
        }
    }
    for extra in [
        "Authorization",
        "Proxy-Authorization",
        "PROXY-AUTHORIZATION",
        "proxy-authorization",
    ] {
        add_new(&mut stdout_text, &mut seen, extra.to_string());
    }
    assert!(stdout_text
        .iter()
        .all(|s| !s.to_ascii_lowercase().contains("qx")));
    stdout_text
}

fn is_auth(key_name: &str) -> bool {
    key_name.to_ascii_lowercase().contains("authorization")
}

fn canary(index: usize) -> String {
    let mut char_buf = [b'a'; 4];
    let mut x = index;
    for slot in (0..4).rev() {
        char_buf[slot] = b'a' + (x % 26) as u8;
        x /= 26;
    }
    assert_eq!(x, 0, "too many combinations for the canary");
    format!(
        "qx{}{}{}{}",
        char_buf[0] as char, char_buf[1] as char, char_buf[2] as char, char_buf[3] as char
    )
}

fn index_of(canary: &str) -> usize {
    let mut n = 0usize;
    for &byte in &canary.as_bytes()[2..] {
        n = n * 26 + (byte - b'a') as usize;
    }
    n
}

/// `None` when the canary would not be inside the secret if the code is correct.
/// An unquoted value ends at the first space, except for authorization.
/// Spaces and a loose digit word belong there only if they fall inside the secret.
/// `4242` stuck to the canary is one token, even without a quote.
fn value_body(value: ValueKind, quote: Quoting, auth: bool, canary: &str) -> Option<String> {
    let spaced = !matches!(quote, Quoting::Unquoted) || auth;
    let unquoted = matches!(quote, Quoting::Unquoted);
    Some(match value {
        ValueKind::Letters => canary.to_string(),
        ValueKind::Spaces if spaced => format!("lead {canary}"),
        ValueKind::Spaces => return None,
        ValueKind::Base64 if spaced => format!("ab+/zz== {canary}"),
        ValueKind::Base64 => format!("ab+/zz=={canary}"),
        ValueKind::Digits if spaced => format!("4242 {canary}"),
        ValueKind::Digits => return None,
        ValueKind::DigitsBase64 if spaced => format!("ab+/zz== 4242 {canary}"),
        ValueKind::DigitsBase64 => return None,
        ValueKind::DigitBefore if unquoted => format!("4242{canary}"),
        ValueKind::DigitAfter if unquoted => format!("{canary}4242"),
        ValueKind::DigitBefore | ValueKind::DigitAfter => return None,
    })
}

/// Quotes around the value. If the separator already ends in `"`,
/// that is the opening quote (`":"` and `": "`). A second `"` would
/// close the value at once and leave the canary outside.
fn wrap(quote: Quoting, value_body: &str, sep: &str) -> String {
    let open = sep.ends_with('"');
    match quote {
        Quoting::Unquoted => value_body.to_string(),
        Quoting::Single => format!("'{value_body}'"),
        Quoting::Double if open => format!("{value_body}\""),
        Quoting::Double => format!("\"{value_body}\""),
        // The canary is after the escaped quote. Stopping there leaks it.
        Quoting::Escape if open => format!("ab\\\" {value_body}\""),
        Quoting::Escape => format!("\"ab\\\" {value_body}\""),
    }
}

fn json_string(s: &str) -> String {
    let mut stdout_text = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => stdout_text.push_str("\\\""),
            '\\' => stdout_text.push_str("\\\\"),
            '\t' => stdout_text.push_str("\\t"),
            _ => stdout_text.push(ch),
        }
    }
    stdout_text.push('"');
    stdout_text
}

fn log_line(context: Context, core: &str) -> String {
    match context {
        Context::Alone => core.to_string(),
        Context::Fields => format!("src=zichtbaar {core} dst=zichtbaar"),
        // Short fields and spaces: otherwise the line is one word and `<VAR>`
        // eats the pattern, including `<SECRET>`.
        Context::JsonCompact => format!("{{\"a\":\"b\", {core} }}"),
        Context::JsonSpaces => {
            format!("{{ \"src\" : \"ada\" , {core} , \"dst\" : \"zichtbaar\" }}")
        }
        Context::Jsonl => format!("{{\"message\":\"ok\",\"note\":{}}}", json_string(core)),
    }
}

fn quote_name(quote: Quoting) -> &'static str {
    match quote {
        Quoting::Unquoted => "none",
        Quoting::Single => "single",
        Quoting::Double => "double",
        Quoting::Escape => "escape",
    }
}

fn value_name(value: ValueKind) -> &'static str {
    match value {
        ValueKind::Letters => "letters",
        ValueKind::Spaces => "spaces",
        ValueKind::Base64 => "base64",
        ValueKind::Digits => "digits",
        ValueKind::DigitsBase64 => "digits-base64",
        ValueKind::DigitBefore => "digit-before",
        ValueKind::DigitAfter => "digit-after",
    }
}

fn inner_name(inner: InnerQuote) -> &'static str {
    match inner {
        InnerQuote::Unquoted => "none",
        InnerQuote::OneApostrophe => "one-apostrophe",
        InnerQuote::MoreApostrophes => "more-apostrophes",
        InnerQuote::DoubleQuote => "double-quote",
    }
}

fn inner_fits(inner: InnerQuote, quote: Quoting) -> bool {
    match inner {
        InnerQuote::Unquoted => true,
        InnerQuote::OneApostrophe | InnerQuote::MoreApostrophes => matches!(quote, Quoting::Single),
        InnerQuote::DoubleQuote => matches!(quote, Quoting::Double),
    }
}

/// The quote is before the canary, one space apart. That space is not a
/// field (`name=` or `name:`), so the new code does not close there. The old code
/// does, and then the canary is a short loose word instead of a tail that
/// `<VAR>` can eat.
fn with_inner(body: &str, canary: &str, inner: InnerQuote) -> String {
    if matches!(inner, InnerQuote::Unquoted) {
        return body.to_string();
    }
    assert!(body.ends_with(canary), "canary is not the tail: {body}");
    let prefix = &body[..body.len() - canary.len()];
    match inner {
        InnerQuote::Unquoted => body.to_string(),
        InnerQuote::OneApostrophe => format!("{prefix}' {canary}"),
        InnerQuote::MoreApostrophes => format!("{prefix}'' {canary}"),
        InnerQuote::DoubleQuote => format!("{prefix}\" {canary}"),
    }
}

fn context_name(context: Context) -> &'static str {
    match context {
        Context::Alone => "alone",
        Context::Fields => "fields",
        Context::JsonCompact => "json-compact",
        Context::JsonSpaces => "json-spaces",
        Context::Jsonl => "jsonl",
    }
}

fn run_tool(args: &[&str]) -> (i32, String, String) {
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

fn leaked(stdout_text: &str, labels: &[String]) -> Vec<String> {
    if !stdout_text.contains("qx") {
        return Vec::new();
    }
    let bytes = stdout_text.as_bytes();
    let mut seen = BTreeSet::new();
    let mut i = 0;
    while i + CANARY_LEN <= bytes.len() {
        if bytes[i] == b'q'
            && bytes[i + 1] == b'x'
            && bytes[i + 2..i + CANARY_LEN]
                .iter()
                .all(|b| b.is_ascii_lowercase())
        {
            let piece = &stdout_text[i..i + CANARY_LEN];
            let index = index_of(piece);
            if let Some(label) = labels.get(index) {
                seen.insert(label.clone());
            } else {
                seen.insert(format!("unknown {piece}"));
            }
            i += CANARY_LEN;
        } else {
            i += 1;
        }
    }
    seen.into_iter().collect()
}

fn class_of(label: &str) -> String {
    let mut parts = label.split('\t');
    let _key = parts.next();
    let _sep = parts.next();
    let quote = parts.next().unwrap_or("?");
    let value = parts.next().unwrap_or("?");
    let context = parts.next().unwrap_or("?");
    let inner = parts.next().unwrap_or("none");
    format!("{quote}/{value}/{context}/{inner}")
}

fn context_of(label: &str) -> String {
    label.split('\t').nth(4).unwrap_or("?").to_string()
}

/// A space separator belongs only with the unquoted digit forms.
fn matches_separator(sep_name: &str, quote: Quoting, value: ValueKind) -> bool {
    if sep_name != "space" {
        return true;
    }
    matches!(quote, Quoting::Unquoted)
        && matches!(value, ValueKind::DigitBefore | ValueKind::DigitAfter)
}

struct Run {
    context: &'static str,
    mode: &'static str,
    code: i32,
    stdout_text: String,
    err: String,
}

fn run_modes(context: &'static str, jsonl: bool, file_path: &str) -> Vec<Run> {
    let modes: Vec<(&str, Vec<&str>)> = if jsonl {
        vec![
            (
                "text",
                vec![
                    "scan",
                    "--format",
                    "jsonl",
                    "--message-field",
                    "note",
                    file_path,
                ],
            ),
            (
                "--explain",
                vec![
                    "scan",
                    "--format",
                    "jsonl",
                    "--message-field",
                    "note",
                    "--explain",
                    file_path,
                ],
            ),
            (
                "--json",
                vec![
                    "scan",
                    "--format",
                    "jsonl",
                    "--message-field",
                    "note",
                    "--json",
                    file_path,
                ],
            ),
            (
                "diff",
                vec![
                    "diff",
                    "--format",
                    "jsonl",
                    "--message-field",
                    "note",
                    file_path,
                    file_path,
                ],
            ),
            (
                "diff --json",
                vec![
                    "diff",
                    "--format",
                    "jsonl",
                    "--message-field",
                    "note",
                    "--json",
                    file_path,
                    file_path,
                ],
            ),
        ]
    } else {
        vec![
            ("text", vec!["scan", file_path]),
            ("--explain", vec!["scan", "--explain", file_path]),
            ("--json", vec!["scan", "--json", file_path]),
            ("diff", vec!["diff", file_path, file_path]),
            ("diff --json", vec!["diff", "--json", file_path, file_path]),
        ]
    };
    let mut stdout_text = Vec::new();
    for (mode, args) in modes {
        let start = Instant::now();
        let (code, stdout, stderr) = run_tool(&args);
        eprintln!(
            "{context} {mode}: exit {code}, {} ms, stdout {} bytes, stderr {} bytes",
            start.elapsed().as_millis(),
            stdout.len(),
            stderr.len()
        );
        stdout_text.push(Run {
            context,
            mode,
            code,
            stdout_text: stdout,
            err: stderr,
        });
    }
    stdout_text
}

/// Lines whose pattern shows `<SECRET>`, and the total from the text output.
fn count_secret(stdout_text: &str) -> Result<(usize, usize), String> {
    let mut expected = None;
    for log_line in stdout_text.lines() {
        if let Some(rest) = log_line.strip_prefix("total: ") {
            let n = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| "total line without a count".to_string())?
                .parse::<usize>()
                .map_err(|_| format!("total line unreadable: {log_line}"))?;
            expected = Some(n);
        }
    }
    let expected = expected.ok_or_else(|| "no total line".to_string())?;
    let mut with = 0usize;
    let mut seen = 0usize;
    for log_line in stdout_text.lines() {
        let Some(pos) = log_line.find("%  ") else {
            continue;
        };
        let mut fields = log_line[..pos].split_whitespace();
        let _id = fields.next();
        let line_count: usize = fields
            .next()
            .ok_or_else(|| format!("no line count in {log_line}"))?
            .parse()
            .map_err(|_| format!("line count unreadable in {log_line}"))?;
        seen += line_count;
        if log_line[pos + 3..].contains("<SECRET>") {
            with += line_count;
        }
    }
    if seen != expected {
        return Err(format!(
            "patterns add up to {seen} lines, total says {expected}"
        ));
    }
    Ok((with, expected))
}

fn overview_text(
    per_mode: &BTreeMap<(String, String, String), BTreeSet<String>>,
    errors: &[String],
) -> String {
    let mut line_list = Vec::new();
    let mut per_class: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for ((context, mode, class), set) in per_mode {
        line_list.push(format!("mode\t{context}\t{mode}\t{class}\t{}", set.len()));
        per_class
            .entry((context.clone(), class.clone()))
            .or_default()
            .extend(set.iter().cloned());
    }
    for ((context, class), set) in &per_class {
        line_list.push(format!("sum\t{context}\t{class}\t{}", set.len()));
    }
    let total: usize = per_class.values().map(|set| set.len()).sum();
    line_list.push(format!("total\t{total}"));
    line_list.extend(errors.iter().cloned());
    line_list.join("\n")
}

/// Tab separator, escaped JSON, a whole array or object, and a value that
/// itself contains a keyword. The canary is `QX` in `body` and is filled in here.
fn append_mask_dimensions(
    paths: &[PathBuf],
    contexts: &[Context],
    keys: &[String],
    labels: &mut Vec<String>,
    counts: &mut [usize],
) -> (usize, usize, usize, usize) {
    let sample = format!("note {{ \\\"{key}\\\" : \\\"QX\\\" }}", key = "password");
    assert_eq!(sample, r#"note { \"password\" : \"QX\" }"#);
    let mut files: Vec<BufWriter<std::fs::File>> = paths
        .iter()
        .map(|path| {
            BufWriter::new(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .expect("append"),
            )
        })
        .collect();
    let mut tab_n = 0usize;
    let mut escaped_n = 0usize;
    let mut bracket_n = 0usize;
    let mut keyword_n = 0usize;
    for key_name in keys {
        for (item_index, context) in contexts.iter().enumerate() {
            let forms = [
                (format!("{key_name}\tQX"), "tab", "tab"),
                (format!("{key_name}\t=\tQX"), "tab-eq", "tab"),
                (format!("{key_name}\t:\tQX"), "tab-colon", "tab"),
                (
                    format!("note {{ \\\"{key_name}\\\" : \\\"QX\\\" }}"),
                    "escaped-json",
                    "escaped-json",
                ),
                (
                    format!("note {{ \\\"{key_name}\\\" : \\\"zz QX\\\" }}"),
                    "escaped-json-space",
                    "escaped-json",
                ),
                (format!("{key_name}=[\"zz\", \"QX\"]"), "array", "array"),
                (
                    format!("{key_name}={{\"a\": \"zz\", \"b\": \"QX\"}}"),
                    "object",
                    "object",
                ),
                (format!("{key_name}=tokenQX next=ok"), "keyword", "keyword"),
                (format!("{key_name}=secretQX"), "keyword-end", "keyword"),
                (
                    format!("{key_name}=TokenQX next=ok"),
                    "keyword-token",
                    "keyword",
                ),
            ];
            for (body, sep, value) in forms {
                let index = labels.len();
                let piece = canary(index);
                assert!(body.contains("QX"), "{body}");
                let line_body = body.replace("QX", &piece);
                assert!(line_body.contains(&piece), "{line_body}");
                assert!(!line_body.contains("QX"), "{line_body}");
                let log_line = log_line(*context, &line_body);
                labels.push(format!(
                    "{key_name}\t{sep}\tnone\t{value}\t{}\tnone",
                    context_name(*context)
                ));
                writeln!(files[item_index], "{log_line}").expect("line");
                counts[item_index] += 1;
                match value {
                    "tab" => tab_n += 1,
                    "escaped-json" => escaped_n += 1,
                    "array" | "object" => bracket_n += 1,
                    "keyword" => keyword_n += 1,
                    other => panic!("unknown dimension {other}"),
                }
            }
        }
    }
    (tab_n, escaped_n, bracket_n, keyword_n)
}

/// Every KEYS word, bare and with `_`/`-` before or after, in three case forms,
/// plus Authorization and Proxy-Authorization. One digit-free canary per combination.
/// Extra: an unescaped quote right before the canary, and unquoted digits stuck to it.
/// Fails only after every context and mode, with a summary per context and class.
#[test]
fn generative_leak_test() {
    assert_eq!(canary(0), "qxaaaa");
    assert_eq!(index_of(&canary(12345)), 12345);
    assert!(canary(0).chars().all(|ch| ch.is_ascii_alphabetic()));
    assert!(canary(0).chars().count() < 32);
    assert_eq!(
        format!(
            "password{}{}",
            "\": \"",
            wrap(Quoting::Double, "lead qxaaaa", "\": \"")
        ),
        "password\": \"lead qxaaaa\""
    );
    assert_eq!(
        format!(
            "token{}{}",
            "\":\"",
            wrap(Quoting::Escape, "qxaaaa", "\":\"")
        ),
        "token\":\"ab\\\" qxaaaa\""
    );
    assert_eq!(wrap(Quoting::Double, "qxaaaa", "="), "\"qxaaaa\"");
    assert_eq!(
        value_body(ValueKind::DigitBefore, Quoting::Unquoted, false, "qxaaaa").as_deref(),
        Some("4242qxaaaa")
    );
    assert_eq!(
        value_body(ValueKind::DigitAfter, Quoting::Unquoted, false, "qxaaaa").as_deref(),
        Some("qxaaaa4242")
    );
    assert!(value_body(ValueKind::DigitBefore, Quoting::Double, false, "qxaaaa").is_none());
    assert_eq!(
        log_line(Context::JsonCompact, "token=4242qxaaaa"),
        "{\"a\":\"b\", token=4242qxaaaa }"
    );
    assert_eq!(
        log_line(Context::JsonCompact, "token qxaaaa4242"),
        "{\"a\":\"b\", token qxaaaa4242 }"
    );

    let clock = Instant::now();
    let _ = std::fs::remove_file(std::env::temp_dir().join("log-bill-leaks-failed.txt"));
    let dir = TempDir(std::env::temp_dir().join(format!("log-bill-leaks-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).expect("temporary directory");
    let mut labels = Vec::new();

    let separators = [
        ("=", "="),
        (" = ", "space=space"),
        (":", ":"),
        (" : ", "space:space"),
        ("\": \"", "json-space"),
        ("\":\"", "json"),
        // Only together with digit-before and digit-after: `token 4242qxaaaa`.
        (" ", "space"),
    ];
    let quotes = [
        Quoting::Unquoted,
        Quoting::Single,
        Quoting::Double,
        Quoting::Escape,
    ];
    let value_kinds = [
        ValueKind::Letters,
        ValueKind::Spaces,
        ValueKind::Base64,
        ValueKind::Digits,
        ValueKind::DigitsBase64,
        ValueKind::DigitBefore,
        ValueKind::DigitAfter,
    ];
    let contexts = [
        Context::Alone,
        Context::Fields,
        Context::JsonCompact,
        Context::JsonSpaces,
        Context::Jsonl,
    ];
    let inner_kinds = [
        InnerQuote::Unquoted,
        InnerQuote::OneApostrophe,
        InnerQuote::MoreApostrophes,
        InnerQuote::DoubleQuote,
    ];
    let paths: Vec<PathBuf> = contexts
        .iter()
        .map(|context| dir.0.join(format!("{}.log", context_name(*context))))
        .collect();
    let mut writers: Vec<std::io::BufWriter<std::fs::File>> = paths
        .iter()
        .map(|file_path| std::io::BufWriter::new(std::fs::File::create(file_path).expect("file")))
        .collect();
    let mut counts = vec![0usize; contexts.len()];

    let keys = keys();
    assert!(keys.iter().any(|s| s == "password"));
    assert!(keys.iter().any(|s| s == "PASSWORD"));
    assert!(keys.iter().any(|s| s == "Password"));
    assert!(keys.iter().any(|s| s == "_password"));
    assert!(keys.iter().any(|s| s == "-password"));
    assert!(keys.iter().any(|s| s == "password_"));
    assert!(keys.iter().any(|s| s == "password-"));
    assert!(keys.iter().any(|s| s == "Authorization"));
    assert!(keys.iter().any(|s| s == "Proxy-Authorization"));
    assert!(keys.iter().any(|s| s == "PROXY-AUTHORIZATION"));
    assert!(keys.iter().any(|s| s == "proxy-authorization"));
    for key in keys_from_source() {
        assert!(keys.iter().any(|s| s == key), "missing bare {key}");
        assert!(
            keys.iter().any(|s| s == &key.to_ascii_uppercase()),
            "missing uppercase forms of {key}"
        );
    }

    for key_name in &keys {
        let auth = is_auth(key_name);
        for (sep, sep_name) in separators {
            for quote in quotes {
                for value in value_kinds {
                    if !matches_separator(sep_name, quote, value) {
                        continue;
                    }
                    if value_body(value, quote, auth, "qxaaaa").is_none() {
                        continue;
                    }
                    for inner in inner_kinds {
                        if !inner_fits(inner, quote) {
                            continue;
                        }
                        for context in contexts {
                            let index = labels.len();
                            let piece = canary(index);
                            let body =
                                value_body(value, quote, auth, &piece).expect("already accepted");
                            let body = with_inner(&body, &piece, inner);
                            let core = format!("{key_name}{sep}{}", wrap(quote, &body, sep));
                            assert!(core.contains(&piece), "canary is outside the core: {core}");
                            if !matches!(inner, InnerQuote::Unquoted) {
                                let marker = with_inner(piece.as_str(), &piece, inner);
                                assert!(
                                    core.contains(&marker),
                                    "canary is not after the quote: {core}"
                                );
                            }
                            let log_line = log_line(context, &core);
                            labels.push(format!(
                                "{key_name}\t{sep_name}\t{}\t{}\t{}\t{}",
                                quote_name(quote),
                                value_name(value),
                                context_name(context),
                                inner_name(inner)
                            ));
                            let item_index = contexts
                                .iter()
                                .position(|item| *item == context)
                                .expect("context");
                            writeln!(writers[item_index], "{log_line}").expect("line");
                            counts[item_index] += 1;
                        }
                    }
                }
            }
        }
    }
    drop(writers);
    let n = labels.len();
    assert_eq!(n, counts.iter().sum::<usize>());
    assert!(n > 1000, "too few combinations: {n}");
    assert!(n >= 128_310, "fewer than 128310 combinations: {n}");
    // 8 key words × 5 forms × 3 cases = 120, plus Proxy-Authorization in three cases.
    let n_auth = keys.iter().filter(|s| is_auth(s)).count();
    let n_ordinary = keys.len() - n_auth;
    assert_eq!(keys.len(), 123);
    assert_eq!(n_auth, 18);
    assert_eq!(n_ordinary, 105);
    // Six separators × 5 contexts. Unquoted ordinary: letters, base64, digit-before, digit-after.
    // Unquoted auth also counts spaces, loose digits, digits-base64. Single: 5×3 inner quotes.
    // Double: 5×2. Escape: 5. 6×(4+15+10+5)×5 = 1020. 6×(7+15+10+5)×5 = 1110.
    // Space as separator: only digit-before and digit-after, 2×5 contexts per key.
    assert_eq!(n, n_ordinary * 1020 + n_auth * 1110 + keys.len() * 10);
    let one_apostrophe = labels
        .iter()
        .filter(|label| label.ends_with("\tone-apostrophe"))
        .count();
    let more_apostrophes = labels
        .iter()
        .filter(|label| label.ends_with("\tmore-apostrophes"))
        .count();
    let double_quote = labels
        .iter()
        .filter(|label| label.ends_with("\tdouble-quote"))
        .count();
    // Each key, 6 separators, 5 values, 5 contexts. The space separator has no inner quote.
    assert_eq!(one_apostrophe, 123 * 6 * 5 * 5);
    assert_eq!(more_apostrophes, 123 * 6 * 5 * 5);
    assert_eq!(double_quote, 123 * 6 * 5 * 5);
    let digit_before = labels
        .iter()
        .filter(|label| label.split('\t').nth(3) == Some("digit-before"))
        .count();
    let digit_after = labels
        .iter()
        .filter(|label| label.split('\t').nth(3) == Some("digit-after"))
        .count();
    // Seven separators (the six plus space), five contexts, every key.
    assert_eq!(digit_before, 123 * 7 * 5);
    assert_eq!(digit_after, 123 * 7 * 5);
    assert!(labels
        .iter()
        .filter(|label| {
            let value = label.split('\t').nth(3);
            value == Some("digit-before") || value == Some("digit-after")
        })
        .all(|label| {
            let mut parts = label.split('\t');
            let _key = parts.next();
            let _sep = parts.next();
            parts.next() == Some("none") && label.ends_with("\tnone")
        }));
    let mut digit_pairs = BTreeSet::new();
    for label in &labels {
        let parts: Vec<&str> = label.split('\t').collect();
        if parts[4] == "alone" && (parts[3] == "digit-before" || parts[3] == "digit-after") {
            digit_pairs.insert((parts[0], parts[1], parts[3]));
        }
    }
    for key_name in &keys {
        for sep in [
            "=",
            "space",
            "space=space",
            ":",
            "space:space",
            "json",
            "json-space",
        ] {
            for form in ["digit-before", "digit-after"] {
                assert!(
                    digit_pairs.contains(&(key_name.as_str(), sep, form)),
                    "missing {key_name} {sep} {form}"
                );
            }
        }
    }
    for key_name in ["Authorization", "Proxy-Authorization"] {
        for tail in ["one-apostrophe", "more-apostrophes", "double-quote"] {
            assert!(
                labels
                    .iter()
                    .any(|label| label.starts_with(&format!("{key_name}\t"))
                        && label.ends_with(tail)),
                "missing {key_name} / {tail}"
            );
        }
    }

    let (tab_n, escaped_n, bracket_n, keyword_n) =
        append_mask_dimensions(&paths, &contexts, &keys, &mut labels, &mut counts);
    let per_key = keys.len() * contexts.len();
    assert_eq!(tab_n, per_key * 3, "tab separator");
    assert_eq!(escaped_n, per_key * 2, "escaped JSON");
    assert_eq!(bracket_n, per_key * 2, "array or object");
    assert_eq!(keyword_n, per_key * 3, "keyword inside the value");

    let mut outcomes = Vec::new();
    for (item_index, context) in contexts.iter().enumerate() {
        let text = std::fs::read_to_string(&paths[item_index]).expect("read back");
        assert_eq!(
            text.lines().count(),
            counts[item_index],
            "{}",
            context_name(*context)
        );
        assert!(
            text.lines().all(|log_line| log_line.contains("qx")),
            "canary is missing in {}",
            context_name(*context)
        );
        if *context == Context::Jsonl {
            assert!(text.contains("\"note\":"));
        }
        if *context == Context::JsonCompact {
            assert!(
                text.lines()
                    .all(|log_line| log_line.starts_with("{\"a\":\"b\", ")),
                "compact line without a short field"
            );
        }
        let file_path = paths[item_index].to_str().expect("path");
        outcomes.extend(run_modes(
            context_name(*context),
            *context == Context::Jsonl,
            file_path,
        ));
    }

    let mut per_mode: BTreeMap<(String, String, String), BTreeSet<String>> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut compact_with = 0usize;
    let mut compact_all = 0usize;
    for run in &outcomes {
        if run.code != 0 || !run.err.is_empty() {
            let short: String = run.err.chars().take(200).collect();
            errors.push(format!(
                "error\t{}\t{}\texit {}\t{short}",
                run.context, run.mode, run.code
            ));
        }
        if run.mode == "text" {
            match count_secret(&run.stdout_text) {
                Ok((with, all_of)) => {
                    if with != all_of {
                        errors.push(format!(
                            "secret\t{}\twith_SECRET={with}\tof {all_of}",
                            run.context
                        ));
                    }
                    if run.context == "json-compact" {
                        compact_with = with;
                        compact_all = all_of;
                    }
                }
                Err(reason) => errors.push(format!("secret\t{}\t{reason}", run.context)),
            }
        }
        for (mode, text) in [
            ("stderr", run.err.as_str()),
            (run.mode, run.stdout_text.as_str()),
        ] {
            for label in leaked(text, &labels) {
                per_mode
                    .entry((context_of(&label), mode.to_string(), class_of(&label)))
                    .or_default()
                    .insert(label);
            }
        }
    }
    let text = overview_text(&per_mode, &errors);
    eprintln!(
        "generative leak test: {n} combinations, {} with an unescaped quote in the value (one apostrophe {one_apostrophe}, several apostrophes {more_apostrophes}, double quote {double_quote}), unquoted digits {} (digit before {digit_before}, digit after {digit_after}), tab separator {tab_n}, escaped JSON {escaped_n}, array/object {bracket_n}, keyword in value {keyword_n}, compact lines with <SECRET>: {compact_with} of {compact_all}, {} ms",
        one_apostrophe + more_apostrophes + double_quote,
        digit_before + digit_after,
        clock.elapsed().as_millis()
    );
    if !per_mode.is_empty() || !errors.is_empty() {
        let file_path = std::env::temp_dir().join(format!(
            "log-bill-leaks-overview-{}.txt",
            std::process::id()
        ));
        std::fs::write(&file_path, &text).expect("summary");
        eprintln!("{text}");
        panic!("leak test failed, summary in {}", file_path.display());
    }
    assert_eq!(
        compact_with, compact_all,
        "not every compact line shows <SECRET>"
    );
    assert!(compact_all > 0, "no compact lines");
}

#[test]
fn quoted_examples_fully_masked() {
    let dir = TempDir(std::env::temp_dir().join(format!("log-bill-quoted-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).expect("temporary directory");
    let file_path = dir.0.join("quoted.log");
    let contents = "\
{\"password\": \"hunter two words\", \"user\": \"ada\"}\n\
{\"token\":\"eyJhbGciOiJIUzI1NiJ9\"}\n";
    std::fs::write(&file_path, contents).expect("quoted file");
    let arg = file_path.to_str().expect("path");
    let secret = ["hunter", "two words", "eyJhbGciOiJIUzI1NiJ9"];
    let patterns = [
        "{\"password\": \"<SECRET>\", \"user\": \"ada\"}",
        "{\"token\":\"<SECRET>\"}",
    ];

    for (mode, args) in [
        ("scan", vec!["scan", arg]),
        ("scan --explain", vec!["scan", "--explain", arg]),
        ("diff", vec!["diff", arg, arg]),
    ] {
        let (code, stdout_text, err) = run_tool(&args);
        assert_eq!(code, 0, "{mode}\n{stdout_text}\n{err}");
        assert!(err.is_empty(), "{mode} stderr: {err}");
        for pattern in patterns {
            assert!(
                stdout_text.contains(pattern),
                "{mode} missing {pattern:?}\n{stdout_text}"
            );
        }
        for piece in secret {
            assert!(
                !stdout_text.contains(piece),
                "{mode} leaks {piece}\n{stdout_text}"
            );
            assert!(!err.contains(piece), "{mode} stderr leaks {piece}");
        }
    }

    let json_patterns = [
        r#"{\"password\": \"<SECRET>\", \"user\": \"ada\"}"#,
        r#"{\"token\":\"<SECRET>\"}"#,
    ];
    for (mode, args) in [
        ("scan --json", vec!["scan", "--json", arg]),
        ("diff --json", vec!["diff", "--json", arg, arg]),
    ] {
        let (code, stdout_text, err) = run_tool(&args);
        assert_eq!(code, 0, "{mode}\n{stdout_text}\n{err}");
        assert!(err.is_empty(), "{mode} stderr: {err}");
        for pattern in json_patterns {
            assert!(
                stdout_text.contains(pattern),
                "{mode} missing {pattern:?}\n{stdout_text}"
            );
        }
        for piece in secret {
            assert!(
                !stdout_text.contains(piece),
                "{mode} leaks {piece}\n{stdout_text}"
            );
            assert!(!err.contains(piece), "{mode} stderr leaks {piece}");
        }
    }
}

/// Text, `--explain`, `--json` and diff must hide `secrets`.
fn assert_masked_line(name: &str, line: &str, secrets: &[&str]) {
    let dir = TempDir(std::env::temp_dir().join(format!("log-bill-{name}-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).expect("temporary directory");
    let file_path = dir.0.join("line.log");
    std::fs::write(&file_path, format!("{line}\n")).expect("line");
    let arg = file_path.to_str().expect("path");
    for (mode, args) in [
        ("scan", vec!["scan", arg]),
        ("scan --explain", vec!["scan", "--explain", arg]),
        ("scan --json", vec!["scan", "--json", arg]),
        ("diff", vec!["diff", arg, arg]),
        ("diff --json", vec!["diff", "--json", arg, arg]),
    ] {
        let (code, stdout_text, err) = run_tool(&args);
        assert_eq!(code, 0, "{name} {mode}\n{stdout_text}\n{err}");
        assert!(err.is_empty(), "{name} {mode} stderr: {err}");
        assert!(
            stdout_text.contains("<SECRET>"),
            "{name} {mode} missing <SECRET>\n{stdout_text}"
        );
        for piece in secrets {
            assert!(
                !stdout_text.contains(piece),
                "{name} {mode} leaks {piece}\n{stdout_text}"
            );
            assert!(!err.contains(piece), "{name} {mode} stderr leaks {piece}");
        }
    }
}

#[test]
fn tab_separator_masks_hunter() {
    assert_masked_line("tab", "login password\thunter", &["hunter"]);
}

#[test]
fn escaped_json_masks_open_sesame() {
    assert_masked_line(
        "escaped-json",
        r#"login {\"password\":\"open sesame\"}"#,
        &["open sesame", "sesame"],
    );
}

#[test]
fn array_value_masks_alpha_and_beta() {
    assert_masked_line(
        "array",
        r#"login password=["alpha", "beta"]"#,
        &["alpha", "beta"],
    );
}

#[test]
fn object_value_masks_hunter_and_two() {
    assert_masked_line(
        "object",
        r#"login secret={"a": "hunter", "b": "two"}"#,
        &["hunter", "two"],
    );
}

#[test]
fn email_does_not_swallow_the_token_key() {
    assert_masked_line(
        "email-key",
        "login id=a@b.co.token=hunterpass",
        &["hunterpass"],
    );
}

#[test]
fn keyword_in_value_masks_token_hunter() {
    assert_masked_line(
        "keyword-token",
        "api_key=tokenHunterWordHere next=ok",
        &["tokenHunterWordHere"],
    );
}

#[test]
fn keyword_in_value_masks_secret_pass() {
    assert_masked_line("keyword-secret", "password=secretPass1", &["secretPass1"]);
}

#[test]
fn keyword_in_value_masks_token_value() {
    assert_masked_line(
        "keyword-token-value",
        "password=TokenValueXyzHere",
        &["TokenValueXyzHere"],
    );
}

#[test]
fn quote_in_free_text_fully_masked() {
    let dir = TempDir(std::env::temp_dir().join(format!("log-bill-quote-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).expect("temporary directory");
    let file_path = dir.0.join("quote.log");
    let contents = "token='the users secret, it's hunterword, done' finished\n";
    std::fs::write(&file_path, contents).expect("quote file");
    let arg = file_path.to_str().expect("path");
    let pattern = "token='<SECRET>";
    let secret = ["hunterword", "finished", "it's"];

    for (mode, args) in [
        ("scan", vec!["scan", arg]),
        ("scan --explain", vec!["scan", "--explain", arg]),
        ("diff", vec!["diff", arg, arg]),
    ] {
        let (code, stdout_text, err) = run_tool(&args);
        assert_eq!(code, 0, "{mode}\n{stdout_text}\n{err}");
        assert!(err.is_empty(), "{mode} stderr: {err}");
        assert!(
            stdout_text.contains(pattern),
            "{mode} missing {pattern:?}\n{stdout_text}"
        );
        for piece in secret {
            assert!(
                !stdout_text.contains(piece),
                "{mode} leaks {piece}\n{stdout_text}"
            );
            assert!(!err.contains(piece), "{mode} stderr leaks {piece}");
        }
    }

    for (mode, args) in [
        ("scan --json", vec!["scan", "--json", arg]),
        ("diff --json", vec!["diff", "--json", arg, arg]),
    ] {
        let (code, stdout_text, err) = run_tool(&args);
        assert_eq!(code, 0, "{mode}\n{stdout_text}\n{err}");
        assert!(err.is_empty(), "{mode} stderr: {err}");
        assert!(
            stdout_text.contains(pattern),
            "{mode} missing {pattern:?}\n{stdout_text}"
        );
        for piece in secret {
            assert!(
                !stdout_text.contains(piece),
                "{mode} leaks {piece}\n{stdout_text}"
            );
            assert!(!err.contains(piece), "{mode} stderr leaks {piece}");
        }
    }
}
