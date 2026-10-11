# Changelog

All notable changes to this project are documented in this file.

## [0.1.0] - 2026-10-11

### Added

- `scan`: group a plain text or JSON lines sample into masked patterns with lines, bytes and share per pattern.
- `--window`, `--days-per-month` and `--prices`: extrapolate the sample to bytes, GB and cost per day and per month, using a price file you supply.
- `--rules`: simulate `drop` and `sample` rules and show the savings.
- `diff`: compare two samples, with new and removed patterns and the share of new patterns.
- `--explain`: show the masks per pattern and the full calculation.
- `--json`: JSON output with `schema_version` 1.
- `--budget-month` and `--max-new-share`: exit 1 in CI when a budget or threshold is exceeded.
