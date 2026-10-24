# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Fixed

- `--rules`, `--index-rules` and `--prices` read the whole file through a plain `read_to_end` with no bound, unlike the log path's capped, streamed reading. A rules file of 3,000,000 lines (62 MB) pushed peak RSS to 14.4 GB, because every rule stays a compiled regex for the run. Each is now refused with exit 2 before it is read if larger than 8 MiB, and `--rules`/`--index-rules` is refused above 300,000 rules, both with a message naming the limit.

## [0.2.0] - 2026-10-24

### Added

- `scan` and `diff` read standard input when the file argument is `-`, for example `kubectl logs deploy/app | log-bill scan -`. The README has a copy-paste quick start that creates its own sample log and price file to try the tool without cloning the repository.
- `SECURITY.md` describes how to report a vulnerability, and Dependabot is configured for Cargo and GitHub Actions.

### Changed

- The quick start sample log and price file live under `tests/fixtures/` instead of the package root, so they are not shipped in the published crate. The copy-paste quick start still creates its own files.

### Build

- Release archives include `THIRD-PARTY-LICENSES.html`. CI checks the licenses of dependencies. The file is generated once on Linux, because cargo-about has no prebuilt binary for Intel macOS.
- Dependabot is limited to Cargo security updates, so the MSRV stays 1.85, and it ignores `dtolnay/rust-toolchain` because that ref is the Rust version.
- A test writes the README quick start files, checks them against `tests/fixtures/`, runs the documented scan command and compares the output with the text block in the README. The quality report's test count matches `cargo test`.
- `tests/readme.rs` is formatted. No behavior change.

## [0.1.3] - 2026-10-19

### Added

- Indexed log cost per million events on `scan`: `--retention` (3, 7, 15 or 30 days), `--billing` (`annual`, `monthly` or `ondemand`), `--indexed-price` and `--index-rules`, only together with `--window`. Text, `--explain` and `--json` show indexed events and cost per month before and after the index rules, plus the savings. The built-in price is the public Datadog list price read on 2026-10-14; `--indexed-price` overrides it. `--budget-month` does not include this estimate.

### Build

- CI also builds and tests the `x86_64-unknown-linux-musl` target, and checks that the glibc binary needs no glibc newer than 2.34.
- Release archives include the quality report (`QUALITY.md`, `QUALITY.nl.md`), and every archive is unpacked and checked before the release is made.
- CI and release workflows use the Node 24 versions of the GitHub actions.
- A `fuzz/` crate (cargo-fuzz) with one target per stage that reads untrusted input: log line reading, rule parsing, masking, price and budget parsing, diff, and rendering to text and JSON. Excluded from the published package and not part of the default build, so stable CI is unaffected. Property tests (proptest) check that masking is idempotent and order-independent, that a secret never survives masking under any whitespace or `\uXXXX` variant, that the parsers never panic, and that the cost calculation matches a plain reference. Found no bugs in the library; a minimal `src/lib.rs` now exposes the modules for the fuzz crate to use, with no behavior change to the binary.

### Fixed

- A price number outside the range of a binary float (`1e400`, a 310-digit number) is refused with exit 2 and the error for that field, not `price file is not JSON`.
- The decimal digit loops in the cost calculation stop with an error after the 39 digits a `u128` can hold instead of running without a limit.
- A price file nested 128 or more levels deep is `price file is nested too deeply` (exit 2), never `price file is not JSON`.
- A line longer than 1 MiB no longer fills memory: a text line is cut at 1 MiB for patterns and masking and still counts every byte; a JSON line that long is refused with exit 2. A 2 GB line used about 6 GB of memory before.

### Security

- Unicode whitespace around `=` or `:` (a no-break space, Ogham space, and the other White_Space characters) separates a key from its value, so the value is masked.
- Lookalike letters in a key name fold through NFKC and the Unicode confusable skeleton, so mathematical alphanumeric and Armenian lookalikes (`sec𝐫et`, `passwoրd`) hide the value too.
- A fullwidth or small-form `=`, `:` or quote, or an ideographic space, separates a key from its value like the ASCII character (`password＝value`), so the value is masked.
- A combining mark or an accented letter in a key name (`ṕassword`, `sécret`) is still recognised, so the value is masked.
- A key name with an invalid byte, a U+FFFD or a control character inside it (`s\xFFecret=`) is still recognised, so the value is masked.
- More lookalike letters fold to Latin in key names, including Cyrillic `н` for `n` (`authorizatioн`, `tokeн`) and lookalikes for `b`, `k`, `m`, `q`, `t`, `w` and `z`.

## [0.1.2] - 2026-10-14

### Security

- Secrets are masked in more shapes: after a tab, inside escaped JSON (`{\"password\":\"...\"}`), in whole array and object values, in keys written with `\uXXXX` escapes, with a line break before `=` or `:`, when the value itself contains a keyword such as `token`, and when another mask (e-mail, UUID, IP) touches the key.
- Quoted and international e-mail addresses (`"john.doe"@example.com`, `"a b"@example.com`, `jöhn@exämple.de`) are masked as `<EMAIL>`.
- Numbers with a sign, a decimal part, non-ASCII digits or trailing punctuation (`-12.5`, `3.14`, `١٢٣٤٥`, `7!`) are masked as `<NUM>`.
- A path that is swapped for a FIFO between the check and the read no longer makes the tool wait; the opened file itself is checked.

### Fixed

- Prices are read as exact decimals instead of through binary floating point, so a budget check can no longer pass on a rounded price. A duplicate key in a price file is refused with exit 2.
- A price fraction that does not fit, or a price file nested more than 128 levels deep, is refused with exit 2 instead of a wrong result or a crash.
- A write error, such as a full disk, is exit 3 with `could not write output` instead of a panic. `--help` and `--version` use that same code and do not exit 0.
- `--window 1é` is a usage error (exit 3, `invalid window`) instead of a panic.
- Control characters and bidirectional marks in a log line and in a price file's currency, source and date are not written raw. Text output shows them as `\uXXXX`. JSON keeps the character.
- A UTF-8 byte order mark at the start of a text file is skipped, and still counts in the bytes of the first line.
- Unused JSON fields with extreme values or deep nesting are ignored, and the line still counts.
- Pattern IDs are longer, so two different patterns no longer share an ID.
- A duplicate `message` key in a JSON line uses the last value; the reference now says so.

### Performance

- Input is read line by line instead of all at once, which keeps memory close to the size of one line.
- Long fractions in a rules file are bounded, and skipping unused JSON strings is linear.

### Build

- CI also runs the test suite in release mode.

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
