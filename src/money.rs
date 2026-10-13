use std::cmp::Ordering;
use std::fmt;

/// 1 GB = 1_000_000_000 bytes, unless the price file sets `gb_bytes`.
pub(crate) const GB_BYTES: u128 = 1_000_000_000;
const SECONDS_PER_DAY: u128 = 86_400;

/// Time window of the sample. Seconds are always at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    seconds: u64,
}

impl Window {
    pub fn seconds(self) -> u64 {
        self.seconds
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct WindowError;

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid window")
    }
}

impl std::error::Error for WindowError {}

/// `--window`: a positive integer and `s`, `m`, `h`, or `d`.
/// `0m`, `5x`, an empty string, and a unit that is not one ASCII letter
/// (`1é`) are invalid. The split is on a character, so a multibyte unit
/// is a usage error and not a panic.
pub fn parse_window(text: &str) -> Result<Window, WindowError> {
    let mut chars = text.chars();
    let Some(unit) = chars.next_back() else {
        return Err(WindowError);
    };
    let num = chars.as_str();
    if num.is_empty() || !num.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(WindowError);
    }
    let value: u64 = num.parse().map_err(|_| WindowError)?;
    if value == 0 {
        return Err(WindowError);
    }
    let unit_seconds: u64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        _ => return Err(WindowError),
    };
    let seconds = value.checked_mul(unit_seconds).ok_or(WindowError)?;
    Ok(Window { seconds })
}

/// Bytes and GB per day and per month. GB is in thousandths: 1440 is 1.440 GB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    pub bytes_per_day: u128,
    pub bytes_per_month: u128,
    pub gb_per_day_milli: u128,
    pub gb_per_month_milli: u128,
}

/// An intermediate product of the GB or cost calculation does not fit.
#[derive(Debug, PartialEq, Eq)]
pub struct CalcError;

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("calculation overflow")
    }
}

impl std::error::Error for CalcError {}

fn checked_mul(left: u128, right: u128) -> Result<u128, CalcError> {
    left.checked_mul(right).ok_or(CalcError)
}

/// Factor per day = 86400 / window seconds. A month is `days_per_month` days.
/// Bytes are integer division; GB rounds only on display, half a step away from zero.
/// `gb_bytes` is 1_000_000_000 or the value from the price file.
/// If a product overflows, that is an error and not 0.
pub fn extrapolate(
    sample_bytes: u64,
    window: Window,
    days_per_month: u32,
    gb_bytes: u128,
) -> Result<Volume, CalcError> {
    let bytes = u128::from(sample_bytes);
    let seconds = u128::from(window.seconds());
    let days = u128::from(days_per_month);
    let bytes_times_day = checked_mul(bytes, SECONDS_PER_DAY)?;
    let bytes_times_month = checked_mul(bytes_times_day, days)?;
    Ok(Volume {
        bytes_per_day: bytes_times_day / seconds,
        bytes_per_month: bytes_times_month / seconds,
        gb_per_day_milli: gb_milli(checked_mul(bytes_times_day, 1_000)?, seconds, gb_bytes)?,
        gb_per_month_milli: gb_milli(checked_mul(bytes_times_month, 1_000)?, seconds, gb_bytes)?,
    })
}

/// Thousandths of a GB. `seconds × gb_bytes` that does not fit in u128 is an error.
fn gb_milli(numer: u128, seconds: u128, gb_bytes: u128) -> Result<u128, CalcError> {
    let den = checked_mul(seconds, gb_bytes)?;
    if den == 0 {
        return Err(CalcError);
    }
    Ok(round_half_away(numer, den))
}

/// Factor as an integer when division is exact, else the fraction `86400/seconds`.
pub fn format_factor(window: Window) -> String {
    let seconds = u128::from(window.seconds());
    // Same as `is_multiple_of`, written out so older Rust toolchains build it.
    if seconds != 0 && SECONDS_PER_DAY % seconds == 0 {
        (SECONDS_PER_DAY / seconds).to_string()
    } else {
        format!("{SECONDS_PER_DAY}/{seconds}")
    }
}

/// GB with 3 decimal places and a decimal point. 1440 becomes `1.440`.
pub fn format_gb(milli: u128) -> String {
    format!("{}.{:03}", milli / 1_000, milli % 1_000)
}

/// Half a step rounds away from zero, as with the share.
fn round_half_away(numer: u128, den: u128) -> u128 {
    let mut whole = numer / den;
    if (numer % den) * 2 >= den {
        whole += 1;
    }
    whole
}

const PRICE_FIELDS: &[&str] = &[
    "currency",
    "source",
    "date",
    "ingest_per_gb",
    "index_per_gb",
    "gb_bytes",
];

/// Price per GB as a reduced fraction `numer/denom`.
/// Calculated with this fraction, without floating point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub numer: u128,
    pub denom: u128,
}

/// The user's price file. `gb_bytes` is 1_000_000_000 when the file
/// omits that field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prices {
    pub currency: String,
    pub source: String,
    pub date: String,
    pub ingest_per_gb: Price,
    pub index_per_gb: Option<Price>,
    pub gb_bytes: u128,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PriceError {
    Unreadable,
    NotJson,
    MissingField,
    EmptyField,
    Negative,
    NotNumeric,
    UnknownField,
    DuplicateKey,
    GbBytes,
    NotRegular,
}

impl fmt::Display for PriceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            PriceError::Unreadable => "price file is not readable",
            PriceError::NotJson => "price file is not JSON",
            PriceError::MissingField => "price file is missing a field",
            PriceError::EmptyField => "price file has an empty field",
            PriceError::Negative => "price file has a negative price",
            PriceError::NotNumeric => "price file has a price that is not a number",
            PriceError::UnknownField => "price file has an unknown field",
            PriceError::DuplicateKey => "price file has a duplicate key",
            PriceError::GbBytes => "gb_bytes must be a whole number greater than 0",
            PriceError::NotRegular => "price file is not a regular file",
        };
        f.write_str(text)
    }
}

impl std::error::Error for PriceError {}

/// Reads a price file. An unreadable path yields an error without that path.
pub fn load_prices(path: &std::path::Path) -> Result<Prices, PriceError> {
    let data = match crate::read::read_regular(path) {
        Ok(data) => data,
        Err(crate::read::OpenError::NotRegular) => return Err(PriceError::NotRegular),
        Err(crate::read::OpenError::Unreadable) => return Err(PriceError::Unreadable),
    };
    parse_prices(&data)
}

/// `currency`, `source`, `date`, and `ingest_per_gb` are required.
/// `index_per_gb` and `gb_bytes` may be absent. An unknown field, an empty
/// string, a negative price, a price that is not a number, or a key that
/// appears twice is invalid. A price is the exact decimal text, not a
/// binary float. The error text does not contain the file contents. One
/// UTF-8 byte order mark at the start is skipped.
pub fn parse_prices(data: &[u8]) -> Result<Prices, PriceError> {
    let data = data.strip_prefix(crate::read::UTF8_BOM).unwrap_or(data);
    let text = std::str::from_utf8(data).map_err(|_| PriceError::NotJson)?;
    let numbers = match scan_numbers(text) {
        Ok(numbers) => numbers,
        Err(ScanError::Duplicate) => return Err(PriceError::DuplicateKey),
        Err(ScanError::Invalid) => {
            let _: serde_json::Value =
                serde_json::from_str(text).map_err(|_| PriceError::NotJson)?;
            return Err(PriceError::NotJson);
        }
    };
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| PriceError::NotJson)?;
    let Some(object) = value.as_object() else {
        return Err(PriceError::NotJson);
    };
    for key in object.keys() {
        if !PRICE_FIELDS.contains(&key.as_str()) {
            return Err(PriceError::UnknownField);
        }
    }
    let gb_bytes = if object.contains_key("gb_bytes") {
        gb_field(object, "gb_bytes", number_text(&numbers, "gb_bytes"))?
    } else {
        GB_BYTES
    };
    let index_per_gb = if object.contains_key("index_per_gb") {
        Some(price_field(
            object,
            "index_per_gb",
            number_text(&numbers, "index_per_gb"),
        )?)
    } else {
        None
    };
    Ok(Prices {
        currency: text_field(object, "currency")?,
        source: text_field(object, "source")?,
        date: text_field(object, "date")?,
        ingest_per_gb: price_field(
            object,
            "ingest_per_gb",
            number_text(&numbers, "ingest_per_gb"),
        )?,
        index_per_gb,
        gb_bytes,
    })
}

fn text_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, PriceError> {
    match object.get(key) {
        None => Err(PriceError::MissingField),
        Some(serde_json::Value::Null) => Err(PriceError::EmptyField),
        Some(serde_json::Value::String(text)) if text.trim().is_empty() => {
            Err(PriceError::EmptyField)
        }
        Some(serde_json::Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(PriceError::NotJson),
    }
}

fn price_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    lexeme: Option<&str>,
) -> Result<Price, PriceError> {
    match object.get(key) {
        None => Err(PriceError::MissingField),
        Some(serde_json::Value::Null) => Err(PriceError::EmptyField),
        Some(serde_json::Value::String(text)) if text.trim().is_empty() => {
            Err(PriceError::EmptyField)
        }
        Some(serde_json::Value::Number(_)) => {
            price_from_lexeme(lexeme.ok_or(PriceError::NotNumeric)?)
        }
        Some(_) => Err(PriceError::NotNumeric),
    }
}

fn gb_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    lexeme: Option<&str>,
) -> Result<u128, PriceError> {
    match object.get(key) {
        None => return Err(PriceError::MissingField),
        Some(serde_json::Value::Null) => return Err(PriceError::EmptyField),
        Some(serde_json::Value::String(text)) if text.trim().is_empty() => {
            return Err(PriceError::EmptyField);
        }
        Some(serde_json::Value::Number(_)) => {}
        Some(_) => return Err(PriceError::NotNumeric),
    }
    let price = price_field(object, key, lexeme).map_err(|err| match err {
        PriceError::Negative => PriceError::GbBytes,
        other => other,
    })?;
    if price.denom != 1 || price.numer == 0 {
        return Err(PriceError::GbBytes);
    }
    Ok(price.numer)
}

/// The JSON number as written. A binary float would turn
/// `1000.0000000000000001` into `1000` and `0.10000000000000000001` into `1/10`.
fn price_from_lexeme(text: &str) -> Result<Price, PriceError> {
    if let Some(rest) = text.strip_prefix('-') {
        rate_from_decimal(rest)?;
        return Err(PriceError::Negative);
    }
    rate_from_decimal(text)
}

fn number_text<'a>(numbers: &'a [(String, String)], key: &str) -> Option<&'a str> {
    numbers
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, text)| text.as_str())
}

enum ScanError {
    Invalid,
    Duplicate,
}

struct Cursor<'a> {
    text: &'a str,
    i: usize,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.i..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.i += ch.len_utf8();
        Some(ch)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn string(&mut self) -> Result<String, ScanError> {
        if !self.eat('"') {
            return Err(ScanError::Invalid);
        }
        let mut out = String::new();
        loop {
            let Some(ch) = self.bump() else {
                return Err(ScanError::Invalid);
            };
            match ch {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c if (c as u32) < 0x20 => return Err(ScanError::Invalid),
                c => out.push(c),
            }
        }
    }

    fn escape(&mut self) -> Result<char, ScanError> {
        let Some(ch) = self.bump() else {
            return Err(ScanError::Invalid);
        };
        match ch {
            '"' | '\\' | '/' => Ok(ch),
            'b' => Ok('\u{0008}'),
            'f' => Ok('\u{000c}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.unicode_escape(),
            _ => Err(ScanError::Invalid),
        }
    }

    fn unicode_escape(&mut self) -> Result<char, ScanError> {
        let unit = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&unit) {
            if self.bump() != Some('\\') || self.bump() != Some('u') {
                return Err(ScanError::Invalid);
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(ScanError::Invalid);
            }
            let cp = 0x10000 + (((unit - 0xD800) << 10) | (low - 0xDC00));
            return char::from_u32(cp).ok_or(ScanError::Invalid);
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            return Err(ScanError::Invalid);
        }
        char::from_u32(unit).ok_or(ScanError::Invalid)
    }

    fn hex4(&mut self) -> Result<u32, ScanError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let ch = self.bump().ok_or(ScanError::Invalid)?;
            let digit = ch.to_digit(16).ok_or(ScanError::Invalid)?;
            value = (value << 4) | digit;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<String, ScanError> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
            }
            _ => return Err(ScanError::Invalid),
        }
        if self.peek() == Some('.') {
            self.bump();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(ScanError::Invalid);
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(ScanError::Invalid);
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }
        Ok(self.text[start..self.i].to_owned())
    }

    fn literal(&mut self, word: &str) -> Result<(), ScanError> {
        if !self.rest().starts_with(word) {
            return Err(ScanError::Invalid);
        }
        self.i += word.len();
        if self.peek().is_some_and(|ch| ch.is_ascii_alphanumeric()) {
            return Err(ScanError::Invalid);
        }
        Ok(())
    }

    fn value(&mut self) -> Result<Option<String>, ScanError> {
        self.skip_ws();
        match self.peek() {
            Some('"') => {
                self.string()?;
                Ok(None)
            }
            Some('{') => {
                self.object(false, &mut Vec::new())?;
                Ok(None)
            }
            Some('[') => {
                self.array()?;
                Ok(None)
            }
            Some('t') => {
                self.literal("true")?;
                Ok(None)
            }
            Some('f') => {
                self.literal("false")?;
                Ok(None)
            }
            Some('n') => {
                self.literal("null")?;
                Ok(None)
            }
            Some('-' | '0'..='9') => Ok(Some(self.number()?)),
            _ => Err(ScanError::Invalid),
        }
    }

    fn array(&mut self) -> Result<(), ScanError> {
        if !self.eat('[') {
            return Err(ScanError::Invalid);
        }
        self.skip_ws();
        if self.eat(']') {
            return Ok(());
        }
        loop {
            self.value()?;
            self.skip_ws();
            if self.eat(']') {
                return Ok(());
            }
            if !self.eat(',') {
                return Err(ScanError::Invalid);
            }
        }
    }

    fn object(
        &mut self,
        record: bool,
        numbers: &mut Vec<(String, String)>,
    ) -> Result<(), ScanError> {
        if !self.eat('{') {
            return Err(ScanError::Invalid);
        }
        self.skip_ws();
        if self.eat('}') {
            return Ok(());
        }
        let mut seen = Vec::new();
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if !self.eat(':') {
                return Err(ScanError::Invalid);
            }
            let lexeme = self.value()?;
            if seen.iter().any(|item| item == &key) {
                return Err(ScanError::Duplicate);
            }
            seen.push(key.clone());
            if record {
                if let Some(lexeme) = lexeme {
                    numbers.push((key, lexeme));
                }
            }
            self.skip_ws();
            if self.eat('}') {
                return Ok(());
            }
            if !self.eat(',') {
                return Err(ScanError::Invalid);
            }
        }
    }
}

/// Top-level numeric values, as written. A repeated key is an error even when
/// the second value would otherwise replace the first.
fn scan_numbers(text: &str) -> Result<Vec<(String, String)>, ScanError> {
    let mut cursor = Cursor { text, i: 0 };
    let mut numbers = Vec::new();
    cursor.skip_ws();
    cursor.object(true, &mut numbers)?;
    cursor.skip_ws();
    if cursor.i != text.len() {
        return Err(ScanError::Invalid);
    }
    Ok(numbers)
}

/// Decimal text, optionally with an `e` exponent, to a reduced fraction.
fn rate_from_decimal(text: &str) -> Result<Price, PriceError> {
    let (body, exp) = match text.find(['e', 'E']) {
        Some(index) => {
            let exp: i32 = text[index + 1..]
                .parse()
                .map_err(|_| PriceError::NotNumeric)?;
            (&text[..index], exp)
        }
        None => (text, 0),
    };
    let (whole, frac) = match body.find('.') {
        Some(index) => (&body[..index], &body[index + 1..]),
        None => (body, ""),
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PriceError::NotNumeric);
    }
    if !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PriceError::NotNumeric);
    }
    let mut numer = digits(whole)?;
    for byte in frac.bytes() {
        numer = numer.checked_mul(10).ok_or(PriceError::NotNumeric)? + u128::from(byte - b'0');
    }
    let frac_len = i32::try_from(frac.len()).map_err(|_| PriceError::NotNumeric)?;
    let scale = exp.checked_sub(frac_len).ok_or(PriceError::NotNumeric)?;
    if scale >= 0 {
        let numer = numer
            .checked_mul(pow10(scale)?)
            .ok_or(PriceError::NotNumeric)?;
        Ok(Price { numer, denom: 1 })
    } else {
        let denom = pow10(scale.checked_neg().ok_or(PriceError::NotNumeric)?)?;
        Ok(reduce(numer, denom))
    }
}

fn digits(text: &str) -> Result<u128, PriceError> {
    let mut value = 0u128;
    for byte in text.bytes() {
        if !byte.is_ascii_digit() {
            return Err(PriceError::NotNumeric);
        }
        value = value
            .checked_mul(10)
            .ok_or(PriceError::NotNumeric)?
            .checked_add(u128::from(byte - b'0'))
            .ok_or(PriceError::NotNumeric)?;
    }
    Ok(value)
}

fn pow10(exp: i32) -> Result<u128, PriceError> {
    let exp = u32::try_from(exp).map_err(|_| PriceError::NotNumeric)?;
    10u128.checked_pow(exp).ok_or(PriceError::NotNumeric)
}

fn reduce(mut numer: u128, mut denom: u128) -> Price {
    let divisor = gcd(numer, denom);
    numer /= divisor;
    denom /= divisor;
    Price { numer, denom }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let rest = a % b;
        a = b;
        b = rest;
    }
    a
}

/// Cost of one stream (ingest or index). Day and month are rounded separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub per_day: Amount,
    pub per_month: Amount,
}

/// Amount in cents. The fraction stays exact until display.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Amount {
    cents: Wide,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_cents(self.cents))
    }
}

impl fmt::Debug for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Amount({})", self)
    }
}

/// Cost per day = bytes × 86400 / window seconds / gb_bytes × price.
/// The month is that fraction × days, not the already rounded daily amount × days.
/// If a product overflows 512 bits, that is an error and not 0.
pub fn cost(
    sample_bytes: u64,
    window: Window,
    days_per_month: u32,
    price: Price,
    gb_bytes: u128,
) -> Result<Cost, CalcError> {
    Ok(Cost {
        per_day: amount(sample_bytes, window, 1, price, gb_bytes)?,
        per_month: amount(
            sample_bytes,
            window,
            u128::from(days_per_month),
            price,
            gb_bytes,
        )?,
    })
}

/// `bytes × factor / gb_bytes × price`: exact, rounded per day; × days, rounded per month.
/// The exact fraction comes before rounding. A repeating fraction stays a fraction.
pub fn explain_steps(
    sample_bytes: u64,
    window: Window,
    days_per_month: u32,
    price: Price,
    gb_bytes: u128,
) -> Result<String, CalcError> {
    let factor = factor_term(window);
    let price_term = price_term(price);
    let denom = ratio_denom(window, gb_bytes, price)?;
    let day_exact = format_ratio(ratio_numer(sample_bytes, 1, price)?, denom)?;
    let month_exact = format_ratio(
        ratio_numer(sample_bytes, u128::from(days_per_month), price)?,
        denom,
    )?;
    let rounded = cost(sample_bytes, window, days_per_month, price, gb_bytes)?;
    Ok(format!(
        "{sample_bytes} × {factor} / {gb_bytes} × {price_term} = {day_exact} → {} per day, × {days_per_month} = {month_exact} → {} per month",
        rounded.per_day, rounded.per_month
    ))
}

fn factor_term(window: Window) -> String {
    let factor = format_factor(window);
    if factor.contains('/') {
        format!("({factor})")
    } else {
        factor
    }
}

fn price_term(price: Price) -> String {
    if price.denom == 1 {
        price.numer.to_string()
    } else {
        format!("({}/{})", price.numer, price.denom)
    }
}

fn ratio_numer(sample_bytes: u64, day_scale: u128, price: Price) -> Result<Wide, CalcError> {
    wide_product(&[
        u128::from(sample_bytes),
        SECONDS_PER_DAY,
        day_scale,
        price.numer,
    ])
}

fn ratio_denom(window: Window, gb_bytes: u128, price: Price) -> Result<Wide, CalcError> {
    wide_product(&[u128::from(window.seconds()), gb_bytes, price.denom])
}

/// Exact decimal text, or a reduced fraction if the decimal does not terminate.
fn format_ratio(numer: Wide, denom: Wide) -> Result<String, CalcError> {
    if denom.is_zero() {
        return Err(CalcError);
    }
    if numer.is_zero() {
        return Ok("0".to_owned());
    }
    let divisor = gcd_wide(numer, denom)?;
    let (numer, _) = div_mod(numer, divisor)?;
    let (denom, _) = div_mod(denom, divisor)?;
    if terminates(denom)? {
        format_decimal(numer, denom)
    } else {
        Ok(format!("{}/{}", format_int(numer), format_int(denom)))
    }
}

fn format_decimal(numer: Wide, denom: Wide) -> Result<String, CalcError> {
    let (quot, mut rem) = div_mod(numer, denom)?;
    if rem.is_zero() {
        return Ok(format_int(quot));
    }
    let mut digits = Vec::new();
    while !rem.is_zero() {
        if digits.len() > 600 {
            return Ok(format!("{}/{}", format_int(numer), format_int(denom)));
        }
        rem = rem.mul_u128(10)?;
        let (digit, next) = div_mod(rem, denom)?;
        digits.push(digit);
        rem = next;
    }
    let mut text = format_int(quot);
    text.push('.');
    for digit in digits {
        text.push_str(&format_int(digit));
    }
    Ok(text)
}

fn format_int(value: Wide) -> String {
    if value.is_zero() {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    let mut rest = value;
    while !rest.is_zero() {
        let (next, rem) = div10(rest);
        digits.push(b'0' + rem);
        rest = next;
    }
    digits.reverse();
    String::from_utf8(digits).expect("digits")
}

fn gcd_wide(mut a: Wide, mut b: Wide) -> Result<Wide, CalcError> {
    while !b.is_zero() {
        let (_, rest) = div_mod(a, b)?;
        a = b;
        b = rest;
    }
    Ok(a)
}

/// With factors 2 and 5 removed, a denominator of 1 means a finite decimal.
fn terminates(mut denom: Wide) -> Result<bool, CalcError> {
    if denom.is_zero() {
        return Err(CalcError);
    }
    while (denom.limb[0] & 1) == 0 {
        denom = denom.shr1();
    }
    let five = Wide::from_u128(5);
    loop {
        let (quot, rem) = div_mod(denom, five)?;
        if !rem.is_zero() {
            break;
        }
        denom = quot;
    }
    Ok(denom == Wide::from_u128(1))
}

fn amount(
    sample_bytes: u64,
    window: Window,
    day_scale: u128,
    price: Price,
    gb_bytes: u128,
) -> Result<Amount, CalcError> {
    // × 100 turns it into cents. Rounding happens once, on that fraction.
    let numer = wide_product(&[
        u128::from(sample_bytes),
        SECONDS_PER_DAY,
        day_scale,
        price.numer,
        100,
    ])?;
    let denom = wide_product(&[u128::from(window.seconds()), gb_bytes, price.denom])?;
    Ok(Amount {
        cents: round_ratio(numer, denom)?,
    })
}

fn wide_product(factors: &[u128]) -> Result<Wide, CalcError> {
    let mut acc = Wide::from_u128(1);
    for &factor in factors {
        acc = acc.mul_u128(factor)?;
    }
    Ok(acc)
}

/// Divides and rounds half a step away from zero. A zero denominator should not occur:
/// a window has at least one second, and the price file requires a GB and a denominator.
fn round_ratio(numer: Wide, denom: Wide) -> Result<Wide, CalcError> {
    if denom.is_zero() {
        return Err(CalcError);
    }
    let (quot, rem) = div_mod(numer, denom)?;
    if rem.is_zero() || rem.shl1()?.cmp(&denom) == Ordering::Less {
        Ok(quot)
    } else {
        quot.add(Wide::from_u128(1))
    }
}

fn format_cents(cents: Wide) -> String {
    let mut digits = Vec::new();
    let mut rest = cents;
    loop {
        let (next, rem) = div10(rest);
        digits.push(b'0' + rem);
        rest = next;
        if rest.is_zero() {
            break;
        }
    }
    while digits.len() < 3 {
        digits.push(b'0');
    }
    digits.reverse();
    let split = digits.len() - 2;
    let whole = std::str::from_utf8(&digits[..split]).expect("digits");
    let frac = std::str::from_utf8(&digits[split..]).expect("digits");
    format!("{whole}.{frac}")
}

fn div10(value: Wide) -> (Wide, u8) {
    let mut rem = 0u128;
    let mut out = Wide::zero();
    for index in (0..8).rev() {
        let cur = (rem << 64) | u128::from(value.limb[index]);
        out.limb[index] = (cur / 10) as u64;
        rem = cur % 10;
    }
    (out, rem as u8)
}

/// 512 bits, little-endian. The numerator fits in 248 bits, the denominator in 320.
#[derive(Clone, Copy, Eq, PartialEq)]
struct Wide {
    limb: [u64; 8],
}

impl Wide {
    fn zero() -> Wide {
        Wide { limb: [0; 8] }
    }

    fn from_u128(value: u128) -> Wide {
        let mut out = Wide::zero();
        out.limb[0] = value as u64;
        out.limb[1] = (value >> 64) as u64;
        out
    }

    fn is_zero(self) -> bool {
        self.limb.iter().all(|limb| *limb == 0)
    }

    fn cmp(self, other: &Wide) -> Ordering {
        self.limb.iter().rev().cmp(other.limb.iter().rev())
    }

    fn bit(self, index: u32) -> bool {
        let limb = (index / 64) as usize;
        ((self.limb[limb] >> (index % 64)) & 1) == 1
    }

    fn set_bit(&mut self, index: u32) {
        let limb = (index / 64) as usize;
        self.limb[limb] |= 1_u64 << (index % 64);
    }

    fn highest_bit(self) -> Option<u32> {
        for (index, limb) in self.limb.iter().enumerate().rev() {
            if *limb != 0 {
                return Some(index as u32 * 64 + (63 - limb.leading_zeros()));
            }
        }
        None
    }

    fn add(self, other: Wide) -> Result<Wide, CalcError> {
        let mut out = Wide::zero();
        let mut carry = 0u64;
        for index in 0..8 {
            let sum =
                u128::from(self.limb[index]) + u128::from(other.limb[index]) + u128::from(carry);
            out.limb[index] = sum as u64;
            carry = (sum >> 64) as u64;
        }
        if carry != 0 {
            return Err(CalcError);
        }
        Ok(out)
    }

    fn sub(self, other: Wide) -> Result<Wide, CalcError> {
        let mut out = Wide::zero();
        let mut borrow = 0u64;
        for index in 0..8 {
            let (diff, borrowed) = self.limb[index].overflowing_sub(other.limb[index]);
            let (diff, borrowed_carry) = diff.overflowing_sub(borrow);
            out.limb[index] = diff;
            borrow = u64::from(borrowed || borrowed_carry);
        }
        if borrow != 0 {
            return Err(CalcError);
        }
        Ok(out)
    }

    fn shr1(self) -> Wide {
        let mut out = Wide::zero();
        for index in 0..8 {
            let high = if index + 1 < 8 {
                self.limb[index + 1] & 1
            } else {
                0
            };
            out.limb[index] = (self.limb[index] >> 1) | (high << 63);
        }
        out
    }

    fn shl1(self) -> Result<Wide, CalcError> {
        let mut out = Wide::zero();
        let mut carry = 0u64;
        for index in 0..8 {
            out.limb[index] = (self.limb[index] << 1) | carry;
            carry = self.limb[index] >> 63;
        }
        if carry != 0 {
            return Err(CalcError);
        }
        Ok(out)
    }

    fn mul_u64(self, value: u64) -> Result<Wide, CalcError> {
        let mut out = Wide::zero();
        let mut carry = 0u128;
        for index in 0..8 {
            let prod = u128::from(self.limb[index]) * u128::from(value) + carry;
            out.limb[index] = prod as u64;
            carry = prod >> 64;
        }
        if carry != 0 {
            return Err(CalcError);
        }
        Ok(out)
    }

    fn mul_u128(self, value: u128) -> Result<Wide, CalcError> {
        let low = self.mul_u64(value as u64)?;
        let high = self.mul_u64((value >> 64) as u64)?.shl_limbs(1)?;
        low.add(high)
    }

    fn shl_limbs(self, n: usize) -> Result<Wide, CalcError> {
        if self.limb[8 - n..].iter().any(|limb| *limb != 0) {
            return Err(CalcError);
        }
        let mut out = Wide::zero();
        for index in 0..(8 - n) {
            out.limb[index + n] = self.limb[index];
        }
        Ok(out)
    }
}

fn div_mod(numer: Wide, denom: Wide) -> Result<(Wide, Wide), CalcError> {
    let mut quot = Wide::zero();
    let mut rem = Wide::zero();
    let Some(top) = numer.highest_bit() else {
        return Ok((quot, rem));
    };
    for index in (0..=top).rev() {
        rem = rem.shl1()?;
        if numer.bit(index) {
            rem.limb[0] |= 1;
        }
        if rem.cmp(&denom) != Ordering::Less {
            rem = rem.sub(denom)?;
            quot.set_bit(index);
        }
    }
    Ok((quot, rem))
}

/// Monthly budget as a reduced fraction. Numerator and denominator fit in 512 bits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    numer: Wide,
    denom: Wide,
}

/// Non-negative amount. The decimal mark is a period, as with a sample fraction.
/// A comma, an exponent, a minus sign, and `5.` are invalid. `.5` is a half, `+4` is 4.
/// More than 80 characters, or digits that do not fit in 512 bits, is invalid.
pub fn parse_budget(text: &str) -> Option<Budget> {
    if text.is_empty() || text.len() > 80 {
        return None;
    }
    let text = text.strip_prefix('+').unwrap_or(text);
    if text.is_empty() {
        return None;
    }
    let (whole, frac) = if let Some((whole, frac)) = text.split_once('.') {
        if frac.is_empty() || frac.contains('.') {
            return None;
        }
        (whole, frac)
    } else {
        (text, "")
    };
    if !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let mut numer = wide_digits(whole)?;
    for byte in frac.bytes() {
        numer = numer.mul_u64(10).ok()?;
        numer = numer.add(Wide::from_u128(u128::from(byte - b'0'))).ok()?;
    }
    let scale = u32::try_from(frac.len()).ok()?;
    let denom = wide_pow10(scale)?;
    let divisor = gcd_wide(numer, denom).ok()?;
    let (numer, _) = div_mod(numer, divisor).ok()?;
    let (denom, _) = div_mod(denom, divisor).ok()?;
    Some(Budget { numer, denom })
}

fn wide_digits(text: &str) -> Option<Wide> {
    let mut value = Wide::zero();
    for byte in text.bytes() {
        value = value.mul_u64(10).ok()?;
        value = value.add(Wide::from_u128(u128::from(byte - b'0'))).ok()?;
    }
    Some(value)
}

fn wide_pow10(exp: u32) -> Option<Wide> {
    let mut value = Wide::from_u128(1);
    for _ in 0..exp {
        value = value.mul_u64(10).ok()?;
    }
    Some(value)
}

/// Monthly cost strictly above the budget. Equal is not above.
/// Ingest and index are added as fractions, before rounding to cents.
/// Zero bytes or zero days is not above a budget of zero or more.
pub fn month_above_budget(
    sample_bytes: u64,
    window: Window,
    days_per_month: u32,
    prices: &Prices,
    budget: Budget,
) -> Result<bool, CalcError> {
    let (left_numer, left_denom) = stream_ratio(
        sample_bytes,
        window,
        days_per_month,
        prices.ingest_per_gb,
        prices.gb_bytes,
    )?;
    let (right_numer, right_denom) = match prices.index_per_gb {
        Some(price) => stream_ratio(sample_bytes, window, days_per_month, price, prices.gb_bytes)?,
        None => (Wide::zero(), Wide::from_u128(1)),
    };
    if left_denom.is_zero() || right_denom.is_zero() || budget.denom.is_zero() {
        return Err(CalcError);
    }
    // (n1/d1 + n2/d2) > bn/bd
    // iff (n1*d2 + n2*d1) * bd > bn * d1 * d2
    let sum = Big::from_product(left_numer, right_denom)?
        .add(Big::from_product(right_numer, left_denom)?)?;
    let left = sum.mul(budget.denom)?;
    let right = Big::from_product(left_denom, right_denom)?.mul(budget.numer)?;
    Ok(left.cmp(right) == Ordering::Greater)
}

fn stream_ratio(
    sample_bytes: u64,
    window: Window,
    days_per_month: u32,
    price: Price,
    gb_bytes: u128,
) -> Result<(Wide, Wide), CalcError> {
    let numer = wide_product(&[
        u128::from(sample_bytes),
        SECONDS_PER_DAY,
        u128::from(days_per_month),
        price.numer,
    ])?;
    let denom = wide_product(&[u128::from(window.seconds()), gb_bytes, price.denom])?;
    Ok((numer, denom))
}

/// 1024 bits, little-endian. The product of two cost denominators does not fit in 512 bits.
#[derive(Clone, Copy, Eq, PartialEq)]
struct Big {
    limb: [u64; 16],
}

impl Big {
    fn from_product(left: Wide, right: Wide) -> Result<Big, CalcError> {
        mul_limbs(&left.limb, &right.limb)
    }

    fn add(self, other: Big) -> Result<Big, CalcError> {
        let mut out = Big { limb: [0; 16] };
        let mut carry = 0u64;
        for index in 0..16 {
            let sum =
                u128::from(self.limb[index]) + u128::from(other.limb[index]) + u128::from(carry);
            out.limb[index] = sum as u64;
            carry = (sum >> 64) as u64;
        }
        if carry != 0 {
            return Err(CalcError);
        }
        Ok(out)
    }

    fn mul(self, right: Wide) -> Result<Big, CalcError> {
        mul_limbs(&self.limb, &right.limb)
    }

    fn cmp(self, other: Big) -> Ordering {
        self.limb.iter().rev().cmp(other.limb.iter().rev())
    }
}

fn mul_limbs(left: &[u64], right: &[u64]) -> Result<Big, CalcError> {
    let mut limb = [0u64; 16];
    for (i, &factor) in left.iter().enumerate() {
        if factor == 0 {
            continue;
        }
        let mut carry = 0u128;
        for (j, &other) in right.iter().enumerate() {
            let idx = i + j;
            if idx >= 16 {
                if other != 0 || carry != 0 {
                    return Err(CalcError);
                }
                continue;
            }
            let prod = u128::from(factor) * u128::from(other) + u128::from(limb[idx]) + carry;
            limb[idx] = prod as u64;
            carry = prod >> 64;
        }
        let mut idx = i + right.len();
        while carry != 0 {
            if idx >= 16 {
                return Err(CalcError);
            }
            let sum = u128::from(limb[idx]) + carry;
            limb[idx] = sum as u64;
            carry = sum >> 64;
            idx += 1;
        }
    }
    Ok(Big { limb })
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{
        cost, explain_steps, extrapolate, format_factor, format_gb, load_prices,
        month_above_budget, parse_budget, parse_prices, parse_window, Price, PriceError, Prices,
        Wide, GB_BYTES,
    };

    #[test]
    fn units() {
        assert_eq!(parse_window("30s").unwrap().seconds(), 30);
        assert_eq!(parse_window("15m").unwrap().seconds(), 900);
        assert_eq!(parse_window("2h").unwrap().seconds(), 7_200);
        assert_eq!(parse_window("1d").unwrap().seconds(), 86_400);
    }

    #[test]
    fn example_1m() {
        // 1_000_000 bytes over 1m: factor 1440.
        // Per day 1_440_000_000 bytes = 1.440 GB.
        // Per month (30) 43_200_000_000 bytes = 43.200 GB.
        let window = parse_window("1m").unwrap();
        assert_eq!(window.seconds(), 60);
        assert_eq!(format_factor(window), "1440");
        let volume = extrapolate(1_000_000, window, 30, GB_BYTES).unwrap();
        assert_eq!(volume.bytes_per_day, 1_440_000_000);
        assert_eq!(volume.bytes_per_month, 43_200_000_000);
        assert_eq!(format_gb(volume.gb_per_day_milli), "1.440");
        assert_eq!(format_gb(volume.gb_per_month_milli), "43.200");
    }

    #[test]
    fn days_per_month_31() {
        let window = parse_window("1m").unwrap();
        let volume = extrapolate(1_000_000, window, 31, GB_BYTES).unwrap();
        assert_eq!(volume.bytes_per_day, 1_440_000_000);
        assert_eq!(format_gb(volume.gb_per_day_milli), "1.440");
        assert_eq!(volume.bytes_per_month, 44_640_000_000);
        assert_eq!(format_gb(volume.gb_per_month_milli), "44.640");
    }

    fn cent_value(amount: &super::Amount) -> u128 {
        assert!(
            amount.cents.limb[2..].iter().all(|limb| *limb == 0),
            "amount does not fit in u128"
        );
        u128::from(amount.cents.limb[0]) | (u128::from(amount.cents.limb[1]) << 64)
    }

    #[test]
    fn example_cost_ingest_and_index() {
        // 1_000_000 bytes, 1m, 30 days, gb 1_000_000_000.
        // Ingest 1/10: 1_000_000 × 1440 / 1_000_000_000 × 1/10 = 144/1000 → 14 cents.
        // Month: 144/1000 × 30 = 432 cents.
        // Index 1/4: 144/100 × 1/4 = 36 cents, times 30 = 1080 cents.
        let window = parse_window("1m").unwrap();
        let ingest = cost(
            1_000_000,
            window,
            30,
            Price {
                numer: 1,
                denom: 10,
            },
            1_000_000_000,
        )
        .unwrap();
        assert_eq!(cent_value(&ingest.per_day), 14);
        assert_eq!(cent_value(&ingest.per_month), 432);
        let index = cost(
            1_000_000,
            window,
            30,
            Price { numer: 1, denom: 4 },
            1_000_000_000,
        )
        .unwrap();
        assert_eq!(cent_value(&index.per_day), 36);
        assert_eq!(cent_value(&index.per_month), 1080);

        // Same sample, gb 500_000_000: 1_440_000_000 / 500_000_000 = 2.880 GB.
        // Month 43_200_000_000 / 500_000_000 = 86.400 GB. The byte extrapolation stays the same.
        let volume = extrapolate(1_000_000, window, 30, 500_000_000).unwrap();
        assert_eq!(volume.bytes_per_day, 1_440_000_000);
        assert_eq!(format_gb(volume.gb_per_day_milli), "2.880");
        assert_eq!(format_gb(volume.gb_per_month_milli), "86.400");

        // 2s × maximum gb_bytes does not fit in u128. No silent 0.
        let narrow = extrapolate(1, parse_window("2s").unwrap(), 30, u128::MAX);
        assert_eq!(narrow.unwrap_err().to_string(), "calculation overflow");
    }

    #[test]
    fn explain_example_1m_shows_the_unrounded_step() {
        // 1_000_000 × 1440 / 1_000_000_000 × 1/10 = 144/1000 = 0.144.
        // 14.4 cents become 14. Month 0.144 × 30 is already a whole cent.
        let window = parse_window("1m").unwrap();
        let price = Price {
            numer: 1,
            denom: 10,
        };
        let line = explain_steps(1_000_000, window, 30, price, GB_BYTES).unwrap();
        let bill = cost(1_000_000, window, 30, price, GB_BYTES).unwrap();
        assert_eq!(
            line,
            format!(
                "1000000 × 1440 / 1000000000 × (1/10) = 0.144 → {} per day, × 30 = {} → {} per month",
                bill.per_day, bill.per_month, bill.per_month
            )
        );
    }

    #[test]
    fn explain_keeps_the_fraction_before_rounding() {
        // 9 / 2000 = 0.0045 → 0 cents per day. Month 0.135 → 14 cents. Not 0 × 30.
        // 1 byte over 7s is 86400/7. That decimal repeats and stays a fraction.
        // 10^18 bytes on gb 10^18 at price 10^30/10^30 reduces to 1 per day.
        let window = parse_window("1d").unwrap();
        let price = Price { numer: 1, denom: 1 };
        let line = explain_steps(9, window, 30, price, 2000).unwrap();
        let bill = cost(9, window, 30, price, 2000).unwrap();
        assert_eq!(
            line,
            format!(
                "9 × 1 / 2000 × 1 = 0.0045 → {} per day, × 30 = 0.135 → {} per month",
                bill.per_day, bill.per_month
            )
        );

        let narrow = parse_window("7s").unwrap();
        let line = explain_steps(1, narrow, 30, price, 1).unwrap();
        let bill = cost(1, narrow, 30, price, 1).unwrap();
        assert_eq!(
            line,
            format!(
                "1 × (86400/7) / 1 × 1 = 86400/7 → {} per day, × 30 = 2592000/7 → {} per month",
                bill.per_day, bill.per_month
            )
        );

        let large = Price {
            numer: 10u128.pow(30),
            denom: 10u128.pow(30),
        };
        let line = explain_steps(10u64.pow(18), window, 30, large, 10u128.pow(18)).unwrap();
        assert!(line.contains("= 1 → "), "{line}");
        assert!(line.contains("× 30 = 30 → "), "{line}");
    }

    #[test]
    fn half_cent_away_from_zero() {
        // gb 2000, price 1, window 1d. 10 bytes → 1000/2000 cents = 1/2 → 1.
        // Month exactly 15 cents. 9 bytes → 900/2000 cents, below half → 0.
        // Month 27000/2000 = 13.5 cents → 14. 1005 bytes on gb 1000 → 100.5 → 101.
        let window = parse_window("1d").unwrap();
        let price = Price { numer: 1, denom: 1 };
        let half = cost(10, window, 30, price, 2000).unwrap();
        assert_eq!(cent_value(&half.per_day), 1);
        assert_eq!(cent_value(&half.per_month), 15);
        let below = cost(9, window, 30, price, 2000).unwrap();
        assert_eq!(cent_value(&below.per_day), 0);
        assert_eq!(cent_value(&below.per_month), 14);
        let above = cost(1005, window, 30, price, 1000).unwrap();
        assert_eq!(cent_value(&above.per_day), 101);
        assert_eq!(cent_value(&above.per_month), 3015);
    }

    #[test]
    fn window_that_does_not_divide_stays_exact() {
        // 1_000_000_000 bytes, 7s, price 1/10, gb 1_000_000_000.
        // Cents = 86400 × 100 / 70 = 8_640_000 / 70 = 123428 remainder 40.
        // 2 × 40 >= 70, so 123429. A truncated factor 12342 would give 123420.
        let window = parse_window("7s").unwrap();
        let bill = cost(
            1_000_000_000,
            window,
            30,
            Price {
                numer: 1,
                denom: 10,
            },
            1_000_000_000,
        )
        .unwrap();
        assert_eq!(cent_value(&bill.per_day), 123_429);
    }

    #[test]
    fn large_factors_cancel() {
        // 10^18 bytes, gb 10^18, price 10^30/10^30, window 1d → exactly 1 per day.
        // The product does not fit in u128; after cancelling, 100 cents remain, times 30 = 3000.
        let window = parse_window("1d").unwrap();
        let price = Price {
            numer: 10u128.pow(30),
            denom: 10u128.pow(30),
        };
        let bill = cost(10u64.pow(18), window, 30, price, 10u128.pow(18)).unwrap();
        assert_eq!(cent_value(&bill.per_day), 100);
        assert_eq!(cent_value(&bill.per_month), 3000);

        let zero_value = cost(
            1_000_000,
            parse_window("1m").unwrap(),
            30,
            Price { numer: 0, denom: 1 },
            1_000_000_000,
        )
        .unwrap();
        assert_eq!(cent_value(&zero_value.per_day), 0);
        assert_eq!(cent_value(&zero_value.per_month), 0);

        let no_days = cost(10, window, 0, Price { numer: 1, denom: 1 }, 2000).unwrap();
        assert_eq!(cent_value(&no_days.per_day), 1);
        assert_eq!(cent_value(&no_days.per_month), 0);

        let small_cost = cost(
            1,
            window,
            30,
            Price {
                numer: 1,
                denom: u128::MAX,
            },
            u128::MAX,
        )
        .unwrap();
        assert_eq!(cent_value(&small_cost.per_day), 0);
        assert_eq!(cent_value(&small_cost.per_month), 0);
    }

    #[test]
    fn large_amount_stays_exact() {
        // 10^18 bytes, window 1d, gb 1, price 10^20 → 10^40 cents.
        // Text: 1, then 38 zeros, a decimal point, and 00. Month × 30 = 3 × 10^41 cents.
        let window = parse_window("1d").unwrap();
        let bill = cost(
            10u64.pow(18),
            window,
            30,
            Price {
                numer: 10u128.pow(20),
                denom: 1,
            },
            1,
        )
        .unwrap();
        assert_eq!(bill.per_day.to_string(), cents_text("1", 40));
        assert_eq!(bill.per_month.to_string(), cents_text("3", 41));
    }

    fn cents_text(head: &str, zeros: usize) -> String {
        let mut digits = String::from(head);
        digits.extend(std::iter::repeat_n('0', zeros));
        let split = digits.len() - 2;
        format!("{}.{}", &digits[..split], &digits[split..])
    }

    #[test]
    fn invalid_window() {
        for text in ["0m", "5x", "", "1é", "é", "1"] {
            assert!(parse_window(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn gb_rounds_half_step_away_from_zero() {
        // 1d, factor 1. 1_500_000 bytes = 0.0015 GB, so 0.002.
        let window = parse_window("1d").unwrap();
        let half = extrapolate(1_500_000, window, 30, GB_BYTES).unwrap();
        assert_eq!(format_gb(half.gb_per_day_milli), "0.002");
        let below = extrapolate(1_499_999, window, 30, GB_BYTES).unwrap();
        assert_eq!(format_gb(below.gb_per_day_milli), "0.001");
    }

    fn expected(json: &str, error_run: PriceError, text_body: &str) {
        let err = parse_prices(json.as_bytes()).expect_err("should fail");
        assert_eq!(err, error_run);
        assert_eq!(err.to_string(), text_body);
        assert!(!text_body.contains("CANARY"));
    }

    #[test]
    fn price_file_with_utf8_bom() {
        let body = br#"{"currency":"EUR","source":"manual","date":"2026-10-08","ingest_per_gb":2}"#;
        let mut raw = b"\xEF\xBB\xBF".to_vec();
        raw.extend_from_slice(body);
        let prices = parse_prices(&raw).expect("BOM is skipped");
        assert_eq!(prices.currency, "EUR");
        // Only one BOM, and only at the start.
        let mut twice = b"\xEF\xBB\xBF".to_vec();
        twice.extend_from_slice(&raw);
        assert!(matches!(parse_prices(&twice), Err(PriceError::NotJson)));
    }

    #[test]
    fn valid_price_file() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prices.json");
        let prices = load_prices(&path).expect("price file");
        assert_eq!(prices.currency, "EUR");
        assert_eq!(prices.source, "manual");
        assert_eq!(prices.date, "2026-10-08");
        assert_eq!(prices.ingest_per_gb.numer, 1);
        assert_eq!(prices.ingest_per_gb.denom, 10);
        let index = prices.index_per_gb.expect("index");
        assert_eq!(index.numer, 1);
        assert_eq!(index.denom, 20);
        assert_eq!(prices.gb_bytes, 500_000_000);

        let min = parse_prices(
            br#"{"currency":"EUR","source":"manual","date":"2026-10-08","ingest_per_gb":2}"#,
        )
        .expect("minimum");
        assert!(min.index_per_gb.is_none());
        assert_eq!(min.gb_bytes, 1_000_000_000);
        assert_eq!(min.ingest_per_gb.numer, 2);
        assert_eq!(min.ingest_per_gb.denom, 1);

        let zero_value = parse_prices(
            br#"{"currency":"EUR","source":"manual","date":"2026-10-08","ingest_per_gb":0}"#,
        )
        .expect("zero is valid");
        assert_eq!(zero_value.ingest_per_gb.numer, 0);
        assert_eq!(zero_value.ingest_per_gb.denom, 1);

        let exp = parse_prices(
            br#"{"currency":"EUR","source":"manual","date":"2026-10-08","ingest_per_gb":1e-1}"#,
        )
        .expect("exponent");
        assert_eq!(exp.ingest_per_gb.numer, 1);
        assert_eq!(exp.ingest_per_gb.denom, 10);
    }

    #[test]
    fn fraction_from_exponent() {
        let price_value = super::rate_from_decimal("1e-1").expect("fraction");
        assert_eq!((price_value.numer, price_value.denom), (1, 10));
        let half = super::rate_from_decimal("25e-1").expect("fraction");
        assert_eq!((half.numer, half.denom), (5, 2));
        let whole_number = super::rate_from_decimal("1e2").expect("fraction");
        assert_eq!((whole_number.numer, whole_number.denom), (100, 1));
    }

    #[test]
    fn price_keeps_digits_past_a_binary_float() {
        let prices = parse_prices(
            br#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1000.0000000000000001}"#,
        )
        .expect("exact decimal");
        assert_eq!(
            (prices.ingest_per_gb.numer, prices.ingest_per_gb.denom),
            (10_000_000_000_000_000_001, 10_000_000_000_000_000)
        );
        let window = parse_window("1m").unwrap();
        let budget = parse_budget("4.320").expect("budget");
        assert!(month_above_budget(100, window, 30, &prices, budget).unwrap());
        let line = explain_steps(100, window, 30, prices.ingest_per_gb, prices.gb_bytes).unwrap();
        assert!(line.contains("0.1440000000000000000144"), "{line}");
        assert!(line.contains("4.320000000000000000432"), "{line}");
        assert!(!line.contains("(1/10)"), "{line}");

        let tenth = parse_prices(
            br#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":0.10000000000000000001,"gb_bytes":100}"#,
        )
        .expect("exact tenth");
        let day = parse_window("1d").unwrap();
        let shown = explain_steps(100, day, 30, tenth.ingest_per_gb, tenth.gb_bytes).unwrap();
        assert!(shown.contains("0.10000000000000000001"), "{shown}");
        assert!(!shown.contains("(1/10)"), "{shown}");
    }

    #[test]
    fn duplicate_price_key_is_rejected() {
        let body = concat!(r#"{"ingest_per_gb":99,"ingest_per_gb":0."#, r#"01}"#);
        let cents = concat!("0.", "01");
        let err = parse_prices(body.as_bytes()).expect_err("duplicate");
        assert_eq!(err, PriceError::DuplicateKey);
        assert_eq!(err.to_string(), "price file has a duplicate key");
        assert!(!err.to_string().contains("99"));
        assert!(!err.to_string().contains(cents));
    }

    #[test]
    fn not_json() {
        expected(
            "{CANARY-PRICE-0001",
            PriceError::NotJson,
            "price file is not JSON",
        );
        expected("", PriceError::NotJson, "price file is not JSON");
        expected("[]", PriceError::NotJson, "price file is not JSON");
        expected("null", PriceError::NotJson, "price file is not JSON");
        let err = parse_prices(&[0xff]).expect_err("should fail");
        assert_eq!(err, PriceError::NotJson);
        assert_eq!(err.to_string(), "price file is not JSON");
    }

    #[test]
    fn missing_field() {
        let basis = [
            ("currency", r#""EUR""#),
            ("source", r#""CANARY-PRICE-0002""#),
            ("date", r#""2026-10-08""#),
            ("ingest_per_gb", "1"),
        ];
        for omitted in ["currency", "source", "date", "ingest_per_gb"] {
            let mut body = String::new();
            for (key, value) in basis {
                if key == omitted {
                    continue;
                }
                if !body.is_empty() {
                    body.push(',');
                }
                body.push_str(&format!(r#""{key}":{value}"#));
            }
            expected(
                &format!("{{{body}}}"),
                PriceError::MissingField,
                "price file is missing a field",
            );
        }
    }

    #[test]
    fn empty_field() {
        let text_body = "price file has an empty field";
        expected(
            r#"{"currency":"","source":"CANARY-PRICE-0003","date":"2026-10-08","ingest_per_gb":1}"#,
            PriceError::EmptyField,
            text_body,
        );
        expected(
            r#"{"currency":"   ","source":"CANARY-PRICE-0003","date":"2026-10-08","ingest_per_gb":1}"#,
            PriceError::EmptyField,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"","date":"2026-10-08","ingest_per_gb":1}"#,
            PriceError::EmptyField,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"","ingest_per_gb":1}"#,
            PriceError::EmptyField,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"CANARY-PRICE-0003","date":"2026-10-08","ingest_per_gb":null}"#,
            PriceError::EmptyField,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"index_per_gb":""}"#,
            PriceError::EmptyField,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"gb_bytes":null}"#,
            PriceError::EmptyField,
            text_body,
        );
    }

    #[test]
    fn negative_price() {
        let text_body = "price file has a negative price";
        expected(
            r#"{"currency":"EUR","source":"CANARY-PRICE-0004","date":"2026-10-08","ingest_per_gb":-2}"#,
            PriceError::Negative,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"CANARY-PRICE-0004","date":"2026-10-08","ingest_per_gb":-1e-1}"#,
            PriceError::Negative,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"index_per_gb":-3}"#,
            PriceError::Negative,
            text_body,
        );
    }

    #[test]
    fn gb_bytes_must_be_an_integer_greater_than_zero() {
        let text_body = "gb_bytes must be a whole number greater than 0";
        for json in [
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"gb_bytes":0.5}"#,
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"gb_bytes":1.5}"#,
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"gb_bytes":0}"#,
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"gb_bytes":-1}"#,
        ] {
            let err = parse_prices(json.as_bytes()).expect_err("should fail");
            assert_eq!(err.to_string(), text_body, "{json}");
            assert!(!err.to_string().contains("CANARY"));
        }
    }

    #[test]
    fn price_is_not_a_number() {
        let text_body = "price file has a price that is not a number";
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":"CANARY-PRICE-0005"}"#,
            PriceError::NotNumeric,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":true}"#,
            PriceError::NotNumeric,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"index_per_gb":"x"}"#,
            PriceError::NotNumeric,
            text_body,
        );
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"gb_bytes":"8"}"#,
            PriceError::NotNumeric,
            text_body,
        );
    }

    #[test]
    fn unknown_field() {
        expected(
            r#"{"currency":"EUR","source":"s","date":"2026-10-08","ingest_per_gb":1,"CANARY-PRICE-0006":1}"#,
            PriceError::UnknownField,
            "price file has an unknown field",
        );
    }

    #[test]
    fn unreadable_price_file_omits_the_path() {
        let err = load_prices(std::path::Path::new("CANARY-PRICE-0007/missing.json"))
            .expect_err("should fail");
        assert_eq!(err, PriceError::Unreadable);
        assert_eq!(err.to_string(), "price file is not readable");
        assert!(!err.to_string().contains("CANARY"));
    }

    /// An amount has exactly two decimal places. An IP has more dots,
    /// GB text has three decimal places, a share one.
    #[test]
    fn source_contains_no_price_amount() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files_read = 0u32;
        read_source(&root, &mut files_read);
        assert!(files_read > 0);
    }

    fn read_source(path: &std::path::Path, files_read: &mut u32) {
        for entry in std::fs::read_dir(path).expect("src") {
            let entry = entry.expect("src");
            let path = entry.path();
            if path.is_dir() {
                read_source(&path, files_read);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let text = std::fs::read_to_string(&path).expect("source");
                *files_read += 1;
                assert!(
                    !contains_price_amount(&text),
                    "{} contains a price amount",
                    path.display()
                );
            }
        }
    }

    fn contains_price_amount(text: &str) -> bool {
        let euro = '\u{20AC}';
        let dollar = '\u{24}';
        if text.contains(euro) || text.contains(dollar) {
            return true;
        }
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes[i].is_ascii_digit() {
                i += 1;
                continue;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let mut groups = 0u32;
            let mut last = 0usize;
            while i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b',') {
                let sep = i;
                i += 1;
                if i >= bytes.len() || !bytes[i].is_ascii_digit() {
                    i = sep;
                    break;
                }
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                groups += 1;
                last = i - start;
            }
            if groups == 1 && last == 2 {
                return true;
            }
        }
        false
    }

    fn fraction(text: &str, numer: u128, denom: u128) {
        let budget = parse_budget(text).unwrap_or_else(|| panic!("valid: {text}"));
        assert_eq!(
            budget.numer.cmp(&Wide::from_u128(numer)),
            Ordering::Equal,
            "{text}"
        );
        assert_eq!(
            budget.denom.cmp(&Wide::from_u128(denom)),
            Ordering::Equal,
            "{text}"
        );
    }

    fn price_set(ingest: Price, index: Option<Price>, gb: u128) -> Prices {
        Prices {
            currency: "EUR".to_owned(),
            source: "t".to_owned(),
            date: "1999-01-02".to_owned(),
            ingest_per_gb: ingest,
            index_per_gb: index,
            gb_bytes: gb,
        }
    }

    fn above(bytes: u64, time_window: &str, day_count: u32, prices: &Prices, budget: &str) -> bool {
        month_above_budget(
            bytes,
            parse_window(time_window).expect("window"),
            day_count,
            prices,
            parse_budget(budget).unwrap_or_else(|| panic!("budget: {budget}")),
        )
        .expect("calculation")
    }

    #[test]
    fn budget_fraction_and_invalid_text() {
        // 432/100 = 108/25. Same value with zeros and a plus.
        fraction(concat!("4.", "32"), 108, 25);
        fraction(concat!("4.", "320"), 108, 25);
        fraction(concat!("+4.", "32"), 108, 25);
        fraction(concat!("04.", "32"), 108, 25);
        fraction(".5", 1, 2);
        fraction("+.5", 1, 2);
        fraction("0", 0, 1);
        fraction("+4", 4, 1);
        // 80 nines fit in 512 bits and form an integer. 81 characters is too long.
        let lang = "9".repeat(80);
        let budget = parse_budget(&lang).expect("80 digits");
        assert!(!budget.numer.is_zero());
        assert_eq!(budget.denom.cmp(&Wide::from_u128(1)), Ordering::Equal);
        assert!(parse_budget(&"9".repeat(81)).is_none());
        for text in [
            concat!("4,", "32"),
            "5.",
            "1e2",
            "-1",
            "",
            "+",
            "++1",
            ".",
            "1 ",
            " 1",
        ] {
            assert!(parse_budget(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn month_above_budget_hand_calculated() {
        // 1_000_000 bytes, 1m, 30 days, ingest 1/10: month exactly 432/100.
        let ingest = price_set(
            Price {
                numer: 1,
                denom: 10,
            },
            None,
            1_000_000_000,
        );
        assert!(!above(1_000_000, "1m", 30, &ingest, concat!("4.", "32")));
        assert!(above(1_000_000, "1m", 30, &ingest, concat!("4.", "319")));
        assert!(above(1_000_000, "1m", 30, &ingest, "0"));
        assert!(!above(1_000_000, "1m", 30, &ingest, concat!("4.", "325")));
        // Zero days is 0. That is not above 0, and a positive amount is not either.
        assert!(!above(1_000_000, "1m", 0, &ingest, "0"));
        assert!(!above(0, "1m", 30, &ingest, "0"));

        // Index 1/4: 1080/100. Together 1512/100.
        let both = price_set(
            Price {
                numer: 1,
                denom: 10,
            },
            Some(Price { numer: 1, denom: 4 }),
            1_000_000_000,
        );
        assert!(!above(1_000_000, "1m", 30, &both, concat!("15.", "12")));
        assert!(above(1_000_000, "1m", 30, &both, concat!("15.", "11")));
        // An index of 0 does not count.
        let index_zero = price_set(
            Price {
                numer: 1,
                denom: 10,
            },
            Some(Price { numer: 0, denom: 1 }),
            1_000_000_000,
        );
        assert!(!above(
            1_000_000,
            "1m",
            30,
            &index_zero,
            concat!("4.", "32")
        ));
        assert!(above(1_000_000, "1m", 30, &index_zero, concat!("4.", "31")));

        // 9 bytes, gb 2000, 1d, 30 days: 270/2000 = 135/1000. Shown as 14 cents.
        let narrow = price_set(Price { numer: 1, denom: 1 }, None, 2000);
        assert!(!above(9, "1d", 30, &narrow, concat!("0.", "135")));
        assert!(above(9, "1d", 30, &narrow, concat!("0.", "134")));
        assert!(!above(9, "1d", 30, &narrow, concat!("0.", "14")));

        // 26 bytes, gb 250, 1 day, two streams of 1: each 104/1000, together 208/1000.
        // Each rounds to 10 cents. The sum of the cents is 20 and lies below the fraction.
        let sum_prices = price_set(
            Price { numer: 1, denom: 1 },
            Some(Price { numer: 1, denom: 1 }),
            250,
        );
        assert!(!above(26, "1d", 1, &sum_prices, concat!("0.", "208")));
        assert!(above(26, "1d", 1, &sum_prices, concat!("0.", "20")));

        // 10^18 bytes, 1d, gb 1, price 10^20, 30 days = 3×10^39 exactly.
        let large = price_set(
            Price {
                numer: 10u128.pow(20),
                denom: 1,
            },
            None,
            1,
        );
        let mut equal = String::from("3");
        equal.extend(std::iter::repeat_n('0', 39));
        let mut just_below = String::from("2");
        just_below.extend(std::iter::repeat_n('9', 39));
        assert!(!above(10u64.pow(18), "1d", 30, &large, &equal));
        assert!(above(10u64.pow(18), "1d", 30, &large, &just_below));

        // Two maximum streams: the intermediate products do not fit in 512 bits.
        // Sum = 2 × 86400 × u32::MAX / u128::MAX, so above 0 and below 1.
        let max = Price {
            numer: u128::MAX,
            denom: u128::MAX,
        };
        let full = price_set(max, Some(max), u128::MAX);
        let time_window = format!("{}s", u64::MAX);
        assert!(above(u64::MAX, &time_window, u32::MAX, &full, "0"));
        assert!(!above(u64::MAX, &time_window, u32::MAX, &full, "1"));

        let broken = price_set(Price { numer: 1, denom: 1 }, None, 0);
        let err = month_above_budget(
            1,
            parse_window("1d").expect("window"),
            30,
            &broken,
            parse_budget("1").expect("budget"),
        );
        assert!(err.is_err());
    }
}
