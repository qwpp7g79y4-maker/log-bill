# Changelog

All notable changes to this project are documented in this file.

## [0.1.1] - 2026-10-11

### Security

- UTF-16 and UTF-32 input (for example from Windows PowerShell 5.1 `Out-File`) was read as UTF-8. Its NUL bytes split keys such as `password=`, so a secret could reach the output unmasked. Such input, with or without a byte order mark, is now refused with exit 2 and a message that says how to convert it to UTF-8.
- NUL bytes inside UTF-8 text are removed from the message before masking, so they can no longer hide a key. The byte counts do not change.

### Fixed

- A JSON lines file or price file that starts with a UTF-8 byte order mark is no longer refused as invalid JSON. The BOM still counts in the bytes of the first line.
- File names that are not valid UTF-8 (possible on Linux) are read instead of refused with exit 3. In `--json` they are shown with U+FFFD for the invalid bytes.
- Usage lines always name the binary `log-bill`, also on Windows.
- `cargo clippy` is clean on Rust 1.85 as well as on stable.
- A JSON test no longer fails when the checkout path contains the letter `q`.

### Changed

- Release binaries: a static `x86_64-unknown-linux-musl` build is added, and the glibc build is made on Ubuntu 22.04 (glibc 2.34 or newer). A release is only made when fmt, clippy and all tests pass on the tag commit. The archives now also contain `README.nl.md`, `docs/`, `CONTRIBUTING.md`, `SECURITY.md` and `tests/fixtures/`.
- CI also tests on macOS Intel, runs clippy on Rust 1.85 and installs Python explicitly. The tests use `python3`, or `python` when that is the only name.

## [0.1.0] - 2026-10-11

### Added

- `scan`: group a plain text or JSON lines sample into masked patterns with lines, bytes and share per pattern.
- `--window`, `--days-per-month` and `--prices`: extrapolate the sample to bytes, GB and cost per day and per month, using a price file you supply.
- `--rules`: simulate `drop` and `sample` rules and show the savings.
- `diff`: compare two samples, with new and removed patterns and the share of new patterns.
- `--explain`: show the masks per pattern and the full calculation.
- `--json`: JSON output with `schema_version` 1.
- `--budget-month` and `--max-new-share`: exit 1 in CI when a budget or threshold is exceeded.
