# log-bill

English · [Nederlands](README.nl.md)

Estimate what your logs cost per pattern, offline, before you ship them.

[Installation](#installation) • [Quick start](#quick-start) • [How it counts](#how-it-counts) • [Alternatives](#compared-with-alternatives) • [Limitations](#limitations) • [Reference](docs/reference.md)

log-bill is a command-line tool written in Rust. It reads a sample of a log file on your machine (plain text or JSON lines), groups the lines into patterns by masking the variable parts, and shows how many lines and bytes each pattern is and what share of the volume it makes up. With a time window and a price file you supply yourself, it extrapolates the sample to an estimated cost per day and per month. It can simulate `drop` and `sample` rules, compare two samples, explain every calculation, write JSON, and fail a CI job when a budget is exceeded. It needs no account and makes no network connection.

## Why

Teams pay per GB for logs and often only see on the invoice which pattern produced the volume. Two Hacker News comments, checked on 8 October 2026: nthypes writes "My bill jumped nearly 1000% before alerts caught it" (https://news.ycombinator.com/item?id=46034896) and mleonhard mentions "the risk of bad logging running up a $70,000/day bill with CloudWatch Logs" (https://news.ycombinator.com/item?id=45630239). log-bill shows, before you ship anything and on a plain file, which patterns make up the bytes and what a drop or sample rule would save.

## Installation

Requires a Rust toolchain (Rust 2021 edition).

```sh
cargo install --path .
log-bill --help
```

Or build it with `cargo build --release`; the binary is `target/release/log-bill`.

## Quick start

These are the real outputs of the binary on the synthetic fixtures in `tests/fixtures`. A test runs every example and compares the output.

Group a log file into patterns:

```console
$ log-bill scan tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab1618  1 line  10 bytes  8.5%  disk full
```

Extrapolate a one-minute sample to a day and a month:

```console
$ log-bill scan --window 1m tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab1618  1 line  10 bytes  8.5%  disk full
factor: 1440
per day: 169920 bytes, 0.000 GB
per month: 5097600 bytes, 0.005 GB
```

118 bytes over one minute is × 1440 = 169920 bytes per day, and × 30 = 5097600 bytes per month. At 1000000000 bytes per GB that is 0.000 GB per day and 0.005 GB per month, shown with three decimals.

Simulate a sample rule:

```console
$ log-bill scan --rules tests/fixtures/sample-0.1.txt tests/fixtures/sample-zero.log
total: 1 line, 10 bytes
P139ac555  1 line  10 bytes  100.0%  xxxxxxxxx
after rules: 1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes
P139ac555  1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes, rule 1
```

`sample-zero.log` is nine letters x plus a newline: 1 line, 10 bytes. `sample 0.1` keeps `floor(1 × 0.1) = 0` lines and `floor(10 × 0.1) = 1` byte.

Compare two samples:

```console
$ log-bill diff tests/fixtures/diff-old.log tests/fixtures/diff-new.log
total: 3 lines, 17 bytes -> 2 lines, 12 bytes, change -1 line, -5 bytes
new P229176bd  0 -> 6 bytes, change +6 bytes, share 0.0% -> 50.0%, change +50.0 percentage points  gamma
gone P7627619b  5 -> 0 bytes, change -5 bytes, share 29.4% -> 0.0%, change -29.4 percentage points  beta
P8ac625bb  12 -> 6 bytes, change -6 bytes, share 70.6% -> 50.0%, change -20.6 percentage points  alpha
new patterns: 6 of 12 bytes, 50.0%
```

The old sample is `alpha` twice (12 bytes) and `beta` once (5 bytes): 17 bytes. The new one is `alpha` (6) and `gamma` (6): 12 bytes. The new pattern `gamma` is 6 of 12 bytes, 50.0%.

Prices come only from a JSON file you pass with `--prices` (together with `--window`); see [the price file format](docs/reference.md#price-file). Every text output with amounts ends with `estimate: extrapolation of one sample with your own prices; not an official rate`. All options, exit codes and error messages are listed in the [reference](docs/reference.md) and in `log-bill --help`.

## How it counts

Every number can be traced back to these rules. The full masking rules are in the [reference](docs/reference.md#masking).

- **Bytes:** the raw bytes of a line plus 1 for the newline. Empty lines do not count. For JSON lines, the bytes of the whole line.
- **Pattern:** the message with variable parts masked (`<TS>`, `<UUID>`, `<IP>`, `<EMAIL>`, `<SECRET>`, `<HEX>`, `<NUM>`, `<VAR>`). The id is `P` plus the first 8 hex characters of the 64-bit FNV-1a hash of the pattern text.
- **Share:** bytes of the pattern divided by the total bytes, shown as a percentage with 1 decimal, rounded half away from zero.
- **Window:** the daily factor is 86400 divided by the window in seconds. A month is `--days-per-month` days, 30 by default.
- **Cost:** per day = bytes in the sample × factor / bytes per GB × price per GB, separately for ingest and index. Per month = per day × days per month. All arithmetic uses whole numbers and fractions; rounding only happens on display: amounts to 2 decimals, GB to 3 decimals, half away from zero.
- **Rules:** `drop <regex>` or `sample <fraction> <regex>`, tested against the raw line, first match wins. A sampled pattern keeps the expected value, rounded down once for lines and once for bytes, independently. That is why a pattern can end up with 0 lines and still some bytes. The regex and the raw line are never shown.
- **Budget:** `--budget-month` is checked against the bytes after `--rules`. The `total` line and its cost show the bytes before the rules; the savings are shown separately.

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
- No contract discounts, tiered pricing, minimum commitments or retention pricing.
- Compression and storage differ per platform. log-bill counts the raw bytes of the lines in the sample. A platform may measure after parsing, include metadata or charge per event.
- No price per million events, only per GB; log-bill also prices indexing per GB. https://docs.datadoghq.com/account_management/billing/pricing says "Datadog charges for ingested logs based on the total number of gigabytes submitted to the Datadog Logs service." and "Datadog charges per million log events submitted for indexing at the rate designated for the retention policy you selected." The ingest price per GB fits in the price file. The index price per million events does not: the tool does not convert a number of events to GB. Leave the index price out unless you already have an amount per GB.
- A sample is one window. Daily and monthly figures are an extrapolation.
- It does not know whether a line is needed for security or compliance. A drop simulation is not advice to drop.
- No gzip input in this version.
- After a keyword (`secret`, `token`, `password`, `bearer` and so on) followed by a space, the next word is always masked, even in ordinary sentences: `This is a secret mission report` → `This is a secret <SECRET> report`. This is deliberate: better to mask than to leak.
- Only regular files are read. A FIFO, a directory or a device is refused with exit 2.

## Privacy

log-bill is fully offline: no network, no API key, no telemetry. No output or error message contains a complete raw log line; only masked patterns, counts and line numbers. Leak tests plant a fake secret in every input type and check that no output contains it. All fixtures are synthetic.

## Support

log-bill is free to use. If it saves you money, you can support the work via [Ko-fi](https://ko-fi.com/pepai).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Copyright 2026 PEPAI.

## Contact

PEPAI, pep_ai@icloud.com
