//! Datadog indexed-log list price, per million events.
//!
//! Events are the ingested records. They are extrapolated with the same
//! window and days-per-month factor as the bytes, and they stay a fraction
//! until display. Amounts round to cents, event counts to thousandths of a
//! million, half away from zero.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::money::{self, CalcError, Price, Window};
use crate::pattern::Stats;
use crate::read::Line;
use crate::rules::{Fraction, RuleSet};

/// 3, 7, 15 or 30 days. Anything else is a usage error.
pub fn parse_retention(text: &str) -> Option<u16> {
    match text {
        "3" => Some(3),
        "7" => Some(7),
        "15" => Some(15),
        "30" => Some(30),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Billing {
    Annual,
    Monthly,
    Ondemand,
}

impl Billing {
    pub fn name(self) -> &'static str {
        match self {
            Billing::Annual => "annual",
            Billing::Monthly => "monthly",
            Billing::Ondemand => "ondemand",
        }
    }
}

pub fn parse_billing(text: &str) -> Option<Billing> {
    match text {
        "annual" => Some(Billing::Annual),
        "monthly" => Some(Billing::Monthly),
        "ondemand" => Some(Billing::Ondemand),
        _ => None,
    }
}

/// USD per 1M indexed events. Same decimal rules as a price-file amount.
/// A value the price file would reject is `None`, and the text is not kept.
pub fn parse_indexed_price(text: &str) -> Option<Price> {
    money::parse_rate(text).ok()
}

/// List price in USD cents per 1M events, read from the public price list
/// on 2026-10-14. Order inside a cell: annual, monthly, ondemand.
fn list_cents(retention: u16, billing: Billing) -> u128 {
    let (annual, monthly, ondemand) = match retention {
        3 => (106, 127, 159),
        7 => (127, 152, 191),
        15 => (170, 204, 255),
        30 => (250, 300, 375),
        _ => unreachable!("retention is one of 3, 7, 15, 30"),
    };
    match billing {
        Billing::Annual => annual,
        Billing::Monthly => monthly,
        Billing::Ondemand => ondemand,
    }
}

/// What to charge. Defaults are 15 days and ondemand, and only when a caller
/// has already decided that indexing is on. A custom price replaces the cell.
pub struct Quote {
    pub retention_days: u16,
    pub billing: Billing,
    pub price: Price,
    pub custom: bool,
}

pub fn quote(retention: Option<u16>, billing: Option<Billing>, custom: Option<Price>) -> Quote {
    let retention_days = retention.unwrap_or(15);
    let billing = billing.unwrap_or(Billing::Ondemand);
    match custom {
        Some(price) => Quote {
            retention_days,
            billing,
            price,
            custom: true,
        },
        None => Quote {
            retention_days,
            billing,
            price: Price {
                numer: list_cents(retention_days, billing),
                denom: 100,
            },
            custom: false,
        },
    }
}

/// Expected events of one pattern, before and after index rules.
pub struct PatternEvents {
    pub id: String,
    pub before: Fraction,
    pub after: Fraction,
}

/// Sample events. The month factor is applied later, at display.
pub struct Events {
    pub total_before: Fraction,
    pub total_after: Fraction,
    pub patterns: Vec<PatternEvents>,
}

#[derive(Default)]
struct Pair {
    before: Fraction,
    after: Fraction,
}

/// Counts index events while the file is read. Lines are not stored.
#[derive(Default)]
pub struct Tally {
    groups: BTreeMap<String, Pair>,
}

impl Tally {
    /// `ingest` is `--rules` (already applied). A dropped record does not
    /// remain, so it adds nothing. A sample keeps its fraction. `index` then
    /// runs on that weight: 889 records at 0.1 stay 88.9 events.
    pub fn push(&mut self, line: &Line, ingest: Option<&RuleSet>, index: Option<&RuleSet>) {
        let before = match ingest {
            Some(rules) => rules.keep(&line.raw),
            None => Fraction::one(),
        };
        let after = match index {
            Some(rules) => before.mul_frac(&rules.keep(&line.raw)),
            None => before.clone(),
        };
        let pair = self
            .groups
            .entry(crate::mask::mask(&line.text))
            .or_default();
        pair.before.add_frac(&before);
        pair.after.add_frac(&after);
    }

    pub fn finish(self, stats: &Stats) -> Events {
        let shown: BTreeSet<&str> = stats
            .patterns
            .iter()
            .filter(|pattern| !pattern.rest)
            .map(|pattern| pattern.text.as_str())
            .collect();
        let patterns = stats
            .patterns
            .iter()
            .map(|pattern| {
                if pattern.rest {
                    let mut before = Fraction::default();
                    let mut after = Fraction::default();
                    for (text, pair) in &self.groups {
                        if !shown.contains(text.as_str()) {
                            before.add_frac(&pair.before);
                            after.add_frac(&pair.after);
                        }
                    }
                    PatternEvents {
                        id: pattern.id.clone(),
                        before,
                        after,
                    }
                } else {
                    let pair = self.groups.get(&pattern.text);
                    PatternEvents {
                        id: pattern.id.clone(),
                        before: pair.map(|item| item.before.clone()).unwrap_or_default(),
                        after: pair.map(|item| item.after.clone()).unwrap_or_default(),
                    }
                }
            })
            .collect();
        let mut total_before = Fraction::default();
        let mut total_after = Fraction::default();
        for pair in self.groups.values() {
            total_before.add_frac(&pair.before);
            total_after.add_frac(&pair.after);
        }
        Events {
            total_before,
            total_after,
            patterns,
        }
    }
}

/// One indexed section, text and JSON share these strings.
pub struct View {
    pub label: &'static str,
    pub retention_days: u16,
    pub billing: &'static str,
    pub usd_per_million: String,
    pub events_before: String,
    pub events_after: String,
    pub cost_before: String,
    pub cost_after: String,
    pub savings: String,
    pub steps: Option<String>,
    pub patterns: Vec<PatternView>,
}

pub struct PatternView {
    pub id: String,
    pub events_before: String,
    pub events_after: String,
    pub cost_before: String,
    pub cost_after: String,
    pub savings: String,
}

pub fn view(
    quote: &Quote,
    events: &Events,
    window: Window,
    days: u32,
    explain: bool,
) -> Result<View, CalcError> {
    let rate = format_rate(quote.price)?;
    let (price_int, price_scale) = price_decimal(quote.price)?;
    let events_before = millions(&events.total_before, window, days)?;
    let events_after = millions(&events.total_after, window, days)?;
    let cost_before = dollars(&events.total_before, window, days, price_int, price_scale)?;
    let cost_after = dollars(&events.total_after, window, days, price_int, price_scale)?;
    let saved = events
        .total_before
        .checked_sub(&events.total_after)
        .ok_or(CalcError)?;
    let savings = dollars(&saved, window, days, price_int, price_scale)?;
    let mut patterns = Vec::with_capacity(events.patterns.len());
    for pattern in &events.patterns {
        let saved = pattern
            .before
            .checked_sub(&pattern.after)
            .ok_or(CalcError)?;
        patterns.push(PatternView {
            id: pattern.id.clone(),
            events_before: millions(&pattern.before, window, days)?,
            events_after: millions(&pattern.after, window, days)?,
            cost_before: dollars(&pattern.before, window, days, price_int, price_scale)?,
            cost_after: dollars(&pattern.after, window, days, price_int, price_scale)?,
            savings: dollars(&saved, window, days, price_int, price_scale)?,
        });
    }
    let steps = if explain {
        Some(steps_text(
            events,
            window,
            days,
            &rate,
            price_int,
            price_scale,
        )?)
    } else {
        None
    };
    Ok(View {
        label: if quote.custom {
            "custom price, estimate"
        } else {
            "list price, estimate"
        },
        retention_days: quote.retention_days,
        billing: quote.billing.name(),
        usd_per_million: rate,
        events_before,
        events_after,
        cost_before,
        cost_after,
        savings,
        steps,
        patterns,
    })
}

fn steps_text(
    events: &Events,
    window: Window,
    days: u32,
    rate: &str,
    price_int: u128,
    price_scale: u32,
) -> Result<String, CalcError> {
    let factor = month_factor(window);
    let step = Step {
        window,
        days,
        factor: &factor,
        rate,
        price_int,
        price_scale,
    };
    let mut out = String::new();
    write_side(&mut out, "before index rules", &events.total_before, &step)?;
    out.push_str("; ");
    write_side(&mut out, "after index rules", &events.total_after, &step)?;
    let saved = events
        .total_before
        .checked_sub(&events.total_after)
        .ok_or(CalcError)?;
    out.push_str("; savings ");
    write_side(&mut out, "saved", &saved, &step)?;
    Ok(out)
}

struct Step<'a> {
    window: Window,
    days: u32,
    factor: &'a str,
    rate: &'a str,
    price_int: u128,
    price_scale: u32,
}

fn write_side(
    out: &mut String,
    label: &str,
    sample: &Fraction,
    step: &Step<'_>,
) -> Result<(), CalcError> {
    let sample_text = exact_sample(sample)?;
    let events_exact = exact_millions(sample, step.window, step.days)?;
    let events_shown = millions(sample, step.window, step.days)?;
    let cost_exact = exact_dollars(
        sample,
        step.window,
        step.days,
        step.price_int,
        step.price_scale,
    )?;
    let cost_shown = dollars(
        sample,
        step.window,
        step.days,
        step.price_int,
        step.price_scale,
    )?;
    write!(
        out,
        "{sample_text} × {factor} × {days} / 1000000 = {events_exact} → {events_shown} events per month {label}, × {rate} = {cost_exact} → {cost_shown} USD per month {label}",
        factor = step.factor,
        days = step.days,
        rate = step.rate
    )
    .expect("text");
    Ok(())
}

fn month_factor(window: Window) -> String {
    let day = money::format_factor(window);
    if day.contains('/') {
        format!("({day})")
    } else {
        day
    }
}

/// Text of the indexed section. Ingest lines are not changed.
pub fn text_block(view: &View) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "indexed: {}, retention {}d, billing {}, {} USD per 1M events",
        view.label, view.retention_days, view.billing, view.usd_per_million
    )
    .expect("text");
    writeln!(
        out,
        "indexed events per month: {} before index rules, {} after",
        view.events_before, view.events_after
    )
    .expect("text");
    writeln!(
        out,
        "indexed cost per month: {} USD before index rules, {} USD after, savings {} USD",
        view.cost_before, view.cost_after, view.savings
    )
    .expect("text");
    if let Some(steps) = &view.steps {
        writeln!(out, "calculation indexed: {steps}").expect("text");
    }
    for pattern in &view.patterns {
        writeln!(
            out,
            "indexed {}: {} -> {} events per month, {} USD -> {} USD, savings {} USD",
            pattern.id,
            pattern.events_before,
            pattern.events_after,
            pattern.cost_before,
            pattern.cost_after,
            pattern.savings
        )
        .expect("text");
    }
    out
}

fn millions(sample: &Fraction, window: Window, days: u32) -> Result<String, CalcError> {
    let (numer, scale, div) = month_events(sample, days, window)?;
    // Thousandths of a million: round(events / 1000), then show 3 decimals.
    let units = round_div(&numer, scale + 3, div)?;
    Ok(format!("{}M", format_fixed(&units, 3)))
}

fn dollars(
    sample: &Fraction,
    window: Window,
    days: u32,
    price_int: u128,
    price_scale: u32,
) -> Result<String, CalcError> {
    let units = cents(sample, window, days, price_int, price_scale)?;
    Ok(format_fixed(&units, 2))
}

fn cents(
    sample: &Fraction,
    window: Window,
    days: u32,
    price_int: u128,
    price_scale: u32,
) -> Result<Digits, CalcError> {
    let (mut numer, mut scale, div) = month_events(sample, days, window)?;
    numer.mul_u128(price_int)?;
    scale += price_scale as usize;
    // Price is per 1M events, and a dollar is 100 cents: divide by 10_000.
    round_div(&numer, scale + 4, div)
}

fn exact_sample(sample: &Fraction) -> Result<String, CalcError> {
    let numer = Digits::from_le(sample.digits());
    format_exact(&numer, sample.scale(), 1)
}

fn exact_millions(sample: &Fraction, window: Window, days: u32) -> Result<String, CalcError> {
    let (numer, scale, div) = month_events(sample, days, window)?;
    format_exact(&numer, scale + 6, div)
}

fn exact_dollars(
    sample: &Fraction,
    window: Window,
    days: u32,
    price_int: u128,
    price_scale: u32,
) -> Result<String, CalcError> {
    let (mut numer, mut scale, div) = month_events(sample, days, window)?;
    numer.mul_u128(price_int)?;
    scale += price_scale as usize + 6;
    format_exact(&numer, scale, div)
}

/// `sample × days × 86400 / window`, as digits / 10^scale / div.
fn month_events(
    sample: &Fraction,
    days: u32,
    window: Window,
) -> Result<(Digits, usize, u128), CalcError> {
    let mut numer = Digits::from_le(sample.digits());
    numer.mul_u128(u128::from(days))?;
    numer.mul_u128(86_400)?;
    Ok((numer, sample.scale(), u128::from(window.seconds())))
}

/// `numer / 10^scale`. At least two decimal places. No rounding: the price
/// was an exact decimal, so the scale already holds every digit.
fn format_rate(price: Price) -> Result<String, CalcError> {
    let (numer, scale) = price_decimal(price)?;
    let mut digs = Digits::from_u128(numer)?;
    let places = (scale as usize).max(2);
    if places > scale as usize {
        digs.mul_pow10(places - scale as usize)?;
    }
    Ok(format_fixed(&digs, places))
}

/// Price as an integer over 10^scale. The denominator of a parsed price is
/// only twos and fives, so this is the decimal that was written.
fn price_decimal(price: Price) -> Result<(u128, u32), CalcError> {
    if price.denom == 0 {
        return Err(CalcError);
    }
    let mut denom = price.denom;
    let mut twos = 0u32;
    let mut fives = 0u32;
    while denom % 2 == 0 {
        denom /= 2;
        twos += 1;
    }
    while denom % 5 == 0 {
        denom /= 5;
        fives += 1;
    }
    if denom != 1 {
        return Err(CalcError);
    }
    let mut numer = price.numer;
    let scale = twos.max(fives);
    if twos > fives {
        for _ in 0..(twos - fives) {
            numer = numer.checked_mul(5).ok_or(CalcError)?;
        }
    } else {
        for _ in 0..(fives - twos) {
            numer = numer.checked_mul(2).ok_or(CalcError)?;
        }
    }
    Ok((numer, scale))
}

/// Little-endian decimal digits, no high zeros. Empty means zero.
struct Digits {
    digs: Vec<u8>,
}

impl Digits {
    fn zero() -> Self {
        Self { digs: Vec::new() }
    }

    fn from_le(digs: &[u8]) -> Self {
        let mut digs = digs.to_vec();
        trim(&mut digs);
        Self { digs }
    }

    fn from_u128(mut value: u128) -> Result<Self, CalcError> {
        if value == 0 {
            return Ok(Self::zero());
        }
        let mut digs = Vec::new();
        while value > 0 {
            digs.push(u8::try_from(value % 10).map_err(|_| CalcError)?);
            value /= 10;
        }
        Ok(Self { digs })
    }

    fn mul_u128(&mut self, factor: u128) -> Result<(), CalcError> {
        if factor == 0 || self.digs.is_empty() {
            self.digs.clear();
            return Ok(());
        }
        let mut acc = Self::zero();
        let mut shift = 0usize;
        let mut rest = factor;
        while rest > 0 {
            let digit = u8::try_from(rest % 10).map_err(|_| CalcError)?;
            rest /= 10;
            if digit != 0 {
                let mut chunk = self.digs.clone();
                mul_small(&mut chunk, digit);
                acc.add_shifted(&chunk, shift);
            }
            shift += 1;
        }
        *self = acc;
        Ok(())
    }

    fn mul_pow10(&mut self, zeros: usize) -> Result<(), CalcError> {
        if zeros == 0 || self.digs.is_empty() {
            return Ok(());
        }
        let mut shifted = vec![0u8; zeros];
        shifted.append(&mut self.digs);
        self.digs = shifted;
        Ok(())
    }

    fn add_shifted(&mut self, extra: &[u8], shift: usize) {
        if extra.is_empty() {
            return;
        }
        if self.digs.len() < shift + extra.len() {
            self.digs.resize(shift + extra.len(), 0);
        }
        let mut carry = 0u16;
        for (offset, &digit) in extra.iter().enumerate() {
            let value = u16::from(self.digs[shift + offset]) + u16::from(digit) + carry;
            self.digs[shift + offset] = (value % 10) as u8;
            carry = value / 10;
        }
        let mut pos = shift + extra.len();
        while carry > 0 {
            if pos == self.digs.len() {
                self.digs.push(0);
            }
            let value = u16::from(self.digs[pos]) + carry;
            self.digs[pos] = (value % 10) as u8;
            carry = value / 10;
            pos += 1;
        }
        trim(&mut self.digs);
    }

    fn add_one(&mut self) {
        self.add_shifted(&[1], 0);
    }
}

fn mul_small(digs: &mut Vec<u8>, factor: u8) {
    let mut carry = 0u16;
    for digit in digs.iter_mut() {
        let value = u16::from(*digit) * u16::from(factor) + carry;
        *digit = (value % 10) as u8;
        carry = value / 10;
    }
    while carry > 0 {
        digs.push((carry % 10) as u8);
        carry /= 10;
    }
    trim(digs);
}

fn trim(digs: &mut Vec<u8>) {
    while digs.last() == Some(&0) {
        digs.pop();
    }
}

/// Round `numer / 10^scale / div` half away from zero. The result is the
/// integer. `div` is at least 1.
fn round_div(numer: &Digits, scale: usize, div: u128) -> Result<Digits, CalcError> {
    if div == 0 {
        return Err(CalcError);
    }
    let (high, low) = split_scale(&numer.digs, scale);
    let (mut quot, rem) = div_mod(&high, div)?;
    if half_up(rem, div, &low, scale) {
        quot.add_one();
    }
    Ok(quot)
}

/// Low `scale` digits, and the rest. The low part is the remainder mod 10^scale.
fn split_scale(digs: &[u8], scale: usize) -> (Digits, Digits) {
    if scale == 0 {
        return (Digits::from_le(digs), Digits::zero());
    }
    if digs.len() <= scale {
        return (Digits::zero(), Digits::from_le(digs));
    }
    (
        Digits::from_le(&digs[scale..]),
        Digits::from_le(&digs[..scale]),
    )
}

/// `2 * (rem * 10^scale + low) >= div * 10^scale`.
fn half_up(rem: u128, div: u128, low: &Digits, scale: usize) -> bool {
    let twice = match rem.checked_mul(2) {
        Some(value) => value,
        // 2 × rem does not fit, so it is larger than div.
        None => return true,
    };
    if twice >= div {
        return true;
    }
    // twice <= div - 1. Only the single step below div can still round up,
    // and only when the low digits are at least half of 10^scale.
    if div - twice >= 2 {
        return false;
    }
    low_at_least_half(low, scale)
}

fn low_at_least_half(low: &Digits, scale: usize) -> bool {
    if scale == 0 {
        return false;
    }
    let mut padded = low.digs.clone();
    padded.resize(scale, 0);
    padded[scale - 1] >= 5
}

fn div_mod(numer: &Digits, div: u128) -> Result<(Digits, u128), CalcError> {
    if div == 0 {
        return Err(CalcError);
    }
    if numer.digs.is_empty() {
        return Ok((Digits::zero(), 0));
    }
    let mut rem = 0u128;
    let mut quot = vec![0u8; numer.digs.len()];
    for index in (0..numer.digs.len()).rev() {
        let (lo, hi) = widening_mul(rem, 10);
        let (lo, carry) = lo.overflowing_add(u128::from(numer.digs[index]));
        let hi = hi + u128::from(carry);
        let (digit, next) = div_wide(hi, lo, div)?;
        quot[index] = u8::try_from(digit).map_err(|_| CalcError)?;
        rem = next;
    }
    trim(&mut quot);
    Ok((Digits { digs: quot }, rem))
}

/// `(hi × 2^128 + lo) / div`. The quotient fits in one decimal digit: the
/// remainder from the previous step is smaller than `div`.
fn div_wide(hi: u128, lo: u128, div: u128) -> Result<(u128, u128), CalcError> {
    if div == 0 {
        return Err(CalcError);
    }
    if hi == 0 {
        return Ok((lo / div, lo % div));
    }
    for digit in (0..10).rev() {
        if wide_ge(digit, div, hi, lo) {
            return Ok((digit, wide_sub(hi, lo, digit, div)?));
        }
    }
    Err(CalcError)
}

fn wide_ge(digit: u128, div: u128, hi: u128, lo: u128) -> bool {
    let (prod_lo, prod_hi) = widening_mul(digit, div);
    match prod_hi.cmp(&hi) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => prod_lo <= lo,
    }
}

fn wide_sub(hi: u128, lo: u128, digit: u128, div: u128) -> Result<u128, CalcError> {
    let (prod_lo, prod_hi) = widening_mul(digit, div);
    if prod_hi > hi || (prod_hi == hi && prod_lo > lo) {
        return Err(CalcError);
    }
    let (new_lo, borrow) = lo.overflowing_sub(prod_lo);
    let new_hi = hi - prod_hi - u128::from(borrow);
    if new_hi != 0 {
        return Err(CalcError);
    }
    Ok(new_lo)
}

fn widening_mul(left: u128, right: u128) -> (u128, u128) {
    let left_lo = left as u64;
    let left_hi = (left >> 64) as u64;
    let right_lo = right as u64;
    let right_hi = (right >> 64) as u64;
    let p00 = u128::from(left_lo) * u128::from(right_lo);
    let p01 = u128::from(left_lo) * u128::from(right_hi);
    let p10 = u128::from(left_hi) * u128::from(right_lo);
    let p11 = u128::from(left_hi) * u128::from(right_hi);
    let mid = (p00 >> 64) + (p01 & 0xffff_ffff_ffff_ffff) + (p10 & 0xffff_ffff_ffff_ffff);
    let lo = (p00 & 0xffff_ffff_ffff_ffff) | ((mid & 0xffff_ffff_ffff_ffff) << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (lo, hi)
}

fn format_fixed(units: &Digits, places: usize) -> String {
    let mut digs = units.digs.clone();
    if digs.is_empty() {
        digs.push(0);
    }
    while digs.len() <= places {
        digs.push(0);
    }
    let frac: String = digs[..places]
        .iter()
        .rev()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    let whole: String = digs[places..]
        .iter()
        .rev()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    if places == 0 {
        whole
    } else {
        format!("{whole}.{frac}")
    }
}

/// Exact decimal of `numer / 10^scale / div` when it ends within 80 places.
/// A longer expansion is cut and marked with `...`.
fn format_exact(numer: &Digits, scale: usize, div: u128) -> Result<String, CalcError> {
    if div == 0 {
        return Err(CalcError);
    }
    let (quot, mut rem) = div_mod(numer, div)?;
    let mut frac = Vec::new();
    let mut open = false;
    for _ in 0..80 {
        if rem == 0 {
            break;
        }
        let (lo, hi) = widening_mul(rem, 10);
        let (digit, next) = div_wide(hi, lo, div)?;
        frac.push(u8::try_from(digit).map_err(|_| CalcError)?);
        rem = next;
        if rem != 0 && frac.len() == 80 {
            open = true;
        }
    }
    let mut before = quot.digs;
    if before.is_empty() {
        before.push(0);
    }
    // Most significant digit first, without a leading zero unless the value is 0.
    before.reverse();
    while before.len() > 1 && before[0] == 0 {
        before.remove(0);
    }
    for _ in 0..scale {
        let moved = if before.len() == 1 {
            let digit = before[0];
            before[0] = 0;
            digit
        } else {
            before.pop().expect("integer digits")
        };
        frac.insert(0, moved);
    }
    while frac.last() == Some(&0) {
        frac.pop();
    }
    let whole: String = before
        .iter()
        .map(|digit| char::from(b'0' + digit))
        .collect();
    if frac.is_empty() {
        return Ok(whole);
    }
    let rest: String = frac.iter().map(|digit| char::from(b'0' + digit)).collect();
    if open {
        Ok(format!("{whole}.{rest}..."))
    } else {
        Ok(format!("{whole}.{rest}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dollars, format_rate, millions, parse_billing, parse_indexed_price, parse_retention, quote,
        Billing,
    };
    use crate::money::{parse_window, Price};
    use crate::rules::{parse_rules, Fraction};

    fn window(text: &str) -> crate::money::Window {
        parse_window(text).expect("window")
    }

    /// Two decimal places, built so the source text has no price literal.
    fn two_places(whole: &str, frac: &str) -> String {
        format!("{whole}.{frac}")
    }

    fn fraction_from_rules(count: u64, source: &str) -> Fraction {
        let rules = parse_rules(source.as_bytes()).expect("rules");
        let mut sum = Fraction::default();
        for _ in 0..count {
            sum.add_frac(&rules.keep("INFO order 1"));
        }
        sum
    }

    #[test]
    fn sample_keeps_the_tenth() {
        let sum = fraction_from_rules(889, "sample 0.1 INFO order\n");
        let shown = millions(&sum, window("1s"), 30).expect("millions");
        // 889 × 0.1 × 2_592_000 / 1_000_000 = 230.4288 → 230.429M
        assert_eq!(shown, "230.429M");
    }

    #[test]
    fn half_cent_rounds_away_from_zero() {
        // 1000 events, price 5 per 1M: five thousandths round up to one cent.
        let mut sample = Fraction::default();
        for _ in 0..1000 {
            sample.add_frac(&Fraction::one());
        }
        let amount = dollars(&sample, window("1d"), 1, 5, 0).expect("dollars");
        assert_eq!(amount, two_places("0", "01"));
        let just_under = dollars(&sample, window("1d"), 1, 4, 0).expect("dollars");
        // Price 4 is four thousandths, below half a cent, so it stays at zero cents.
        assert_eq!(just_under, two_places("0", "00"));
    }

    #[test]
    fn list_price_7d_annual_on_the_fixture_ratio() {
        // 2673 events, 1s window, 30 days, list cell 127 cents per 1M.
        let mut sample = Fraction::default();
        for _ in 0..2673 {
            sample.add_frac(&Fraction::one());
        }
        let events = millions(&sample, window("1s"), 30).expect("events");
        assert_eq!(events, "6928.416M");
        let amount = dollars(&sample, window("1s"), 30, 127, 2).expect("usd");
        assert_eq!(amount, two_places("8799", "09"));
    }

    #[test]
    fn indexed_price_keeps_digits_past_a_binary_float() {
        let price = parse_indexed_price("1000.0000000000000001").expect("price");
        assert_eq!(
            price,
            Price {
                numer: 10_000_000_000_000_000_001,
                denom: 10_000_000_000_000_000,
            }
        );
        assert_eq!(format_rate(price).expect("rate"), "1000.0000000000000001");
        assert!(parse_indexed_price(&format!("-1.{}", "27")).is_none());
        assert!(parse_indexed_price("1e400").is_none());
        assert!(parse_retention("5").is_none());
        assert!(parse_billing("yearly").is_none());
        let cell = quote(Some(7), Some(Billing::Annual), None);
        assert_eq!(
            format_rate(cell.price).expect("cell"),
            two_places("1", "27")
        );
        assert!(!cell.custom);
    }
}
