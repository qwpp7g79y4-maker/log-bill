use std::fmt;

/// One line. `bytes` is the raw count, not the length of `text`.
/// `text` is the message (for JSON the field, otherwise the line). `raw` is the
/// line text without a newline, for the regex; for JSON the whole line.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub raw: String,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum ReadError {
    Unreadable,
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
            ReadError::InvalidJson { line } => write!(f, "invalid JSON on line {line}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Reads a text file. Each line weighs its raw bytes plus 1 for the
/// newline; without `\n` that +1 still counts. A `\r` just before `\n` counts.
/// Empty lines are dropped. Invalid UTF-8 counts and is read lossily.
pub fn read_text(path: &str) -> Result<Vec<Line>, ReadError> {
    let data = read_bytes(path)?;
    Ok(text_lines(&data))
}

const NO_MESSAGE: &str = "<NO-MESSAGE>";

/// Reads JSON lines. Bytes are those of the whole line. The message comes
/// from `message_field` (`--message-field`), else the first present
/// field of `message`, `msg`, and `log`. With no message text it is
/// `<NO-MESSAGE>`. Invalid JSON stops with the line number, without the line.
pub fn read_jsonl(path: &str, message_field: Option<&str>) -> Result<Vec<Line>, ReadError> {
    let data = read_bytes(path)?;
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
pub fn read_regular(path: &str) -> Result<Vec<u8>, OpenError> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => std::fs::read(path).map_err(|_| OpenError::Unreadable),
        Ok(_) => Err(OpenError::NotRegular),
        Err(_) => Err(OpenError::Unreadable),
    }
}

fn read_bytes(path: &str) -> Result<Vec<u8>, ReadError> {
    match read_regular(path) {
        Ok(data) => Ok(data),
        Err(OpenError::Unreadable) => Err(ReadError::Unreadable),
        Err(OpenError::NotRegular) => Err(ReadError::NotRegular),
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
    let content = line_body(raw, had_newline);
    if content.is_empty() {
        return Ok(());
    }
    let raw_text =
        std::str::from_utf8(content).map_err(|_| ReadError::InvalidJson { line: line_no })?;
    let value: serde_json::Value =
        serde_json::from_str(raw_text).map_err(|_| ReadError::InvalidJson { line: line_no })?;
    lines.push(Line {
        text: message_text(&value, message_field),
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
    let text = String::from_utf8_lossy(content).into_owned();
    lines.push(Line {
        raw: text.clone(),
        text,
        bytes: (raw.len() + 1) as u64,
    });
}

#[cfg(test)]
mod tests {
    use super::{json_lines, text_lines, Line};

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
