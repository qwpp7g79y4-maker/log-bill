use std::cmp::Ordering;
use std::fmt::Write;

use serde_json::Value;

use crate::diff::{Report, Row, Status};
use crate::mask::MaskCounts;
use crate::money::{self, Prices, Volume, Window};
use crate::pattern::{self, Stats};
use crate::rules::{Effect, PatternEffect};

const ESTIMATE: &str =
    "estimate: extrapolation of one sample with your own prices; not an official rate";

/// Text of `scan`: the total, and per pattern the id, lines, bytes, share, and
/// the masked pattern. Raw lines do not appear here.
pub fn scan_text(stats: &Stats) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "total: {} {}, {} {}",
        stats.total_lines,
        line_word(stats.total_lines),
        stats.total_bytes,
        bytes_word(u128::from(stats.total_bytes))
    )
    .expect("text");
    for pattern in &stats.patterns {
        writeln!(
            out,
            "{}  {} {}  {} {}  {}%  {}",
            pattern.id,
            pattern.lines,
            line_word(pattern.lines),
            pattern.bytes,
            bytes_word(u128::from(pattern.bytes)),
            pattern::format_share(pattern.share_tenths),
            pattern.text
        )
        .expect("text");
    }
    out
}

/// Masks applied per pattern. With `effect`, also the rule line that matched.
/// A literal tag does not count; a pattern with no mask says `none`.
pub fn explain_patterns(stats: &Stats, effect: Option<&Effect>) -> String {
    let mut out = String::new();
    for (index, pattern) in stats.patterns.iter().enumerate() {
        writeln!(out, "masks {}: {}", pattern.id, mask_text(pattern.masks)).expect("text");
        if let Some(effect) = effect {
            debug_assert_eq!(
                effect.patterns.get(index).map(|item| item.id.as_str()),
                Some(pattern.id.as_str())
            );
            let line_word = effect
                .patterns
                .get(index)
                .map(|item| item.rule_lines.as_slice())
                .unwrap_or(&[]);
            write_rule_lines(&mut out, &pattern.id, line_word);
        }
    }
    out
}

fn mask_text(counts: MaskCounts) -> String {
    let pairs = [
        ("<TS>", counts.ts, ""),
        ("<UUID>", counts.uuid, ""),
        ("<IP>", counts.ip, ""),
        ("<EMAIL>", counts.email, ""),
        ("<SECRET>", counts.secret, " (possible secret)"),
        ("<HEX>", counts.hex, ""),
        ("<NUM>", counts.num, ""),
        ("<VAR>", counts.var, ""),
    ];
    let text_body = pairs
        .into_iter()
        .filter(|(_, count, _)| *count > 0)
        .map(|(name, count, extra)| format!("{name} {count}{extra}"))
        .collect::<Vec<_>>()
        .join(", ");
    if text_body.is_empty() {
        "none".to_owned()
    } else {
        text_body
    }
}

fn write_rule_lines(out: &mut String, id: &str, line_word: &[u64]) {
    match line_word {
        [] => writeln!(out, "rule line {id}: none"),
        [one] => writeln!(out, "rule line {id}: {one}"),
        many => {
            let list = many
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "rule lines {id}: {list}")
        }
    }
    .expect("text");
}

/// Factor, bytes, and GB per day and per month. Call only with a valid window.
pub fn volume_text(window: Window, volume: &Volume) -> String {
    let mut out = String::new();
    writeln!(out, "factor: {}", money::format_factor(window)).expect("text");
    writeln!(
        out,
        "per day: {} {}, {} GB",
        volume.bytes_per_day,
        bytes_word(volume.bytes_per_day),
        money::format_gb(volume.gb_per_day_milli)
    )
    .expect("text");
    writeln!(
        out,
        "per month: {} {}, {} GB",
        volume.bytes_per_month,
        bytes_word(volume.bytes_per_month),
        money::format_gb(volume.gb_per_month_milli)
    )
    .expect("text");
    out
}

/// Cost per pattern and total, with currency, source, date, and the estimate line.
/// Call only when both `--prices` and `--window` are set.
/// With `effect`, savings are included, before the estimate line.
pub fn costs_text(
    stats: &Stats,
    window: Window,
    days_per_month: u32,
    prices: &Prices,
    effect: Option<&Effect>,
    explain: bool,
) -> Result<String, money::CalcError> {
    let mut out = String::new();
    writeln!(
        out,
        "prices: {}, source {}, date {}",
        prices.currency, prices.source, prices.date
    )
    .expect("text");
    let mut write_line = |label: &str, bytes: u64| -> Result<(), money::CalcError> {
        let mut write_stream = |name: &str, price: money::Price| -> Result<(), money::CalcError> {
            let cost = money::cost(bytes, window, days_per_month, price, prices.gb_bytes)?;
            writeln!(
                out,
                "{label} {name}: {} {} per day, {} {} per month",
                cost.per_day, prices.currency, cost.per_month, prices.currency
            )
            .expect("text");
            if explain {
                let steps =
                    money::explain_steps(bytes, window, days_per_month, price, prices.gb_bytes)?;
                writeln!(out, "calculation {label} {name}: {steps}").expect("text");
            }
            Ok(())
        };
        write_stream("ingest", prices.ingest_per_gb)?;
        if let Some(index) = prices.index_per_gb {
            write_stream("index", index)?;
        }
        Ok(())
    };
    write_line("total", stats.total_bytes)?;
    for pattern in &stats.patterns {
        write_line(&pattern.id, pattern.bytes)?;
    }
    if let Some(effect) = effect {
        write_line("savings total", effect.old_bytes - effect.new_bytes)?;
        for pattern in &effect.patterns {
            let label = format!("savings {}", pattern.id);
            write_line(&label, pattern.old_bytes - pattern.new_bytes)?;
        }
    }
    writeln!(out, "{ESTIMATE}").expect("text");
    Ok(out)
}

/// Old, new, and savings in lines and bytes. No raw line and no regex.
pub fn rules_bytes(effect: &Effect) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "after rules: {} -> {}, savings {}",
        piece(effect.old_lines, effect.old_bytes),
        piece(effect.new_lines, effect.new_bytes),
        piece(
            effect.old_lines - effect.new_lines,
            effect.old_bytes - effect.new_bytes
        )
    )
    .expect("text");
    for pattern in &effect.patterns {
        write!(
            out,
            "{}  {} -> {}, savings {}",
            pattern.id,
            piece(pattern.old_lines, pattern.old_bytes),
            piece(pattern.new_lines, pattern.new_bytes),
            piece(
                pattern.old_lines - pattern.new_lines,
                pattern.old_bytes - pattern.new_bytes
            )
        )
        .expect("text");
        match pattern.rule_lines.as_slice() {
            [] => {}
            [one] => write!(out, ", rule {one}").expect("text"),
            many => {
                let list = many
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(out, ", rules {list}").expect("text");
            }
        }
        writeln!(out).expect("text");
    }
    out
}

/// Text of `diff`: total change, new and gone patterns, and per pattern
/// the byte and share change, plus the share of the new patterns.
/// `line_word` is the rule count when `--rules` is set. The regex is omitted.
pub fn diff_text(report: &Report, line_word: Option<usize>) -> String {
    let mut out = String::new();
    let line_delta = i128::from(report.new_lines) - i128::from(report.old_lines);
    let byte_delta = i128::from(report.new_bytes) - i128::from(report.old_bytes);
    writeln!(
        out,
        "total: {} -> {}, change {}, {} {}",
        piece(report.old_lines, report.old_bytes),
        piece(report.new_lines, report.new_bytes),
        signed_lines(line_delta),
        signed(byte_delta),
        bytes_word(byte_delta.unsigned_abs())
    )
    .expect("text");
    if let Some(item_count) = line_word {
        writeln!(out, "rules applied: {item_count}").expect("text");
    }
    for row in &report.rows {
        let label = match row.status {
            Status::New => "new ",
            Status::Gone => "gone ",
            Status::Both => "",
        };
        let bytes = i128::from(row.new_bytes) - i128::from(row.old_bytes);
        let share = i128::from(row.new_share) - i128::from(row.old_share);
        writeln!(
            out,
            "{label}{}  {} -> {} {}, change {} {}, share {}% -> {}%, change {} percentage points  {}",
            row.id,
            row.old_bytes,
            row.new_bytes,
            bytes_word(u128::from(row.new_bytes)),
            signed(bytes),
            bytes_word(bytes.unsigned_abs()),
            pattern::format_share(row.old_share),
            pattern::format_share(row.new_share),
            signed_share(share),
            row.text
        )
        .expect("text");
    }
    writeln!(
        out,
        "new patterns: {} of {} {}, {}%",
        report.added_bytes,
        report.new_bytes,
        bytes_word(u128::from(report.new_bytes)),
        pattern::format_share(pattern::share_tenths(report.added_bytes, report.new_bytes))
    )
    .expect("text");
    out
}

/// Monthly amount of the byte difference, separate for ingest and index.
/// The difference is rounded once, not the difference of two rounded amounts.
/// Call only when both `--prices` and `--window` are set.
pub fn diff_costs(
    old_bytes: u64,
    new_bytes: u64,
    window: Window,
    days_per_month: u32,
    prices: &Prices,
) -> Result<String, money::CalcError> {
    let delta = i128::from(new_bytes) - i128::from(old_bytes);
    let sample = u64::try_from(delta.unsigned_abs()).expect("bytes");
    let sign = match delta.cmp(&0) {
        Ordering::Greater => "+",
        Ordering::Less => "-",
        Ordering::Equal => "",
    };
    let mut out = String::new();
    writeln!(
        out,
        "prices: {}, source {}, date {}",
        prices.currency, prices.source, prices.date
    )
    .expect("text");
    let mut write_line = |name: &str, price: money::Price| -> Result<(), money::CalcError> {
        let cost = money::cost(sample, window, days_per_month, price, prices.gb_bytes)?;
        writeln!(
            out,
            "change per month {name}: {sign}{} {}",
            cost.per_month, prices.currency
        )
        .expect("text");
        Ok(())
    };
    write_line("ingest", prices.ingest_per_gb)?;
    if let Some(index) = prices.index_per_gb {
        write_line("index", index)?;
    }
    writeln!(out, "{ESTIMATE}").expect("text");
    Ok(out)
}

fn piece(lines: u64, bytes: u64) -> String {
    format!(
        "{lines} {}, {bytes} {}",
        line_word(lines),
        bytes_word(u128::from(bytes))
    )
}

fn signed(delta: i128) -> String {
    if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}

fn signed_lines(delta: i128) -> String {
    let n = u64::try_from(delta.unsigned_abs()).expect("lines");
    let word = line_word(n);
    match delta.cmp(&0) {
        Ordering::Greater => format!("+{n} {word}"),
        Ordering::Less => format!("-{n} {word}"),
        Ordering::Equal => format!("0 {word}"),
    }
}

fn signed_share(delta: i128) -> String {
    let tenths = delta.unsigned_abs();
    let text_body = format!("{}.{}", tenths / 10, tenths % 10);
    match delta.cmp(&0) {
        Ordering::Greater => format!("+{text_body}"),
        Ordering::Less => format!("-{text_body}"),
        Ordering::Equal => text_body,
    }
}

/// The unit agrees with the number directly before it: `1 byte`, `0 bytes`, `2 bytes`.
fn bytes_word(n: u128) -> &'static str {
    if n == 1 {
        "byte"
    } else {
        "bytes"
    }
}

fn line_word(n: u64) -> &'static str {
    if n == 1 {
        "line"
    } else {
        "lines"
    }
}

/// What `--json` stores about the call. Paths are the arguments, not the contents.
pub struct JsonMeta<'a> {
    pub command: &'a str,
    pub format: &'a str,
    pub message_field: Option<&'a str>,
    pub days_per_month: u32,
    pub top: Option<u32>,
    pub paths: &'a [&'a str],
    pub window: Option<Window>,
    pub explain: bool,
}

/// JSON of `scan`. `rules` is set exactly when `--rules` is set.
/// `costs` is included only when both `window` and `prices` are set.
/// Amounts and shares use the same text as the text output.
pub fn scan_json(
    meta: &JsonMeta<'_>,
    stats: &Stats,
    rules: Option<(&Effect, usize)>,
    prices: Option<&Prices>,
) -> Result<String, money::CalcError> {
    let gb_bytes = prices.map(|item| item.gb_bytes).unwrap_or(money::GB_BYTES);
    let volume = match meta.window {
        Some(window) => Some(money::extrapolate(
            stats.total_bytes,
            window,
            meta.days_per_month,
            gb_bytes,
        )?),
        None => None,
    };
    let with_costs = meta.window.is_some() && prices.is_some();
    let mut fields = vec![
        ("schema_version", num_u64(1)),
        ("tool", lit("log-bill")),
        ("estimate", estimate_value(with_costs)),
        ("inputs", inputs_json(meta)),
        ("window", window_json(meta.window)),
        ("totals", scan_totals(stats, volume.as_ref())),
        (
            "patterns",
            Value::Array(
                stats
                    .patterns
                    .iter()
                    .map(|pattern| scan_pattern(pattern, meta.explain))
                    .collect(),
            ),
        ),
    ];
    if let Some((effect, applied)) = rules {
        fields.push(("rules", rules_json(effect, applied)));
    }
    if let (Some(window), Some(prices)) = (meta.window, prices) {
        fields.push((
            "costs",
            scan_costs(
                stats,
                window,
                meta.days_per_month,
                prices,
                rules.map(|(effect, _)| effect),
                meta.explain,
            )?,
        ));
    }
    Ok(finish(obj(fields)))
}

/// JSON of `diff`. `patterns` is `added`, `removed`, and `changed`, in order.
/// `changed` is every pattern in both samples, even when the difference is 0.
/// `rules` names only the rule count: totals are already after the rules.
pub fn diff_json(
    meta: &JsonMeta<'_>,
    report: &Report,
    rules_applied: Option<usize>,
    prices: Option<&Prices>,
) -> Result<String, money::CalcError> {
    let (patterns, added, removed, changed) = split_rows(&report.rows);
    let with_costs = meta.window.is_some() && prices.is_some();
    let mut fields = vec![
        ("schema_version", num_u64(1)),
        ("tool", lit("log-bill")),
        ("estimate", estimate_value(with_costs)),
        ("inputs", inputs_json(meta)),
        ("window", window_json(meta.window)),
        ("totals", diff_totals(report)),
        ("patterns", Value::Array(patterns)),
        ("added", Value::Array(added)),
        ("removed", Value::Array(removed)),
        ("changed", Value::Array(changed)),
        (
            "totals_delta",
            diff_delta(
                report.old_lines,
                report.old_bytes,
                report.new_lines,
                report.new_bytes,
            ),
        ),
    ];
    if let Some(applied) = rules_applied {
        fields.push(("rules", obj([("applied", num_usize(applied))])));
    }
    if let (Some(window), Some(prices)) = (meta.window, prices) {
        fields.push((
            "costs",
            diff_costs_json(
                report.old_bytes,
                report.new_bytes,
                window,
                meta.days_per_month,
                prices,
            )?,
        ));
    }
    Ok(finish(obj(fields)))
}

fn estimate_value(with_costs: bool) -> Value {
    if with_costs {
        lit(ESTIMATE)
    } else {
        Value::Null
    }
}

fn inputs_json(meta: &JsonMeta<'_>) -> Value {
    obj([
        ("command", lit(meta.command)),
        ("format", lit(meta.format)),
        ("message_field", opt_lit(meta.message_field)),
        ("days_per_month", num_u64(u64::from(meta.days_per_month))),
        (
            "top",
            meta.top.map_or(Value::Null, |n| num_u64(u64::from(n))),
        ),
        ("paths", strings(meta.paths)),
    ])
}

fn window_json(window: Option<Window>) -> Value {
    match window {
        None => Value::Null,
        Some(window) => obj([
            ("seconds", num_u64(window.seconds())),
            ("factor", lit(&money::format_factor(window))),
        ]),
    }
}

fn scan_totals(stats: &Stats, volume: Option<&Volume>) -> Value {
    let mut fields = vec![
        ("lines", num_u64(stats.total_lines)),
        ("bytes", num_u64(stats.total_bytes)),
    ];
    if let Some(volume) = volume {
        fields.push((
            "per_day",
            volume_side(volume.bytes_per_day, volume.gb_per_day_milli),
        ));
        fields.push((
            "per_month",
            volume_side(volume.bytes_per_month, volume.gb_per_month_milli),
        ));
    }
    obj(fields)
}

fn volume_side(bytes: u128, gb_milli: u128) -> Value {
    obj([
        ("bytes", num_u128(bytes)),
        ("gb", lit(&money::format_gb(gb_milli))),
    ])
}

fn scan_pattern(pattern: &pattern::Pattern, explain: bool) -> Value {
    let mut fields = vec![
        ("id", lit(&pattern.id)),
        ("text", lit(&pattern.text)),
        ("lines", num_u64(pattern.lines)),
        ("bytes", num_u64(pattern.bytes)),
        ("share", lit(&pattern::format_share(pattern.share_tenths))),
        ("rest", Value::Bool(pattern.rest)),
    ];
    if explain {
        fields.push(("masks", masks_json(pattern.masks)));
    }
    if pattern.masks.secret > 0 {
        fields.push(("note", lit("possible secret")));
    }
    obj(fields)
}

fn masks_json(counts: MaskCounts) -> Value {
    obj([
        ("ts", num_u64(counts.ts)),
        ("uuid", num_u64(counts.uuid)),
        ("ip", num_u64(counts.ip)),
        ("email", num_u64(counts.email)),
        ("secret", num_u64(counts.secret)),
        ("hex", num_u64(counts.hex)),
        ("num", num_u64(counts.num)),
        ("var", num_u64(counts.var)),
    ])
}

fn rules_json(effect: &Effect, applied: usize) -> Value {
    obj([
        ("applied", num_usize(applied)),
        ("old", count_pair(effect.old_lines, effect.old_bytes)),
        ("new", count_pair(effect.new_lines, effect.new_bytes)),
        (
            "saved",
            count_pair(
                effect.old_lines - effect.new_lines,
                effect.old_bytes - effect.new_bytes,
            ),
        ),
        (
            "patterns",
            Value::Array(effect.patterns.iter().map(rule_pattern).collect()),
        ),
    ])
}

fn rule_pattern(pattern: &PatternEffect) -> Value {
    obj([
        ("id", lit(&pattern.id)),
        ("text", lit(&pattern.text)),
        ("old_lines", num_u64(pattern.old_lines)),
        ("old_bytes", num_u64(pattern.old_bytes)),
        ("new_lines", num_u64(pattern.new_lines)),
        ("new_bytes", num_u64(pattern.new_bytes)),
        (
            "saved_lines",
            num_u64(pattern.old_lines - pattern.new_lines),
        ),
        (
            "saved_bytes",
            num_u64(pattern.old_bytes - pattern.new_bytes),
        ),
        ("rule_lines", u64s(&pattern.rule_lines)),
    ])
}

fn scan_costs(
    stats: &Stats,
    window: Window,
    days: u32,
    prices: &Prices,
    effect: Option<&Effect>,
    explain: bool,
) -> Result<Value, money::CalcError> {
    let mut totals = vec![("bytes", num_u64(stats.total_bytes))];
    push_streams(
        &mut totals,
        stats.total_bytes,
        window,
        days,
        prices,
        explain,
    )?;
    let mut patterns = Vec::new();
    for pattern in &stats.patterns {
        let mut fields = vec![("id", lit(&pattern.id)), ("bytes", num_u64(pattern.bytes))];
        push_streams(&mut fields, pattern.bytes, window, days, prices, explain)?;
        patterns.push(obj(fields));
    }
    let mut fields = vec![
        ("currency", lit(&prices.currency)),
        ("source", lit(&prices.source)),
        ("date", lit(&prices.date)),
        ("gb_bytes", num_u128(prices.gb_bytes)),
        ("totals", obj(totals)),
        ("patterns", Value::Array(patterns)),
    ];
    if let Some(effect) = effect {
        fields.push((
            "savings",
            savings_json(effect, window, days, prices, explain)?,
        ));
    }
    Ok(obj(fields))
}

fn savings_json(
    effect: &Effect,
    window: Window,
    days: u32,
    prices: &Prices,
    explain: bool,
) -> Result<Value, money::CalcError> {
    let bytes = effect.old_bytes - effect.new_bytes;
    let mut fields = vec![("bytes", num_u64(bytes))];
    push_streams(&mut fields, bytes, window, days, prices, explain)?;
    let mut patterns = Vec::new();
    for pattern in &effect.patterns {
        let saved = pattern.old_bytes - pattern.new_bytes;
        let mut item = vec![("id", lit(&pattern.id)), ("bytes", num_u64(saved))];
        push_streams(&mut item, saved, window, days, prices, explain)?;
        patterns.push(obj(item));
    }
    fields.push(("patterns", Value::Array(patterns)));
    Ok(obj(fields))
}

fn push_streams(
    fields: &mut Vec<(&'static str, Value)>,
    bytes: u64,
    window: Window,
    days: u32,
    prices: &Prices,
    explain: bool,
) -> Result<(), money::CalcError> {
    fields.push((
        "ingest",
        stream(
            bytes,
            window,
            days,
            prices.ingest_per_gb,
            prices.gb_bytes,
            explain,
        )?,
    ));
    if let Some(index) = prices.index_per_gb {
        fields.push((
            "index",
            stream(bytes, window, days, index, prices.gb_bytes, explain)?,
        ));
    }
    Ok(())
}

fn stream(
    bytes: u64,
    window: Window,
    days: u32,
    price: money::Price,
    gb_bytes: u128,
    explain: bool,
) -> Result<Value, money::CalcError> {
    let cost = money::cost(bytes, window, days, price, gb_bytes)?;
    let mut fields = vec![
        ("per_day", lit(&cost.per_day.to_string())),
        ("per_month", lit(&cost.per_month.to_string())),
    ];
    if explain {
        fields.push((
            "steps",
            lit(&money::explain_steps(bytes, window, days, price, gb_bytes)?),
        ));
    }
    Ok(obj(fields))
}

fn diff_totals(report: &Report) -> Value {
    obj([
        ("old", count_pair(report.old_lines, report.old_bytes)),
        ("new", count_pair(report.new_lines, report.new_bytes)),
        ("added_bytes", num_u64(report.added_bytes)),
        (
            "added_share",
            lit(&pattern::format_share(pattern::share_tenths(
                report.added_bytes,
                report.new_bytes,
            ))),
        ),
    ])
}

fn diff_delta(old_lines: u64, old_bytes: u64, new_lines: u64, new_bytes: u64) -> Value {
    obj([
        (
            "lines",
            num_i128(i128::from(new_lines) - i128::from(old_lines)),
        ),
        (
            "bytes",
            num_i128(i128::from(new_bytes) - i128::from(old_bytes)),
        ),
    ])
}

fn split_rows(rows: &[Row]) -> (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut patterns = Vec::new();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for row in rows {
        let value = diff_row(row);
        match row.status {
            Status::New => added.push(value.clone()),
            Status::Gone => removed.push(value.clone()),
            Status::Both => changed.push(value.clone()),
        }
        patterns.push(value);
    }
    (patterns, added, removed, changed)
}

fn diff_row(row: &Row) -> Value {
    obj([
        ("id", lit(&row.id)),
        ("text", lit(&row.text)),
        (
            "lines",
            ends(
                row.old_lines,
                row.new_lines,
                num_i128(i128::from(row.new_lines) - i128::from(row.old_lines)),
            ),
        ),
        (
            "bytes",
            ends(
                row.old_bytes,
                row.new_bytes,
                num_i128(i128::from(row.new_bytes) - i128::from(row.old_bytes)),
            ),
        ),
        (
            "share",
            obj([
                ("old", lit(&pattern::format_share(row.old_share))),
                ("new", lit(&pattern::format_share(row.new_share))),
                (
                    "delta",
                    lit(&signed_share(
                        i128::from(row.new_share) - i128::from(row.old_share),
                    )),
                ),
            ]),
        ),
    ])
}

fn ends(old: u64, new: u64, delta: Value) -> Value {
    obj([
        ("old", num_u64(old)),
        ("new", num_u64(new)),
        ("delta", delta),
    ])
}

fn diff_costs_json(
    old_bytes: u64,
    new_bytes: u64,
    window: Window,
    days: u32,
    prices: &Prices,
) -> Result<Value, money::CalcError> {
    let delta = i128::from(new_bytes) - i128::from(old_bytes);
    let sample = u64::try_from(delta.unsigned_abs()).expect("bytes");
    let ingest = money::cost(sample, window, days, prices.ingest_per_gb, prices.gb_bytes)?;
    let mut month = vec![
        ("bytes_delta", num_i128(delta)),
        (
            "ingest",
            lit(&signed_money(delta, &ingest.per_month.to_string())),
        ),
    ];
    if let Some(index) = prices.index_per_gb {
        let cost = money::cost(sample, window, days, index, prices.gb_bytes)?;
        month.push((
            "index",
            lit(&signed_money(delta, &cost.per_month.to_string())),
        ));
    }
    Ok(obj([
        ("currency", lit(&prices.currency)),
        ("source", lit(&prices.source)),
        ("date", lit(&prices.date)),
        ("gb_bytes", num_u128(prices.gb_bytes)),
        ("month", obj(month)),
    ]))
}

fn signed_money(delta: i128, amount: &str) -> String {
    match delta.cmp(&0) {
        Ordering::Greater => format!("+{amount}"),
        Ordering::Less => format!("-{amount}"),
        Ordering::Equal => amount.to_owned(),
    }
}

fn count_pair(lines: u64, bytes: u64) -> Value {
    obj([("lines", num_u64(lines)), ("bytes", num_u64(bytes))])
}

fn strings(items: &[&str]) -> Value {
    Value::Array(items.iter().copied().map(lit).collect())
}

fn u64s(items: &[u64]) -> Value {
    Value::Array(items.iter().copied().map(num_u64).collect())
}

fn opt_lit(text: Option<&str>) -> Value {
    match text {
        Some(text) => lit(text),
        None => Value::Null,
    }
}

fn lit(text: &str) -> Value {
    Value::String(text.to_owned())
}

fn num_u64(value: u64) -> Value {
    Value::Number(value.into())
}

fn num_usize(value: usize) -> Value {
    num_u64(u64::try_from(value).expect("lines"))
}

/// A JSON number goes up to 2^64-1. Above that, decimal text, no truncation.
fn num_u128(value: u128) -> Value {
    match u64::try_from(value) {
        Ok(small) => num_u64(small),
        Err(_) => Value::String(value.to_string()),
    }
}

/// A negative value below i64::MIN does not fit in a JSON number.
fn num_i128(value: i128) -> Value {
    if let Ok(small) = i64::try_from(value) {
        Value::Number(small.into())
    } else if let Ok(big) = u64::try_from(value) {
        Value::Number(big.into())
    } else {
        Value::String(value.to_string())
    }
}

fn obj(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in fields {
        map.insert(key.to_owned(), value);
    }
    Value::Object(map)
}

fn finish(value: Value) -> String {
    let mut out = serde_json::to_string_pretty(&value).expect("json");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::{costs_text, explain_patterns, rules_bytes};
    use crate::mask::MaskCounts;
    use crate::money::{parse_prices, parse_window};
    use crate::pattern::{Pattern, Stats};
    use crate::rules::{Effect, PatternEffect};

    fn cents_text(cents: u64) -> String {
        format!("{}.{:02}", cents / 100, cents % 100)
    }

    #[test]
    fn savings_round_the_difference_not_the_amounts() {
        // gb 2000, price 1, window 1d, 30 days.
        // 20 bytes: 20*100/2000 = 1 cent per day. Month 20*30*100/2000 = 30 cents.
        // 10 bytes: 10*100/2000 = half a cent, rounded to 1 cent per day.
        // The difference of those rounded daily amounts is 0 cents.
        // The savings are those 10 bytes themselves and round to 1 cent.
        // Month of 10 bytes: 10*30*100/2000 = 15 cents, not 1 cent times 30.
        let stats = Stats {
            total_lines: 2,
            total_bytes: 20,
            patterns: vec![Pattern {
                id: "Ptest".to_owned(),
                text: "abc".to_owned(),
                lines: 2,
                bytes: 20,
                share_tenths: 1000,
                rest: false,
                masks: MaskCounts::default(),
            }],
        };
        let effect = Effect {
            old_lines: 2,
            old_bytes: 20,
            new_lines: 1,
            new_bytes: 10,
            patterns: vec![PatternEffect {
                id: "Ptest".to_owned(),
                text: "abc".to_owned(),
                old_lines: 2,
                old_bytes: 20,
                new_lines: 1,
                new_bytes: 10,
                rule_lines: vec![1],
            }],
        };
        let prices = parse_prices(
            br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":2000}"#,
        )
        .unwrap();
        let text_body = costs_text(
            &stats,
            parse_window("1d").unwrap(),
            30,
            &prices,
            Some(&effect),
            false,
        )
        .unwrap();
        let one_cent = cents_text(1);
        let thirty_cents = cents_text(30);
        let fifteen_cents = cents_text(15);
        assert!(
            text_body.contains(&format!(
                "total ingest: {one_cent} EUR per day, {thirty_cents} EUR per month\n"
            )),
            "{text_body}"
        );
        assert!(
            text_body.contains(&format!(
                "savings total ingest: {one_cent} EUR per day, {fifteen_cents} EUR per month\n"
            )),
            "{text_body}"
        );
        assert!(
            text_body.contains(&format!(
                "savings Ptest ingest: {one_cent} EUR per day, {fifteen_cents} EUR per month\n"
            )),
            "{text_body}"
        );
        assert!(text_body.ends_with(
            "estimate: extrapolation of one sample with your own prices; not an official rate\n"
        ));
    }

    #[test]
    fn several_lines_stay_in_order() {
        // Two rules matched. The text names both numbers, not the regex.
        let effect = Effect {
            old_lines: 2,
            old_bytes: 14,
            new_lines: 0,
            new_bytes: 0,
            patterns: vec![PatternEffect {
                id: "Puser".to_owned(),
                text: "user".to_owned(),
                old_lines: 2,
                old_bytes: 14,
                new_lines: 0,
                new_bytes: 0,
                rule_lines: vec![1, 2],
            }],
        };
        let text_body = rules_bytes(&effect);
        assert_eq!(
            text_body,
            "after rules: 2 lines, 14 bytes -> 0 lines, 0 bytes, savings 2 lines, 14 bytes\n\
             Puser  2 lines, 14 bytes -> 0 lines, 0 bytes, savings 2 lines, 14 bytes, rules 1, 2\n"
        );
    }

    #[test]
    fn explain_names_masks_and_each_line_number() {
        // Two lines, together two `<NUM>`. Rules 1 and 2 both matched.
        // Without an effect there is no rule line. Mask order is fixed.
        let stats = Stats {
            total_lines: 2,
            total_bytes: 14,
            patterns: vec![Pattern {
                id: "Puser".to_owned(),
                text: "user <NUM>".to_owned(),
                lines: 2,
                bytes: 14,
                share_tenths: 1000,
                rest: false,
                masks: MaskCounts {
                    ts: 1,
                    num: 2,
                    var: 4,
                    ..MaskCounts::default()
                },
            }],
        };
        let effect = Effect {
            old_lines: 2,
            old_bytes: 14,
            new_lines: 0,
            new_bytes: 0,
            patterns: vec![PatternEffect {
                id: "Puser".to_owned(),
                text: "user <NUM>".to_owned(),
                old_lines: 2,
                old_bytes: 14,
                new_lines: 0,
                new_bytes: 0,
                rule_lines: vec![1, 2],
            }],
        };
        assert_eq!(
            explain_patterns(&stats, None),
            "masks Puser: <TS> 1, <NUM> 2, <VAR> 4\n"
        );
        assert_eq!(
            explain_patterns(&stats, Some(&effect)),
            "masks Puser: <TS> 1, <NUM> 2, <VAR> 4\nrule lines Puser: 1, 2\n"
        );
    }

    #[test]
    fn secret_count_says_possible_secret() {
        let stats = Stats {
            total_lines: 1,
            total_bytes: 32,
            patterns: vec![Pattern {
                id: "Pzin".to_owned(),
                text: "This is a secret <SECRET> report".to_owned(),
                lines: 1,
                bytes: 32,
                share_tenths: 1000,
                rest: false,
                masks: MaskCounts {
                    secret: 1,
                    ..MaskCounts::default()
                },
            }],
        };
        assert_eq!(
            explain_patterns(&stats, None),
            "masks Pzin: <SECRET> 1 (possible secret)\n"
        );
        let mut call = meta(None);
        call.explain = false;
        let without = super::scan_json(&call, &stats, None, None).unwrap();
        let without: serde_json::Value = serde_json::from_str(&without).unwrap();
        assert!(without["patterns"][0].get("masks").is_none());
        assert_eq!(without["patterns"][0]["note"], "possible secret");
        assert_eq!(
            without["patterns"][0]["text"],
            "This is a secret <SECRET> report"
        );
        call.explain = true;
        let with = super::scan_json(&call, &stats, None, None).unwrap();
        let with: serde_json::Value = serde_json::from_str(&with).unwrap();
        assert_eq!(with["patterns"][0]["masks"]["secret"], 1);
        assert_eq!(with["patterns"][0]["note"], "possible secret");
        for key_name in ["ts", "uuid", "ip", "email", "secret", "hex", "num", "var"] {
            assert!(
                with["patterns"][0]["masks"].get(key_name).is_some(),
                "{key_name}"
            );
        }
    }

    fn meta(window: Option<crate::money::Window>) -> super::JsonMeta<'static> {
        super::JsonMeta {
            command: "scan",
            format: "text",
            message_field: None,
            days_per_month: 30,
            top: None,
            paths: &["x"],
            window,
            explain: false,
        }
    }

    #[test]
    fn json_savings_round_the_difference_not_the_amounts() {
        // Same fraction as the text test: 10 bytes is half a cent per day, rounded to 1.
        // The difference of the rounded daily amounts would be 0. The savings are 1 cent.
        // Month of those 10 bytes: 15 cents, not 1 cent times 30.
        let stats = Stats {
            total_lines: 2,
            total_bytes: 20,
            patterns: vec![Pattern {
                id: "Ptest".to_owned(),
                text: "abc".to_owned(),
                lines: 2,
                bytes: 20,
                share_tenths: 1000,
                rest: false,
                masks: MaskCounts::default(),
            }],
        };
        let effect = Effect {
            old_lines: 2,
            old_bytes: 20,
            new_lines: 1,
            new_bytes: 10,
            patterns: vec![PatternEffect {
                id: "Ptest".to_owned(),
                text: "abc".to_owned(),
                old_lines: 2,
                old_bytes: 20,
                new_lines: 1,
                new_bytes: 10,
                rule_lines: vec![1],
            }],
        };
        let prices = parse_prices(
            br#"{"currency":"EUR","source":"scratchpad","date":"1999-01-02","ingest_per_gb":1,"gb_bytes":2000}"#,
        )
        .unwrap();
        let body = super::scan_json(
            &meta(Some(parse_window("1d").unwrap())),
            &stats,
            Some((&effect, 1)),
            Some(&prices),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let one_cent = cents_text(1);
        let fifteen_cents = cents_text(15);
        let thirty_cents = cents_text(30);
        assert_eq!(value["costs"]["totals"]["ingest"]["per_day"], one_cent);
        assert_eq!(
            value["costs"]["totals"]["ingest"]["per_month"],
            thirty_cents
        );
        assert_eq!(value["costs"]["savings"]["bytes"], 10);
        assert_eq!(value["costs"]["savings"]["ingest"]["per_day"], one_cent);
        assert_eq!(
            value["costs"]["savings"]["ingest"]["per_month"],
            fifteen_cents
        );
        assert_eq!(
            value["costs"]["savings"]["patterns"][0]["ingest"]["per_month"],
            fifteen_cents
        );
        assert_ne!(
            value["costs"]["savings"]["ingest"]["per_day"],
            cents_text(0)
        );
    }

    #[test]
    fn volume_and_difference_past_json_number_stay_text() {
        // 300_000_000_000_000 × 86400 = 25_920_000_000_000_000_000, above u64::MAX.
        // Month × 30 = 777_600_000_000_000_000_000.
        let stats = Stats {
            total_lines: 1,
            total_bytes: 300_000_000_000_000,
            patterns: vec![],
        };
        let body =
            super::scan_json(&meta(Some(parse_window("1s").unwrap())), &stats, None, None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["totals"]["per_day"]["bytes"], "25920000000000000000");
        assert_eq!(
            value["totals"]["per_month"]["bytes"],
            "777600000000000000000"
        );

        // -u64::MAX does not fit in i64. +u64::MAX does fit in a JSON number.
        let below = super::diff_json(
            &meta(None),
            &crate::diff::Report {
                old_lines: u64::MAX,
                old_bytes: u64::MAX,
                new_lines: 0,
                new_bytes: 0,
                rows: vec![crate::diff::Row {
                    id: "P".to_owned(),
                    text: "x".to_owned(),
                    status: crate::diff::Status::Both,
                    old_lines: u64::MAX,
                    new_lines: 0,
                    old_bytes: u64::MAX,
                    new_bytes: 0,
                    old_share: 1000,
                    new_share: 0,
                }],
                added_bytes: 0,
            },
            None,
            None,
        )
        .unwrap();
        let below: serde_json::Value = serde_json::from_str(&below).unwrap();
        assert_eq!(below["totals_delta"]["bytes"], "-18446744073709551615");
        assert_eq!(below["totals_delta"]["lines"], "-18446744073709551615");
        assert_eq!(
            below["changed"][0]["bytes"]["delta"],
            "-18446744073709551615"
        );

        let above = super::diff_json(
            &meta(None),
            &crate::diff::Report {
                old_lines: 0,
                old_bytes: 0,
                new_lines: u64::MAX,
                new_bytes: u64::MAX,
                rows: vec![],
                added_bytes: 0,
            },
            None,
            None,
        )
        .unwrap();
        let above: serde_json::Value = serde_json::from_str(&above).unwrap();
        assert_eq!(above["totals_delta"]["bytes"], u64::MAX);
    }
}
