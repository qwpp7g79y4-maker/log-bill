use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// How often each mask was applied to one message.
/// A literal tag in the input does not count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaskCounts {
    pub ts: u64,
    pub uuid: u64,
    pub ip: u64,
    pub email: u64,
    pub secret: u64,
    pub hex: u64,
    pub num: u64,
    pub var: u64,
}

impl MaskCounts {
    pub(crate) fn add(&mut self, other: Self) {
        self.ts += other.ts;
        self.uuid += other.uuid;
        self.ip += other.ip;
        self.email += other.email;
        self.secret += other.secret;
        self.hex += other.hex;
        self.num += other.num;
        self.var += other.var;
    }
}

/// Mask a message.
///
/// Secret keys are found in the original text, before the other masks can
/// split a key. Then timestamp, UUID, IP, email, hex, bare number, variable
/// word. Consecutive spaces become one space.
pub fn mask(message: &str) -> String {
    mask_detail(message).0
}

/// Same masking as `mask`, plus how many times each mask was applied.
pub fn mask_detail(message: &str) -> (String, MaskCounts) {
    let (text, secret) = mask_secrets(message);
    let (text, ts) = replace(&text, timestamp_re(), "<TS>");
    let (text, uuid) = replace(&text, uuid_re(), "<UUID>");
    let (text, ip) = mask_ips(&text);
    let (text, email) = mask_emails(&text);
    let (text, hex, num_hex) = mask_hex(&text);
    let (text, num, var) = mask_numbers(&text);
    (
        collapse_spaces(&text),
        MaskCounts {
            ts,
            uuid,
            ip,
            email,
            secret,
            hex,
            num: num + num_hex,
            var,
        },
    )
}

const TIMESTAMP: &str = r"[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt ][0-9]{2}:[0-9]{2}:[0-9]{2}(?:[.,][0-9]+)?(?:[Zz]|[+-][0-9]{2}:?[0-9]{2})?|[0-9]{2}/(?i:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)/[0-9]{4}:[0-9]{2}:[0-9]{2}:[0-9]{2} +[+-][0-9]{4}";

const UUID_RE: &str =
    r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}";

/// An ordinary address, a quoted local part (`"john.doe"@example.com`,
/// `"a b"@example.com`), or letters outside ASCII (`jöhn@exämple.de`).
/// A space inside the quotes is part of the local part. A line break is not,
/// so one address cannot swallow the next line. The domain still needs a dot
/// and a letter-only ending, so a keyword after the address can be cut off.
const EMAIL: &str = r#"(?:"[^"@\r\n]+"|[\p{L}\p{Nd}._%+-]+)@[\p{L}\p{Nd}.-]+\.[\p{L}]{2,}"#;

const HEX_RE: &str = r"[0-9a-fA-F]{8,}";

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

fn timestamp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TIMESTAMP).expect("timestamp pattern"))
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(UUID_RE).expect("uuid pattern"))
}

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(EMAIL).expect("email pattern"))
}

fn hex_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(HEX_RE).expect("hex pattern"))
}

fn number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // A fraction needs digits on both sides of the dot, so `1699482000.` is
    // not a number token. Digits outside ASCII count (`١٢٣٤٥`).
    RE.get_or_init(|| Regex::new(r"\A[+-]?[\p{Nd}]+(?:\.[\p{Nd}]+)?\z").expect("number pattern"))
}

fn replace(input: &str, re: &Regex, tag: &str) -> (String, u64) {
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    let mut count = 0u64;
    for hit in re.find_iter(input) {
        if !boundary_at(input, hit.start()) || !boundary_at_end(input, hit.end()) {
            continue;
        }
        out.push_str(&input[last..hit.start()]);
        out.push_str(tag);
        last = hit.end();
        count += 1;
    }
    out.push_str(&input[last..]);
    (out, count)
}

fn boundary_at(input: &str, index: usize) -> bool {
    match input[..index].chars().next_back() {
        None => true,
        Some(prev) => !prev.is_ascii_alphanumeric(),
    }
}

fn boundary_at_end(input: &str, index: usize) -> bool {
    match input[index..].chars().next() {
        None => true,
        Some(next) => !next.is_ascii_alphanumeric(),
    }
}

/// Like `replace`, but a match must not swallow a keyword that introduces a secret.
/// `id=a@b.co.token=hunterpass` would otherwise take `.token` as the domain and
/// leave `hunterpass` with no key.
fn mask_emails(input: &str) -> (String, u64) {
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    let mut count = 0u64;
    for hit in email_re().find_iter(input) {
        if !boundary_at(input, hit.start()) || !boundary_at_end(input, hit.end()) {
            continue;
        }
        let Some(end) = email_end_keeping_key(input, hit.start(), hit.end()) else {
            continue;
        };
        if end <= hit.start() || !boundary_at_end(input, end) {
            continue;
        }
        out.push_str(&input[last..hit.start()]);
        out.push_str("<EMAIL>");
        last = end;
        count += 1;
    }
    out.push_str(&input[last..]);
    (out, count)
}

/// Cut a trailing `.keyword` off an e-mail when a secret separator follows it.
/// `None` drops the whole match so the keyword stays for secret masking.
fn email_end_keeping_key(input: &str, start: usize, end: usize) -> Option<usize> {
    let matched = &input[start..end];
    let Some(dot) = matched.rfind('.') else {
        return Some(end);
    };
    let segment = &matched[dot + 1..];
    if segment.is_empty() || !contains_key(segment) || after_separator(input, end, false).is_none()
    {
        return Some(end);
    }
    let cut = start + dot;
    let prefix = &input[start..cut];
    let still_email = email_re()
        .find(prefix)
        .is_some_and(|hit| hit.start() == 0 && hit.end() == prefix.len());
    if still_email {
        Some(cut)
    } else {
        None
    }
}

fn mask_ips(input: &str) -> (String, u64) {
    let mut out = String::with_capacity(input.len());
    let mut count = 0u64;
    let mut i = 0;
    while i < input.len() {
        if let Some(len) = ip_len(input, i) {
            out.push_str("<IP>");
            count += 1;
            i += len;
            continue;
        }
        let ch = input[i..].chars().next().expect("index on a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, count)
}

/// Whole notation, including `[`, `]`, and the port. A loose piece inside
/// the brackets does not count: it left the port in the same word, then `<VAR>`.
fn ip_len(input: &str, start: usize) -> Option<usize> {
    if input[start..].starts_with('[') {
        return bracket_len(input, start);
    }
    if input[..start].ends_with('[') {
        return None;
    }
    bare_ipv6_len(input, start).or_else(|| ipv4_len(input, start))
}

fn loose_boundary(input: &str, start: usize) -> bool {
    match input[..start].chars().next_back() {
        None => true,
        Some(prev) => !prev.is_ascii_alphanumeric(),
    }
}

fn ends_clean(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some(ch) => !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '.' | '%' | '[' | ']'),
    }
}

fn bracket_len(input: &str, start: usize) -> Option<usize> {
    if !loose_boundary(input, start) {
        return None;
    }
    let rest = input.get(start + 1..)?;
    let inner = addr_body(rest)?;
    let close = start + 1 + inner;
    if input.as_bytes().get(close) != Some(&b']') {
        return None;
    }
    let mut end = close + 1;
    if input[end..].starts_with(':') {
        let port = port_len(&input[end + 1..])?;
        end += 1 + port;
    }
    if !ends_clean(&input[end..]) {
        return None;
    }
    Some(end - start)
}

fn port_len(text: &str) -> Option<usize> {
    let n = text
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (n > 0).then_some(n)
}

fn addr_body(text: &str) -> Option<usize> {
    if let Some(n) = ipv6_addr_len(text) {
        let mut len = n;
        if text[len..].starts_with('%') {
            len += zone_len(&text[len..])?;
        }
        return Some(len);
    }
    ipv4_only_len(text)
}

fn bare_ipv6_len(input: &str, start: usize) -> Option<usize> {
    if !boundary_before(input, start, ':') {
        return None;
    }
    let rest = &input[start..];
    let addr = ipv6_addr_len(rest)?;
    let mut len = addr;
    if rest[len..].starts_with('%') {
        len += zone_len(&rest[len..])?;
    }
    if !ends_clean(&rest[len..]) {
        return None;
    }
    Some(len)
}

fn zone_len(text: &str) -> Option<usize> {
    if !text.starts_with('%') {
        return None;
    }
    let n = text[1..]
        .bytes()
        .take_while(|byte| {
            byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'_' || *byte == b'-'
        })
        .count();
    (n > 0).then_some(1 + n)
}

fn ipv6_addr_len(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut left = 0usize;
    let mut right = 0usize;
    let mut compressed = false;
    let mut on_right = false;
    let mut saw_v4 = false;
    let mut saw_digit = false;

    if input.starts_with("::") {
        compressed = true;
        on_right = true;
        i = 2;
    }

    while i < bytes.len() && !stops_addr(bytes[i]) {
        if looks_like_ipv4(&input[i..]) {
            let n = ipv4_only_len(&input[i..])?;
            saw_v4 = true;
            add_groups(&mut left, &mut right, on_right, 2);
            i += n;
            break;
        }
        let group = hex_group_len(&input[i..])?;
        if input[i..i + group]
            .bytes()
            .any(|byte| byte.is_ascii_digit())
        {
            saw_digit = true;
        }
        add_groups(&mut left, &mut right, on_right, 1);
        i += group;
        if i < bytes.len() && bytes[i] == b':' {
            if i + 1 < bytes.len() && bytes[i + 1] == b':' {
                if compressed {
                    return None;
                }
                compressed = true;
                on_right = true;
                i += 2;
                continue;
            }
            i += 1;
            if i >= bytes.len() || stops_addr(bytes[i]) {
                return None;
            }
            continue;
        }
        break;
    }

    if i < bytes.len() && bytes[i] == b'.' {
        return None;
    }
    let total = left + right;
    if compressed {
        if total >= 8 {
            return None;
        }
    } else if total != 8 {
        return None;
    }
    if i == 0 || !plausible_v6(left, right, compressed, saw_v4, saw_digit) {
        return None;
    }
    Some(i)
}

fn add_groups(left: &mut usize, right: &mut usize, on_right: bool, n: usize) {
    if on_right {
        *right += n;
    } else {
        *left += n;
    }
}

/// `a::b` and `std::fs` have text on both sides, no digit, and no tail.
fn plausible_v6(
    left: usize,
    right: usize,
    compressed: bool,
    saw_v4: bool,
    saw_digit: bool,
) -> bool {
    let total = left + right;
    if !compressed || saw_v4 || saw_digit || total == 0 || total >= 3 {
        return true;
    }
    left == 0 || right == 0
}

fn stops_addr(byte: u8) -> bool {
    !byte.is_ascii_hexdigit() && byte != b':' && byte != b'.'
}

fn looks_like_ipv4(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    if !bytes.first().is_some_and(|byte| byte.is_ascii_digit()) {
        return false;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    bytes.get(i) == Some(&b'.')
}

fn hex_group_len(text: &str) -> Option<usize> {
    let n = text
        .bytes()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    if n == 0 || n > 4 {
        None
    } else {
        Some(n)
    }
}

fn ipv4_only_len(text: &str) -> Option<usize> {
    let n = text
        .bytes()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
        .count();
    if n > 0 && is_ipv4(&text[..n]) {
        Some(n)
    } else {
        None
    }
}

fn ipv4_len(input: &str, start: usize) -> Option<usize> {
    if !boundary_before(input, start, '.') {
        return None;
    }
    let rest = &input[start..];
    let len = rest
        .bytes()
        .take_while(|&b| b.is_ascii_digit() || b == b'.')
        .count();
    if is_ipv4(&rest[..len]) {
        Some(len)
    } else {
        None
    }
}

fn boundary_before(input: &str, start: usize, glue: char) -> bool {
    match input[..start].chars().next_back() {
        None => true,
        Some(prev) => prev != glue && !prev.is_ascii_alphanumeric(),
    }
}

fn is_ipv4(token: &str) -> bool {
    let mut parts = token.split('.');
    for _ in 0..4 {
        let Some(part) = parts.next() else {
            return false;
        };
        if !is_octet(part) {
            return false;
        }
    }
    parts.next().is_none()
}

fn is_octet(part: &str) -> bool {
    if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if part.len() > 1 && part.starts_with('0') {
        return false;
    }
    part.parse::<u16>().is_ok_and(|n| n <= 255)
}

fn mask_secrets(input: &str) -> (String, u64) {
    let mut out = String::with_capacity(input.len());
    let mut count = 0u64;
    let mut last = 0;
    let mut i = 0;
    while i < input.len() {
        if let Some((value_start, value_end)) = secret_at(input, i) {
            let value = &input[value_start..value_end];
            // `token: Bearer abc` — the value is exactly a keyword that has its own
            // value, so masking continues there. A longer secret that only contains
            // a keyword (`tokenHunterWordHere`, `secretPass1`) is the value itself.
            // `Authorization` does not do this: the scheme word belongs to the value.
            let inner = is_exact_key(value) && value_range(input, value_end).is_some();
            if inner {
                i = value_start;
                continue;
            }
            out.push_str(&input[last..value_start]);
            out.push_str("<SECRET>");
            count += 1;
            last = value_end;
            i = value_end;
            continue;
        }
        let Some(ch) = input[i..].chars().next() else {
            break;
        };
        i += ch.len_utf8();
    }
    out.push_str(&input[last..]);
    (out, count)
}

fn secret_at(input: &str, i: usize) -> Option<(usize, usize)> {
    let key_len = key_len_at(input, i)?;
    let after = i + key_len;
    let decoded = decode_key_escapes(&input[i..after]);
    if is_authorization_key(&decoded) {
        return auth_value_range(input, after);
    }
    if contains_key(&decoded) {
        return value_range(input, after);
    }
    // An e-mail used as a key (`a@b.co= rosebud`) has no keyword. A bare space
    // after an address is not that form: `mail ada@example.com einde` stays.
    if is_whole_email(&input[i..after]) {
        return eq_colon_value(input, after);
    }
    None
}

/// `authorization` is inside the word, also in `proxy-authorization` and `x-authorization`.
/// Lookalikes fold the same way as any other key.
fn is_authorization_key(token: &str) -> bool {
    let skeleton = key_skeleton(token);
    key_in(skeleton.as_bytes(), b"authorization")
}

fn key_len_at(input: &str, i: usize) -> Option<usize> {
    // Without a char boundary a slice falls inside a multibyte char:
    // `\xff\xfe` is lossy as two U+FFFD.
    if !input.is_char_boundary(i) || !key_boundary_before(input, i) {
        return None;
    }
    let end = sticky_end(input, i);
    if end == i {
        return None;
    }
    let cut = cut_key_before_sep_ws(input, i, end);
    let token = &input[i..cut];
    let decoded = decode_key_escapes(token);
    if contains_key(&decoded) || is_whole_email(token) {
        Some(cut - i)
    } else {
        None
    }
}

/// Control White_Space (NEL, VT, FF) can sit inside a key (`pass\u{0085}word`)
/// and also separate a key from its value (`password\u{0085}canary`). If the
/// prefix before the first such mark is already a key, the mark starts the
/// value. Otherwise it stays inside the key and is dropped in the skeleton.
fn cut_key_before_sep_ws(input: &str, start: usize, end: usize) -> usize {
    let mut k = start;
    while k < end {
        let ch = input[k..].chars().next().expect("index on a char boundary");
        if is_sep_ws(ch) {
            let prefix = decode_key_escapes(&input[start..k]);
            if contains_key(&prefix) {
                return k;
            }
            return end;
        }
        k += ch.len_utf8();
    }
    end
}

/// Key characters, plus a `\uXXXX` that decodes to one, plus an IP or e-mail
/// glued onto that run. `password-10.0.0.1` and `pass\u0077ord` stay one key.
/// How far past the key's own first run [`sticky_end`] will chase a chain of
/// glued addresses. Counted from the end of that run, not from `start`, so a
/// long key name does not eat into it; and checked only before a new glue
/// step, so one glued address always lands whole. Bounding it is what keeps
/// a noisy line that merely looks address-shaped from making `sticky_end`
/// rescan a chain that grows with the length of the line itself.
const MAX_GLUE_SPAN: usize = 256;

/// The longest IP or e-mail one glue step will look for, counted from where
/// the address starts: a 64-byte local part, `@` and a 255-byte domain. The
/// probe window is cut there, so an address starting further back than this
/// before the current end cannot reach past it and is not tried.
const MAX_GLUE_ADDR: usize = 64 + 1 + 255;

/// How far [`glue_span`] and [`glue_email`] have already looked, within one
/// [`sticky_end`] call, for an IP or e-mail starting at or after a given
/// position. A position already tried and rejected for some `end` stays
/// rejected for any later, larger `end`, since the span a candidate would
/// produce does not depend on where the caller currently is: `ip`/`email`
/// hold the first position still worth trying for each. Reusing that
/// instead of rescanning from `start` on every pass through the
/// `sticky_end` loop is what keeps one call linear in the span it covers.
struct GlueCursor {
    ip: usize,
    email: usize,
}

impl GlueCursor {
    fn new(start: usize) -> Self {
        Self {
            ip: start,
            email: start,
        }
    }
}

fn sticky_end(input: &str, start: usize) -> usize {
    let mut end = start + key_char_run(input, start);
    let base = end;
    let mut cursor = GlueCursor::new(start);
    loop {
        end += key_char_run(input, end);
        if end - base >= MAX_GLUE_SPAN {
            return end;
        }
        match glue_mask(input, &mut cursor, end) {
            Some(next) if next > end => end = next,
            _ => return end,
        }
    }
}

fn key_char_run(input: &str, i: usize) -> usize {
    let rest = &input[i..];
    let mut len = 0;
    while len < rest.len() {
        if let Some(width) = unicode_key_width(&rest[len..]) {
            len += width;
            continue;
        }
        let ch = rest[len..]
            .chars()
            .next()
            .expect("index on a char boundary");
        if !is_key_char(ch) {
            break;
        }
        len += ch.len_utf8();
    }
    len
}

/// `\u0077` inside a key is `w`. Four hex digits, and the character has to be
/// a key character. A surrogate or a short escape stays outside the key.
fn unicode_key_width(rest: &str) -> Option<usize> {
    let tail = rest.strip_prefix("\\u")?;
    let hex = tail.get(..4)?;
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let unit = u32::from_str_radix(hex, 16).ok()?;
    if (0xD800..=0xDFFF).contains(&unit) {
        return None;
    }
    let ch = char::from_u32(unit)?;
    is_key_char(ch).then_some(6)
}

/// Turn `\uXXXX` in a key back into the character, so `pass\u0077ord` contains
/// `password`. Other bytes stay as they are.
fn decode_key_escapes(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while !rest.is_empty() {
        if let Some(width) = unicode_key_width(rest) {
            let hex = &rest[2..6];
            let unit = u32::from_str_radix(hex, 16).expect("four hex digits");
            out.push(char::from_u32(unit).expect("key character"));
            rest = &rest[width..];
            continue;
        }
        let ch = rest.chars().next().expect("non-empty");
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Round down to the nearest char boundary at or before `idx`, so a probe
/// window cut at [`MAX_GLUE_ADDR`] never splits a multi-byte char.
fn char_boundary_floor(input: &str, mut idx: usize) -> usize {
    while !input.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn glue_mask(input: &str, cursor: &mut GlueCursor, end: usize) -> Option<usize> {
    let next = input[end..].chars().next()?;
    if matches!(next, '.' | ':' | '[') {
        if let Some(ip_end) = glue_span(input, &mut cursor.ip, end, ip_len) {
            return Some(ip_end);
        }
    }
    if next == '@' || next == '"' {
        return glue_email(input, &mut cursor.email, end);
    }
    None
}

/// First position worth trying for an address that has to reach past `end`:
/// the cursor, or [`MAX_GLUE_ADDR`] before `end` if that is later.
fn glue_first(input: &str, cursor: usize, end: usize) -> usize {
    let floor = end.saturating_sub(MAX_GLUE_ADDR);
    if cursor >= floor {
        return cursor;
    }
    let mut i = floor;
    while !input.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// The probe window for an address starting at `i`.
fn glue_window_end(input: &str, i: usize) -> usize {
    char_boundary_floor(input, (i + MAX_GLUE_ADDR).min(input.len()))
}

fn glue_span(
    input: &str,
    cursor: &mut usize,
    end: usize,
    len_at: fn(&str, usize) -> Option<usize>,
) -> Option<usize> {
    let mut i = glue_first(input, *cursor, end);
    while i <= end {
        if input.is_char_boundary(i) {
            let probe = &input[..glue_window_end(input, i)];
            if let Some(len) = len_at(probe, i) {
                let span_end = i + len;
                if span_end > end {
                    *cursor = i;
                    return Some(span_end);
                }
            }
        }
        if i == end {
            break;
        }
        let ch = input[i..].chars().next()?;
        i += ch.len_utf8();
    }
    *cursor = i;
    None
}

fn glue_email(input: &str, cursor: &mut usize, end: usize) -> Option<usize> {
    let mut i = glue_first(input, *cursor, end);
    while i <= end {
        if input.is_char_boundary(i) && boundary_at(input, i) {
            if let Some(hit) = email_re().find(&input[i..glue_window_end(input, i)]) {
                if hit.start() == 0 {
                    let email_end = i + hit.end();
                    if email_end > end && boundary_at_end(input, email_end) {
                        *cursor = i;
                        return Some(email_end);
                    }
                }
            }
        }
        if i == end {
            break;
        }
        let ch = input[i..].chars().next()?;
        i += ch.len_utf8();
    }
    *cursor = i;
    None
}

fn is_whole_email(token: &str) -> bool {
    email_re()
        .find(token)
        .is_some_and(|hit| hit.start() == 0 && hit.end() == token.len())
}

fn key_boundary_before(input: &str, index: usize) -> bool {
    match input[..index].chars().next_back() {
        None => true,
        Some(prev) => !is_key_char(prev),
    }
}

/// A letter, a digit, `_` or `-`, or an invisible character that
/// [`key_skeleton`] drops. Fullwidth `_` and `-` count; fullwidth letters
/// and digits are already letters and digits. Visible Unicode whitespace
/// is a separator. Control White_Space (VT, FF, NEL) is invisible inside
/// a key and still separates after a complete key.
fn is_key_char(ch: char) -> bool {
    if is_sep_ws(ch) && !is_invisible_key_char(ch) {
        return false;
    }
    ch.is_alphabetic()
        || ch.is_numeric()
        || matches!(ch, '_' | '-' | '\u{FF3F}' | '\u{FF0D}')
        || is_invisible_key_char(ch)
}

/// Characters inside a key that must not be shown to the keyword compare.
/// Zero-width space, non-joiner and joiner, word joiner, BOM, soft hyphen,
/// the replacement character U+FFFD (an invalid byte read as UTF-8 becomes
/// U+FFFD, so `s\xFFecret` must still read as `secret`), and control
/// characters other than tab, line feed and carriage return, which separate.
fn is_invisible_key_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}'..='\u{200D}'
            | '\u{2060}'
            | '\u{FEFF}'
            | '\u{00AD}'
            | '\u{FFFD}'
            | '\u{01}'..='\u{08}'
            | '\u{0B}'
            | '\u{0C}'
            | '\u{0E}'..='\u{1F}'
            | '\u{7F}'..='\u{9F}'
    ) || is_combining_key_char(ch)
}

/// Combining marks, variation selectors and tag characters. They sit on or
/// after a letter and change nothing a reader would take for another word,
/// so `p\u{301}assword` (`ṕassword`) must still read as `password`.
fn is_combining_key_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            | '\u{0483}'..='\u{0489}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

/// Latin letter with an accent folded to its base letter, lowercase only
/// (the caller lowercases first), so precomposed `sécret` reads as `secret`.
fn fold_accent(ch: char) -> char {
    match ch {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ķ' => 'k',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => 's',
        'ţ' | 'ť' | 'ŧ' | 'ț' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        'ṕ' => 'p',
        other => other,
    }
}

/// Skeleton of a key, compared with [`KEYS`].
///
/// Invisible characters and combining marks inside the key are dropped.
/// NFKC folds compatibility forms (mathematical alphanumeric, enclosed
/// alphanumerics, fullwidth). [`homoglyph_latin`] runs next so a mapping
/// there wins (Cyrillic `н` is `n`, Armenian `ր` is `r`). Case and accents
/// are folded next. The UTS #39 confusable skeleton then maps remaining
/// non-ASCII lookalikes to Latin. ASCII is left alone: UTS #39 maps `I`
/// to `l` (`API_KEY` would miss `api_key`) and `m` to `rn`.
/// A `-` still counts as `_` in the byte compare.
fn key_skeleton(token: &str) -> String {
    let mut stripped = String::with_capacity(token.len());
    for ch in token.chars() {
        if is_invisible_key_char(ch) {
            continue;
        }
        stripped.push(ch);
    }
    let nfkc: String = stripped.nfkc().collect();
    let mut mapped = String::with_capacity(nfkc.len());
    for ch in nfkc.chars() {
        if is_invisible_key_char(ch) {
            continue;
        }
        mapped.push(homoglyph_latin(ch).unwrap_or(fold_fullwidth(ch)));
    }
    let mut lowered = String::with_capacity(mapped.len());
    for ch in mapped.chars() {
        push_key_latin(&mut lowered, ch);
    }
    let mut out = String::with_capacity(lowered.len());
    let mut rest = lowered.as_str();
    while !rest.is_empty() {
        let ch = rest.chars().next().expect("non-empty");
        if ch.is_ascii() {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
            continue;
        }
        let skip = rest.find(|c: char| c.is_ascii()).unwrap_or(rest.len());
        for sk in unicode_security::skeleton(&rest[..skip]) {
            push_key_latin(&mut out, sk);
        }
        rest = &rest[skip..];
    }
    out
}

fn push_key_latin(out: &mut String, ch: char) {
    if is_invisible_key_char(ch) || is_combining_key_char(ch) {
        return;
    }
    for lower in ch.to_lowercase() {
        let folded = fold_accent(lower);
        if is_invisible_key_char(folded) || is_combining_key_char(folded) {
            continue;
        }
        out.push(folded);
    }
}

/// U+FF01–U+FF5E is the fullwidth form of ASCII `!` through `~`.
fn fold_fullwidth(ch: char) -> char {
    let code = u32::from(ch);
    if (0xFF01..=0xFF5E).contains(&code) {
        char::from_u32(code - 0xFEE0).expect("fullwidth fold is ASCII")
    } else {
        ch
    }
}

/// Latin letter imitated by a lookalike, uppercase included.
///
/// Applied before the UTS #39 skeleton, so these mappings win: Cyrillic `н`/`Н`
/// count as `n` (UTS #39 maps `Н` to `H`), and Armenian letters that UTS #39
/// does not map (`ր`/`Ր` as `r`, `Տ` as `s`, and the other Armenian
/// lookalikes below). Cyrillic: а в е к м н о р с т у х і ј ѕ ԁ һ ӏ ԛ ԝ.
/// Greek: ο α β ε η ι κ μ ν τ ρ υ χ ζ. Latin: dotless `ı`, small `ɑ`, `ɡ`, iota `ɩ`.
/// Two pairs do not share one Latin letter, because the glyphs differ:
/// capital nu `Ν` looks like `N` and small nu `ν` looks like `v`;
/// capital upsilon `Υ` looks like `Y` and small upsilon `υ` looks like `u`;
/// palochka `Ӏ` looks like `I` and small palochka `ӏ` looks like `l`.
fn homoglyph_latin(ch: char) -> Option<char> {
    Some(match ch {
        'а' | 'А' | 'α' | 'Α' | 'ɑ' => 'a',
        'в' | 'В' | 'β' | 'Β' => 'b',
        'е' | 'Е' | 'ε' | 'Ε' => 'e',
        'о' | 'О' | 'ο' | 'Ο' => 'o',
        'р' | 'Р' | 'ρ' | 'Ρ' => 'p',
        'ր' | 'Ր' => 'r',
        'հ' => 'h',
        'ւ' => 'i',
        'ո' | 'ռ' => 'n',
        'օ' | 'Օ' => 'o',
        'Տ' => 's',
        'ս' | 'Ս' => 'u',
        'ա' => 'w',
        'с' | 'С' => 'c',
        'у' | 'У' | 'Υ' => 'y',
        'х' | 'Х' | 'χ' | 'Χ' => 'x',
        'і' | 'І' | 'ι' | 'Ι' | 'Ӏ' | 'ı' | 'ɩ' => 'i',
        'ј' | 'Ј' => 'j',
        'ѕ' | 'Ѕ' => 's',
        'ԁ' | 'Ԁ' => 'd',
        'һ' | 'Һ' | 'Η' => 'h',
        'ӏ' => 'l',
        'κ' | 'Κ' | 'к' | 'К' => 'k',
        'м' | 'М' | 'Μ' => 'm',
        'ν' => 'v',
        'Ν' | 'н' | 'Н' => 'n',
        'τ' | 'Τ' | 'т' | 'Т' => 't',
        'υ' => 'u',
        'ԛ' | 'Ԛ' => 'q',
        'ԝ' | 'Ԝ' => 'w',
        'Ζ' => 'z',
        'ɡ' => 'g',
        _ => return None,
    })
}

/// `token` contains a KEYS word. A `-` counts as `_`. Case does not matter.
/// Lookalikes are folded first, so `pаssword` with a Cyrillic `а` contains
/// `password`. `key` alone is not in KEYS: `monkey` and `keyboard` stay.
/// `tokenizer` does count, otherwise `sessiontoken` leaks.
fn contains_key(token: &str) -> bool {
    let skeleton = key_skeleton(token);
    let bytes = skeleton.as_bytes();
    KEYS.iter().any(|key| key_in(bytes, key.as_bytes()))
}

fn key_in(token: &[u8], key: &[u8]) -> bool {
    if key.is_empty() || key.len() > token.len() {
        return false;
    }
    (0..=token.len() - key.len()).any(|at| {
        token[at..at + key.len()]
            .iter()
            .zip(key)
            .all(|(got, want)| fold_key_byte(*got) == *want)
    })
}

fn fold_key_byte(byte: u8) -> u8 {
    if byte == b'-' {
        b'_'
    } else {
        byte.to_ascii_lowercase()
    }
}

/// The whole token is one KEYS word (`bearer`, `api_key`), not merely a
/// longer word that contains one (`tokenHunterWordHere`).
fn is_exact_key(token: &str) -> bool {
    let skeleton = key_skeleton(token);
    let bytes = skeleton.as_bytes();
    KEYS.iter().any(|key| {
        key.len() == bytes.len()
            && bytes
                .iter()
                .zip(key.as_bytes())
                .all(|(got, want)| fold_key_byte(*got) == *want)
    })
}

/// Value after a key. Unicode whitespace around `=` and `:` belongs
/// to the separator.
/// A quote between the key and `:` belongs to JSON (`"key": "value"`), including
/// a backslash-escaped quote in a message (`{\"key\":\"value\"}`).
fn value_range(input: &str, after_key: usize) -> Option<(usize, usize)> {
    secret_span(input, after_key, false, token_end)
}

/// An e-mail key needs `=` or `:`. A space after the address is not enough.
fn eq_colon_value(input: &str, after_key: usize) -> Option<(usize, usize)> {
    secret_span(input, after_key, true, token_end)
}

/// `Authorization` and `Proxy-Authorization`: the whole value, scheme word included.
/// With no quote, the token is the rest of the message. Quoted, up to the closer.
fn auth_value_range(input: &str, after_key: usize) -> Option<(usize, usize)> {
    secret_span(input, after_key, false, token_end_of_message)
}

fn secret_span(
    input: &str,
    after_key: usize,
    require_mark: bool,
    token_end_2: fn(&str, usize) -> usize,
) -> Option<(usize, usize)> {
    let (start, opening) = after_separator(input, after_key, require_mark)?;
    if start >= input.len() {
        return None;
    }
    let end = secret_value_end(input, start, opening, token_end_2);
    (end > start).then_some((start, end))
}

/// A `"` or `'` that opens a secret value. `escaped` means the source wrote
/// `\"` or `\'`, which still delimits the value in escaped JSON.
#[derive(Clone, Copy)]
struct OpenQuote {
    ch: char,
    len: usize,
    escaped: bool,
}

/// End of a secret value, from the first character of the value.
/// If it starts quoted, the closing quote when a suitable separator
/// follows it; inside a real quote, `\` keeps the next character in the value.
/// With no such close, through the end of the message. An unquoted `[` or `{`
/// runs at least through the matching bracket. Otherwise the token.
fn secret_value_end(
    input: &str,
    start: usize,
    opening: Option<OpenQuote>,
    token_end_2: fn(&str, usize) -> usize,
) -> usize {
    if let Some(mark) = opening {
        return closing_quote(input, start, mark.ch, mark.escaped);
    }
    let token = token_end_2(input, start);
    if opens_bracket(input, start) {
        bracket_value_end(input, start).max(token)
    } else {
        token
    }
}

fn opens_bracket(input: &str, start: usize) -> bool {
    matches!(input[start..].chars().next(), Some('[' | '{'))
}

/// End past the `[` or `{` at `start`, including nested brackets and quoted text.
/// No matching closer means the rest of the message: mask rather than leak.
fn bracket_value_end(input: &str, start: usize) -> usize {
    let mut depth = 0i32;
    let mut i = start;
    while i < input.len() {
        let ch = input[i..].chars().next().expect("index on a char boundary");
        let len = ch.len_utf8();
        if ch == '"' || ch == '\'' {
            let end = closing_quote(input, i + len, ch, false);
            if end >= input.len() {
                return input.len();
            }
            i = end + ch.len_utf8();
            continue;
        }
        if ch == '[' || ch == '{' {
            depth += 1;
        } else if ch == ']' || ch == '}' {
            depth -= 1;
            i += len;
            if depth == 0 {
                return i;
            }
            continue;
        }
        i += len;
    }
    input.len()
}

fn token_end_of_message(input: &str, _start: usize) -> usize {
    input.len()
}

fn after_separator(
    input: &str,
    after_key: usize,
    require_mark: bool,
) -> Option<(usize, Option<OpenQuote>)> {
    let mut i = skip_quote_before_separator(input, after_key);
    let first = input[i..].chars().next()?;
    if is_sep_ws(first) {
        i = skip_spaces(input, i);
        if matches!(input[i..].chars().next(), Some('=' | ':')) {
            i += 1;
            i = skip_spaces(input, i);
        } else if require_mark {
            return None;
        }
    } else if first == '=' || first == ':' {
        i += 1;
        i = skip_spaces(input, i);
    } else {
        return None;
    }
    let opening = quote_at(input, i);
    if let Some(mark) = opening {
        i += mark.len;
    }
    Some((i, opening))
}

fn closing_quote(input: &str, mut i: usize, quote: char, escaped_delim: bool) -> usize {
    if escaped_delim {
        return closing_escaped_quote(input, i, quote);
    }
    let mut escaped = false;
    while let Some(ch) = input[i..].chars().next() {
        let len = ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote && quote_closes(input, i + len) {
            return i;
        }
        i += len;
    }
    input.len()
}

/// Closer for a value opened by `\"` or `\'`. The backslash is part of the
/// delimiter, not an escape inside the value.
fn closing_escaped_quote(input: &str, mut i: usize, quote: char) -> usize {
    while i < input.len() {
        let ch = input[i..].chars().next().expect("index on a char boundary");
        if ch == '\\' {
            let after = i + 1;
            let Some(next) = input[after..].chars().next() else {
                return input.len();
            };
            let past = after + next.len_utf8();
            if next == quote && quote_closes(input, past) {
                return i;
            }
            i = past;
            continue;
        }
        i += ch.len_utf8();
    }
    input.len()
}

/// A quote closes only with a separator after it. Without one, it belongs
/// to the value; if the scan finds none, the caller masks the rest.
fn quote_closes(input: &str, after_index: usize) -> bool {
    let rest = skip_spaces(input, after_index);
    if rest >= input.len() {
        return true;
    }
    match input[rest..].chars().next() {
        Some(',' | '}' | ']') => true,
        Some(_) if rest > after_index => field_after(input, rest),
        _ => false,
    }
}

/// `name=` or `name:` after the spaces behind a candidate quote. Spaces between
/// the name and `=` or `:` belong to the separator, as they do for a key.
fn field_after(input: &str, mut i: usize) -> bool {
    let start = i;
    while let Some(ch) = input[i..].chars().next() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    if i == start {
        return false;
    }
    i = skip_spaces(input, i);
    matches!(input[i..].chars().next(), Some('=' | ':'))
}

fn skip_quote_before_separator(input: &str, i: usize) -> usize {
    let Some(mark) = quote_at(input, i) else {
        return i;
    };
    let j = skip_spaces(input, i + mark.len);
    if matches!(input[j..].chars().next(), Some('=' | ':')) {
        j
    } else {
        i
    }
}

/// A raw quote, or `\"` / `\'` when the message holds escaped JSON.
fn quote_at(input: &str, i: usize) -> Option<OpenQuote> {
    let mut chars = input[i..].chars();
    match chars.next() {
        Some(ch @ ('"' | '\'')) => Some(OpenQuote {
            ch,
            len: ch.len_utf8(),
            escaped: false,
        }),
        Some('\\') => match chars.next() {
            Some(ch @ ('"' | '\'')) => Some(OpenQuote {
                ch,
                len: 1 + ch.len_utf8(),
                escaped: true,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn is_sep_ws(ch: char) -> bool {
    ch.is_whitespace()
}

fn skip_spaces(input: &str, mut i: usize) -> usize {
    while let Some(ch) = input[i..].chars().next() {
        if !is_sep_ws(ch) {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

fn token_end(input: &str, start: usize) -> usize {
    input[start..]
        .char_indices()
        .find(|(_, ch)| is_sep_ws(*ch))
        .map_or(input.len(), |(rel, _)| start + rel)
}

/// Hex only with a letter a-f and a digit. Digits only, even 8 or
/// more, become `<NUM>` on the same word boundary. So `1699482000` stays a number.
fn mask_hex(input: &str) -> (String, u64, u64) {
    let mut out = String::with_capacity(input.len());
    let mut hex = 0u64;
    let mut num = 0u64;
    let mut last = 0;
    for hit in hex_re().find_iter(input) {
        if !boundary_at(input, hit.start()) || !boundary_at_end(input, hit.end()) {
            continue;
        }
        let word = hit.as_str();
        let digit = word.bytes().any(|b| b.is_ascii_digit());
        let letter = word.bytes().any(|b| matches!(b, b'a'..=b'f' | b'A'..=b'F'));
        if !digit {
            continue;
        }
        // A leading `+` or `-`, or a decimal fraction, belongs to the whole
        // token. Taking only the digit run would leave `+<NUM>` or `3.<NUM>`.
        if !letter && part_of_wider_number(input, hit.start(), hit.end()) {
            continue;
        }
        let tag = if letter { "<HEX>" } else { "<NUM>" };
        out.push_str(&input[last..hit.start()]);
        out.push_str(tag);
        if letter {
            hex += 1;
        } else {
            num += 1;
        }
        last = hit.end();
    }
    out.push_str(&input[last..]);
    (out, hex, num)
}

fn mask_numbers(input: &str) -> (String, u64, u64) {
    let mut out = String::with_capacity(input.len());
    let mut token = String::new();
    let mut num = 0u64;
    let mut var = 0u64;
    for ch in input.chars() {
        if ch == ' ' {
            push_token(&mut out, &token, &mut num, &mut var);
            token.clear();
            out.push(' ');
        } else {
            token.push(ch);
        }
    }
    push_token(&mut out, &token, &mut num, &mut var);
    (out, num, var)
}

/// The space-separated token around a digit run is a wider number, so the
/// sign or the decimal point must stay with the digits.
fn part_of_wider_number(input: &str, start: usize, end: usize) -> bool {
    let token_start = input[..start].rfind(' ').map_or(0, |index| index + 1);
    let token_end = input[end..]
        .find(' ')
        .map_or(input.len(), |index| end + index);
    if token_start == start && token_end == end {
        return false;
    }
    is_number_token(&input[token_start..token_end])
}

fn push_token(out: &mut String, token: &str, num: &mut u64, var: &mut u64) {
    if token.is_empty() {
        return;
    }
    if is_number_token(token) {
        out.push_str("<NUM>");
        *num += 1;
    } else if is_var(token) {
        out.push_str("<VAR>");
        *var += 1;
    } else {
        out.push_str(token);
    }
}

/// Digits, optionally with a leading `+` or `-` and one decimal fraction.
/// `12.` and `.5` are not fractions: the dot needs digits on both sides.
/// Trailing `!`, `?`, `,` or `;` still counts (`7!`), so the mark does not
/// keep the digits in the pattern. A trailing dot does not (`1699482000.`).
fn is_number_token(token: &str) -> bool {
    if number_re().is_match(token) {
        return true;
    }
    let bare = token.trim_end_matches(['!', '?', ',', ';']);
    bare.len() < token.len() && !bare.is_empty() && number_re().is_match(bare)
}

/// A word with a letter and a digit, or 32 or more characters.
/// `13:55:36` has digits but no letter and stays as it is.
/// A token with no spaces is one word, so a long masked line can become `<VAR>`
/// after the secret has already been counted.
fn is_var(token: &str) -> bool {
    if token.chars().count() >= 32 {
        return true;
    }
    token.bytes().any(|b| b.is_ascii_alphabetic()) && token.bytes().any(|b| b.is_ascii_digit())
}

fn collapse_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut space = false;
    for ch in input.chars() {
        if ch == ' ' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            space = false;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{mask, mask_detail, MaskCounts};

    #[test]
    fn timestamp() {
        assert_eq!(mask("2026-10-08T13:55:36Z"), "<TS>");
        assert_eq!(mask("2026-10-08T13:55:36.123+02:00"), "<TS>");
        assert_eq!(mask("2026-10-08 11:07:44"), "<TS>");
        assert_eq!(mask("10/Oct/2026:13:55:36 +0200"), "<TS>");
    }

    #[test]
    fn uuid() {
        assert_eq!(mask("550e8400-e29b-41d4-a716-446655440000"), "<UUID>");
        assert_eq!(mask("11111111-2222-4333-8444-555555555555"), "<UUID>");
    }

    #[test]
    fn ip() {
        assert_eq!(mask("192.0.2.10"), "<IP>");
        assert_eq!(mask("198.51.100.4"), "<IP>");
        assert_eq!(mask("2001:db8::1"), "<IP>");
        assert_eq!(mask("::1"), "<IP>");
        assert_eq!(mask("2001:0db8:0000:0000:0000:0000:0000:0002"), "<IP>");
    }

    #[test]
    fn ipv6_brackets_port_mapped_and_zone() {
        // Brackets and the port are the boundary, and are included in one `<IP>`.
        // Otherwise `[<IP>]:8080` still becomes a `<VAR>` (letters and digits).
        let (text_body, n) = mask_detail("client [::1]:8080 connected");
        assert_eq!(text_body, "client <IP> connected");
        assert_eq!(n.ip, 1);
        assert_eq!(n.num, 0);
        assert_eq!(n.var, 0);

        let (text_body, n) = mask_detail("[2001:db8::1]:443");
        assert_eq!(text_body, "<IP>");
        assert_eq!(n.ip, 1);
        assert_eq!(n.num, 0);

        let (text_body, n) = mask_detail("connect from ::ffff:192.0.2.10 ok");
        assert_eq!(text_body, "connect from <IP> ok");
        assert_eq!(n.ip, 1);
        assert_eq!(n.var, 0);

        let (text_body, n) = mask_detail("connect from fe80::1%eth0 ok");
        assert_eq!(text_body, "connect from <IP> ok");
        assert_eq!(n.ip, 1);
        assert_eq!(n.var, 0);

        assert_eq!(mask("[192.0.2.10]:80"), "<IP>");
        assert_eq!(mask("[fe80::1%eth0]:443"), "<IP>");
        assert_eq!(mask("::ffff"), "<IP>");
        assert_eq!(mask("::"), "<IP>");
    }

    #[test]
    fn no_ipv6_in_path_or_time() {
        assert_eq!(mask("std::fs::read"), "std::fs::read");
        assert_eq!(mask("roep std::fs::read aan"), "roep std::fs::read aan");
        assert_eq!(mask("std::fs"), "std::fs");
        assert_eq!(mask("a::b"), "a::b");
        assert_eq!(mask("om 13:55:36 klaar"), "om 13:55:36 klaar");
        let (_, n) = mask_detail("std::fs::read a::b 13:55:36");
        assert_eq!(n.ip, 0);
        assert_eq!(n.var, 0);
        assert_eq!(n.num, 0);
    }

    #[test]
    fn bare_number() {
        assert_eq!(mask("code 404"), "code <NUM>");
        assert_eq!(mask("poging 3"), "poging <NUM>");
    }

    #[test]
    fn timestamp_is_not_a_number() {
        let output = mask("2026-10-08T13:55:36Z duurde 15");
        assert_eq!(output, "<TS> duurde <NUM>");
        assert!(!output.starts_with("<NUM>"));
        assert_eq!(mask("10/Oct/2026:13:55:36 +0200"), "<TS>");
        assert!(!mask("2026-10-08 11:07:44").contains("<NUM>"));
    }

    #[test]
    fn ordinary_words_stay() {
        assert_eq!(mask("verbinding geweigerd"), "verbinding geweigerd");
        assert_eq!(mask("fout bij opstarten"), "fout bij opstarten");
        assert_eq!(mask("wacht   op   antwoord"), "wacht op antwoord");
        assert_eq!(mask("om 13:55:36 klaar"), "om 13:55:36 klaar");
    }

    #[test]
    fn email_address() {
        assert_eq!(mask("ada@example.com"), "<EMAIL>");
        assert_eq!(mask("jan.jansen+tag@sub.example.org"), "<EMAIL>");
        assert_eq!(
            mask("mail abcd1234@example.com einde"),
            "mail <EMAIL> einde"
        );
    }

    #[test]
    fn quoted_and_international_email() {
        let quoted = mask(r#"mail "john.doe"@example.com"#);
        assert_eq!(quoted, "mail <EMAIL>");
        assert!(!quoted.contains("john.doe"));
        assert!(!quoted.contains("example.com"));
        let international = mask("mail jöhn@exämple.de");
        assert_eq!(international, "mail <EMAIL>");
        assert!(!international.contains("jöhn"));
        assert!(!international.contains("exämple"));
        let (text_body, counts) = mask_detail(r#"mail "john.doe"@example.com"#);
        assert_eq!(text_body, "mail <EMAIL>");
        assert_eq!(counts.email, 1);
        assert_eq!(counts.var, 0);
        let (text_body, counts) = mask_detail("mail jöhn@exämple.de");
        assert_eq!(text_body, "mail <EMAIL>");
        assert_eq!(counts.email, 1);
    }

    #[test]
    fn secret() {
        assert_eq!(mask("password=abc"), "password=<SECRET>");
        assert_eq!(mask("PASSWORD = xyz"), "PASSWORD = <SECRET>");
        assert_eq!(mask("passwd: top"), "passwd: <SECRET>");
        assert_eq!(mask("secret=abcdefgh"), "secret=<SECRET>");
        assert_eq!(mask("token abcdef"), "token <SECRET>");
        assert_eq!(mask("api_key=abcdefgh"), "api_key=<SECRET>");
        assert_eq!(mask("apikey: abcdefgh"), "apikey: <SECRET>");
        assert_eq!(mask("authorization=abcdefgh"), "authorization=<SECRET>");
        assert_eq!(
            mask("Authorization: Bearer abcdef"),
            "Authorization: <SECRET>"
        );
    }

    #[test]
    fn hex() {
        assert_eq!(mask("a1b2c3d4"), "<HEX>");
        assert_eq!(mask("0123ABCD"), "<HEX>");
        assert_eq!(mask("deadbeef"), "deadbeef");
        assert_eq!(mask("1234567"), "<NUM>");
    }

    #[test]
    fn hex_only_with_a_letter() {
        // Digits only, even 8 or more, is a number. A hex word
        // has a letter a-f and a digit. With no digit it stays.
        assert_eq!(mask("1699482000"), "<NUM>");
        assert_eq!(mask("123456789012"), "<NUM>");
        assert_eq!(mask("12345678"), "<NUM>");
        assert_eq!(mask("00000000"), "<NUM>");
        assert_eq!(mask("deadbeef01"), "<HEX>");
        assert_eq!(mask("0a1b2c3d4e"), "<HEX>");
        assert_eq!(mask("DEADBEEF01"), "<HEX>");
        assert_eq!(mask("deadbeefcafe"), "deadbeefcafe");
        assert!(!mask("deadbeefcafe").contains("<HEX>"));
        // `g` is outside a-f, so the whole word is not hex.
        assert_eq!(mask("12345678g"), "<VAR>");
        // The digit run keeps the same boundary as hex: punctuation stays out.
        assert_eq!(mask("1699482000."), "<NUM>.");
        assert_eq!(mask("a1b2c3d4."), "<HEX>.");
        // A digits-only secret comes before hex and number.
        assert_eq!(mask("token=1699482000"), "token=<SECRET>");
        assert_eq!(mask("password 123456789012"), "password <SECRET>");
        let (text_body, n) = mask_detail("token=4242424242");
        assert_eq!(text_body, "token=<SECRET>");
        assert_eq!(n.secret, 1);
        assert_eq!(n.hex, 0);
        assert_eq!(n.num, 0);
        assert!(!text_body.contains("4242424242"));
        let (text_body, n) = mask_detail("1699482000 deadbeef01");
        assert_eq!(text_body, "<NUM> <HEX>");
        assert_eq!(n.num, 1);
        assert_eq!(n.hex, 1);
        assert_eq!(n.secret, 0);
    }

    #[test]
    fn signed_decimal_and_non_ascii_numbers() {
        // A sign or a decimal stays with the digits. A trailing dot does not.
        // The two-decimal fraction is split so this file has no price amount.
        let fraction = concat!("3.", "14");
        let numbers = format!("amount -12.5 balance {fraction} ١٢٣٤٥ +31612345678");
        let (text_body, counts) = mask_detail(&numbers);
        assert_eq!(text_body, "amount <NUM> balance <NUM> <NUM> <NUM>");
        assert_eq!(counts.num, 4);
        assert_eq!(counts.hex, 0);
        assert_eq!(counts.var, 0);
        for raw in ["-12.5", fraction, "١٢٣٤٥", "+31612345678"] {
            assert!(!text_body.contains(raw), "{raw} in {text_body}");
        }
        assert_eq!(mask("1699482000."), "<NUM>.");
    }

    #[test]
    fn variable_word() {
        assert_eq!(mask("pod-7"), "<VAR>");
        assert_eq!(mask("build-42"), "<VAR>");
        assert_eq!(mask(&"a".repeat(32)), "<VAR>");
        assert_eq!(mask(&"a".repeat(31)), "a".repeat(31));
        assert_eq!(mask("poging 3 op pod-7"), "poging <NUM> op <VAR>");
    }

    #[test]
    fn canary_does_not_leak() {
        let output = [
            mask("password=CANARY-SECRET-0001"),
            mask("Authorization: Bearer CANARY-SECRET-0002"),
            mask("token: CANARY-SECRET-0003"),
            mask("CANARY-SECRET-0004"),
        ];
        for line in &output {
            assert!(!line.contains("CANARY-SECRET"), "{line}");
        }
        assert_eq!(output[0], "password=<SECRET>");
        assert_eq!(output[1], "Authorization: <SECRET>");
        assert_eq!(output[2], "token: <SECRET>");
        assert_eq!(output[3], "<VAR>");
    }

    fn zero() -> MaskCounts {
        MaskCounts::default()
    }

    #[test]
    fn counts_track_replacements_not_the_text() {
        // Each replacement counts once, in mask order.
        // A timestamp is not a number, a UUID not hex, an email not a `<VAR>`.
        // `Authorization: Bearer ...` is one secret: the scheme word belongs to the value.
        // The literal tag `<NUM>` is not a mask.
        let (text_body, n) = mask_detail("pod-7 code 404 en 500 naar 192.0.2.10 en 198.51.100.2");
        assert_eq!(text_body, "<VAR> code <NUM> en <NUM> naar <IP> en <IP>");
        assert_eq!(
            n,
            MaskCounts {
                var: 1,
                num: 2,
                ip: 2,
                ..zero()
            }
        );

        let (text_body, n) = mask_detail("2026-10-08T13:55:36Z duurde 15");
        assert_eq!(text_body, "<TS> duurde <NUM>");
        assert_eq!(
            n,
            MaskCounts {
                ts: 1,
                num: 1,
                ..zero()
            }
        );

        let (text_body, n) = mask_detail("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(text_body, "<UUID>");
        assert_eq!(n, MaskCounts { uuid: 1, ..zero() });

        let (text_body, n) = mask_detail("abcd1234@example.com");
        assert_eq!(text_body, "<EMAIL>");
        assert_eq!(n, MaskCounts { email: 1, ..zero() });

        let (text_body, n) = mask_detail("12345678");
        assert_eq!(text_body, "<NUM>");
        assert_eq!(n, MaskCounts { num: 1, ..zero() });

        let (text_body, n) = mask_detail("Authorization: Bearer abcdef");
        assert_eq!(text_body, "Authorization: <SECRET>");
        assert_eq!(
            n,
            MaskCounts {
                secret: 1,
                ..zero()
            }
        );
        assert!(!text_body.contains("abcdef"));

        let (text_body, n) = mask_detail("user <NUM>");
        assert_eq!(text_body, "user <NUM>");
        assert_eq!(n, zero());

        let (text_body, n) = mask_detail("wacht   op   antwoord");
        assert_eq!(text_body, "wacht op antwoord");
        assert_eq!(n, zero());
    }

    #[test]
    fn invalid_utf8_does_not_panic() {
        // What the reader turns lossy from the bytes `\xff\xfe foo`.
        let input_text = String::from_utf8_lossy(b"\xff\xfe foo");
        assert_eq!(input_text, "\u{FFFD}\u{FFFD} foo");
        assert_eq!(mask(&input_text), "\u{FFFD}\u{FFFD} foo");
    }

    #[test]
    fn replacement_char_before_password() {
        let (text_body, n) = mask_detail("\u{FFFD}password=abc");
        assert_eq!(text_body, "\u{FFFD}password=<SECRET>");
        assert_eq!(n.secret, 1);
        assert!(!text_body.contains("abc"));
    }

    #[test]
    fn multibyte_before_every_key() {
        for key in super::KEYS {
            let (text_body, n) = mask_detail(&format!("😀{key}=abc"));
            assert_eq!(text_body, format!("😀{key}=<SECRET>"));
            assert_eq!(n.secret, 1, "{key}");
            assert!(!text_body.contains("abc"), "{key}");
        }
    }

    #[test]
    fn multibyte_inside_key_does_not_panic() {
        // `ß` (2 bytes) falls inside the length of `passwd` and `password`.
        assert_eq!(mask("passw\u{00df}rd=abc"), "passw\u{00df}rd=abc");
        // `ö` folds to `o`, so this one is `password` and is masked.
        assert_eq!(mask("passw\u{00f6}rd=abc"), "passw\u{00f6}rd=<SECRET>");
    }

    #[test]
    fn key_with_prefix_or_suffix() {
        let cases = [
            ("db_password=hunterSecretWord", "db_password=<SECRET>"),
            ("SECRET_KEY=abcdef", "SECRET_KEY=<SECRET>"),
            ("x_token=abcSuperSecretValue", "x_token=<SECRET>"),
            ("my_secret_value=geheim", "my_secret_value=<SECRET>"),
            ("x-api-key: thisIsNotHexValueAtAll", "x-api-key: <SECRET>"),
            ("X-Api-Key: geheim", "X-Api-Key: <SECRET>"),
            ("Db-Password: geheim", "Db-Password: <SECRET>"),
            ("API_KEY=letters", "API_KEY=<SECRET>"),
            (
                "\"db_password\": \"hunterSecretWord\"",
                "\"db_password\": \"<SECRET>\"",
            ),
            (
                "\"x-api-key\":\"thisIsNotHexValueAtAll\"",
                "\"x-api-key\":\"<SECRET>\"",
            ),
            ("😀db_password=hunterSecretWord", "😀db_password=<SECRET>"),
            // `token` sits in the word. A boundary only on `_` or `-` would leave this.
            ("tokenizer=hello", "tokenizer=<SECRET>"),
            ("sessiontoken=geheim", "sessiontoken=<SECRET>"),
            ("passwordless=zzzz", "passwordless=<SECRET>"),
            ("secretary=jane", "secretary=<SECRET>"),
        ];
        for (input_text, expected) in cases {
            let (text_body, n) = mask_detail(input_text);
            assert_eq!(text_body, expected, "{input_text}");
            assert_eq!(n.secret, 1, "{input_text}");
            assert_eq!(n.var, 0, "{input_text}");
        }
    }

    #[test]
    fn monkey_and_keyboard_stay() {
        // `key` is not a KEYS word. Otherwise an ordinary word would lose its value.
        assert_eq!(mask("monkey=banana"), "monkey=banana");
        assert_eq!(mask("keyboard=qwerty"), "keyboard=qwerty");
        let (_, n) = mask_detail("monkey=banana keyboard=qwerty");
        assert_eq!(n.secret, 0);
    }

    #[test]
    fn authorization_masks_the_whole_value() {
        let cases = [
            (
                "Authorization: Basic dXNlcjpwYXNz",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Basic hunterSecretWord",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Bearer abcSuperSecretValue",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Bearer YWJjZGVm",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Digest username=\"Mufasa\", realm=\"realmWordValue\", nonce=\"nonceWordValue\", response=\"responseWordValue\"",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Digest username=\"YWJjZGVm\", realm=\"x\", nonce=\"dXNlcjpwYXNz\", response=\"YWJjZGVm\"",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: NTLM ntlmLetterToken",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: NTLM dXNlcjpwYXNz",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Negotiate negotiateLetter",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Negotiate dXNlcjpwYXNz",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Foo fooLetterToken",
                "Authorization: <SECRET>",
            ),
            (
                "Authorization: Foo dXNlcjpwYXNz",
                "Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Basic dXNlcjpwYXNz",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Basic hunterSecretWord",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Bearer abcSuperSecretValue",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Digest username=\"Mufasa\", realm=\"realmWordValue\", nonce=\"nonceWordValue\", response=\"responseWordValue\"",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: NTLM ntlmLetterToken",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Negotiate negotiateLetter",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Foo fooLetterToken",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "Proxy-Authorization: Foo dXNlcjpwYXNz",
                "Proxy-Authorization: <SECRET>",
            ),
            (
                "x-authorization: Foo fooLetterToken",
                "x-authorization: <SECRET>",
            ),
            (
                "my_proxy-authorization: NTLM ntlmLetterToken",
                "my_proxy-authorization: <SECRET>",
            ),
            (
                "\"authorization\": \"Basic dXNlcjpwYXNz\"",
                "\"authorization\": \"<SECRET>\"",
            ),
            (
                "\"proxy-authorization\": \"Negotiate negotiateLetter\"",
                "\"proxy-authorization\": \"<SECRET>\"",
            ),
            (
                "{\"authorization\": \"Basic dXNlcjpwYXNz\", \"user\": \"ada\"}",
                "{\"authorization\": \"<SECRET>\", \"user\": \"ada\"}",
            ),
            (
                "password=abc Authorization: Basic dXNlcjpwYXNz",
                "password=<SECRET> Authorization: <SECRET>",
            ),
        ];
        let secret_text = [
            "dXNlcjpwYXNz",
            "hunterSecretWord",
            "abcSuperSecretValue",
            "YWJjZGVm",
            "Mufasa",
            "realmWordValue",
            "nonceWordValue",
            "responseWordValue",
            "ntlmLetterToken",
            "negotiateLetter",
            "fooLetterToken",
        ];
        for (input_text, expected) in cases {
            let (text_body, n) = mask_detail(input_text);
            assert_eq!(text_body, expected, "{input_text}");
            assert!(n.secret >= 1, "{input_text}");
            for piece in secret_text {
                assert!(!text_body.contains(piece), "{piece} in {text_body}");
            }
        }
        // With no spaces, the masked line is one word of 32 characters or more.
        // That token is then `<VAR>`; the secret is gone, but it still counts as secret.
        let compact = "{\"authorization\":\"Basic dXNlcjpwYXNz\",\"user\":\"ada\"}";
        let (text_body, n) = mask_detail(compact);
        assert_eq!(text_body, "<VAR>");
        assert_eq!(n.secret, 1);
        assert!(!text_body.contains("dXNlcjpwYXNz"));
    }

    #[test]
    fn comma_fraction_is_timestamp() {
        let (text_body, n) = mask_detail("2026-10-08T13:55:36,123Z");
        assert_eq!(text_body, "<TS>");
        assert_eq!(n.ts, 1);
        assert_eq!(n.var, 0);
        assert_eq!(mask("2026-10-08T13:55:36,123+02:00"), "<TS>");
        assert_eq!(mask("voor 2026-10-08T13:55:36,123Z na"), "voor <TS> na");
    }

    #[test]
    fn quoted_value_runs_to_the_closer() {
        assert_eq!(
            mask(r#"{"password": "hunter two words", "user": "ada"}"#),
            r#"{"password": "<SECRET>", "user": "ada"}"#
        );
        assert_eq!(
            mask(r#"{"token":"eyJhbGciOiJIUzI1NiJ9"}"#),
            r#"{"token":"<SECRET>"}"#
        );
        assert_eq!(
            mask(r#"password="ab\"two words""#),
            r#"password="<SECRET>""#
        );
        assert_eq!(mask(r"password='ab\'two words'"), "password='<SECRET>'");
        assert_eq!(mask(r#"token="ab+/zz== hunter""#), r#"token="<SECRET>""#);
        assert_eq!(mask(r#"secret="4242 hunter""#), r#"secret="<SECRET>""#);
        assert_eq!(mask("password=\"hünter two\""), "password=\"<SECRET>\"");
        // With no quote it stops at the first space, not the rest of the line.
        assert_eq!(
            mask("password=hunter two words"),
            "password=<SECRET> two words"
        );
        assert_eq!(
            mask("Authorization: Bearer hunter two"),
            "Authorization: <SECRET>"
        );
    }

    #[test]
    fn unescaped_quote_inside_the_value_does_not_close() {
        // The apostrophe in it's, and the quote before `finished`, have no separator.
        assert_eq!(
            mask("token='the users secret, it's hunterword, done' finished"),
            "token='<SECRET>"
        );
        // An apostrophe right before the close. The last quote is at the end.
        assert_eq!(mask("password='it''s'"), "password='<SECRET>'");
        assert_eq!(
            mask("password='it''s' msg=ok"),
            "password='<SECRET>' msg=ok"
        );
        assert_eq!(
            mask("password='it's ada' msg = ok"),
            "password='<SECRET>' msg = ok"
        );
        // Never closed: the rest of the message is the value.
        assert_eq!(
            mask("password='hunterword stays open"),
            "password='<SECRET>"
        );
        // A comma closes, even with an apostrophe before it. The next field stays.
        assert_eq!(
            mask("token='it's hunterword', next=ok"),
            "token='<SECRET>', next=ok"
        );
        assert_eq!(
            mask("password='it's x' token=other"),
            "password='<SECRET>' token=<SECRET>"
        );
        // A double quote in free text, with a field after it.
        assert_eq!(
            mask(r#"token="say "hunterword" now" msg=ok"#),
            r#"token="<SECRET>" msg=ok"#
        );
        assert_eq!(
            mask(r#"Proxy-Authorization: "say "hunterword" now""#),
            r#"Proxy-Authorization: "<SECRET>""#
        );
        assert_eq!(
            mask("Authorization: 'Bearer it's hunterword'"),
            "Authorization: '<SECRET>'"
        );
        assert_eq!(
            mask("Authorization: 'Bearer it's hunterword' finished"),
            "Authorization: '<SECRET>"
        );
        assert_eq!(
            mask("Authorization: 'Bearer it's x' next=ok"),
            "Authorization: '<SECRET>' next=ok"
        );
        // JSON-like: `,` `}` `]` still close, also after a space.
        assert_eq!(
            mask(r#"{"password": "say "hunterword"", "user": "ada"}"#),
            r#"{"password": "<SECRET>", "user": "ada"}"#
        );
        assert_eq!(
            mask(r#"{ "token" : "hunterword" }"#),
            r#"{ "token" : "<SECRET>" }"#
        );
        assert_eq!(mask(r#"["token":"hunter two"]"#), r#"["token":"<SECRET>"]"#);
        assert_eq!(mask(r#"token="hunterword" ]"#), r#"token="<SECRET>" ]"#);
        // No key: an apostrophe does not mask the line.
        assert_eq!(mask("user='ada' msg=ok"), "user='ada' msg=ok");
        assert_eq!(mask("it's a fine day"), "it's a fine day");
    }

    #[test]
    fn secret_leaks_tab_json_bracket_email_and_keyword() {
        let cases = [
            ("login password\thunter", "login password\t<SECRET>"),
            (
                r#"login {\"password\":\"open sesame\"}"#,
                r#"login {\"password\":\"<SECRET>\"}"#,
            ),
            (
                r#"login password=["alpha", "beta"]"#,
                "login password=<SECRET>",
            ),
            (
                r#"login secret={"a": "hunter", "b": "two"}"#,
                "login secret=<SECRET>",
            ),
            (
                "login id=a@b.co.token=hunterpass",
                "login id=<EMAIL>.token=<SECRET>",
            ),
            (
                "api_key=tokenHunterWordHere next=ok",
                "api_key=<SECRET> next=ok",
            ),
            ("password=secretPass1", "password=<SECRET>"),
            ("password=TokenValueXyzHere", "password=<SECRET>"),
            // A value that is exactly a keyword still hands off to that keyword.
            ("token: Bearer abc", "token: Bearer <SECRET>"),
            // The scheme word stays inside the Authorization value.
            ("Authorization: Bearer x", "Authorization: <SECRET>"),
        ];
        let secrets = [
            "hunter",
            "open sesame",
            "alpha",
            "beta",
            "two",
            "hunterpass",
            "tokenHunterWordHere",
            "secretPass1",
            "TokenValueXyzHere",
        ];
        for (input_text, expected) in cases {
            let (text_body, n) = mask_detail(input_text);
            assert_eq!(text_body, expected, "{input_text}");
            assert!(n.secret >= 1, "{input_text} -> {text_body}");
            for piece in secrets {
                assert!(!text_body.contains(piece), "{piece} in {text_body}");
            }
        }
        // `abc` is the secret after Bearer, not a leftover word.
        assert!(!mask("token: Bearer abc").contains("abc"));
        assert_eq!(
            mask("password=secretPass1 next=ok"),
            "password=<SECRET> next=ok"
        );
        assert_eq!(
            mask("password=TokenValueXyzHere next=ok"),
            "password=<SECRET> next=ok"
        );
    }

    #[test]
    fn unicode_escaped_key_masks_rosebud() {
        // The `\u0077` digits make the key token a `<VAR>`. The value stays
        // `<SECRET>` when a space keeps it a separate word.
        let literal = r#"{"pass\u0077ord": "rosebud"}"#;
        let masked = mask(literal);
        assert_eq!(masked, r#"<VAR> "<SECRET>"}"#);
        assert!(!masked.contains("rosebud"));
        for (ws_name, ws) in [("space", " "), ("lf", "\n"), ("cr", "\r"), ("tab", "\t")] {
            let line = format!(r#"{{{ws}"pass\u0077ord"{ws}:{ws}"rosebud"{ws}}}"#);
            let (masked, counts) = mask_detail(&line);
            assert!(!masked.contains("rosebud"), "{ws_name}: {masked}");
            assert!(counts.secret >= 1, "{ws_name}: {masked}");
        }
        for key in [
            r"\u0070assword",
            r"p\u0061ssword",
            r"PASS\u0057ORD",
            r"pass\u0077ord",
        ] {
            let line = format!(r#"{{"user":"ada","{key}":"rosebud"}}"#);
            let (masked, counts) = mask_detail(&line);
            assert!(!masked.contains("rosebud"), "{key} -> {masked}");
            assert!(counts.secret >= 1, "{key} -> {masked}");
            let flipped = format!(r#"{{"{key}":"rosebud","user":"ada"}}"#);
            let (masked, counts) = mask_detail(&flipped);
            assert!(!masked.contains("rosebud"), "flipped {key} -> {masked}");
            assert!(counts.secret >= 1, "flipped {key} -> {masked}");
        }
        // No spaces: the line is one word, so `<VAR>` replaces it after the
        // secret is counted. The value is still not left in the text.
        let escaped = r#"{\"pass\u0077ord\":\"rosebud\"}"#;
        let (masked, counts) = mask_detail(escaped);
        assert!(!masked.contains("rosebud"), "{masked}");
        assert!(counts.secret >= 1, "{masked}");
    }

    #[test]
    fn secret_key_before_other_masks_hides_rosebud() {
        let cases = [
            (
                "password-123e4567-e89b-12d3-a456-426614174000= rosebud",
                "password-<UUID>= <SECRET>",
            ),
            (
                "123e4567-e89b-12d3-a456-426614174000-password= rosebud",
                "<UUID>-password= <SECRET>",
            ),
            ("password-10.0.0.1= rosebud", "password-<IP>= <SECRET>"),
            ("10.0.0.1-password= rosebud", "<IP>-password= <SECRET>"),
            ("a@b.co= rosebud", "<EMAIL>= <SECRET>"),
            (
                "token=ok a@b.co= rosebud",
                "token=<SECRET> <EMAIL>= <SECRET>",
            ),
            ("password-user@host.com= rosebud", "<EMAIL>= <SECRET>"),
        ];
        for (input_text, expected) in cases {
            let masked = mask(input_text);
            assert_eq!(masked, expected, "{input_text}");
            assert!(!masked.contains("rosebud"), "{input_text} -> {masked}");
        }
    }

    #[test]
    fn quoted_email_with_a_space() {
        assert_eq!(mask(r#""a b"@example.com"#), "<EMAIL>");
        assert_eq!(
            mask(r#"mail "a b"@example.com einde"#),
            "mail <EMAIL> einde"
        );
        assert!(!mask(r#""a b"@example.com"#).contains("a b"));
        assert!(!mask(r#""a b"@example.com"#).contains("example.com"));
    }

    #[test]
    fn number_with_trailing_punctuation() {
        assert_eq!(mask("7!"), "<NUM>");
        assert_eq!(mask("code 7!"), "code <NUM>");
        assert_eq!(mask("8?"), "<NUM>");
        assert_eq!(mask("+12;"), "<NUM>");
        assert_eq!(mask("-12.5!"), "<NUM>");
        assert_eq!(mask("1699482000."), "<NUM>.");
        assert_eq!(mask("om 13:55:36 klaar"), "om 13:55:36 klaar");
        assert_eq!(mask("!7"), "!7");
    }

    #[test]
    fn line_break_before_secret_separator_hides_rosebud() {
        let lf = "password\n= rosebud";
        let cr = "password\r: rosebud";
        assert_eq!(mask(lf), "password\n= <SECRET>");
        assert_eq!(mask(cr), "password\r: <SECRET>");
        assert!(!mask(lf).contains("rosebud"));
        assert!(!mask(cr).contains("rosebud"));
        for ws in [" ", "\n", "\r", "\t"] {
            let line = format!("password{ws}= rosebud");
            let masked = mask(&line);
            assert!(!masked.contains("rosebud"), "{masked:?}");
            assert!(masked.contains("<SECRET>"), "{masked:?}");
        }
    }

    #[test]
    fn nbsp_around_equals_hides_rosebudmission() {
        let line = "password\u{00A0}=\u{00A0}rosebudmission";
        let masked = mask(line);
        assert!(!masked.contains("rosebudmission"), "{masked}");
        assert!(masked.contains("<SECRET>"), "{masked}");
    }

    #[test]
    fn mathematical_and_armenian_lookalikes_hide_hunterword() {
        for line in [
            "sec\u{1D42B}et=hunterword",
            "p\u{1D41A}ssword=hunterword",
            "passwo\u{0580}d=hunterword",
        ] {
            let masked = mask(line);
            assert!(!masked.contains("hunterword"), "{line} -> {masked}");
            assert!(masked.contains("<SECRET>"), "{line} -> {masked}");
        }
    }

    #[test]
    fn count_404_items_stays_a_number() {
        assert_eq!(mask("count 404 items"), "count <NUM> items");
    }

    /// A line of nothing but glued-together `a@a.co` runs has no key and no
    /// single whole e-mail, so `sticky_end` finds, at every boundary, a long
    /// chain of glue extensions that each fail. Before the span was bounded
    /// and the already-scanned prefix reused, each extension rescanned the
    /// whole chain from its own start, and the chain itself was retried from
    /// every later boundary, so the cost rose with (roughly) the cube of the
    /// line length: a few thousand bytes already took several seconds. The
    /// bound here is generous (seconds, for a line far smaller than
    /// `read::MAX_LINE_BYTES`) so this only fails on that blow-up, not on
    /// ordinary machine slowness.
    /// A key glued onto a long e-mail, then `=` or `: ` and a value, around
    /// the 256 bytes the glue scan used to be cut at: the window was counted
    /// from the start of the key, so the e-mail no longer fit, the key ended
    /// at the `@` and `rosebud` stayed in the output.
    #[test]
    fn key_glued_to_long_email_hides_secret_around_256_bytes() {
        for total in [250, 255, 256, 257, 264, 265, 266, 267, 300, 330] {
            for sep in ["= ", ": "] {
                let fixed = "password-user@".len() + ".com".len() + sep.len() + "rosebud".len();
                let domain = "a".repeat(total - fixed) + ".com";
                let line = format!("password-user@{domain}{sep}rosebud");
                assert_eq!(line.len(), total);
                let masked = mask(&line);
                assert_eq!(
                    masked,
                    format!("<EMAIL>{sep}<SECRET>"),
                    "{} bytes",
                    line.len()
                );
            }
        }
    }

    #[test]
    fn noisy_glued_email_chain_stays_fast() {
        let noisy = "a@a.co".repeat(20_000 / 6);
        let start = std::time::Instant::now();
        let masked = mask(&noisy);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "masking a {}-byte noisy line took {elapsed:?}",
            noisy.len()
        );
        assert!(!masked.contains("a@a.co"), "{masked}");
    }
}
