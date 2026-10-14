//! Documentation tests: README.md, README.nl.md, docs/reference.md and
//! docs/reference.nl.md.
//!
//! Every ```console example is run against the binary. Every exit-3 row in the
//! usage-error tables is run against the binary. The English and Dutch pages
//! must have the same sections and the same source URLs.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const README: &str = "README.md";
const README_NL: &str = "README.nl.md";
const REF: &str = "docs/reference.md";
const REF_NL: &str = "docs/reference.nl.md";

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-bill"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {rel}: {err}"))
}

/// The text under a level-2 heading, up to the next level-2 heading.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let marker = format!("{heading}\n");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing heading {heading}"))
        + marker.len();
    let rest = &text[start..];
    match rest.find("\n## ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

fn headings(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("## "))
        .collect()
}

fn urls(text: &str) -> BTreeSet<String> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|word| word.starts_with("https://"))
        .map(|word| word.trim_end_matches(['.', ',', '"']).to_string())
        .collect()
}

fn backtick(cell: &str) -> &str {
    let cell = cell.trim();
    cell.strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or_else(|| panic!("cell not in backticks: {cell}"))
}

/// `\n` in a table cell is a real newline. The cell itself has none.
fn message_of(cell: &str) -> String {
    let mut text = backtick(cell).replace("\\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

struct Row {
    call: String,
    args: Vec<String>,
    message: String,
}

fn exit3_rows(doc: &str, heading: &str) -> Vec<Row> {
    let part = section(doc, heading);
    let mut rows = Vec::new();
    for line in part.lines().filter(|line| line.starts_with('|')) {
        let parts: Vec<&str> = line.split('|').collect();
        // The exit-code table has two columns; only the six-column table counts.
        if parts.len() < 8 {
            continue;
        }
        let cells: Vec<&str> = parts[1..7].iter().map(|cell| cell.trim()).collect();
        if cells[4] != "3" {
            continue;
        }
        let call = backtick(cells[3]).to_string();
        let args = call.split_whitespace().map(str::to_string).collect();
        rows.push(Row {
            call,
            args,
            message: message_of(cells[5]),
        });
    }
    assert!(!rows.is_empty(), "{heading} has no exit-3 row");
    rows
}

/// Every ```console block: the first line is `$ log-bill ARGS`, the rest is stdout.
fn examples(doc: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut lines = doc.lines();
    while let Some(line) = lines.next() {
        if line != "```console" {
            continue;
        }
        let call = lines.next().expect("empty console block");
        let args: Vec<String> = call
            .strip_prefix("$ log-bill ")
            .unwrap_or_else(|| panic!("console block does not start with `$ log-bill `: {call}"))
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let mut body = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line == "```" {
                closed = true;
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        assert!(closed, "console block not closed: {call}");
        out.push((args, body));
    }
    out
}

#[test]
fn readme_has_the_sections_and_names() {
    let readme = read(README);
    for heading in [
        "## Why",
        "## Installation",
        "## Quick start",
        "## How it counts",
        "## Compared with alternatives",
        "## Limitations",
        "## Support",
        "## License",
        "## Contact",
    ] {
        assert!(
            readme.lines().any(|line| line == heading),
            "missing {heading}"
        );
    }
    for name in [
        "Log10x",
        "Cribl",
        "Patterns",
        "Adaptive Logs",
        "Logs Drilldown",
    ] {
        assert!(readme.contains(name), "missing {name}");
    }
    let limits = section(&readme, "## Limitations");
    for part in [
        "official rate",
        "contract discounts",
        "Compression",
        "storage",
        "per million events",
        "extrapolation",
        "security or compliance",
        "gzip",
        "Only regular files are read",
    ] {
        assert!(
            limits.contains(part),
            "Limitations is missing {part}\n{limits}"
        );
    }
    assert!(section(&readme, "## Support").contains("(https://ko-fi.com/pepai)"));
    assert!(section(&read(README_NL), "## Steunen").contains("(https://ko-fi.com/pepai)"));
    assert_eq!(read(".github/FUNDING.yml"), "ko_fi: pepai\n");
    let install = section(&readme, "## Installation");
    let crates_io = install
        .find("cargo install --locked log-bill")
        .expect("crates.io install line");
    let from_git = install
        .find("cargo install --locked --git https://github.com/qwpp7g79y4-maker/log-bill")
        .expect("git install line");
    assert!(crates_io < from_git, "crates.io is the first install line");
    assert!(readme.contains("(https://crates.io/crates/log-bill)"));
    assert!(install.contains("Rust 1.85 or newer"));
    assert!(install.contains("(https://github.com/qwpp7g79y4-maker/log-bill/releases)"));
    let manifest = read("Cargo.toml");
    assert!(manifest.contains("rust-version = \"1.85\""));
    let readme_nl = read(README_NL);
    let install_nl = section(&readme_nl, "## Installatie");
    assert!(install_nl.contains("Rust 1.85 of nieuwer"));
    assert!(install_nl.contains("cargo install --locked log-bill"));
    assert!(section(&readme, "## License").contains("PEPAI"));
    let contact = section(&readme, "## Contact");
    assert!(contact.contains("pep_ai@icloud.com"));
    assert!(contact.contains("(https://github.com/qwpp7g79y4-maker/log-bill/issues)"));
    assert!(contact.contains("[SECURITY.md](SECURITY.md)"));
    let contact_nl = section(&readme_nl, "## Contact");
    assert!(contact_nl.contains("[SECURITY.md](SECURITY.md)"));
    let security = read("SECURITY.md");
    assert!(security.contains("pep_ai@icloud.com"));
    assert!(security.contains("| 0.1.x | Yes |"));
}

#[test]
fn language_switch_on_top_of_every_page() {
    for (rel, line) in [
        (README, "English · [Nederlands](README.nl.md)"),
        (README_NL, "[English](README.md) · Nederlands"),
        (REF, "English · [Nederlands](reference.nl.md)"),
        (REF_NL, "[English](reference.md) · Nederlands"),
    ] {
        let text = read(rel);
        let third = text.lines().nth(2).unwrap_or_default();
        assert_eq!(third, line, "{rel} has no language switch on line 3");
    }
}

#[test]
fn english_and_dutch_pages_match() {
    for (en, nl) in [(README, README_NL), (REF, REF_NL)] {
        let en_text = read(en);
        let nl_text = read(nl);
        assert_eq!(
            headings(&en_text).len(),
            headings(&nl_text).len(),
            "{en} and {nl} have a different number of sections"
        );
        assert_eq!(
            urls(&en_text),
            urls(&nl_text),
            "{en} and {nl} cite different URLs"
        );
        assert_eq!(
            examples(&en_text),
            examples(&nl_text),
            "{en} and {nl} have different examples"
        );
    }
}

#[test]
fn licenses_and_manifest_name_pepai() {
    assert!(read("LICENSE-MIT").contains("Copyright (c) 2026 PEPAI"));
    assert!(read("LICENSE-APACHE").contains("Copyright 2026 PEPAI"));
    let cargo = read("Cargo.toml");
    assert!(cargo.contains("PEPAI <pep_ai@icloud.com>"), "{cargo}");
}

#[test]
fn rounding_comma_and_budget_are_explained() {
    let reference = read(REF);
    assert!(
        reference.contains("a pattern can end up with 0 lines and still some bytes"),
        "missing the zero-lines explanation"
    );
    assert!(
        reference.contains("(`0.1`, not `0,1`)"),
        "missing the comma tip"
    );
    assert!(
        reference.contains("`--budget-month` is checked against the bytes after `--rules`."),
        "missing the budget explanation"
    );
    let rules = reference
        .lines()
        .find(|line| line.starts_with("- **Rules (`--rules FILE`):**"))
        .expect("rules bullet");
    for part in [
        "floor(count × fraction)",
        "floor(bytes × fraction)",
        "independently",
        "`1 × fraction`",
        "`bytes × fraction`",
    ] {
        assert!(rules.contains(part), "rules bullet is missing {part}");
    }
}

#[test]
fn sources_are_quoted_verbatim() {
    let readme = read(README);
    let why = section(&readme, "## Why");
    for part in [
        "https://news.ycombinator.com/item?id=46034896",
        "My bill jumped nearly 1000% before alerts caught it",
        "https://news.ycombinator.com/item?id=45630239",
        "the risk of bad logging running up a $70,000/day bill with CloudWatch Logs",
        "8 October 2026",
    ] {
        assert!(why.contains(part), "Why is missing {part}");
    }
    let quotes = [
        "https://doc.log10x.com/faq/pricing/tiers-billing/",
        "running the engine needs no account at all",
        "synthesizes a built-in evaluation license",
        "up to 10 nodes, for 30 days from process start. It runs air-gapped",
        "Pricing is per node, with unlimited log volume and all products included at every tier.",
        "Free | up to 10 | $0 | $0",
        "Starter | up to 30 | $99 | $990",
        "https://github.com/log-10x/pipeline-releases",
        "Log10x binaries and runtime | Proprietary (commercial license required)",
        "The installation scripts are open source",
        "Log10x Proprietary Software License",
        "https://cribl.io/pricing/stream/",
        "Route data from any source to any destination.",
        "1 credit = $1",
        "0.32 Credits / GB (Enterprise, Cloud Worker)",
        "0.26 Credits / GB (Enterprise, Hybrid Worker)",
        "0.27 Credits / GB (Standard, Cloud Worker)",
        "https://docs.datadoghq.com/logs/explorer/analytics/patterns/",
        "The pattern detection is based on 10,000 log samples.",
        "https://grafana.com/docs/grafana-cloud/adaptive-telemetry/adaptive-logs/",
        "is a log optimization tool in Grafana Cloud",
        "before they reach Grafana Cloud",
        "https://grafana.com/docs/grafana/latest/visualizations/simplified-exploration/logs/patterns/",
        "The Grafana Logs Drilldown app shows you the patterns alongside their log volumes.",
        "Patterns are ephemeral and are only mined from the previous three hours of your logs.",
        "To see patterns on the Patterns tab, you must have the pattern ingester enabled in your Loki configuration file.",
    ];
    let compared = section(&readme, "## Compared with alternatives");
    assert!(compared.contains("8 October 2026"));
    assert!(compared.contains("not verified"));
    let nl = read(README_NL);
    let compared_2 = section(&nl, "## Vergelijking met alternatieven");
    assert!(compared_2.contains("8 oktober 2026"));
    assert!(compared_2.contains("niet geverifieerd"));
    for part in quotes {
        assert!(compared.contains(part), "comparison is missing {part}");
        assert!(
            compared_2.contains(part),
            "Dutch comparison is missing {part}"
        );
    }
    let datadog = [
        "https://docs.datadoghq.com/account_management/billing/pricing",
        "Datadog charges for ingested logs based on the total number of gigabytes submitted to the Datadog Logs service.",
        "Datadog charges per million log events submitted for indexing at the rate designated for the retention policy you selected.",
    ];
    let limits = section(&readme, "## Limitations");
    let limits_nl = section(&nl, "## Beperkingen");
    for part in datadog {
        assert!(limits.contains(part), "Limitations is missing {part}");
        assert!(limits_nl.contains(part), "Beperkingen is missing {part}");
    }
    // Errors that were corrected earlier must not come back.
    for wrong in [
        "per node vanaf $99",
        "$99 per node,",
        "Gratis binnen het product",
        "Route to Destinations",
    ] {
        assert!(!readme.contains(wrong), "README still has {wrong}");
        assert!(!nl.contains(wrong), "README.nl.md still has {wrong}");
    }
}

#[test]
fn examples_match_the_binary() {
    let sample = [
        "scan",
        "--rules",
        "tests/fixtures/sample-0.1.txt",
        "tests/fixtures/sample-zero.log",
    ];
    for rel in [README, README_NL] {
        let doc = read(rel);
        let mut saw_sample = false;
        let mut count = 0;
        for (args, expected) in examples(&doc) {
            count += 1;
            let output = bin().args(&args).output().expect("start binary");
            let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
            let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
            assert_eq!(
                output.status.code(),
                Some(0),
                "{rel} {args:?}\nstderr: {err}"
            );
            assert!(err.is_empty(), "{rel} {args:?} stderr: {err}");
            assert_eq!(out, expected, "{rel} {args:?}");
            if args == sample {
                saw_sample = true;
                // `xxxxxxxxx\n` = 10 bytes, 1 line. sample 0.1: 0 lines, 1 byte.
                assert!(expected.contains("-> 0 lines, 1 byte, savings 1 line, 9 bytes\n"));
            }
        }
        assert!(count >= 4, "{rel} has only {count} examples");
        assert!(saw_sample, "{rel} is missing the 0 lines, 1 byte example");
    }
}

#[test]
fn price_example_is_the_fixture() {
    let fixture = read("tests/fixtures/prices.json");
    let block = format!("```json\n{fixture}");
    for rel in [REF, REF_NL] {
        assert!(
            read(rel).contains(block.trim_end()),
            "{rel}: price example differs from tests/fixtures/prices.json"
        );
    }
}

#[test]
fn option_and_error_tables_match_the_binary() {
    let help = bin().arg("--help").output().expect("start binary");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help is utf-8");
    for (rel, options, errors) in [
        (REF, "## Options", "## Exit codes and usage errors"),
        (REF_NL, "## Opties", "## Exitcodes en gebruiksfouten"),
    ] {
        let doc = read(rel);
        let table = section(&doc, options);
        for line in help.lines() {
            let Some(start) = line.find("--") else {
                continue;
            };
            let name = line[start..].split_whitespace().next().unwrap();
            assert!(
                table.contains(name),
                "{rel}: option table is missing {name}"
            );
        }
        let mut messages = BTreeSet::new();
        let mut calls = BTreeSet::new();
        for row in exit3_rows(&doc, errors) {
            let output = bin().args(&row.args).output().expect("start binary");
            let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
            let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
            assert_eq!(
                output.status.code(),
                Some(3),
                "{rel} {:?} stderr: {err}",
                row.args
            );
            assert!(out.is_empty(), "{rel} {:?} stdout: {out}", row.args);
            assert_eq!(err, row.message, "{rel} {:?}", row.args);
            messages.insert(row.message);
            calls.insert(row.call);
        }
        for needed in [
            "--top only works with scan\n",
            "--explain only works with scan\n",
            "--max-new-share only works with diff\n",
            "--prices only works together with --window\n",
            "--message-field only works with --format jsonl\n",
            "--days-per-month only works together with --window\n",
            "the monthly budget only applies to scan\n",
            "budget requires a window\n",
            "budget requires a price file\n",
            "--window with diff only works together with --prices or --json\n",
            "invalid window\n",
            "invalid budget\n",
            "invalid percentage\n",
        ] {
            assert!(
                messages.contains(needed),
                "{rel}: table is missing {needed:?}"
            );
        }
        // `--json` does not change a message. `--explain` on diff wins over a
        // later check, but not over an earlier one (percentage, prices, budget).
        for call in [
            "diff --top 1 OLD NEW",
            "diff --json --top 1 OLD NEW",
            "diff --explain OLD NEW",
            "diff --json --explain OLD NEW",
            "scan --prices no-prices.json file.log",
            "diff --prices no-prices.json OLD NEW",
            "scan --json --explain --prices no-prices.json file.log",
            "diff --explain --prices no-prices.json OLD NEW",
            "diff --explain --window 1m OLD NEW",
            "diff --explain --days-per-month 31 OLD NEW",
            "diff --budget-month 1 OLD NEW",
            "diff --json --explain --budget-month 1 OLD NEW",
            "scan --max-new-share 10 file.log",
            "scan --max-new-share abc file.log",
            "diff --max-new-share abc OLD NEW",
            "diff --explain --max-new-share abc OLD NEW",
            "scan --budget-month 1 --prices no-prices.json file.log",
            "scan --json --message-field message file.log",
            "scan --format nope file.log",
        ] {
            assert!(calls.contains(call), "{rel}: table is missing {call}");
        }
    }
}

#[test]
fn reference_describes_masks_and_input_errors() {
    let reference = read(REF);
    for part in [
        "`[::1]:8080`",
        "`[2001:db8::1]:443`",
        "`::ffff:192.0.2.10`",
        "`fe80::1%eth0`",
        "and `[address]` without a port are one `<IP>`",
        "`2026-10-08T13:55:36,123Z`",
        "regardless of its length or digits",
        "`monkey=` and `keyboard=` stay",
        "`tokenizer=`, `passwordless=` and `secretary=` are masked",
        "otherwise `sessiontoken=` would leak",
        "`x-api-key` contains `api_key`",
        "including the scheme word",
        "an unquoted value runs to the end of the message",
        "a value without quotes ends at the first space.",
        "ends at its closing quote only if a separator follows: the end of the message, a comma, `}` or `]` (also after spaces), or spaces followed by a next field like `name=` or `name:`",
        "A quote without such a separator belongs to the value",
        "when in doubt, mask rather than leak",
        "`a::b`",
        "`std::fs::read`",
        "`13:55:36`",
        "`<NO-MESSAGE>`",
        "gb_bytes must be a whole number greater than 0",
        "price file has a price that is not a number",
        "price file has a duplicate key",
        "invalid rule on line 1 of the rules file",
        "`file is not a regular file`",
        "`price file is not a regular file`",
        "`rules file is not a regular file`",
        "without waiting for a writer to open it",
        "`file is UTF-16LE, not UTF-8; convert it first, for example with iconv -f UTF-16LE -t UTF-8, or in PowerShell with Out-File -Encoding utf8`",
        "One UTF-8 byte order mark at the start of the file is skipped for parsing",
    ] {
        assert!(reference.contains(part), "reference is missing {part}");
    }
    let readme = read(README);
    assert!(readme.contains(
        "estimate: extrapolation of one sample with your own prices; not an official rate"
    ));
}

const PLAIN_SENTENCE: &str = "After a keyword (`secret`, `token`, `password`, `bearer` and so on) followed by a space, the next word is always masked, even in ordinary sentences: `This is a secret mission report` → `This is a secret <SECRET> report`. This is deliberate: better to mask than to leak.";

const HEX_SENTENCE: &str = "A word of 8 or more hex characters → `<HEX>`, but only if it contains at least one letter a-f (or A-F) and at least one digit. A word of only digits, such as `1699482000`, becomes `<NUM>`.";

const SHARE_SENTENCE: &str = "bytes of the pattern divided by the total bytes, shown as a percentage with 1 decimal, rounded half away from zero";

const EXPLAIN_SENTENCE: &str = "With `--explain`, a `<SECRET>` count is marked `possible secret`. In `--json` such a pattern gets an extra `note` key with that text, and the `secret` key in `masks` holds the count.";

const PRIVACY_SENTENCE: &str = "Patterns keep the static words of a line and replace timestamps, UUIDs, IP addresses, e-mail addresses (an ordinary address, a quoted local part such as `\"john.doe\"@example.com`, and letters outside ASCII such as `jöhn@exämple.de`), secrets, hex, numbers (digits, also with a leading `+` or `-` or a decimal fraction, such as `-12.5`, `3.14` and `+31612345678`, and digits outside ASCII such as `١٢٣٤٥`) and other variable words. A line with nothing to replace, such as `disk full`, is shown in full, with its pattern id, line count, byte count and share. With `--prices`, the currency, source and date from the price file are shown too. An error about a log file, a price file or a rules file does not include the path, the raw log line, the rules line or the price-file contents.";

const PRIVACY_SENTENCE_NL: &str = "Patronen houden de vaste woorden van een regel en vervangen tijdstempels, UUID's, IP-adressen, e-mailadressen (een gewoon adres, een gequote lokaal deel zoals `\"john.doe\"@example.com`, en letters buiten ASCII zoals `jöhn@exämple.de`), geheimen, hex, getallen (cijfers, ook met een `+` of `-` ervoor of een decimale breuk, zoals `-12.5`, `3.14` en `+31612345678`, en cijfers buiten ASCII zoals `١٢٣٤٥`) en andere variabele woorden. Een regel waar niets te vervangen is, zoals `disk full`, wordt helemaal getoond, met het patroon-id, het aantal regels, het aantal bytes en het aandeel. Met `--prices` worden ook de valuta, de bron en de datum uit het prijsbestand getoond. Een fout over een logbestand, een prijsbestand of een regelsbestand bevat het pad niet, noch de ruwe logregel, de regel uit het regelsbestand of de inhoud van het prijsbestand.";

#[test]
fn key_sentences_appear_once() {
    let readme = read(README);
    let reference = read(REF);
    let limits = section(&readme, "## Limitations");
    assert_eq!(limits.matches(PLAIN_SENTENCE).count(), 1, "plain sentence");
    assert_eq!(reference.matches(HEX_SENTENCE).count(), 1, "hex sentence");
    assert_eq!(readme.matches(SHARE_SENTENCE).count(), 1, "share in README");
    assert_eq!(
        reference.matches(SHARE_SENTENCE).count(),
        1,
        "share in reference"
    );
    assert_eq!(
        reference.matches(EXPLAIN_SENTENCE).count(),
        1,
        "explain sentence"
    );
}

#[test]
fn privacy_sentence_matches_what_the_tool_shows() {
    let readme = read(README);
    let readme_nl = read(README_NL);
    assert_eq!(
        section(&readme, "## Privacy")
            .matches(PRIVACY_SENTENCE)
            .count(),
        1,
        "privacy sentence"
    );
    assert_eq!(
        section(&readme_nl, "## Privacy")
            .matches(PRIVACY_SENTENCE_NL)
            .count(),
        1,
        "Dutch privacy sentence"
    );
    for rel in [README, README_NL, REF, REF_NL] {
        let text = read(rel);
        assert!(
            !text.contains("No output or error message contains a complete raw log line"),
            "{rel} still overclaims a raw log line"
        );
        assert!(
            !text.contains("Geen uitvoer of foutmelding bevat een volledige ruwe logregel"),
            "{rel} still overclaims a raw log line"
        );
        assert!(
            !text.contains("only masked patterns, counts and line numbers"),
            "{rel} still overclaims what the output contains"
        );
        assert!(
            !text.contains("alleen gemaskeerde patronen, aantallen en regelnummers"),
            "{rel} still overclaims what the output contains"
        );
    }
}

/// The `--version` example in both references is the package version.
#[test]
fn reference_version_example_matches_package() {
    let version = env!("CARGO_PKG_VERSION");
    for rel in [REF, REF_NL] {
        let text = read(rel);
        let mut found = Vec::new();
        let mut rest = text.as_str();
        let marker = "`log-bill ";
        while let Some(start) = rest.find(marker) {
            let after = &rest[start + marker.len()..];
            let example: String = after
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
                .collect();
            let consumed = example.len();
            if !example.is_empty() {
                found.push(example);
            }
            rest = &after[consumed..];
        }
        assert_eq!(found, vec![version.to_string()], "{rel}");
    }
}

#[test]
fn plain_sentence_matches_the_binary() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("readme-plain-sentence.log");
    fs::write(&path, "This is a secret mission report\n").expect("write");
    let output = bin().arg("scan").arg(&path).output().expect("start binary");
    assert_eq!(output.status.code(), Some(0));
    let out = String::from_utf8(output.stdout).expect("utf-8");
    assert!(out.contains("This is a secret <SECRET> report"), "{out}");
}

#[test]
fn fixture_readme_describes_every_file_as_synthetic() {
    let text = read("tests/fixtures/README.md");
    assert!(
        !text.contains("CANARY-SECRET"),
        "fixture README shows a canary"
    );
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("fixtures") {
        let name = entry
            .expect("entry")
            .file_name()
            .into_string()
            .expect("utf-8");
        if name == "README.md" {
            continue;
        }
        count += 1;
        let part = section(&text, &format!("## {name}"));
        assert!(
            part.to_lowercase().contains("synthetic"),
            "{name} is not described as synthetic"
        );
    }
    assert!(count > 0);
}
