use std::fmt;
use std::path::Path;

/// One line. `bytes` is the raw count, not the length of `text`.
/// `text` is the message (for JSON the field, otherwise the line). `raw` is the
/// line text without a newline, for the regex; for JSON the whole line.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub raw: String,
    pub bytes: u64,
}

/// A wide text encoding that log-bill refuses. Masking works on UTF-8 text;
/// the NUL bytes of UTF-16 and UTF-32 would split keys such as `password`
/// so that they are no longer recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideEncoding {
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl WideEncoding {
    pub fn name(self) -> &'static str {
        match self {
            WideEncoding::Utf16Le => "UTF-16LE",
            WideEncoding::Utf16Be => "UTF-16BE",
            WideEncoding::Utf32Le => "UTF-32LE",
            WideEncoding::Utf32Be => "UTF-32BE",
        }
    }
}

#[derive(Debug)]
pub enum ReadError {
    Unreadable,
    /// UTF-16 or UTF-32 input, with or without a byte order mark. Nothing of
    /// the file is read into patterns, so nothing of it can reach the output.
    NotUtf8 {
        encoding: WideEncoding,
    },
    /// Not a regular file (FIFO, directory, socket). The path is not included.
    NotRegular,
    /// Line number in the file. The line itself is not included.
    InvalidJson {
        line: u64,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Unreadable => f.write_str("file is not readable"),
            ReadError::NotRegular => f.write_str("file is not a regular file"),
            ReadError::NotUtf8 { encoding } => {
                let name = encoding.name();
                write!(
                    f,
                    "file is {name}, not UTF-8; convert it first, for example with iconv -f {name} -t UTF-8, or in PowerShell with Out-File -Encoding utf8"
                )
            }
            ReadError::InvalidJson { line } => write!(f, "invalid JSON on line {line}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Reads a text file. Each line weighs its raw bytes plus 1 for the
/// newline; without `\n` that +1 still counts. A `\r` just before `\n` counts.
/// Empty lines are dropped. Invalid UTF-8 counts and is read lossily.
/// UTF-16 and UTF-32 are refused, see [`detect_wide`].
pub fn read_text(path: &Path) -> Result<Vec<Line>, ReadError> {
    let data = read_bytes(path)?;
    refuse_wide(&data)?;
    Ok(text_lines(&data))
}

const NO_MESSAGE: &str = "<NO-MESSAGE>";

/// Reads JSON lines. Bytes are those of the whole line. The message comes
/// from `message_field` (`--message-field`), else the first present
/// field of `message`, `msg`, and `log`. With no message text it is
/// `<NO-MESSAGE>`. Invalid JSON stops with the line number, without the line.
/// One UTF-8 byte order mark at the start is skipped for parsing but still
/// counts in the bytes of the first line. UTF-16 and UTF-32 are refused.
pub fn read_jsonl(path: &Path, message_field: Option<&str>) -> Result<Vec<Line>, ReadError> {
    let data = read_bytes(path)?;
    refuse_wide(&data)?;
    json_lines(&data, message_field)
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    Unreadable,
    NotRegular,
}

/// Reads bytes of a regular file. `metadata` runs before `read`, so a
/// FIFO with no writer does not hang. A symlink to a regular
/// file counts; a symlink to a FIFO does not.
pub fn read_regular(path: &Path) -> Result<Vec<u8>, OpenError> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => std::fs::read(path).map_err(|_| OpenError::Unreadable),
        Ok(_) => Err(OpenError::NotRegular),
        Err(_) => Err(OpenError::Unreadable),
    }
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, ReadError> {
    match read_regular(path) {
        Ok(data) => Ok(data),
        Err(OpenError::Unreadable) => Err(ReadError::Unreadable),
        Err(OpenError::NotRegular) => Err(ReadError::NotRegular),
    }
}

/// The UTF-8 byte order mark.
pub const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// How many bytes at the start are inspected for an encoding without a BOM.
const SNIFF_BYTES: usize = 4096;

fn refuse_wide(data: &[u8]) -> Result<(), ReadError> {
    match detect_wide(data) {
        Some(encoding) => Err(ReadError::NotUtf8 { encoding }),
        None => Ok(()),
    }
}

/// Detects UTF-16 and UTF-32 by their byte order mark, or without one by
/// the NUL bytes that ASCII text gets in those encodings: per byte position
/// modulo 4, in the first 4096 bytes. UTF-32 is checked before UTF-16.
/// A run of NUL bytes on every position (a truncated, zero-filled log) is not
/// taken for a wide encoding; [`without_nul`] removes such NUL bytes before masking.
pub fn detect_wide(data: &[u8]) -> Option<WideEncoding> {
    if data.starts_with(b"\xFF\xFE\x00\x00") {
        return Some(WideEncoding::Utf32Le);
    }
    if data.starts_with(b"\x00\x00\xFE\xFF") {
        return Some(WideEncoding::Utf32Be);
    }
    if data.starts_with(b"\xFF\xFE") {
        return Some(WideEncoding::Utf16Le);
    }
    if data.starts_with(b"\xFE\xFF") {
        return Some(WideEncoding::Utf16Be);
    }
    let sample = &data[..data.len().min(SNIFF_BYTES)];
    if !sample.contains(&0) {
        return None;
    }
    let mut total = [0usize; 4];
    let mut nul = [0usize; 4];
    for (index, &byte) in sample.iter().enumerate() {
        total[index % 4] += 1;
        if byte == 0 {
            nul[index % 4] += 1;
        }
    }
    // At least 90 percent NUL, or at most 10 percent NUL, on one position.
    let high = |k: usize| total[k] > 0 && nul[k] * 10 >= total[k] * 9;
    let low = |k: usize| nul[k] * 10 <= total[k];
    if high(1) && high(2) && high(3) && low(0) {
        return Some(WideEncoding::Utf32Le);
    }
    if high(0) && high(1) && high(2) && low(3) {
        return Some(WideEncoding::Utf32Be);
    }
    let (even_total, even_nul) = (total[0] + total[2], nul[0] + nul[2]);
    let (odd_total, odd_nul) = (total[1] + total[3], nul[1] + nul[3]);
    // Half of the odd bytes NUL leaves room for text that is not only ASCII.
    if odd_total > 0 && odd_nul * 2 >= odd_total && even_nul * 10 <= even_total {
        return Some(WideEncoding::Utf16Le);
    }
    if even_total > 0 && even_nul * 2 >= even_total && odd_nul * 10 <= odd_total {
        return Some(WideEncoding::Utf16Be);
    }
    None
}

/// NUL bytes never carry meaning in a log message, but they can split a key
/// such as `password` so that masking misses it. They are removed from the
/// text that is masked; the byte counts and the raw line for the rules keep them.
fn without_nul(text: String) -> String {
    if text.contains('\0') {
        text.replace('\0', "")
    } else {
        text
    }
}

fn json_lines(data: &[u8], message_field: Option<&str>) -> Result<Vec<Line>, ReadError> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut line_no = 0u64;
    for (index, &byte) in data.iter().enumerate() {
        if byte != b'\n' {
            continue;
        }
        line_no += 1;
        push_json_line(
            &mut lines,
            &data[start..index],
            true,
            line_no,
            message_field,
        )?;
        start = index + 1;
    }
    if start < data.len() {
        line_no += 1;
        push_json_line(&mut lines, &data[start..], false, line_no, message_field)?;
    }
    Ok(lines)
}

fn push_json_line(
    lines: &mut Vec<Line>,
    raw: &[u8],
    had_newline: bool,
    line_no: u64,
    message_field: Option<&str>,
) -> Result<(), ReadError> {
    let mut content = line_body(raw, had_newline);
    if line_no == 1 {
        // The BOM is skipped for parsing; `raw.len()` below still counts it.
        content = content.strip_prefix(UTF8_BOM).unwrap_or(content);
    }
    if content.is_empty() {
        return Ok(());
    }
    let raw_text =
        std::str::from_utf8(content).map_err(|_| ReadError::InvalidJson { line: line_no })?;
    let value: serde_json::Value =
        serde_json::from_str(raw_text).map_err(|_| ReadError::InvalidJson { line: line_no })?;
    lines.push(Line {
        text: without_nul(message_text(&value, message_field)),
        raw: raw_text.to_owned(),
        bytes: (raw.len() + 1) as u64,
    });
    Ok(())
}

fn line_body(raw: &[u8], had_newline: bool) -> &[u8] {
    if had_newline {
        raw.strip_suffix(b"\r").unwrap_or(raw)
    } else {
        raw
    }
}

/// The first present key wins. A key with no text is `<NO-MESSAGE>`
/// and does not fall through to a later field.
fn message_text(value: &serde_json::Value, message_field: Option<&str>) -> String {
    if let Some(name) = message_field {
        return text_at(value, name);
    }
    for name in ["message", "msg", "log"] {
        if value.get(name).is_some() {
            return text_at(value, name);
        }
    }
    NO_MESSAGE.to_owned()
}

fn text_at(value: &serde_json::Value, name: &str) -> String {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(NO_MESSAGE)
        .to_owned()
}

fn text_lines(data: &[u8]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, &byte) in data.iter().enumerate() {
        if byte != b'\n' {
            continue;
        }
        // `raw` ends before the `\n`; the +1 is that newline.
        push_line(&mut lines, &data[start..index], true);
        start = index + 1;
    }
    if start < data.len() {
        // Last line without `\n`: the same +1, so each line weighs the same.
        push_line(&mut lines, &data[start..], false);
    }
    lines
}

fn push_line(lines: &mut Vec<Line>, raw: &[u8], had_newline: bool) {
    let content = line_body(raw, had_newline);
    if content.is_empty() {
        return;
    }
    let raw_text = String::from_utf8_lossy(content).into_owned();
    lines.push(Line {
        text: without_nul(raw_text.clone()),
        raw: raw_text,
        bytes: (raw.len() + 1) as u64,
    });
}

#[cfg(test)]
mod tests {
    use super::{detect_wide, json_lines, text_lines, Line, WideEncoding};

    #[test]
    fn wide_encodings_by_bom() {
        assert_eq!(detect_wide(b"\xFF\xFEa\x00"), Some(WideEncoding::Utf16Le));
        assert_eq!(detect_wide(b"\xFE\xFF\x00a"), Some(WideEncoding::Utf16Be));
        assert_eq!(
            detect_wide(b"\xFF\xFE\x00\x00a\x00\x00\x00"),
            Some(WideEncoding::Utf32Le)
        );
        assert_eq!(
            detect_wide(b"\x00\x00\xFE\xFF\x00\x00\x00a"),
            Some(WideEncoding::Utf32Be)
        );
    }

    #[test]
    fn wide_encodings_without_bom() {
        assert_eq!(
            detect_wide(b"a\x00b\x00\n\x00"),
            Some(WideEncoding::Utf16Le)
        );
        assert_eq!(
            detect_wide(b"\x00a\x00b\x00\n"),
            Some(WideEncoding::Utf16Be)
        );
        assert_eq!(
            detect_wide(b"a\x00\x00\x00\n\x00\x00\x00"),
            Some(WideEncoding::Utf32Le)
        );
        assert_eq!(
            detect_wide(b"\x00\x00\x00a\x00\x00\x00\n"),
            Some(WideEncoding::Utf32Be)
        );
        // Mixed scripts in UTF-16LE: "ab日本" has 2 of 4 odd bytes NUL.
        assert_eq!(
            detect_wide(b"a\x00b\x00\xE5\x65\x2C\x67"),
            Some(WideEncoding::Utf16Le)
        );
    }

    #[test]
    fn utf8_is_not_a_wide_encoding() {
        assert_eq!(detect_wide(b""), None);
        assert_eq!(detect_wide(b"plain text\n"), None);
        assert_eq!(detect_wide("caf\u{e9} \u{1F600}\n".as_bytes()), None);
        assert_eq!(detect_wide(b"\xEF\xBB\xBFtext\n"), None);
        // A few NUL bytes in UTF-8 text, and a zero-filled start.
        assert_eq!(detect_wide(b"pa\x00ssword=x\n"), None);
        assert_eq!(detect_wide(&[0u8; 64]), None);
    }

    #[test]
    fn nul_bytes_are_removed_from_the_text_only() {
        let lines = text_lines(b"pa\x00ss\n");
        assert_eq!(
            lines,
            vec![Line {
                text: "pass".into(),
                raw: "pa\u{0}ss".into(),
                bytes: 6
            }]
        );
    }

    #[test]
    fn json_bom_is_skipped_and_counted() {
        // 3 BOM bytes + 15 + `\n` = 19. Only the first line can carry it.
        let raw = b"\xEF\xBB\xBF{\"message\":\"a\"}\n{\"message\":\"a\"}\n";
        let lines = json_lines(raw, None).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].raw, "{\"message\":\"a\"}");
        assert_eq!(lines[0].bytes, 19);
        assert_eq!(lines[1].bytes, 16);
        // A BOM on a later line is not skipped.
        let raw = b"{\"message\":\"a\"}\n\xEF\xBB\xBF{\"message\":\"a\"}\n";
        assert!(json_lines(raw, None).is_err());
    }

    #[test]
    fn hand_calculated_bytes() {
        // abc\n = 4, abc\r\n = 5, empty line dropped, last line xy = 3.
        let lines = text_lines(b"abc\nabc\r\n\nxy");
        assert_eq!(
            lines,
            vec![
                Line {
                    text: "abc".into(),
                    raw: "abc".into(),
                    bytes: 4
                },
                Line {
                    text: "abc".into(),
                    raw: "abc".into(),
                    bytes: 5
                },
                Line {
                    text: "xy".into(),
                    raw: "xy".into(),
                    bytes: 3
                },
            ]
        );
    }

    #[test]
    fn empty_lines_do_not_count() {
        assert!(text_lines(b"").is_empty());
        assert!(text_lines(b"\n").is_empty());
        assert!(text_lines(b"\r\n").is_empty());
        assert!(text_lines(b"\n\r\n\n").is_empty());
    }

    #[test]
    fn invalid_utf8_counts_and_becomes_lossy() {
        let lines = text_lines(b"a\xffb\n\xfe");
        assert_eq!(
            lines,
            vec![
                Line {
                    text: "a\u{FFFD}b".into(),
                    raw: "a\u{FFFD}b".into(),
                    bytes: 4
                },
                Line {
                    text: "\u{FFFD}".into(),
                    raw: "\u{FFFD}".into(),
                    bytes: 2
                },
            ]
        );
    }

    #[test]
    fn field_message() {
        // 45 = the whole line, including `\r\n`. `message` wins over `msg` and `log`.
        let raw = b"{\"message\":\"hallo\",\"msg\":\"nee\",\"log\":\"ook\"}\r\n";
        let lines = json_lines(raw, None).unwrap();
        assert_eq!(
            lines,
            vec![Line {
                text: "hallo".into(),
                raw: "{\"message\":\"hallo\",\"msg\":\"nee\",\"log\":\"ook\"}".into(),
                bytes: 45
            }]
        );
    }

    #[test]
    fn field_msg() {
        // 28 = the whole line, including `\n`. No `message`, so `msg`.
        let raw = b"{\"msg\":\"hallo\",\"log\":\"nee\"}\n";
        let lines = json_lines(raw, None).unwrap();
        assert_eq!(
            lines,
            vec![Line {
                text: "hallo".into(),
                raw: "{\"msg\":\"hallo\",\"log\":\"nee\"}".into(),
                bytes: 28
            }]
        );
    }

    #[test]
    fn field_log() {
        // 12 raw bytes with no newline, plus 1. Only `log`.
        let raw = b"{\"log\":\"xy\"}";
        let lines = json_lines(raw, None).unwrap();
        assert_eq!(
            lines,
            vec![Line {
                text: "xy".into(),
                raw: "{\"log\":\"xy\"}".into(),
                bytes: 13
            }]
        );
    }

    #[test]
    fn message_field_selects_that_field() {
        let raw = b"{\"note\":\"eigen\",\"message\":\"ander\"}\n";
        let lines = json_lines(raw, Some("note")).unwrap();
        assert_eq!(
            lines,
            vec![Line {
                text: "eigen".into(),
                raw: "{\"note\":\"eigen\",\"message\":\"ander\"}".into(),
                bytes: 35
            }]
        );

        // The named field is missing: do not fall back to `message`.
        let raw = b"{\"message\":\"hallo\"}\n";
        let lines = json_lines(raw, Some("note")).unwrap();
        assert_eq!(
            lines,
            vec![Line {
                text: "<NO-MESSAGE>".into(),
                raw: "{\"message\":\"hallo\"}".into(),
                bytes: 20
            }]
        );
    }

    #[test]
    fn without_message_field() {
        let raw = b"{\"level\":\"info\",\"host\":\"a\"}\n";
        let lines = json_lines(raw, None).unwrap();
        assert_eq!(
            lines,
            vec![Line {
                text: "<NO-MESSAGE>".into(),
                raw: "{\"level\":\"info\",\"host\":\"a\"}".into(),
                bytes: 28
            }]
        );
    }

    #[test]
    fn broken_line_has_no_content() {
        // Empty line 2 still counts for the number. The text of line 3 stays out.
        let raw = b"{\"message\":\"ok\"}\n\n{DO-NOT-SHOW-0001\n";
        let err = json_lines(raw, None).unwrap_err();
        assert_eq!(err.to_string(), "invalid JSON on line 3");
        assert!(!err.to_string().contains("DO-NOT-SHOW"));
    }
}
