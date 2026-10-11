mod diff;
mod mask;
mod money;
mod pattern;
mod read;
mod render;
mod rules;

use std::path::{Path, PathBuf};

use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

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
}

#[derive(Subcommand)]
enum Cmd {
    /// Group a log file into patterns.
    Scan {
        /// Log file.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Compare two samples.
    Diff {
        /// Old sample.
        #[arg(value_name = "OLD")]
        old: PathBuf,
        /// New sample.
        #[arg(value_name = "NEW")]
        new: PathBuf,
    },
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
                Ok(cli) => execute(&cli, days_explicit),
                Err(error) => report(error),
            }
        }
        Err(error) => report(error),
    };
    std::process::exit(code);
}

fn report(error: clap::Error) -> i32 {
    let code = if error.use_stderr() { 3 } else { 0 };
    if error.print().is_err() {
        return 3;
    }
    code
}

fn execute(cli: &Cli, days_explicit: bool) -> i32 {
    let window = if let Some(text) = &cli.window {
        match money::parse_window(text) {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("{error}");
                return 3;
            }
        }
    } else {
        None
    };
    let budget = match cli.budget_month.as_deref() {
        None => None,
        Some(text) => match money::parse_budget(text) {
            Some(budget) => Some(budget),
            None => {
                eprintln!("invalid budget");
                return 3;
            }
        },
    };
    // Budget keeps its own message, even when --prices is set.
    // Then: --prices without a window, before the price file and the logs.
    if let Some(message) = check_before_read(cli, window.is_some(), budget.is_some()) {
        eprintln!("{message}");
        return 3;
    }
    let prices = if let Some(path) = &cli.prices {
        match money::load_prices(path) {
            Ok(prices) => Some(prices),
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    } else {
        None
    };
    let rules = if let Some(path) = &cli.rules {
        match rules::load_rules(path) {
            Ok(rules) => Some(rules),
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    } else {
        None
    };
    let max_new_share = match cli.max_new_share.as_deref() {
        None => None,
        Some(text) => match diff::parse_percent(text) {
            Some(percent) => Some(percent),
            None => {
                eprintln!("invalid percentage");
                return 3;
            }
        },
    };
    match &cli.command {
        Cmd::Scan { file } => {
            if let Some(message) = ignored_option(cli, days_explicit) {
                eprintln!("{message}");
                return 3;
            }
            match read_input(cli, file) {
                Ok(lines) => {
                    let stats = pattern::group(&lines, cli.top);
                    let effect = rules.as_ref().map(|set| rules::effect(&lines, set, &stats));
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
                            },
                            &stats,
                            rules_json,
                            prices.as_ref(),
                        ) {
                            Ok(body) => body,
                            Err(error) => {
                                eprintln!("{error}");
                                return 3;
                            }
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
                                Err(error) => {
                                    eprintln!("{error}");
                                    return 3;
                                }
                            };
                            text.push_str(&render::volume_text(window, &volume));
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
                                    Err(error) => {
                                        eprintln!("{error}");
                                        return 3;
                                    }
                                }
                            }
                        }
                        text
                    };
                    // Rounding happens only on display. The budget compares the fraction,
                    // ingest plus index, on the bytes after the rules.
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
                            Err(error) => {
                                eprintln!("{error}");
                                return 3;
                            }
                        }
                    } else {
                        false
                    };
                    print!("{text}");
                    if over_budget {
                        eprintln!("monthly cost above budget");
                        1
                    } else {
                        0
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    2
                }
            }
        }
        Cmd::Diff { old, new } => {
            // `--explain` and `--top` belong with scan. So does a valid monthly budget.
            if cli.explain {
                eprintln!("--explain only works with scan");
                return 3;
            }
            if cli.top.is_some() {
                eprintln!("--top only works with scan");
                return 3;
            }
            // Without prices or JSON the window is not in the output.
            if window.is_some() && prices.is_none() && !cli.json {
                eprintln!("--window with diff only works together with --prices or --json");
                return 3;
            }
            if let Some(message) = ignored_option(cli, days_explicit) {
                eprintln!("{message}");
                return 3;
            }
            let old_path = path_text(old);
            let new_path = path_text(new);
            let old = match read_input(cli, old) {
                Ok(lines) => lines,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            let new = match read_input(cli, new) {
                Ok(lines) => lines,
                Err(error) => {
                    eprintln!("{error}");
                    return 2;
                }
            };
            // Match is on the pattern text. `--top` would give the rest one id, `other`.
            // Counting matches scan: the diff is taken after the rules.
            let applied = rules.as_ref().map(|set| set.len());
            let report = diff::compare(
                &sampled_stats(&old, rules.as_ref()),
                &sampled_stats(&new, rules.as_ref()),
            );
            if let Some(window) = window {
                let gb_bytes = prices
                    .as_ref()
                    .map(|prices| prices.gb_bytes)
                    .unwrap_or(money::GB_BYTES);
                for bytes in [report.old_bytes, report.new_bytes] {
                    if let Err(error) =
                        money::extrapolate(bytes, window, cli.days_per_month, gb_bytes)
                    {
                        eprintln!("{error}");
                        return 3;
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
                    },
                    &report,
                    applied,
                    prices.as_ref(),
                ) {
                    Ok(body) => body,
                    Err(error) => {
                        eprintln!("{error}");
                        return 3;
                    }
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
                        Err(error) => {
                            eprintln!("{error}");
                            return 3;
                        }
                    }
                }
                text
            };
            let over = max_new_share
                .as_ref()
                .is_some_and(|percent| percent.exceeded_by(report.added_bytes, report.new_bytes));
            if over {
                eprintln!("new patterns above the threshold");
            }
            print!("{text}");
            if over {
                1
            } else {
                0
            }
        }
    }
}

/// Patterns as scan counts them after `--rules`. Without rules this is `group` itself.
/// A pattern that drops out entirely (0 lines and 0 bytes) no longer counts.
fn sampled_stats(lines: &[read::Line], rules: Option<&rules::RuleSet>) -> pattern::Stats {
    let stats = pattern::group(lines, None);
    let Some(set) = rules else {
        return stats;
    };
    let effect = rules::effect(lines, set, &stats);
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
        Cmd::Scan { .. } if has_budget && !has_window => Some("budget requires a window"),
        Cmd::Scan { .. } if has_budget && cli.prices.is_none() => {
            Some("budget requires a price file")
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

fn read_input(cli: &Cli, path: &Path) -> Result<Vec<read::Line>, read::ReadError> {
    match cli.format {
        InputFormat::Text => read::read_text(path),
        InputFormat::Jsonl => read::read_jsonl(path, cli.message_field.as_deref()),
    }
}
