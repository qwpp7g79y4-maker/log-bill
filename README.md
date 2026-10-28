# log-bill

English · [Nederlands](README.nl.md)

Estimate what your logs cost per pattern, offline, before you ship them.

[![crates.io](https://img.shields.io/crates/v/log-bill.svg)](https://crates.io/crates/log-bill) [![downloads](https://img.shields.io/crates/d/log-bill.svg)](https://crates.io/crates/log-bill) [![CI](https://github.com/qwpp7g79y4-maker/log-bill/actions/workflows/ci.yml/badge.svg)](https://github.com/qwpp7g79y4-maker/log-bill/actions/workflows/ci.yml) [![PEPAI checked](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fqwpp7g79y4-maker%2Flog-bill%2Fmain%2F.github%2Fbadge.json)](QUALITY.md) [![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/Cargo.toml) [![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

[Installation](#installation) • [Quick start](#quick-start) • [How it counts](#how-it-counts) • [Alternatives](#compared-with-alternatives) • [Limitations](#limitations) • [Reference](docs/reference.md)

log-bill is a command-line tool written in Rust. It reads a sample of a log file on your machine (plain text or JSON lines), groups the lines into patterns by masking the variable parts, and shows how many lines and bytes each pattern is and what share of the volume it makes up. With a time window and a price file you supply yourself, it extrapolates the sample to an estimated cost per day and per month. It can simulate `drop` and `sample` rules, compare two samples, explain every calculation, write JSON, and fail a CI job when a budget is exceeded. It needs no account and makes no network connection.

## Why

Teams pay per GB for logs and often only see on the invoice which pattern produced the volume. Two Hacker News comments, checked on 8 October 2026: nthypes writes "My bill jumped nearly 1000% before alerts caught it" (https://news.ycombinator.com/item?id=46034896) and mleonhard mentions "the risk of bad logging running up a $70,000/day bill with CloudWatch Logs" (https://news.ycombinator.com/item?id=45630239). log-bill shows, before you ship anything and on a plain file, which patterns make up the bytes and what a drop or sample rule would save.

## Installation

With Rust 1.85 or newer, install from [crates.io](https://crates.io/crates/log-bill):

```sh
cargo install --locked log-bill
log-bill --help
```

Ready-made binaries for Linux (x86_64; from v0.1.1 also a static `musl` build that needs no particular glibc and runs on Alpine), Windows (x86_64) and macOS (Intel and Apple Silicon), each with a SHA-256 checksum, are on the [Releases page](https://github.com/qwpp7g79y4-maker/log-bill/releases).

Supported systems for the ready-made binaries:

- **Linux x86_64 (`x86_64-unknown-linux-gnu`):** built on Ubuntu 22.04, needs glibc 2.34 or newer (for example Ubuntu 22.04, Debian 12, RHEL 9 and later).
- **Linux x86_64 (`x86_64-unknown-linux-musl`, from v0.1.1):** statically linked, no glibc needed; for Alpine and older distributions.
- **Windows x86_64 (`x86_64-pc-windows-msvc`).**
- **macOS Intel (`x86_64-apple-darwin`) and Apple Silicon (`aarch64-apple-darwin`).**

The tests run in CI on Linux, Windows, macOS on Apple Silicon and macOS on Intel, and on Rust 1.85. Other systems may work with `cargo install`, but are not tested.

For the latest `main`: `cargo install --locked --git https://github.com/qwpp7g79y4-maker/log-bill`.

From a local clone: `cargo install --locked --path .`, or `cargo build --release`; the binary is `target/release/log-bill`. A debug build (`cargo run` without `--release`) is much slower on large files; for large logs use a release build (`cargo install` or `cargo build --release`).

## Quick start

These are the real outputs of the binary on the synthetic fixtures in `tests/fixtures`. A test runs every example and compares the output.

Group a log file into patterns:

```console
$ log-bill scan tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d8299c276  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab161881a7d411  1 line  10 bytes  8.5%  disk full
```

Extrapolate a one-minute sample to a day and a month:

```console
$ log-bill scan --window 1m tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d8299c276  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab161881a7d411  1 line  10 bytes  8.5%  disk full
factor: 1440
per day: 169920 bytes, 0.000 GB
per month: 5097600 bytes, 0.005 GB
```

118 bytes over one minute is × 1440 = 169920 bytes per day, and × 30 = 5097600 bytes per month. At 1000000000 bytes per GB that is 0.000 GB per day and 0.005 GB per month, shown with three decimals.

Indexed events use that same window. Three records over one second, kept for 7 days on the annual list price:

```console
$ log-bill scan --window 1s --days-per-month 30 --retention 7 --billing annual tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d8299c276  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab161881a7d411  1 line  10 bytes  8.5%  disk full
factor: 86400
per day: 10195200 bytes, 0.010 GB
per month: 305856000 bytes, 0.306 GB
indexed: list price, estimate, retention 7d, billing annual, 1.27 USD per 1M events
indexed events per month: 7.776M before index rules, 7.776M after
indexed cost per month: 9.88 USD before index rules, 9.88 USD after, savings 0.00 USD
indexed Pe19dc70d8299c276: 5.184M -> 5.184M events per month, 6.58 USD -> 6.58 USD, savings 0.00 USD
indexed P46ab161881a7d411: 2.592M -> 2.592M events per month, 3.29 USD -> 3.29 USD, savings 0.00 USD
```

3 records × 30 × 86400 / 1000000 = 7.776 million events per month. At 1.27 USD per million that is 9.87552, shown as 9.88. The synthetic fixture `tests/fixtures/indexed/app.log` (6000 lines) with the same window, `--retention 7`, `--billing annual` and an index rule `drop DEBUG` is 15552.000M events per month before the rule and 6928.416M after. 2673/6000 × 15552 × 1.27 is 8799.08832, shown as 8799.09 USD per month. Adding `drop GET /health` and `sample 0.1 INFO order` leaves 982.109M events and 1247.28 USD. The list was read from https://www.datadoghq.com/pricing/list/ on 2026-10-14. Without `--retention`, `--billing`, `--indexed-price` or `--index-rules` this section is absent.

Simulate a sample rule:

```console
$ log-bill scan --rules tests/fixtures/sample-0.1.txt tests/fixtures/sample-zero.log
total: 1 line, 10 bytes
P139ac555f8a88ea7  1 line  10 bytes  100.0%  xxxxxxxxx
after rules: 1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes
P139ac555f8a88ea7  1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes, rule 1
```

`sample-zero.log` is nine letters x plus a newline: 1 line, 10 bytes. `sample 0.1` keeps `floor(1 × 0.1) = 0` lines and `floor(10 × 0.1) = 1` byte.

Compare two samples:

```console
$ log-bill diff tests/fixtures/diff-old.log tests/fixtures/diff-new.log
total: 3 lines, 17 bytes -> 2 lines, 12 bytes, change -1 line, -5 bytes
new P229176bd1f6ba96a  0 -> 6 bytes, change +6 bytes, share 0.0% -> 50.0%, change +50.0 percentage points  gamma
gone P7627619b954620a7  5 -> 0 bytes, change -5 bytes, share 29.4% -> 0.0%, change -29.4 percentage points  beta
P8ac625bb85ed202b  12 -> 6 bytes, change -6 bytes, share 70.6% -> 50.0%, change -20.6 percentage points  alpha
new patterns: 6 of 12 bytes, 50.0%
```

The old sample is `alpha` twice (12 bytes) and `beta` once (5 bytes): 17 bytes. The new one is `alpha` (6) and `gamma` (6): 12 bytes. The new pattern `gamma` is 6 of 12 bytes, 50.0%.

### Try it on your own data

No clone and no fixtures needed: these two files are copy-pasteable and are enough to see a cost in your own currency.

```sh
cat > quickstart.log <<'LOG'
2026-10-24T00:00:00Z ERROR disk full on host web-1
2026-10-24T00:00:01Z ERROR disk full on host web-2
2026-10-24T00:00:02Z ERROR disk full on host web-3
2026-10-24T00:00:03Z ERROR disk full on host web-1
2026-10-24T00:00:04Z ERROR disk full on host web-2
2026-10-24T00:00:05Z ERROR disk full on host web-3
2026-10-24T00:00:06Z ERROR disk full on host web-1
2026-10-24T00:00:07Z ERROR disk full on host web-2
2026-10-24T00:00:08Z ERROR disk full on host web-3
2026-10-24T00:00:09Z ERROR disk full on host web-1
2026-10-24T00:00:10Z ERROR disk full on host web-2
2026-10-24T00:00:11Z ERROR disk full on host web-3
2026-10-24T00:00:12Z ERROR disk full on host web-1
2026-10-24T00:00:13Z ERROR disk full on host web-2
2026-10-24T00:00:14Z ERROR disk full on host web-3
2026-10-24T00:00:15Z ERROR disk full on host web-1
2026-10-24T00:00:16Z ERROR disk full on host web-2
2026-10-24T00:00:17Z ERROR disk full on host web-3
2026-10-24T00:00:18Z ERROR disk full on host web-1
2026-10-24T00:00:19Z ERROR disk full on host web-2
LOG
cat > quickstart-prices.json <<'JSON'
{
  "currency": "USD",
  "source": "manual",
  "date": "2026-10-24",
  "ingest_per_gb": 2
}
JSON
log-bill scan --window 10s --prices quickstart-prices.json quickstart.log
```

```text
total: 20 lines, 1020 bytes
Pccb846bfc4fff644  20 lines  1020 bytes  100.0%  <TS> ERROR disk full on host <VAR>
factor: 8640
per day: 8812800 bytes, 0.009 GB
per month: 264384000 bytes, 0.264 GB
prices: USD, source manual, date 2026-10-24
total ingest: 0.02 USD per day, 0.53 USD per month
Pccb846bfc4fff644 ingest: 0.02 USD per day, 0.53 USD per month
estimate: extrapolation of one sample with your own prices; not an official rate
```

20 lines in 10 seconds is a factor of 8640 to a day; at 2 USD per GB that is 0.02 USD per day and 0.53 USD per month. Use your own price per GB (ask whoever pays the bill, or see your provider's pricing page) and `--window` set to how long the sample actually covers.

`scan` and `diff` also read standard input when the file argument is `-`, for example `kubectl logs deploy/app | log-bill scan -`; output and exit code are the same as for a real file with the same bytes.

Prices come only from a JSON file you pass with `--prices` (together with `--window`); see [the price file format](docs/reference.md#price-file). Each price is read as an exact decimal. A fraction that does not fit, a number outside the range of a binary float, or a file nested more than 128 arrays or objects deep, is rejected, and so is a key that appears twice. Every text output with per-GB amounts ends with `estimate: extrapolation of one sample with your own prices; not an official rate`. An indexed-event estimate is labeled `list price, estimate`, or `custom price, estimate` when you pass `--indexed-price`. All options, exit codes and error messages are listed in the [reference](docs/reference.md) and in `log-bill --help`.

## How it counts

Every number can be traced back to these rules. The full masking rules are in the [reference](docs/reference.md#masking).

- **Bytes:** the raw bytes of a line plus 1 for the newline. Empty lines do not count. For JSON lines, the bytes of the whole line.
- **Pattern:** the message with variable parts masked (`<TS>`, `<UUID>`, `<IP>`, `<EMAIL>`, `<SECRET>`, `<HEX>`, `<NUM>`, `<VAR>`). The id is `P` plus the 16 hex characters of the 64-bit FNV-1a hash of the pattern text.
- **Share:** bytes of the pattern divided by the total bytes, shown as a percentage with 1 decimal, rounded half away from zero.
- **Window:** the daily factor is 86400 divided by the window in seconds. A month is `--days-per-month` days, 30 by default.
- **Cost:** per day = bytes in the sample × factor / bytes per GB × price per GB, separately for ingest and index. Per month = per day × days per month. All arithmetic uses whole numbers and fractions; rounding only happens on display: amounts to 2 decimals, GB to 3 decimals, half away from zero.
- **Rules:** `drop <regex>` or `sample <fraction> <regex>`, tested against the raw line, first match wins. A fraction longer than 80 characters is invalid. A sampled pattern keeps the expected value, rounded down once for lines and once for bytes, independently. That is why a pattern can end up with 0 lines and still some bytes. The regex and the raw line are never shown.
- **Indexed events:** optional, and only with `--window` plus `--retention`, `--billing`, `--indexed-price` or `--index-rules`. Events are ingested records. They use the same month factor as the bytes and stay an exact fraction until display (millions with 3 decimals, amounts with 2 decimals, half away from zero). Per-pattern index costs are rounded separately and can therefore differ from the total by at most half a cent per pattern, so up to N/2 cents at N patterns. `--index-rules` has the same syntax as `--rules` and applies after `--rules`; a sample is not rounded down to whole events. The built-in price is USD per million events from the public list. `--budget-month` does not include that estimate.
- **Budget:** `--budget-month` is checked against the bytes after `--rules`. The `total` line and its cost show the bytes before the rules; the savings are shown separately. The indexed-event estimate is not part of the budget.

Worked example (covered by a unit test): a sample of 1000000 bytes with `--window 1m` gives factor 1440, so 1440000000 bytes = 1.440 GB per day. At an ingest price of 0.10 per GB that is 0.144, shown as 0.14 per day, and 4.32 per month (30 days).

## Compared with alternatives

An honest comparison with five products. The pages were checked on 8 October 2026. Anything those pages do not state is marked here as not verified.

**Log10x** (https://doc.log10x.com/faq/pricing/tiers-billing/) does pattern analysis and cost reduction. The page says "Yes, and running the engine needs no account at all." An engine without a license "synthesizes a built-in evaluation license" and runs "up to 10 nodes, for 30 days from process start. It runs air-gapped". Above the table it says "Pricing is per node, with unlimited log volume and all products included at every tier." The columns are Tier, Nodes, Monthly and Annual. Free is listed as "Free | up to 10 | $0 | $0", Starter as "Starter | up to 30 | $99 | $990". $99 per month is the price of the whole Starter tier up to 30 nodes, not $99 per node. The binaries and runtime are proprietary: https://github.com/log-10x/pipeline-releases lists "Log10x binaries and runtime | Proprietary (commercial license required)" and "The installation scripts are open source", and the LICENSE file is called "Log10x Proprietary Software License". Choose Log10x for a product with a pipeline. Choose log-bill for one open binary that reads a file and explains every count.

**Cribl Stream** (https://cribl.io/pricing/stream/) is a pipeline. The page says "Route data from any source to any destination." The free plan goes up to 1 TB/day. The credit prices on that page are 0.32 Credits / GB (Enterprise, Cloud Worker), 0.26 Credits / GB (Enterprise, Hybrid Worker) and 0.27 Credits / GB (Standard, Cloud Worker), with "1 credit = $1". Cribl actually routes and samples data. log-bill does not execute rules: it simulates them and sends nothing.

**Datadog Log Patterns** (https://docs.datadoghq.com/logs/explorer/analytics/patterns/) groups logs in Datadog's Log Explorer. The page says "The pattern detection is based on 10,000 log samples." Whether Patterns is free is not verified: that page lists no price. It works on logs you already query there, not on a file on disk, and only inside Datadog.

**Grafana Adaptive Logs** (https://grafana.com/docs/grafana-cloud/adaptive-telemetry/adaptive-logs/) "is a log optimization tool in Grafana Cloud". The page says it lets you drop logs "before they reach Grafana Cloud". It is described as part of Grafana Cloud.

**Grafana Logs Drilldown** (https://grafana.com/docs/grafana/latest/visualizations/simplified-exploration/logs/patterns/) shows patterns next to the logs. The page says "The Grafana Logs Drilldown app shows you the patterns alongside their log volumes." "Patterns are ephemeral and are only mined from the previous three hours of your logs." "To see patterns on the Patterns tab, you must have the pattern ingester enabled in your Loki configuration file." It works on logs already in Loki, and only on the last three hours.

**log-bill** is MIT OR Apache-2.0, a single binary, no account, no network, not tied to one vendor. Prices come from the file you pass, and the calculation can be followed per pattern.

## Limitations

- No official rate from any vendor. Prices come only from your price file. With `--prices` and `--window` the text output ends with `estimate: extrapolation of one sample with your own prices; not an official rate`.
- No contract discounts, tiered pricing or minimum commitments. Indexed retention uses the public list price, or the amount you pass with `--indexed-price`, not a private contract.
- Compression and storage differ per platform. log-bill counts the raw bytes of the lines in the sample. A platform may measure after parsing, include metadata or charge per event.
- A price per million events is optional and separate from the per-GB price. https://docs.datadoghq.com/account_management/billing/pricing says "Datadog charges for ingested logs based on the total number of gigabytes submitted to the Datadog Logs service." and "Datadog charges per million log events submitted for indexing at the rate designated for the retention policy you selected." The ingest price per GB stays in the price file. `--retention`, `--billing`, `--indexed-price` or `--index-rules` adds the indexed-event estimate from https://www.datadoghq.com/pricing/list/ (read 2026-10-14). Without those options the output does not change. The tool does not convert a number of events to GB. `--budget-month` does not include the indexed-event estimate. Flex Logs are not priced.
- A sample is one window. Daily and monthly figures are an extrapolation.
- It does not know whether a line is needed for security or compliance. A drop simulation is not advice to drop.
- No gzip input in this version.
- A line longer than 1 MiB is cut at 1 MiB for patterns and masking; all of its bytes still count. A JSON line longer than 1 MiB is refused with exit 2 and the line number. This keeps memory small for a file with one huge line.
- After a keyword (`secret`, `token`, `password`, `bearer` and so on) followed by a space, the next word is always masked, even in ordinary sentences: `This is a secret mission report` → `This is a secret <SECRET> report`. This is deliberate: better to mask than to leak.
- A tab and other Unicode whitespace (for example a no-break space) are separators too. Escaped quotes in a message still hide the value (`{\"password\":\"open sesame\"}`). A value that starts with `[` or `{` is masked through the matching bracket (`password=["alpha", "beta"]`). A value that contains a keyword (`password=secretPass1`) is masked in full. An e-mail does not swallow a keyword that introduces a secret (`id=a@b.co.token=hunterpass`).
- Lookalike characters in a key name are recognised too, so a key that only looks like `password` or `secret` still hides the value. NFKC folds mathematical alphanumeric letters, enclosed alphanumerics and fullwidth forms; the Unicode confusable skeleton maps remaining lookalikes to Latin, on top of the existing table (Armenian `ր` counts as `r`). Invisible characters inside the key are ignored, as are an invalid byte (read as U+FFFD), control characters other than tab, line feed and carriage return, and combining marks; an accented Latin letter counts as its base letter, so `sécret` and `ṕassword` still hide the value. Fullwidth and small-form ASCII characters and Unicode whitespace other than space, tab, LF and CR are read as plain ASCII before masking, so `password＝value`, `token：value` and `password\u{00A0}=\u{00A0}value` hide the value too; patterns show them as plain ASCII, and the bytes still count as written.
- A line break or a carriage return before `=` or `:` is a separator, so `password` then a newline then `= rosebud` hides the value. A `\uXXXX` escape in a key is that character, so `pass\u0077ord` hides the value. A secret is found before a UUID, an IP or an e-mail mask, so `password-123e4567-e89b-12d3-a456-426614174000=` and `password-10.0.0.1=` still hide the value, and an e-mail used as a key (`a@b.co=`) does too. `"a b"@example.com` is an e-mail. A number with a trailing `!`, `?`, `,` or `;`, such as `7!`, is `<NUM>`; a trailing dot is left in place (`1699482000.`). When `message`, `msg`, `log` or the `--message-field` name appears twice, the later value is used.
- Only regular files are read, except the log file argument of `scan` and `diff` given as `-`, which reads standard input instead. A FIFO, a symlink to a FIFO, a directory or a device is refused with exit 2, without waiting for a writer.
- Input must be UTF-8. UTF-16 and UTF-32 (for example from Windows PowerShell 5.1 `Out-File` or `>`), with or without a byte order mark, are refused with exit 2 and a message that says how to convert them: `iconv -f UTF-16LE -t UTF-8 in.log > out.log`, or in PowerShell `Get-Content in.log | Out-File -Encoding utf8 out.log`. Their NUL bytes would otherwise split a key such as `password` so that masking misses it. Without a byte order mark the encoding is recognised by the NUL bytes in the first 4096 bytes; UTF-16 text that is mostly not ASCII may not be recognised. NUL bytes inside UTF-8 text are removed before masking, so they cannot hide a key either; the bytes still count. A UTF-8 byte order mark at the start of a text file is skipped and still counts in the bytes of the first line, so the same line with and without that mark is one pattern. Control characters in a log line and in the currency, source and date of a price file are not written raw. Text output shows them as `\u00XX` (ESC is `\u001b`) and escapes bidi controls the same way (U+202E is `\u202e`). JSON keeps the character (a control is escaped by JSON; a bidi mark stays UTF-8), so the output stays valid JSON.
- If the result cannot be written, the exit code is 3 and the message is `could not write output`. `--help` and `--version` use that same code when their text cannot be written.
- In `--json`, `inputs.paths` shows each path as text. A file name that is not valid UTF-8 (possible on Linux) is still read, but shown with U+FFFD for the invalid bytes.

## Privacy

log-bill is fully offline: no network, no API key, no telemetry. Patterns keep the static words of a line and replace timestamps, UUIDs, IP addresses, e-mail addresses (an ordinary address, a quoted local part such as `"john.doe"@example.com`, and letters outside ASCII such as `jöhn@exämple.de`), secrets, hex, numbers (digits, also with a leading `+` or `-` or a decimal fraction, such as `-12.5`, `3.14` and `+31612345678`, and digits outside ASCII such as `١٢٣٤٥`) and other variable words. A line with nothing to replace, such as `disk full`, is shown in full, with its pattern id, line count, byte count and share. With `--prices`, the currency, source and date from the price file are shown too. An error about a log file, a price file or a rules file does not include the path, the raw log line, the rules line or the price-file contents. An error about an index-rules file does not include the path or the rule line. Lookalike characters in a key name are recognised too, so a key that only looks like `password` or `secret` still hides the value. Leak tests plant a fake secret in every input type and check that no output contains it. All fixtures are synthetic.

## Support

log-bill is free to use. If it saves you money, you can support the work via [Ko-fi](https://ko-fi.com/pepai).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Copyright 2026 PEPAI.

Release archives include THIRD-PARTY-LICENSES.html with the licenses of the third-party crates in the binary.

## Contact

PEPAI, pep_ai@icloud.com

Report bugs and ideas via [Issues](https://github.com/qwpp7g79y4-maker/log-bill/issues); report security problems privately as described in [SECURITY.md](SECURITY.md).
