//! Read a log sample one line at a time.
//!
//! Text and JSON lines are accepted. A name ending in `.gz` or `.zst` is
//! unpacked first. More than [`MAX_INPUT_BYTES`] of those unpacked bytes is
//! refused. UTF-16 and UTF-32 are refused. The reader does not keep the
//! lines. SIGINT stops the read with [`ReadError::Interrupted`].

use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set from the SIGINT handler. The read loops notice it and stop.
static STOP: AtomicBool = AtomicBool::new(false);

/// `true` after SIGINT. The program then exits 130 and does not write a report.
pub fn stop_requested() -> bool {
    STOP.load(Ordering::Relaxed)
}

/// Asks the in-progress read to stop. A signal handler may call this.
/// It only stores a flag.
#[cfg(unix)]
fn request_stop() {
    STOP.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
extern "C" fn on_sigint(_signal: i32) {
    request_stop();
}

/// Installs the SIGINT handler. Without `SA_RESTART`, a blocked read returns
/// so the flag is noticed. On Windows this does nothing.
#[cfg(unix)]
pub fn install_interrupt_handler() {
    // The handler only stores an atomic. `sigaction` without `SA_RESTART`
    // makes a blocked read return `EINTR` instead of running to the end.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_sigint as *const () as libc::sighandler_t;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
    }
}

/// No SIGINT handler on this platform. Reads run to the end.
#[cfg(not(unix))]
pub fn install_interrupt_handler() {}

fn interrupted_read(error: &std::io::Error) -> Option<Result<(), ReadError>> {
    if error.kind() != std::io::ErrorKind::Interrupted {
        return None;
    }
    if stop_requested() {
        Some(Err(ReadError::Interrupted))
    } else {
        Some(Ok(()))
    }
}

/// One line. `bytes` is the raw count, not the length of `text`.
/// `text` is the message (for JSON the field, otherwise the line). `raw` is the
/// line text without a newline, for the regex; for JSON the whole line.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    /// Message. For JSON, the chosen field; otherwise the line.
    pub text: String,
    /// Line text without a newline. For JSON, the whole line, for the regex.
    pub raw: String,
    /// Raw byte count, not the length of `text`.
    pub bytes: u64,
}

/// A wide text encoding that log-bill refuses. Masking works on UTF-8 text;
/// the NUL bytes of UTF-16 and UTF-32 would split keys such as `password`
/// so that they are no longer recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideEncoding {
    /// UTF-16, little endian.
    Utf16Le,
    /// UTF-16, big endian.
    Utf16Be,
    /// UTF-32, little endian.
    Utf32Le,
    /// UTF-32, big endian.
    Utf32Be,
}

impl WideEncoding {
    /// `UTF-16LE`, `UTF-16BE`, `UTF-32LE`, or `UTF-32BE`.
    pub fn name(self) -> &'static str {
        match self {
            WideEncoding::Utf16Le => "UTF-16LE",
            WideEncoding::Utf16Be => "UTF-16BE",
            WideEncoding::Utf32Le => "UTF-32LE",
            WideEncoding::Utf32Be => "UTF-32BE",
        }
    }
}

/// Why a log sample was refused. The text has no path and no line contents.
#[derive(Debug)]
pub enum ReadError {
    /// The path could not be read.
    Unreadable,
    /// UTF-16 or UTF-32 input, with or without a byte order mark. Nothing of
    /// the file is read into patterns, so nothing of it can reach the output.
    NotUtf8 {
        /// Which wide encoding was detected.
        encoding: WideEncoding,
    },
    /// Not a regular file (FIFO, directory, socket). The path is not included.
    NotRegular,
    /// Line number in the file. The line itself is not included.
    InvalidJson {
        /// 1-based line number. The line itself is not included.
        line: u64,
    },
    /// A JSON line longer than [`MAX_LINE_BYTES`]. The line is not included.
    LineTooLong {
        /// 1-based line number. The line itself is not included.
        line: u64,
    },
    /// The log input is larger than [`MAX_INPUT_BYTES`]. Nothing of it is
    /// included. The path is not included.
    InputTooLarge,
    /// SIGINT asked the read to stop. Nothing further is written. This is
    /// not an input error: the program exits 130, not 2.
    Interrupted,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Unreadable => f.write_str("file is not readable; check the path and the read permission"),
            ReadError::NotRegular => f.write_str("file is not a regular file; pass a regular file, not a directory or device"),
            ReadError::NotUtf8 { encoding } => {
                let name = encoding.name();
                write!(
                    f,
                    "file is {name}, not UTF-8; convert it first, for example with iconv -f {name} -t UTF-8, or in PowerShell with Out-File -Encoding utf8"
                )
            }
            ReadError::InvalidJson { line } => write!(f, "invalid JSON on line {line}; each line must be one JSON object, or use --format text"),
            ReadError::LineTooLong { line } => write!(
                f,
                "line {line} is longer than 1 MiB; JSON lines that long are not read; shorten or remove that line"
            ),
            ReadError::InputTooLarge => f.write_str(
                "file is larger than 256 MiB; split it or pass a smaller sample",
            ),
            ReadError::Interrupted => f.write_str("interrupted; the report was not written"),
        }
    }
}

impl ReadError {
    /// Next step, shown only with extra context (`--verbose` or a terminal).
    /// The text has no path and no bytes from the file.
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Unreadable => "check the path and the read permission",
            Self::NotRegular => "pass a regular file, not a directory or device",
            Self::NotUtf8 { .. } => "convert the file to UTF-8 and run again",
            Self::InvalidJson { .. } => "fix that line, or use --format text",
            Self::LineTooLong { .. } => "shorten or remove that line",
            Self::InputTooLarge => "split it or pass a smaller sample",
            Self::Interrupted => "run the command again",
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
/// More than [`MAX_INPUT_BYTES`] is refused. A plain file is refused from its
/// size, before it is read. A `.gz` or `.zst` file is refused from the
/// unpacked size.
pub fn visit_text(path: &Path, visit: impl FnMut(Line)) -> Result<(), ReadError> {
    visit_file(path, false, None, visit)
}

/// [`visit_text`] on standard input, for the input path `-`.
/// Reading stops with an error once more than [`MAX_INPUT_BYTES`] are seen.
pub fn visit_text_stdin(visit: impl FnMut(Line)) -> Result<(), ReadError> {
    visit_reader(io::stdin().lock(), false, None, visit)
}

const NO_MESSAGE: &str = "<NO-MESSAGE>";

/// Reads JSON lines one at a time. Bytes are those of the whole line. The
/// message comes from `message_field` (`--message-field`), else the first
/// present field of `message`, `msg`, and `log`. With no message text it is
/// `<NO-MESSAGE>`. Unknown fields are ignored, including a number that does
/// not fit in a binary float and a value nested hundreds of times. Invalid
/// JSON stops with the line number, without the line. One UTF-8 byte order
/// mark at the start is skipped for parsing but still counts in the bytes of
/// the first line. UTF-16 and UTF-32 are refused. More than [`MAX_INPUT_BYTES`]
/// is refused. A plain file is refused from its size, before it is read. A
/// `.gz` or `.zst` file is refused from the unpacked size.
pub fn visit_jsonl(
    path: &Path,
    message_field: Option<&str>,
    visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    visit_file(path, true, message_field, visit)
}

/// [`visit_jsonl`] on standard input, for the input path `-`.
/// Reading stops with an error once more than [`MAX_INPUT_BYTES`] are seen.
pub fn visit_jsonl_stdin(
    message_field: Option<&str>,
    visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    visit_reader(io::stdin().lock(), true, message_field, visit)
}

/// Why a path could not be opened as a regular file.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The path could not be read.
    Unreadable,
    /// Not a regular file. The path is not included.
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

/// A log name ending in `.gz` or `.zst`. Other names, including `.log`, are
/// the bytes on disk. Price files and rules files do not use this.
#[derive(Clone, Copy)]
enum LogCompression {
    Gzip,
    Zstd,
}

fn log_compression(path: &Path) -> Option<LogCompression> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("gz") => Some(LogCompression::Gzip),
        Some("zst") => Some(LogCompression::Zstd),
        _ => None,
    }
}

/// The reader passed to the line loop. Compressed variants are boxed so the
/// enum stays small: the decoders hold their own windows on the heap.
enum LogSource {
    Plain(File),
    Gzip(Box<flate2::read::MultiGzDecoder<File>>),
    Zstd(Box<ruzstd::decoding::StreamingDecoder<File, ruzstd::decoding::FrameDecoder>>),
}

impl Read for LogSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(file) => file.read(buf),
            Self::Gzip(decoder) => decoder.read(buf),
            Self::Zstd(decoder) => decoder.read(buf),
        }
    }
}

fn visit_file(
    path: &Path,
    jsonl: bool,
    message_field: Option<&str>,
    visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    let file = match open_regular(path) {
        Ok(file) => file,
        Err(OpenError::Unreadable) => return Err(ReadError::Unreadable),
        Err(OpenError::NotRegular) => return Err(ReadError::NotRegular),
    };
    let source = open_log(path, file)?;
    visit_reader(source, jsonl, message_field, visit)
}

/// Plain files are refused from the size on disk. `.gz` and `.zst` are
/// unpacked, and the limit applies to those bytes, so the totals match the
/// unpacked file. A broken frame is [`ReadError::Unreadable`]. The message
/// stays the fixed text: no path and no bytes from the file.
fn open_log(path: &Path, mut file: File) -> Result<LogSource, ReadError> {
    match log_compression(path) {
        None => {
            refuse_oversized(&file)?;
            Ok(LogSource::Plain(file))
        }
        Some(LogCompression::Gzip) => {
            // Every gzip member, so a file made by concatenating members
            // matches `gzip -dc`.
            Ok(LogSource::Gzip(Box::new(
                flate2::read::MultiGzDecoder::new(file),
            )))
        }
        Some(LogCompression::Zstd) => {
            // One frame. The window is checked before the decoder allocates it.
            let limits = zstd_header_limits(&mut file)?;
            if limits.content.is_some_and(|size| size > MAX_INPUT_BYTES) {
                return Err(ReadError::InputTooLarge);
            }
            // A window above the input limit is not allocated. The unpacked
            // log would not need it; refusing keeps a bad frame from reserving
            // a huge buffer.
            if limits.window > MAX_INPUT_BYTES {
                return Err(ReadError::Unreadable);
            }
            let decoder =
                ruzstd::decoding::StreamingDecoder::new(file).map_err(|_| ReadError::Unreadable)?;
            Ok(LogSource::Zstd(Box::new(decoder)))
        }
    }
}

/// Unpacked size, when the frame header states it, and the window the
/// decoder would allocate. `content` is absent when the header omits it
/// (`0` from the decoder is that same unknown, not an empty frame).
#[derive(Debug)]
struct ZstdLimits {
    content: Option<u64>,
    window: u64,
}

/// Reads the zstd frame header, then puts the file back at the start so the
/// decoder sees the same bytes. A header that is not a zstd frame is `None`:
/// the decoder then refuses it. A zstd magic with a header we cannot size is
/// an error, so a huge window is never allocated.
fn zstd_header_limits(file: &mut File) -> Result<ZstdLimits, ReadError> {
    let mut buf = [0u8; 18];
    let filled = read_head(file, &mut buf)?;
    match zstd_frame_limits(&buf[..filled]) {
        ZstdHeader::NotZstd => Ok(ZstdLimits {
            content: None,
            window: 0,
        }),
        ZstdHeader::Limits(limits) => Ok(limits),
        ZstdHeader::Refuse => Err(ReadError::Unreadable),
    }
}

fn read_head(file: &mut File, buf: &mut [u8]) -> Result<usize, ReadError> {
    let mut filled = 0;
    while filled < buf.len() {
        if stop_requested() {
            return Err(ReadError::Interrupted);
        }
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) => match interrupted_read(&err) {
                Some(Ok(())) => continue,
                Some(Err(error)) => return Err(error),
                None => return Err(ReadError::Unreadable),
            },
        }
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ReadError::Unreadable)?;
    Ok(filled)
}

#[derive(Debug)]
enum ZstdHeader {
    NotZstd,
    Limits(ZstdLimits),
    Refuse,
}

/// Zstd frame magic, little-endian `0xFD2FB528`.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

fn zstd_frame_limits(header: &[u8]) -> ZstdHeader {
    if header.len() < 4 || header[..4] != ZSTD_MAGIC {
        return ZstdHeader::NotZstd;
    }
    if header.len() < 5 {
        return ZstdHeader::Refuse;
    }
    let desc = header[4];
    // Bit 3 is reserved. A set bit is not a frame we size.
    if desc & 0b0000_1000 != 0 {
        return ZstdHeader::Refuse;
    }
    let fcs_flag = desc >> 6;
    let single = desc & 0b0010_0000 != 0;
    let did_len: usize = match desc & 0b0000_0011 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    let fcs_len: usize = match (fcs_flag, single) {
        (0, false) => 0,
        (0, true) => 1,
        (1, _) => 2,
        (2, _) => 4,
        _ => 8,
    };
    let mut index = 5;
    let window_byte = if single {
        None
    } else {
        if header.len() < index + 1 {
            return ZstdHeader::Refuse;
        }
        let byte = header[index];
        index += 1;
        Some(byte)
    };
    if header.len() < index + did_len + fcs_len {
        return ZstdHeader::Refuse;
    }
    index += did_len;
    let content = if fcs_len == 0 {
        None
    } else {
        let mut size = 0u64;
        for step in 0..fcs_len {
            size |= u64::from(header[index + step]) << (8 * step);
        }
        if fcs_len == 2 {
            size += 256;
        }
        Some(size)
    };
    let window = if single {
        content.unwrap_or(0)
    } else {
        zstd_window(window_byte.unwrap_or(0))
    };
    ZstdHeader::Limits(ZstdLimits { content, window })
}

/// Window size from a window descriptor. The spec's exponent is 5 bits, so
/// the shift stays inside `u64`.
fn zstd_window(byte: u8) -> u64 {
    let exponent = u32::from(byte >> 3);
    let mantissa = u64::from(byte & 7);
    let window_log = 10 + exponent;
    let base = 1u64 << window_log;
    let add = (base / 8) * mantissa;
    base + add
}

/// A regular file larger than [`MAX_INPUT_BYTES`] is refused from its size,
/// so the bytes are not read. A stream has no size; [`add_input`] stops that.
fn refuse_oversized(file: &File) -> Result<(), ReadError> {
    if file
        .metadata()
        .is_ok_and(|meta| meta.len() > MAX_INPUT_BYTES)
    {
        return Err(ReadError::InputTooLarge);
    }
    Ok(())
}

/// Sniffs the first [`SNIFF_BYTES`] for a wide encoding, then streams the
/// sniffed bytes followed by the rest of `reader`. Working on the sniffed
/// prefix plus the remainder, instead of seeking back to the start, lets
/// this run on anything that implements [`Read`], including standard input.
fn visit_reader(
    reader: impl Read,
    jsonl: bool,
    message_field: Option<&str>,
    visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    let mut reader = reader;
    let prefix = read_prefix(&mut reader)?;
    refuse_wide(&prefix)?;
    let chained = Cursor::new(prefix).chain(reader);
    if jsonl {
        stream_json(chained, message_field, visit)
    } else {
        stream_text(chained, visit)
    }
}

fn read_prefix(reader: &mut impl Read) -> Result<Vec<u8>, ReadError> {
    let mut buf = vec![0u8; SNIFF_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        if stop_requested() {
            return Err(ReadError::Interrupted);
        }
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) => match interrupted_read(&err) {
                Some(Ok(())) => continue,
                Some(Err(error)) => return Err(error),
                None => return Err(ReadError::Unreadable),
            },
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Longest part of one line that is kept in memory: 1 MiB. A longer text
/// line is cut there for patterns and masking; all of its bytes still count.
/// A longer JSON line is refused, because cut JSON cannot be parsed.
pub const MAX_LINE_BYTES: usize = 1 << 20;

/// Largest log input that is read: 256 MiB. The measured sample of 100 MiB
/// (104857600 bytes) still fits. Exactly this many bytes is still read.
/// One byte more is [`ReadError::InputTooLarge`], exit 2, not a new code.
pub const MAX_INPUT_BYTES: u64 = 268_435_456;

/// Counts bytes actually read. A missing final newline does not add a byte:
/// that extra byte belongs to the line weight, not to the file size.
fn add_input(seen: &mut u64, content: u64, newline: bool) -> Result<(), ReadError> {
    let next = seen
        .checked_add(content)
        .and_then(|sum| sum.checked_add(u64::from(newline)))
        .ok_or(ReadError::InputTooLarge)?;
    if next > MAX_INPUT_BYTES {
        return Err(ReadError::InputTooLarge);
    }
    *seen = next;
    Ok(())
}

/// Reads one line up to the next `\n`, which is consumed but not stored.
/// At most [`MAX_LINE_BYTES`] of it go into `buf`; the rest is skipped, so a
/// huge line cannot fill memory. Returns the content bytes of the whole line
/// (newline not included), whether a newline ended it, and whether it was cut.
/// `(0, false, _)` means end of file.
fn read_capped(
    reader: &mut impl BufRead,
    buf: &mut Vec<u8>,
) -> Result<(u64, bool, bool), ReadError> {
    let mut content = 0u64;
    let mut cut = false;
    let mut newline = false;
    loop {
        if stop_requested() {
            return Err(ReadError::Interrupted);
        }
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(err) => match interrupted_read(&err) {
                Some(Ok(())) => continue,
                Some(Err(error)) => return Err(error),
                None => return Err(ReadError::Unreadable),
            },
        };
        if chunk.is_empty() {
            break;
        }
        let end = chunk.iter().position(|&byte| byte == b'\n');
        let len = end.unwrap_or(chunk.len());
        let room = MAX_LINE_BYTES - buf.len();
        if len > room {
            cut = true;
        }
        buf.extend_from_slice(&chunk[..len.min(room)]);
        content += len as u64;
        let used = if end.is_some() { len + 1 } else { len };
        reader.consume(used);
        if end.is_some() {
            newline = true;
            break;
        }
        // Stop a line that already passed the input limit. The caller refuses
        // it, and the rest of a huge stream is not read.
        if content > MAX_INPUT_BYTES {
            break;
        }
    }
    Ok((content, newline, cut))
}

fn stream_text(source: impl Read, mut visit: impl FnMut(Line)) -> Result<(), ReadError> {
    let mut reader = std::io::BufReader::new(source);
    let mut buf = Vec::new();
    let mut line_no = 0u64;
    let mut seen = 0u64;
    loop {
        if stop_requested() {
            return Err(ReadError::Interrupted);
        }
        buf.clear();
        let (content, newline, cut) = read_capped(&mut reader, &mut buf)?;
        if content == 0 && !newline {
            break;
        }
        add_input(&mut seen, content, newline)?;
        line_no += 1;
        // A cut line keeps its `\r` out of reach; nothing is stripped then.
        if let Some(line) = text_line_weighed(&buf, newline && !cut, line_no, content + 1) {
            visit(line);
        }
    }
    if stop_requested() {
        return Err(ReadError::Interrupted);
    }
    Ok(())
}

fn stream_json(
    source: impl Read,
    message_field: Option<&str>,
    mut visit: impl FnMut(Line),
) -> Result<(), ReadError> {
    let mut reader = std::io::BufReader::new(source);
    let mut buf = Vec::new();
    let mut line_no = 0u64;
    let mut seen = 0u64;
    loop {
        if stop_requested() {
            return Err(ReadError::Interrupted);
        }
        buf.clear();
        let (content, newline, cut) = read_capped(&mut reader, &mut buf)?;
        if content == 0 && !newline {
            break;
        }
        add_input(&mut seen, content, newline)?;
        line_no += 1;
        if cut {
            return Err(ReadError::LineTooLong { line: line_no });
        }
        if let Some(line) = json_line(&buf, newline, line_no, message_field)? {
            visit(line);
        }
    }
    if stop_requested() {
        return Err(ReadError::Interrupted);
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
/// taken for a wide encoding. Those NUL bytes are removed before masking.
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
/// Fullwidth ASCII (U+FF01-U+FF5E), visible Unicode whitespace other than
/// space, tab, LF and CR, and the small form variants of ASCII punctuation
/// (U+FE50-U+FE6B) become plain ASCII there too, so `password＝value`,
/// `token：value` and `password\u{00A0}=\u{00A0}value` separate like `=` and `:`.
/// Control White_Space (VT, FF, NEL) is left for the masker: inside a key
/// it is dropped, around `=` / `:` it still separates.
fn without_nul(text: String) -> String {
    if text
        .chars()
        .any(|ch| ch == '\0' || ascii_form(ch).is_some())
    {
        text.chars()
            .filter(|&ch| ch != '\0')
            .map(|ch| ascii_form(ch).unwrap_or(ch))
            .collect()
    } else {
        text
    }
}

/// Plain ASCII for a fullwidth, Unicode-whitespace or small-form character.
fn ascii_form(ch: char) -> Option<char> {
    if ch.is_whitespace()
        && !matches!(
            ch,
            ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}'
        )
    {
        return Some(' ');
    }
    let code = u32::from(ch);
    match code {
        0xFF01..=0xFF5E => char::from_u32(code - 0xFEE0),
        _ => Some(match ch {
            '\u{FE50}' => ',',
            '\u{FE52}' => '.',
            '\u{FE54}' => ';',
            '\u{FE55}' => ':',
            '\u{FE56}' => '?',
            '\u{FE57}' => '!',
            '\u{FE59}' => '(',
            '\u{FE5A}' => ')',
            '\u{FE5B}' => '{',
            '\u{FE5C}' => '}',
            '\u{FE5F}' => '#',
            '\u{FE60}' => '&',
            '\u{FE61}' => '*',
            '\u{FE62}' => '+',
            '\u{FE63}' => '-',
            '\u{FE64}' => '<',
            '\u{FE65}' => '>',
            '\u{FE66}' => '=',
            '\u{FE68}' => '\\',
            '\u{FE69}' => '\u{24}',
            '\u{FE6A}' => '%',
            '\u{FE6B}' => '@',
            _ => return None,
        }),
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

#[cfg(test)]
fn text_line(raw: &[u8], had_newline: bool, line_no: u64) -> Option<Line> {
    text_line_weighed(raw, had_newline, line_no, (raw.len() + 1) as u64)
}

/// [`text_line`] with the weight given, for a line cut at [`MAX_LINE_BYTES`].
fn text_line_weighed(raw: &[u8], had_newline: bool, line_no: u64, weight: u64) -> Option<Line> {
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
        bytes: weight,
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
    use super::{
        add_input, detect_wide, json_lines, text_lines, Line, ReadError, WideEncoding,
        MAX_INPUT_BYTES,
    };

    #[test]
    fn input_limit_allows_the_measured_100_mib_and_refuses_one_extra_byte() {
        assert_eq!(MAX_INPUT_BYTES, 268_435_456);
        const {
            assert!(104_857_600 < MAX_INPUT_BYTES);
        }
        let mut seen = MAX_INPUT_BYTES - 1;
        add_input(&mut seen, 1, false).expect("one byte under the limit");
        assert_eq!(seen, MAX_INPUT_BYTES);
        let mut on_limit = MAX_INPUT_BYTES;
        let error = add_input(&mut on_limit, 0, true).expect_err("one byte over");
        assert!(matches!(error, ReadError::InputTooLarge));
        assert_eq!(
            error.to_string(),
            "file is larger than 256 MiB; split it or pass a smaller sample"
        );
        assert_eq!(on_limit, MAX_INPUT_BYTES);
    }

    #[test]
    fn gz_and_zst_lines_match_the_unpacked_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut plain = Vec::new();
        super::visit_text(&root.join("packed.log"), |line| {
            plain.push((line.text, line.raw, line.bytes));
        })
        .expect("plain");
        assert!(plain.len() > 1, "fixture has lines");
        for name in ["packed.log.gz", "packed.log.zst"] {
            let mut got = Vec::new();
            super::visit_text(&root.join(name), |line| {
                got.push((line.text, line.raw, line.bytes));
            })
            .unwrap_or_else(|err| panic!("{name}: {err}"));
            assert_eq!(got, plain, "{name}");
        }
    }

    #[test]
    fn zstd_window_over_the_input_limit_is_not_read() {
        // Exponent 23 → windowLog 33 → 8 GiB. The frame is only a header.
        let header = [0x28, 0xB5, 0x2F, 0xFD, 0x00, 23 << 3];
        match super::zstd_frame_limits(&header) {
            super::ZstdHeader::Limits(limits) => {
                assert!(limits.content.is_none());
                assert!(limits.window > MAX_INPUT_BYTES);
            }
            other => panic!("header was not sized: {other:?}"),
        }
        let mut file = tempfile::Builder::new()
            .suffix(".zst")
            .tempfile()
            .expect("temp");
        std::io::Write::write_all(&mut file, &header).expect("write");
        let err = super::visit_text(file.path(), |_| panic!("a huge window was read")).unwrap_err();
        assert!(matches!(err, ReadError::Unreadable), "{err}");
        assert!(!err.to_string().contains("zst"), "{err}");
    }

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
        assert_eq!(
            err.to_string(),
            "invalid JSON on line 3; each line must be one JSON object, or use --format text"
        );
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

    #[cfg(not(coverage))]
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
