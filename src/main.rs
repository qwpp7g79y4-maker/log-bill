use log_bill::{diff, group, indexed, mask, money, pattern, read, render, rules};

use std::cell::Cell;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

/// Estimate log costs per pattern from a sample, offline.
#[derive(Parser)]
// `bin_name` keeps usage lines at `log-bill`, also as `log-bill.exe` on Windows.
#[command(name = "log-bill", bin_name = "log-bill", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,

    /// Input format: text or jsonl.
    #[arg(long, global = true, value_enum, default_value = "text")]
    format: InputFormat,

    /// JSON field that holds the message.
    #[arg(long, global = true, value_name = "NAME")]
    message_field: Option<String>,

    /// Group costs per source: the value of this JSON field (scan, jsonl).
    #[arg(long, global = true, value_name = "FIELD")]
    group_by: Option<String>,

    /// Time window of the sample, for example 15m.
    #[arg(long, global = true, value_name = "WINDOW")]
    window: Option<String>,

    /// Days in a month for the extrapolation.
    #[arg(long, global = true, default_value_t = 30, value_name = "DAYS")]
    days_per_month: u32,

    /// Price file (JSON).
    #[arg(long, global = true, value_name = "FILE")]
    prices: Option<PathBuf>,

    /// File with drop and sample rules.
    #[arg(long, global = true, value_name = "FILE")]
    rules: Option<PathBuf>,

    /// Show the N largest patterns; the rest as other.
    #[arg(long, global = true, value_name = "N")]
    top: Option<u32>,

    /// Show the masks and the calculation per pattern.
    #[arg(long, global = true)]
    explain: bool,

    /// Write JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    /// Monthly budget; exit with code 1 if the estimate is above it.
    #[arg(long, global = true, value_name = "AMOUNT")]
    budget_month: Option<String>,

    /// Maximum share of new patterns, in percent.
    #[arg(long, global = true, value_name = "PCT")]
    max_new_share: Option<String>,

    /// Indexed-log retention in days: 3, 7, 15 or 30.
    #[arg(long, global = true, value_name = "DAYS")]
    retention: Option<String>,

    /// Indexed-log billing plan: annual, monthly or ondemand.
    #[arg(long, global = true, value_name = "PLAN")]
    billing: Option<String>,

    /// Indexed-log price in USD per 1M events. Overrides the list price.
    #[arg(long, global = true, allow_negative_numbers = true, value_name = "USD")]
    indexed_price: Option<String>,

    /// Index exclusion rules (drop and sample), applied after --rules.
    #[arg(long, global = true, value_name = "FILE")]
    index_rules: Option<PathBuf>,

    /// Hide the hint line. No context line.
    #[arg(long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Print the hint line, and on success one context line on stderr.
    #[arg(long, global = true, conflicts_with = "quiet")]
    verbose: bool,

    /// Write the report to this file instead of stdout.
    #[arg(long, global = true, value_name = "PATH")]
    output: Option<PathBuf>,
}

thread_local! {
    /// Hint lines. A terminal shows them. A pipe does not, unless `--verbose`.
    static EXTRA: Cell<bool> = const { Cell::new(false) };
}

fn set_extra(extra: bool) {
    EXTRA.with(|cell| cell.set(extra));
}

fn extra() -> bool {
    EXTRA.with(|cell| cell.get())
}

/// `--quiet` hides hints even on a terminal. `--verbose` shows them even on a pipe.
/// Both flags together are a clap usage error and never reach a report.
fn show_extra(quiet: bool, verbose: bool) -> bool {
    show_extra_on(quiet, verbose, io::stderr().is_terminal())
}

fn show_extra_on(quiet: bool, verbose: bool, terminal: bool) -> bool {
    if quiet {
        false
    } else if verbose {
        true
    } else {
        terminal
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Group a log file into patterns.
    Scan {
        /// Log file. `-` reads standard input.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Compare two samples.
    Diff {
        /// Old sample. `-` reads standard input.
        #[arg(value_name = "OLD")]
        old: PathBuf,
        /// New sample. `-` reads standard input.
        #[arg(value_name = "NEW")]
        new: PathBuf,
    },
    /// Write a shell completion script to stdout
    #[command(hide = true)]
    Completions {
        /// Shell to generate for
        #[arg(value_name = "SHELL", value_enum)]
        shell: CompletionShell,
    },
    /// Write the manual page (roff) to stdout
    #[command(hide = true)]
    Man,
}

#[derive(Clone, Copy, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
}

#[derive(Clone, Copy, ValueEnum)]
enum InputFormat {
    Text,
    Jsonl,
}

fn main() {
    let code = match Cli::command().try_get_matches() {
        Ok(matches) => {
            // `days_per_month` has a default. Only an explicitly set flag counts.
            let days_explicit =
                matches.value_source("days_per_month") == Some(ValueSource::CommandLine);
            match Cli::from_arg_matches(&matches) {
                Ok(cli) => match &cli.command {
                    Cmd::Completions { shell } => emit_generated(Generated::Completions(*shell)),
                    Cmd::Man => emit_generated(Generated::Man),
                    _ => execute(&cli, days_explicit),
                },
                Err(error) => report(error),
            }
        }
        Err(error) => report(error),
    };
    std::process::exit(code);
}

enum Generated {
    Completions(CompletionShell),
    Man,
}

/// Writes a completion script or the manual page. A failed write is exit 3.
fn emit_generated(what: Generated) -> i32 {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    match what {
        Generated::Completions(shell) => {
            let shell = match shell {
                CompletionShell::Bash => Shell::Bash,
                CompletionShell::Zsh => Shell::Zsh,
            };
            clap_complete::generate(shell, &mut command, "log-bill", &mut buffer);
        }
        Generated::Man => {
            command.build();
            if clap_mangen::Man::new(command).render(&mut buffer).is_err() {
                return fail("could not render the manual page", 3);
            }
        }
    }
    match String::from_utf8(buffer) {
        Ok(text) => emit(None, &text),
        Err(_) => fail("generated text is not UTF-8", 3),
    }
}

fn report(error: clap::Error) -> i32 {
    let to_stderr = error.use_stderr();
    let code = if to_stderr { 3 } else { 0 };
    // Help and version are buffered. Flush, or a full disk still exits 0.
    if error.print().is_err() || flush_stream(to_stderr).is_err() {
        return fail(
            "could not write output; free disk space or check the output target, then run again",
            3,
        );
    }
    code
}

fn flush_stream(stderr: bool) -> io::Result<()> {
    if stderr {
        io::stderr().flush()
    } else {
        io::stdout().flush()
    }
}

/// Writes the scan or diff result.
///
/// A full disk is exit 3, not a panic. `--output PATH` writes that same
/// report to the file and leaves stdout empty. An existing file is replaced.
/// A missing parent directory is the same exit 3, not exit 1, and is not created.
fn emit(path: Option<&Path>, text: &str) -> i32 {
    if write_report(path, text).is_err() {
        let message = match path {
            Some(path) => format!(
                "could not write the report to {}; check that the parent directory exists and is writable, the tool does not create it",
                pattern::show_controls(&path.to_string_lossy())
            ),
            None => "could not write output; free disk space or check the output target, then run again".to_string(),
        };
        return fail(&message, 3);
    }
    0
}

/// Write `text` to `--output`, or to stdout when that flag is absent.
fn write_report(path: Option<&Path>, text: &str) -> io::Result<()> {
    let Some(path) = path else {
        return write_stdout(text);
    };
    let mut file = std::fs::File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.flush()
}

/// Writes `message` to stderr and returns `code`.
/// If stderr itself cannot be written, the code is 3 and there is no panic.
fn fail(message: &str, code: i32) -> i32 {
    fail_hint(message, None, code)
}

/// Like [`fail`], and when extra context is on, a second `hint:` line.
///
/// A pipe, a redirect and `--quiet` keep the first line only. `--verbose`
/// and a terminal add the hint. The exit code is unchanged.
fn fail_hint(message: &str, hint: Option<&str>, code: i32) -> i32 {
    let text = match hint {
        Some(hint) if extra() => format!("{message}\nhint: {hint}"),
        _ => message.to_string(),
    };
    if write_stderr(&text).is_err() {
        return 3;
    }
    code
}

/// One stderr line. A failed write is ignored so an optional line cannot
/// turn a finished report into exit 3.
fn note_line(text: &str) {
    let _ = write_stderr(text);
}

fn write_stdout(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

fn write_stderr(text: &str) -> io::Result<()> {
    let mut err = io::stderr().lock();
    err.write_all(text.as_bytes())?;
    err.write_all(b"\n")?;
    err.flush()
}

fn execute(cli: &Cli, days_explicit: bool) -> i32 {
    set_extra(show_extra(cli.quiet, cli.verbose));
    let window = if let Some(text) = &cli.window {
        match money::parse_window(text) {
            Ok(window) => Some(window),
            Err(error) => return fail(&error.to_string(), 3),
        }
    } else {
        None
    };
    let budget = match cli.budget_month.as_deref() {
        None => None,
        Some(text) => match money::parse_budget(text) {
            Some(budget) => Some(budget),
            None => return fail(
                "invalid budget; use a non-negative number with a period, with a period as the decimal mark",
                3,
            ),
        },
    };
    // Budget keeps its own message, even when --prices is set.
    // Then: --prices without a window, before the price file and the logs.
    if let Some(message) = check_before_read(cli, window.is_some(), budget.is_some()) {
        return fail(message, 3);
    }
    let prices = if let Some(path) = &cli.prices {
        match money::load_prices(path) {
            Ok(prices) => Some(prices),
            Err(error) => return fail_hint(&error.to_string(), Some(error.hint()), 2),
        }
    } else {
        None
    };
    let rules = if let Some(path) = &cli.rules {
        match rules::load_rules(path) {
            Ok(rules) => Some(rules),
            Err(error) => return fail_hint(&error.to_string(), Some(error.hint()), 2),
        }
    } else {
        None
    };
    let max_new_share = match cli.max_new_share.as_deref() {
        None => None,
        Some(text) => match diff::parse_percent(text) {
            Some(percent) => Some(percent),
            None => return fail(
                "invalid percentage; use a non-negative number with a period, with a period as the decimal mark",
                3,
            ),
        },
    };
    let retention = match cli.retention.as_deref() {
        None => None,
        Some(text) => match indexed::parse_retention(text) {
            Some(days) => Some(days),
            None => return fail("invalid retention; use 3, 7, 15 or 30", 3),
        },
    };
    let billing = match cli.billing.as_deref() {
        None => None,
        Some(text) => match indexed::parse_billing(text) {
            Some(plan) => Some(plan),
            None => return fail("invalid billing; use annual, monthly or ondemand", 3),
        },
    };
    let indexed_price = match cli.indexed_price.as_deref() {
        None => None,
        Some(text) => match indexed::parse_indexed_price(text) {
            Some(price) => Some(price),
            None => return fail(
                "invalid indexed price; use a non-negative number with a period, with a period as the decimal mark",
                3,
            ),
        },
    };
    let indexing = retention.is_some()
        || billing.is_some()
        || indexed_price.is_some()
        || cli.index_rules.is_some();
    match &cli.command {
        Cmd::Completions { .. } | Cmd::Man => 0,
        Cmd::Scan { file } => {
            // A bad retention, billing plan or price was already rejected.
            // The window check is next, before the index-rules file is opened.
            if window.is_none() {
                if retention.is_some() {
                    return fail("--retention only works together with --window", 3);
                }
                if billing.is_some() {
                    return fail("--billing only works together with --window", 3);
                }
                if indexed_price.is_some() {
                    return fail("--indexed-price only works together with --window", 3);
                }
                if cli.index_rules.is_some() {
                    return fail("--index-rules only works together with --window", 3);
                }
            }
            if let Some(message) = ignored_option(cli, days_explicit) {
                return fail(message, 3);
            }
            let index_rules = if let Some(path) = &cli.index_rules {
                match rules::load_rules(path) {
                    Ok(set) => Some(set),
                    Err(error) => {
                        return fail_hint(&index_rules_message(&error), Some(error.hint()), 2);
                    }
                }
            } else {
                None
            };
            let index_quote = indexing.then(|| indexed::quote(retention, billing, indexed_price));
            match fold_file(
                cli,
                file,
                rules.as_ref(),
                index_rules.as_ref(),
                indexing,
                cli.top,
            ) {
                Ok(Folded {
                    stats,
                    effect,
                    events,
                    sources,
                }) => {
                    let text = if cli.json {
                        let file_text = path_text(file);
                        let paths = [file_text.as_str()];
                        let rules_json = match (rules.as_ref(), effect.as_ref()) {
                            (Some(set), Some(effect)) => Some((effect, set.len())),
                            _ => None,
                        };
                        match render::scan_json(
                            &render::JsonMeta {
                                command: "scan",
                                format: format_name(cli.format),
                                message_field: cli.message_field.as_deref(),
                                days_per_month: cli.days_per_month,
                                top: cli.top,
                                paths: &paths,
                                window,
                                explain: cli.explain,
                                group_by: cli.group_by.as_deref(),
                            },
                            &stats,
                            rules_json,
                            prices.as_ref(),
                            index_quote.as_ref().zip(events.as_ref()),
                            sources.as_deref(),
                        ) {
                            Ok(body) => body,
                            Err(error) => return fail(&error.to_string(), 3),
                        }
                    } else {
                        let mut text = render::scan_text(&stats);
                        if cli.explain {
                            text.push_str(&render::explain_patterns(&stats, effect.as_ref()));
                        }
                        if let Some(window) = window {
                            let gb_bytes = prices
                                .as_ref()
                                .map(|prices| prices.gb_bytes)
                                .unwrap_or(money::GB_BYTES);
                            let volume = match money::extrapolate(
                                stats.total_bytes,
                                window,
                                cli.days_per_month,
                                gb_bytes,
                            ) {
                                Ok(volume) => volume,
                                Err(error) => return fail(&error.to_string(), 3),
                            };
                            text.push_str(&render::volume_text(window, &volume));
                        }
                        if let (Some(field), Some(sources)) =
                            (cli.group_by.as_deref(), sources.as_deref())
                        {
                            let costs = window
                                .zip(prices.as_ref())
                                .map(|(window, prices)| (window, cli.days_per_month, prices));
                            match render::sources_text(field, sources, costs) {
                                Ok(block) => text.push_str(&block),
                                Err(error) => return fail(&error.to_string(), 3),
                            }
                        }
                        if let Some(effect) = &effect {
                            text.push_str(&render::rules_bytes(effect));
                        }
                        if let Some(window) = window {
                            if let Some(prices) = &prices {
                                match render::costs_text(
                                    &stats,
                                    window,
                                    cli.days_per_month,
                                    prices,
                                    effect.as_ref(),
                                    cli.explain,
                                ) {
                                    Ok(costs) => text.push_str(&costs),
                                    Err(error) => return fail(&error.to_string(), 3),
                                }
                            }
                        }
                        if let (Some(quote), Some(events)) = (&index_quote, &events) {
                            let time_window = window.expect("indexing has a window");
                            match render::indexed_text(
                                quote,
                                events,
                                time_window,
                                cli.days_per_month,
                                cli.explain,
                            ) {
                                Ok(block) => text.push_str(&block),
                                Err(error) => return fail(&error.to_string(), 3),
                            }
                        }
                        text
                    };
                    // Rounding happens only on display. The budget compares the fraction,
                    // ingest plus the per-GB index, on the bytes after the rules.
                    // The indexed-event estimate is not part of the budget.
                    let over_budget = if let Some(budget) = budget {
                        let time_window = window.expect("budget has a window");
                        let price_set = prices.as_ref().expect("budget has prices");
                        let bytes = effect
                            .as_ref()
                            .map(|item| item.new_bytes)
                            .unwrap_or(stats.total_bytes);
                        match money::month_above_budget(
                            bytes,
                            time_window,
                            cli.days_per_month,
                            price_set,
                            budget,
                        ) {
                            Ok(above) => above,
                            Err(error) => return fail(&error.to_string(), 3),
                        }
                    } else {
                        false
                    };
                    let written = emit(cli.output.as_deref(), &text);
                    if written != 0 {
                        return written;
                    }
                    if over_budget {
                        return fail_hint(
                            "monthly cost above budget; reduce the log volume or raise --budget-month",
                            Some("reduce the log volume or raise --budget-month"),
                            1,
                        );
                    }
                    if cli.verbose {
                        note_line(&format!(
                            "context: lines {}, bytes {}",
                            stats.total_lines, stats.total_bytes
                        ));
                    }
                    0
                }
                Err(error) => fail_hint(&error.to_string(), Some(error.hint()), 2),
            }
        }
        Cmd::Diff { old, new } => {
            // `--explain` and `--top` belong with scan. So does a valid monthly budget.
            if cli.explain {
                return fail("--explain only works with scan", 3);
            }
            if cli.top.is_some() {
                return fail("--top only works with scan", 3);
            }
            // Without prices or JSON the window is not in the output.
            if window.is_some() && prices.is_none() && !cli.json {
                return fail(
                    "--window with diff only works together with --prices or --json",
                    3,
                );
            }
            if cli.group_by.is_some() {
                return fail("--group-by only works with scan", 3);
            }
            if cli.retention.is_some() {
                return fail("--retention only works with scan", 3);
            }
            if cli.billing.is_some() {
                return fail("--billing only works with scan", 3);
            }
            if cli.indexed_price.is_some() {
                return fail("--indexed-price only works with scan", 3);
            }
            if cli.index_rules.is_some() {
                return fail("--index-rules only works with scan", 3);
            }
            if let Some(message) = ignored_option(cli, days_explicit) {
                return fail(message, 3);
            }
            let old_path = path_text(old);
            let new_path = path_text(new);
            let old = match fold_file(cli, old, rules.as_ref(), None, false, None) {
                Ok(folded) => sampled_from(folded.stats, folded.effect),
                Err(error) => return fail_hint(&error.to_string(), Some(error.hint()), 2),
            };
            let new = match fold_file(cli, new, rules.as_ref(), None, false, None) {
                Ok(folded) => sampled_from(folded.stats, folded.effect),
                Err(error) => return fail_hint(&error.to_string(), Some(error.hint()), 2),
            };
            // Match is on the pattern text. `--top` would give the rest one id, `other`.
            // Counting matches scan: the diff is taken after the rules.
            let applied = rules.as_ref().map(|set| set.len());
            let report = diff::compare(&old, &new);
            if let Some(window) = window {
                let gb_bytes = prices
                    .as_ref()
                    .map(|prices| prices.gb_bytes)
                    .unwrap_or(money::GB_BYTES);
                for bytes in [report.old_bytes, report.new_bytes] {
                    if let Err(error) =
                        money::extrapolate(bytes, window, cli.days_per_month, gb_bytes)
                    {
                        return fail(&error.to_string(), 3);
                    }
                }
            }
            let paths = [old_path.as_str(), new_path.as_str()];
            let text = if cli.json {
                match render::diff_json(
                    &render::JsonMeta {
                        command: "diff",
                        format: format_name(cli.format),
                        message_field: cli.message_field.as_deref(),
                        days_per_month: cli.days_per_month,
                        top: cli.top,
                        paths: &paths,
                        window,
                        explain: cli.explain,
                        group_by: None,
                    },
                    &report,
                    applied,
                    prices.as_ref(),
                ) {
                    Ok(body) => body,
                    Err(error) => return fail(&error.to_string(), 3),
                }
            } else {
                let mut text = render::diff_text(&report, applied);
                if let (Some(window), Some(prices)) = (window, prices.as_ref()) {
                    match render::diff_costs(
                        report.old_bytes,
                        report.new_bytes,
                        window,
                        cli.days_per_month,
                        prices,
                    ) {
                        Ok(costs) => text.push_str(&costs),
                        Err(error) => return fail(&error.to_string(), 3),
                    }
                }
                text
            };
            let over = max_new_share
                .as_ref()
                .is_some_and(|percent| percent.exceeded_by(report.added_bytes, report.new_bytes));
            if over
                && notice(
                    "new patterns above the threshold",
                    "raise --max-new-share, or reduce how much of the new sample is new",
                )
                .is_err()
            {
                return 3;
            }
            let written = emit(cli.output.as_deref(), &text);
            if written != 0 {
                return written;
            }
            if over {
                return 1;
            }
            if cli.verbose {
                note_line(&format!(
                    "context: bytes {} -> {}",
                    report.old_bytes, report.new_bytes
                ));
            }
            0
        }
    }
}

/// Threshold line for `--max-new-share`. A pipe keeps one line. Extra context
/// adds `hint:`. A failed write is exit 3, same as before.
fn notice(message: &str, hint: &str) -> io::Result<()> {
    if extra() {
        write_stderr(&format!("{message}\nhint: {hint}"))
    } else {
        write_stderr(message)
    }
}

/// Everything one pass over a file produces.
struct Folded {
    stats: pattern::Stats,
    effect: Option<rules::Effect>,
    events: Option<indexed::Events>,
    sources: Option<Vec<group::Source>>,
}

/// One pass over the file. Lines are counted and dropped, not stored.
fn fold_file(
    cli: &Cli,
    path: &Path,
    rules: Option<&rules::RuleSet>,
    index_rules: Option<&rules::RuleSet>,
    tally_index: bool,
    top: Option<u32>,
) -> Result<Folded, read::ReadError> {
    let mut grouped = pattern::Grouper::default();
    let mut folded = rules.map(|_| rules::Folder::default());
    let mut tally = tally_index.then(indexed::Tally::default);
    let mut by_source = cli.group_by.as_deref().map(group::SourceTally::new);
    let mut visit = |line: read::Line| {
        grouped.push(&line);
        if let Some(tally) = by_source.as_mut() {
            tally.push(&line);
        }
        if let (Some(folder), Some(set)) = (folded.as_mut(), rules) {
            folder.push(&line, set);
        }
        if let Some(tally) = tally.as_mut() {
            tally.push(&line, rules, index_rules);
        }
    };
    let stdin = path == Path::new("-");
    match (cli.format, stdin) {
        (InputFormat::Text, false) => read::visit_text(path, &mut visit)?,
        (InputFormat::Text, true) => read::visit_text_stdin(&mut visit)?,
        (InputFormat::Jsonl, false) => {
            read::visit_jsonl(path, cli.message_field.as_deref(), &mut visit)?;
        }
        (InputFormat::Jsonl, true) => {
            read::visit_jsonl_stdin(cli.message_field.as_deref(), &mut visit)?;
        }
    }
    let stats = grouped.finish(top);
    let effect = folded.map(|folder| folder.finish(&stats));
    let events = tally.map(|tally| tally.finish(&stats));
    let sources = by_source.map(group::SourceTally::finish);
    Ok(Folded {
        stats,
        effect,
        events,
        sources,
    })
}

fn index_rules_message(error: &rules::RulesError) -> String {
    match error {
        rules::RulesError::Unreadable => {
            "index rules file is not readable; check the path and the read permission".to_owned()
        }
        rules::RulesError::NotRegular => {
            "index rules file is not a regular file; pass a regular file, not a directory or device"
                .to_owned()
        }
        rules::RulesError::Invalid { line } => {
            format!("invalid rule on line {line} of the index rules file; fix or remove that line")
        }
    }
}

/// Patterns as scan counts them after `--rules`. Without rules this is `group` itself.
/// A pattern that drops out entirely (0 lines and 0 bytes) no longer counts.
fn sampled_from(stats: pattern::Stats, effect: Option<rules::Effect>) -> pattern::Stats {
    let Some(effect) = effect else {
        return stats;
    };
    let mut patterns: Vec<pattern::Pattern> = effect
        .patterns
        .iter()
        .filter(|pattern| pattern.new_lines > 0 || pattern.new_bytes > 0)
        .map(|pattern| pattern::Pattern {
            id: pattern.id.clone(),
            text: pattern.text.clone(),
            lines: pattern.new_lines,
            bytes: pattern.new_bytes,
            share_tenths: pattern::share_tenths(pattern.new_bytes, effect.new_bytes),
            rest: false,
            masks: mask::MaskCounts::default(),
        })
        .collect();
    patterns.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.text.cmp(&b.text)));
    pattern::Stats {
        total_lines: effect.new_lines,
        total_bytes: effect.new_bytes,
        patterns,
    }
}

/// Messages fixed before reading. Budget comes before `--prices`.
fn check_before_read(cli: &Cli, has_window: bool, has_budget: bool) -> Option<&'static str> {
    match &cli.command {
        Cmd::Scan { .. } if has_budget && !has_window => {
            Some("budget requires a window; add --window, for example --window 15m")
        }
        Cmd::Scan { .. } if has_budget && cli.prices.is_none() => {
            Some("budget requires a price file; add --prices FILE")
        }
        Cmd::Diff { .. } if has_budget => Some("the monthly budget only applies to scan"),
        _ if cli.prices.is_some() && !has_window => {
            Some("--prices only works together with --window")
        }
        Cmd::Scan { .. } if cli.max_new_share.is_some() => {
            Some("--max-new-share only works with diff")
        }
        _ => None,
    }
}

/// Options parsed for this command that do not affect the output.
fn ignored_option(cli: &Cli, days_explicit: bool) -> Option<&'static str> {
    if cli.message_field.is_some() && !matches!(cli.format, InputFormat::Jsonl) {
        return Some("--message-field only works with --format jsonl");
    }
    if cli.group_by.is_some() && !matches!(cli.format, InputFormat::Jsonl) {
        return Some("--group-by only works with --format jsonl");
    }
    if days_explicit && cli.window.is_none() {
        return Some("--days-per-month only works together with --window");
    }
    None
}

fn format_name(format: InputFormat) -> &'static str {
    match format {
        InputFormat::Text => "text",
        InputFormat::Jsonl => "jsonl",
    }
}

/// A path as shown in `--json` (`inputs.paths`). A path that is not valid
/// UTF-8 is shown with U+FFFD for the bytes that are not; it is read as is.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::show_extra_on;

    #[test]
    fn extra_context_follows_the_terminal_unless_a_flag_is_set() {
        assert!(!show_extra_on(false, false, false));
        assert!(show_extra_on(false, false, true));
        assert!(show_extra_on(false, true, false));
        assert!(!show_extra_on(true, false, true));
        assert!(!show_extra_on(true, true, true));
    }
}
