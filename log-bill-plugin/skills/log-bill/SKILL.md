---
name: log-bill
description: Estimate log volume and cost per pattern from a local log file, offline, before sending logs to a vendor. Use when the user mentions log cost, ingest pricing, noisy log patterns, sampling rules, or log-bill.
---

# log-bill

Use the local `log-bill` CLI (version 0.2.1). It groups a log file into patterns and can estimate volume and cost offline. It does not send logs anywhere.

## Install

If `log-bill` is not on `PATH`, install it with Rust 1.85 or newer:

```sh
cargo install --locked log-bill
log-bill --help
```

Do not invent flags. If you are unsure, run `log-bill --help` or `log-bill scan --help`. A debug `cargo run` is slow on large files; prefer an installed or release build.

## Commands (from `--help`)

```
log-bill [OPTIONS] <COMMAND>
```

- `scan` — Group a log file into patterns. Usage: `log-bill scan [OPTIONS] <FILE>`
- `diff` — Compare two samples. Usage: `log-bill diff [OPTIONS] <OLD> <NEW>`

The file arguments accept `-` for standard input, for example `kubectl logs deploy/app | log-bill scan -`. `-` is not accepted for `--prices`, `--rules`, or `--index-rules`. Do not pass `-` more than once in the same command.

Options:

- `--format <FORMAT>` — `text` (default) or `jsonl`
- `--message-field <NAME>` — JSON field that holds the message (only with `--format jsonl`)
- `--window <WINDOW>` — Time window of the sample, for example `15m` (units `s`, `m`, `h`, `d`)
- `--days-per-month <DAYS>` — Days in a month for the extrapolation (default 30; only with `--window`)
- `--prices <FILE>` — Price file (JSON) (only with `--window`)
- `--rules <FILE>` — File with drop and sample rules
- `--top <N>` — Show the N largest patterns; the rest as other (scan only)
- `--explain` — Show the masks and the calculation per pattern (scan only)
- `--json` — Write JSON instead of text
- `--budget-month <AMOUNT>` — Monthly budget; exit 1 if the estimate is above it
- `--max-new-share <PCT>` — Maximum share of new patterns, in percent (0 to 100) (diff)
- `--retention <DAYS>` — Indexed-log retention in days: 3, 7, 15 or 30 (scan; only with `--window`)
- `--billing <PLAN>` — Indexed-log billing plan: `annual`, `monthly` or `ondemand` (scan; only with `--window`)
- `--indexed-price <USD>` — Indexed-log price in USD per 1M events; overrides the list price (only with `--window`)
- `--index-rules <FILE>` — Index exclusion rules, applied after `--rules` (only with `--window`)
- `-h`, `--help` / `-V`, `--version`

Combinations that do not apply are rejected with exit 3. `--window` with `diff` only works together with `--prices` or `--json`. Prices for per-GB ingest come only from `--prices`. Do not invent a vendor rate. Text output with per-GB amounts ends with: `estimate: extrapolation of one sample with your own prices; not an official rate`.

## Examples from the project README

On your own data, copy-pasteable without a clone:

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

Fixture examples (need a clone):

```sh
log-bill scan tests/fixtures/scan-text.log
log-bill scan --window 1m tests/fixtures/scan-text.log
log-bill scan --window 1s --days-per-month 30 --retention 7 --billing annual tests/fixtures/scan-text.log
log-bill scan --rules tests/fixtures/sample-0.1.txt tests/fixtures/sample-zero.log
log-bill diff tests/fixtures/diff-old.log tests/fixtures/diff-new.log
```

`--budget-month` is checked against the bytes after `--rules`, and does not include the indexed-event estimate.
