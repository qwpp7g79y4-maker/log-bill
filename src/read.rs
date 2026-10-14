use std::fmt;
use std::fs::File;
use std::io::{BufRead, Read, Seek, SeekFrom};
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

/// Reads a text file one line at a time. Each line weighs its raw bytes plus 1
/// for the newline; without `\n` that +1 still counts. A `\r` just before `\n`
/// counts. Empty lines are dropped. Invalid UTF-8 counts and is read lossily.
/// One UTF-8 byte order mark at the start is skipped for the line text and
/// still counts in the bytes of the first line. UTF-16 and UTF-32 are
/// refused, see [`detect_wide`]. The whole file is not held in memory.
pub fn visit_text(path: &Path, visit: impl FnMut(Line)) -> Result<(), ReadError> {
    visit_file(path, false, None, visit)
}

const NO_MESSAGE: &str = "<NO-MESSAGE>";

/// Reads JSON lines one at a time. Bytes are those of the whole line. The
/// message comes from `message_field` (`--message-field`), else the first
/// present field of `message`, `msg`, and `log`. With no message text it is
/// `<NO-MESSAGE>`. Unknown fields are ignored, including a number that does
/// not fit in a binary float and a value nested hundreds of times. Invalid
/// JSON stops with the line number, without the line. One UTF-8 byte order
/// mark at the start is skipped for parsing but still counts in the bytes of
/// the first line. UTF-16 and UTF-32 are refused.
pub fn visit_jsonl(
    path: &Path,
    message_field: Option<&str>,
    visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    visit_file(path, true, message_field, visit)
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    Unreadable,
    NotRegular,
}

/// Reads bytes of a regular file. The open does not block, and the file type
/// comes from the opened file, so a path that turns into a FIFO between the
/// name and the read does not hang. A symlink to a regular file counts; a
/// symlink to a FIFO does not.
pub fn read_regular(path: &Path) -> Result<Vec<u8>, OpenError> {
    let mut file = open_regular(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|_| OpenError::Unreadable)?;
    Ok(data)
}

/// Open for reading without blocking, then keep the file only when `fstat`
/// says it is a regular file. The type is taken from the opened descriptor,
/// not from a second look at the path.
fn open_regular(path: &Path) -> Result<File, OpenError> {
    let file = open_read(path)?;
    match file.metadata() {
        Ok(meta) if meta.is_file() => Ok(file),
        Ok(_) => Err(OpenError::NotRegular),
        Err(_) => Err(OpenError::Unreadable),
    }
}

#[cfg(unix)]
fn open_read(path: &Path) -> Result<File, OpenError> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(o_nonblock())
        .open(path)
        .map_err(|_| OpenError::Unreadable)
}

/// `O_NONBLOCK`. Linux and Android use 0x800. The BSDs, including macOS, use 0x4.
#[cfg(unix)]
fn o_nonblock() -> i32 {
    if cfg!(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "fuchsia",
    )) {
        0x4
    } else {
        0x800
    }
}

#[cfg(not(unix))]
fn open_read(path: &Path) -> Result<File, OpenError> {
    File::open(path).map_err(|_| OpenError::Unreadable)
}

fn visit_file(
    path: &Path,
    jsonl: bool,
    message_field: Option<&str>,
    visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    let mut file = match open_regular(path) {
        Ok(file) => file,
        Err(OpenError::Unreadable) => return Err(ReadError::Unreadable),
        Err(OpenError::NotRegular) => return Err(ReadError::NotRegular),
    };
    let prefix = read_prefix(&mut file)?;
    refuse_wide(&prefix)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ReadError::Unreadable)?;
    if jsonl {
        stream_json(file, message_field, visit)
    } else {
        stream_text(file, visit)
    }
}

fn read_prefix(file: &mut File) -> Result<Vec<u8>, ReadError> {
    let mut buf = vec![0u8; SNIFF_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReadError::Unreadable),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

fn stream_text(file: File, mut visit: impl FnMut(Line)) -> Result<(), ReadError> {
    let mut reader = std::io::BufReader::new(file);
    let mut buf = Vec::new();
    let mut line_no = 0u64;
    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .map_err(|_| ReadError::Unreadable)?;
        if n == 0 {
            break;
        }
        line_no += 1;
        let had_newline = buf.last() == Some(&b'\n');
        if had_newline {
            buf.pop();
        }
        if let Some(line) = text_line(&buf, had_newline, line_no) {
            visit(line);
        }
    }
    Ok(())
}

fn stream_json(
    file: File,
    message_field: Option<&str>,
    mut visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    let mut reader = std::io::BufReader::new(file);
    let mut buf = Vec::new();
    let mut line_no = 0u64;
    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .map_err(|_| ReadError::Unreadable)?;
        if n == 0 {
            break;
        }
        line_no += 1;
        let had_newline = buf.last() == Some(&b'\n');
        if had_newline {
            buf.pop();
        }
        if let Some(line) = json_line(&buf, had_newline, line_no, message_field)? {
            visit(line);
        }
    }
    Ok(())
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
    // Three quarters of one parity NUL: mostly ASCII text in UTF-16. Exactly
    // half is not enough, UTF-8 with one NUL every 4 bytes reaches that.
    // Text that this misses still has its NUL bytes removed before masking.
    if odd_total > 0 && odd_nul * 4 >= odd_total * 3 && even_nul * 10 <= even_total {
        return Some(WideEncoding::Utf16Le);
    }
    if even_total > 0 && even_nul * 4 >= even_total * 3 && odd_nul * 10 <= odd_total {
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

#[cfg(test)]
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

#[cfg(test)]
fn push_json_line(
    lines: &mut Vec<Line>,
    raw: &[u8],
    had_newline: bool,
    line_no: u64,
    message_field: Option<&str>,
) -> Result<(), ReadError> {
    if let Some(line) = json_line(raw, had_newline, line_no, message_field)? {
        lines.push(line);
    }
    Ok(())
}

fn json_line(
    raw: &[u8],
    had_newline: bool,
    line_no: u64,
    message_field: Option<&str>,
) -> Result<Option<Line>, ReadError> {
    let mut content = line_body(raw, had_newline);
    if line_no == 1 {
        // The BOM is skipped for parsing; `raw.len()` below still counts it.
        content = content.strip_prefix(UTF8_BOM).unwrap_or(content);
    }
    if content.is_empty() {
        return Ok(None);
    }
    let raw_text =
        std::str::from_utf8(content).map_err(|_| ReadError::InvalidJson { line: line_no })?;
    let text = json_message(raw_text, message_field)
        .map_err(|_| ReadError::InvalidJson { line: line_no })?;
    Ok(Some(Line {
        text: without_nul(text),
        raw: raw_text.to_owned(),
        bytes: (raw.len() + 1) as u64,
    }))
}

fn line_body(raw: &[u8], had_newline: bool) -> &[u8] {
    if had_newline {
        raw.strip_suffix(b"\r").unwrap_or(raw)
    } else {
        raw
    }
}

#[cfg(test)]
fn push_line(lines: &mut Vec<Line>, raw: &[u8], had_newline: bool, line_no: u64) {
    if let Some(line) = text_line(raw, had_newline, line_no) {
        lines.push(line);
    }
}

fn text_line(raw: &[u8], had_newline: bool, line_no: u64) -> Option<Line> {
    let mut content = line_body(raw, had_newline);
    if line_no == 1 {
        // Skipped for the text; `raw.len()` below still counts the mark.
        content = content.strip_prefix(UTF8_BOM).unwrap_or(content);
    }
    if content.is_empty() {
        return None;
    }
    let raw_text = String::from_utf8_lossy(content).into_owned();
    Some(Line {
        text: without_nul(raw_text.clone()),
        raw: raw_text,
        bytes: (raw.len() + 1) as u64,
    })
}

#[cfg(test)]
fn text_lines(data: &[u8]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut line_no = 0u64;
    for (index, &byte) in data.iter().enumerate() {
        if byte != b'\n' {
            continue;
        }
        line_no += 1;
        // `raw` ends before the `\n`; the +1 is that newline.
        push_line(&mut lines, &data[start..index], true, line_no);
        start = index + 1;
    }
    if start < data.len() {
        line_no += 1;
        // Last line without `\n`: the same +1, so each line weighs the same.
        push_line(&mut lines, &data[start..], false, line_no);
    }
    lines
}

/// Message text of one JSON value. `Err` is invalid JSON.
/// The first present key wins. A key with no text is `<NO-MESSAGE>`
/// and does not fall through to a later field. Unknown fields are skipped
/// without building them, so a huge number or a deep value does not fail
/// the line.
fn json_message(text: &str, message_field: Option<&str>) -> Result<String, ()> {
    let mut parser = Json::new(text.as_bytes());
    parser.message(message_field)
}

struct Json<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Json<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, index: 0 }
    }

    fn message(&mut self, message_field: Option<&str>) -> Result<String, ()> {
        self.skip_ws();
        let found = if self.peek() == Some(b'{') {
            self.bump();
            self.object_message(message_field)?
        } else {
            self.skip_value()?;
            None
        };
        self.skip_ws();
        if self.index != self.bytes.len() {
            return Err(());
        }
        Ok(found.unwrap_or_else(|| NO_MESSAGE.to_owned()))
    }

    /// `None` when no chosen key is present. `Some(None)` when the key is
    /// present but its value is not a string. `Some(Some(text))` for a string.
    /// A later duplicate key replaces the earlier one. `message`, `msg`, `log`
    /// and a named field all work that way: the last value of that key is kept.
    fn object_message(&mut self, message_field: Option<&str>) -> Result<Option<String>, ()> {
        let mut message = None;
        let mut msg = None;
        let mut log = None;
        let mut custom = None;
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(pick(message_field, message, msg, log, custom));
        }
        loop {
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.string_or_skip()?;
            if let Some(name) = message_field {
                if key == name {
                    custom = Some(value);
                }
            } else if key == "message" {
                message = Some(value);
            } else if key == "msg" {
                msg = Some(value);
            } else if key == "log" {
                log = Some(value);
            }
            self.skip_ws();
            match self.bump() {
                Some(b'}') => return Ok(pick(message_field, message, msg, log, custom)),
                Some(b',') => {
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        return Err(());
                    }
                }
                _ => return Err(()),
            }
        }
    }

    fn string_or_skip(&mut self) -> Result<Option<String>, ()> {
        if self.peek() == Some(b'"') {
            Ok(Some(self.parse_string()?))
        } else {
            self.skip_value()?;
            Ok(None)
        }
    }

    fn skip_value(&mut self) -> Result<(), ()> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => self.literal(b"null"),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'"') => {
                self.parse_string()?;
                Ok(())
            }
            Some(b'-' | b'0'..=b'9') => self.skip_number(),
            Some(b'{' | b'[') => self.skip_nested(),
            _ => Err(()),
        }
    }

    fn skip_nested(&mut self) -> Result<(), ()> {
        let mut frames = Vec::new();
        self.push_frame(&mut frames)?;
        while !frames.is_empty() {
            self.skip_ws();
            let byte = self.peek().ok_or(())?;
            let frame = frames.last().ok_or(())?;
            let object = frame.object;
            let phase = frame.phase;
            let allow_empty = frame.allow_empty;
            let close = if object { b'}' } else { b']' };
            if byte == close {
                let trailing = (phase == Phase::Value || phase == Phase::Key) && !allow_empty;
                if trailing || phase == Phase::Colon {
                    return Err(());
                }
                self.bump();
                frames.pop();
                if let Some(parent) = frames.last_mut() {
                    parent.phase = Phase::After;
                    parent.allow_empty = false;
                }
                continue;
            }
            match phase {
                Phase::Key => {
                    if byte != b'"' {
                        return Err(());
                    }
                    self.parse_string()?;
                    frames.last_mut().ok_or(())?.phase = Phase::Colon;
                }
                Phase::Colon => {
                    self.skip_ws();
                    self.expect(b':')?;
                    let frame = frames.last_mut().ok_or(())?;
                    frame.phase = Phase::Value;
                    frame.allow_empty = false;
                }
                Phase::Value => {
                    if byte == b'{' || byte == b'[' {
                        self.push_frame(&mut frames)?;
                    } else {
                        self.skip_value()?;
                        let frame = frames.last_mut().ok_or(())?;
                        frame.phase = Phase::After;
                        frame.allow_empty = false;
                    }
                }
                Phase::After => {
                    if byte != b',' {
                        return Err(());
                    }
                    self.bump();
                    let frame = frames.last_mut().ok_or(())?;
                    frame.allow_empty = false;
                    frame.phase = if object { Phase::Key } else { Phase::Value };
                }
            }
        }
        Ok(())
    }

    fn push_frame(&mut self, frames: &mut Vec<Frame>) -> Result<(), ()> {
        let object = match self.bump() {
            Some(b'{') => true,
            Some(b'[') => false,
            _ => return Err(()),
        };
        frames.push(Frame {
            object,
            phase: if object { Phase::Key } else { Phase::Value },
            allow_empty: true,
        });
        Ok(())
    }

    fn skip_number(&mut self) -> Result<(), ()> {
        if self.peek() == Some(b'-') {
            self.bump();
        }
        match self.bump() {
            Some(b'0') => {}
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.bump();
                }
            }
            _ => return Err(()),
        }
        if self.peek() == Some(b'.') {
            self.bump();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        Ok(())
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), ()> {
        if !self.bytes[self.index..].starts_with(word) {
            return Err(());
        }
        self.index += word.len();
        if matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
        ) {
            return Err(());
        }
        Ok(())
    }

    fn parse_string(&mut self) -> Result<String, ()> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let ch = self.bump_char()?;
            match ch {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c if (c as u32) < 0x20 => return Err(()),
                c => out.push(c),
            }
        }
    }

    fn escape(&mut self) -> Result<char, ()> {
        match self.bump_char()? {
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '/' => Ok('/'),
            'b' => Ok('\u{0008}'),
            'f' => Ok('\u{000c}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.unicode_escape(),
            _ => Err(()),
        }
    }

    fn unicode_escape(&mut self) -> Result<char, ()> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bump_char()? != '\\' || self.bump_char()? != 'u' {
                return Err(());
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(());
            }
            let cp = 0x10000 + (((unit - 0xD800) << 10) | (low - 0xDC00));
            return char::from_u32(cp).ok_or(());
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(());
        }
        char::from_u32(unit).ok_or(())
    }

    fn hex4(&mut self) -> Result<u32, ()> {
        let mut value = 0u32;
        for _ in 0..4 {
            let ch = self.bump_char()?;
            let digit = match ch {
                '0'..='9' => ch as u32 - '0' as u32,
                'a'..='f' => ch as u32 - 'a' as u32 + 10,
                'A'..='F' => ch as u32 - 'A' as u32 + 10,
                _ => return Err(()),
            };
            value = (value << 4) | digit;
        }
        Ok(value)
    }

    fn bump_char(&mut self) -> Result<char, ()> {
        // One character only. Validating the whole remainder on every call
        // made skipping a long unused string quadratic.
        let rest = self.bytes.get(self.index..).ok_or(())?;
        if rest.is_empty() {
            return Err(());
        }
        let width = match rest[0] {
            0x00..=0x7F => 1,
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Err(()),
        };
        if rest.len() < width {
            return Err(());
        }
        let ch = std::str::from_utf8(&rest[..width])
            .map_err(|_| ())?
            .chars()
            .next()
            .ok_or(())?;
        self.index += ch.len_utf8();
        Ok(ch)
    }

    fn expect(&mut self, byte: u8) -> Result<(), ()> {
        if self.bump() == Some(byte) {
            Ok(())
        } else {
            Err(())
        }
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }
}

fn pick(
    message_field: Option<&str>,
    message: Option<Option<String>>,
    msg: Option<Option<String>>,
    log: Option<Option<String>>,
    custom: Option<Option<String>>,
) -> Option<String> {
    let found = if message_field.is_some() {
        custom
    } else {
        message.or(msg).or(log)
    };
    found.map(|text| text.unwrap_or_else(|| NO_MESSAGE.to_owned()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Key,
    Colon,
    Value,
    After,
}

struct Frame {
    object: bool,
    phase: Phase,
    allow_empty: bool,
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
        // Mixed scripts in UTF-16LE: "abc日" has 3 of 4 odd bytes NUL.
        assert_eq!(
            detect_wide(b"a\x00b\x00c\x00\xE5\x65"),
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
        // One NUL every 4 bytes is half of one parity: still UTF-8.
        assert_eq!(detect_wide(&b"abc\x00".repeat(50)), None);
        assert_eq!(detect_wide(&b"AB\x00C".repeat(50)), None);
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
    fn text_bom_is_skipped_and_counted() {
        // 3 BOM bytes + `disk full` (9) + `\n` = 13. The second line is 10.
        // The mark is not part of the text, so both lines are `disk full`.
        let lines = text_lines(b"\xEF\xBB\xBFdisk full\ndisk full\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "disk full");
        assert_eq!(lines[1].text, "disk full");
        assert_eq!(lines[0].raw, "disk full");
        assert_eq!(lines[0].bytes, 13);
        assert_eq!(lines[1].bytes, 10);
        // A mark on a later line stays in that line.
        let later = text_lines(b"disk full\n\xEF\xBB\xBFdisk full\n");
        assert_eq!(later[0].text, "disk full");
        assert!(later[1].text.starts_with('\u{feff}'));
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

    #[test]
    fn extreme_unknown_json_fields_still_count() {
        // A number past f64, and 200 nested arrays, sit in a field that is not
        // the message. Both lines still count.
        let huge = "{\"msg\":\"ok\",\"x\":1e400}\n";
        let mut deep = String::from("{\"msg\":\"ok\",\"x\":");
        deep.push_str(&"[".repeat(200));
        deep.push('1');
        deep.push_str(&"]".repeat(200));
        deep.push_str("}\n");
        let mut raw = huge.as_bytes().to_vec();
        raw.extend(deep.as_bytes());
        let lines = json_lines(&raw, None).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "ok");
        assert_eq!(lines[1].text, "ok");
        assert!(!lines[0].raw.contains("not a number"));
    }

    #[test]
    fn message_matches_ordinary_json() {
        let samples = [
            r#"{"message":"hallo","msg":"nee","log":"ook"}"#,
            r#"{"msg":"hallo","log":"nee"}"#,
            r#"{"log":"xy"}"#,
            r#"{"note":"eigen","message":"ander"}"#,
            r#"{"level":"info","host":"a"}"#,
            r#"{"message":{"raw":"x"},"user":"y"}"#,
            r#"["x"]"#,
            r#"{"message":""}"#,
            r#"{"message":null}"#,
            r#"{"message":"a\nb\t\"c"}"#,
            r#"{"message":"\u0041\u00e9"}"#,
            r#"{"message":"\uD83D\uDE00"}"#,
            "  { \"msg\" : \"ok\" }  ",
            r#"{"message":"a","message":"b"}"#,
            r#"{"log":"first","message":"wins"}"#,
            "1",
            "true",
            "null",
            r#"{"msg":"ok","x":[1,{"a":true},null]}"#,
            r#"{"message":"a\/b"}"#,
        ];
        for sample in samples {
            let value: serde_json::Value = serde_json::from_str(sample).unwrap();
            for field in [None, Some("note"), Some("message")] {
                let expected = serde_message(&value, field);
                let got = super::json_message(sample, field).unwrap();
                assert_eq!(got, expected, "{field:?} {sample}");
            }
        }
        assert!(super::json_message(r#"{"message":"\uD800"}"#, None).is_err());
        assert!(super::json_message(r#"{"a":1,}"#, None).is_err());
        assert!(super::json_message("01", None).is_err());
    }

    fn serde_message(value: &serde_json::Value, message_field: Option<&str>) -> String {
        if let Some(name) = message_field {
            return value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<NO-MESSAGE>")
                .to_owned();
        }
        for name in ["message", "msg", "log"] {
            if value.get(name).is_some() {
                return value
                    .get(name)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<NO-MESSAGE>")
                    .to_owned();
            }
        }
        "<NO-MESSAGE>".to_owned()
    }

    #[test]
    fn duplicate_message_key_keeps_the_later_value() {
        // The later `message` wins, including when `msg` sits between them.
        // A named field does the same. `message` still beats a later `msg`.
        assert_eq!(
            super::json_message(r#"{"message":"alpha","message":"beta"}"#, None).unwrap(),
            "beta"
        );
        assert_eq!(
            super::json_message(
                r#"{"msg":"alpha","message":"beta","message":"gamma"}"#,
                None
            )
            .unwrap(),
            "gamma"
        );
        assert_eq!(
            super::json_message(r#"{"message":"alpha","msg":"beta"}"#, None).unwrap(),
            "alpha"
        );
        assert_eq!(
            super::json_message(r#"{"note":"alpha","note":"beta"}"#, Some("note")).unwrap(),
            "beta"
        );
    }

    #[test]
    fn skipping_an_unused_string_is_linear() {
        fn once(n: usize) -> std::time::Duration {
            let mut body = String::with_capacity(n + 32);
            body.push_str(r#"{"message":"ok","x":""#);
            body.extend(std::iter::repeat_n('a', n));
            body.push_str("\"}\n");
            let start = std::time::Instant::now();
            let lines = json_lines(body.as_bytes(), None).unwrap();
            let elapsed = start.elapsed();
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].text, "ok");
            elapsed
        }
        let _ = once(8_000);
        let n = 40_000;
        let small = (0..3).map(|_| once(n)).min().expect("timing");
        let large = (0..3).map(|_| once(n * 8)).min().expect("timing");
        let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
        // Eight times the bytes. Linear stays near 8. Quadratic is near 64.
        assert!(
            ratio < 40.0,
            "skip grew {ratio:.1}× from {small:?} to {large:?}"
        );
    }
}
