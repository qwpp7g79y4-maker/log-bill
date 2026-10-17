# Quality report

This page shows how each release of log-bill was checked, what we found, and what we fixed. It describes checks we did. It is not a guarantee that the software is free of bugs or secure.

## Unreleased

| | |
|---|---|
| Commit checked | [`8ea5157`](https://github.com/qwpp7g79y4-maker/log-bill/commit/8ea5157) |
| Date | 2026-10-17 |
| Tests | 325 automated tests, run in debug mode, release mode, and on Rust 1.85 (all three: 325 passed, 0 failed) |

### What was added

- A `fuzz/` crate (cargo-fuzz, not part of the published package or the default build) with one target per stage that handles untrusted input:

  | Target | Invariant it checks |
  |---|---|
  | `fuzz_diff` | Comparing two arbitrary samples never panics; the reported totals match the inputs, and no row's bytes exceed its sample's total |
  | `fuzz_mask` | Masking arbitrary text never panics, and a secret placed after a recognised key and separator never appears in the masked output |
  | `fuzz_money` | Price, window and budget parsing of arbitrary text never panics, and every amount the cost calculator produces is a non-negative decimal, never NaN |
  | `fuzz_read` | Reading a file of arbitrary bytes, in either format, never panics and never reports more lines than the file has newlines |
  | `fuzz_render` | Rendering to text and JSON never panics, every JSON body it produces parses back, and a masked secret never resurfaces in either rendering |
  | `fuzz_rules` | Parsing a rules file of arbitrary bytes never panics, and applying it never grows a pattern's lines or bytes |

- Property tests (proptest), run as part of the normal test suite: masking is idempotent and order-independent, a secret after a recognised key never leaks under any whitespace or `\uXXXX` variant, the cost calculation matches a plain reference computation, and the rules, window, budget, price-file and line-reading parsers never panic on arbitrary input.
- New regression tests ([`tests/edge_cases.rs`](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/tests/edge_cases.rs)) for an empty file, a file of only newlines, a lone `\r` with no `\n`, a byte-order mark with nothing else, a raw NUL byte inside a JSON string, a JSON Unicode escape for NUL splitting a key, a duplicate key nested inside a price-file object, a negative-zero price, and a budget past the limit of a 64-bit float.
- A `Fuzz build` job in CI that builds all six targets on nightly Rust; it builds them but does not run them (see "Not covered" below).

### How we checked

- `cargo test` and `cargo test --release`: 325 passed, 0 failed, both exit 0.
- `cargo +1.85 test` (oldest supported Rust version): 325 passed, 0 failed, exit 0.
- `cargo clippy --all-targets -- -D warnings`: exit 0, no warnings, in the main crate and in `fuzz/`.
- `cargo fmt --check`: exit 0, in the main crate and in `fuzz/`.
- `cargo +nightly fuzz build --target x86_64-unknown-linux-gnu`: exit 0, all six targets build.
- Each of the six targets fuzzed for 660 seconds, sequentially, one pinned CPU core:

  | Target | Wall time | Executions | Crashes |
  |---|---|---|---|
  | `fuzz_diff` | 662 s | 2,042,449 | none |
  | `fuzz_mask` | 661 s | 534,625 | none |
  | `fuzz_money` | 662 s | 1,399,109 | none |
  | `fuzz_read` | 662 s | 5,482,286 | none |
  | `fuzz_render` | 661 s | 662,055 | none |
  | `fuzz_rules` | 661 s | 650,763 | none |

- Automated tests in CI for this commit: [run 38972694602](https://github.com/qwpp7g79y4-maker/log-bill/actions/runs/38972694602), success, including the new fuzz-build job.

### Real bugs found

None. While writing the fuzz targets and property tests we found and corrected one mistake in the tests themselves: the fuzz and property tests for masking originally listed fullwidth `＝`/`：` as separators for `mask::mask` to recognise directly, but that folding happens earlier, while a line is read, not inside the masker. The test lists were corrected to match; this is a test-harness correction, not a product bug.

### Not covered

- These are short local fuzzing runs (11 minutes per target, one CPU core), not continuous fuzzing.
- CI builds the six fuzz targets on every change, but does not run them; the 11-minutes-per-target runs above were done locally, separately from CI.
- This commit has not yet had an independent final review; that happens after this commit, and the "PEPAI checked" badge in the README stays as it is now until that review is recorded.

## v0.1.2

| | |
|---|---|
| Version | 0.1.2 ([release](https://github.com/qwpp7g79y4-maker/log-bill/releases/tag/v0.1.2), [crates.io](https://crates.io/crates/log-bill/0.1.2)) |
| Commit checked | [`e626c10`](https://github.com/qwpp7g79y4-maker/log-bill/commit/e626c10) |
| Date | 2026-10-14 |
| Tests | 247 automated tests, run in debug and release mode |

### How we checked

- The same checks as for the previous version (below), on every platform.
- A second external automated code review. Every finding we could reproduce is fixed in this version, each with a regression test that uses the exact input.
- Automated tests in CI: [run 38539431902](https://github.com/qwpp7g79y4-maker/log-bill/actions/runs/38539431902) on Linux, Windows, macOS on Apple Silicon and Intel, and Rust 1.85, plus `cargo audit` (no known vulnerabilities).
- A fresh independent final review of commit [`76180be`](https://github.com/qwpp7g79y4-maker/log-bill/commit/76180be), which is later than the release commit above and adds fixes from an earlier final review (correct message for prices outside the float range, musl tests in CI, a docs version check), against a fixed edge-case checklist, every item tried with a concrete bad input. It ended DONE, with no blocking findings.
- Unreleased: the indexed-cost code added after this version had a fresh independent final review of commit [`7e8f630`](https://github.com/qwpp7g79y4-maker/log-bill/commit/7e8f630) against the same fixed edge-case checklist. It ended DONE, with no blocking findings. Its non-blocking findings (the quality report missing from release archives, a glibc check looser than the README) were fixed afterwards with tests. A later final review of commit `416696c` found a secret leak when a key name uses lookalike characters (for example a Cyrillic letter in `password`); that is fixed in `f9da99c` with regression tests. A final review of commit `5b41dc4` found two more leaks of the same kind (a Cyrillic `н` in `authorization` or `token`, and an invalid byte inside a key); both are fixed with regression tests that try every lookalike and bad byte at every position of every keyword. A final review of the fixed code found that one huge line was held in memory several times; lines are now cut at 1 MiB, with tests. A further final review found that a combining mark in a key (`ṕassword`) hid the key; combining marks and accented letters are now folded, with tests over every keyword and position. The next final review found that a fullwidth `=`, `:` or quote hid the value; such characters are now read as ASCII before masking, with tests. The next final review of commit [`72b3cef`](https://github.com/qwpp7g79y4-maker/log-bill/commit/72b3cef) found that a Unicode space around `=` and lookalike letters outside the table hid the value; that is fixed in [`49c2a70`](https://github.com/qwpp7g79y4-maker/log-bill/commit/49c2a70) (every Unicode space separates, lookalikes fold through Unicode confusables), with tests. A fresh independent final review of commit [`49c2a70`](https://github.com/qwpp7g79y4-maker/log-bill/commit/49c2a70), against the same fixed edge-case checklist with every item tried with a concrete bad input, ended DONE, with no blocking findings.

### What we found and fixed

| Problem | Fix |
|---|---|
| Secrets could stay visible in some shapes (after a tab, in escaped JSON, in array or object values, in `\uXXXX`-escaped keys, after a line break, or when another mask touched the key) | All these shapes are masked; a generated leak test covers them |
| Quoted and international e-mail addresses, and numbers with a sign, decimals, non-ASCII digits or trailing punctuation, were not masked | They are masked as `<EMAIL>` and `<NUM>` |
| Prices went through binary floating point, so a budget check could pass on a rounded price; duplicate price keys were accepted | Prices are exact decimals; duplicate keys, overflowing fractions and very deep nesting are refused with exit 2 |
| A full disk or an invalid `--window` caused a panic | Clean errors with documented exit codes |
| Control and bidirectional characters from input reached the terminal raw | They are shown as visible escapes in text output |
| A path swapped for a FIFO could make the tool wait; large files were read into memory at once | The opened file is checked; input is read line by line |

All fixes are listed in the [changelog](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/CHANGELOG.md).

## v0.1.1

| | |
|---|---|
| Version | 0.1.1 ([release](https://github.com/qwpp7g79y4-maker/log-bill/releases/tag/v0.1.1), [crates.io](https://crates.io/crates/log-bill/0.1.1)) |
| Commit checked | [`2054eba`](https://github.com/qwpp7g79y4-maker/log-bill/commit/2054eba) |
| Date | 2026-10-11 |
| Tests | 197 automated tests |

### What we checked

**Code review.** The code was read against nine fixed questions:
1. Does it give the right answer?
2. What happens with bad or missing input?
3. What happens when it fails halfway, for example when output cannot be written?
4. Which assumptions does it make, and are they written down?
5. Is it safe when things happen at the same time?
6. Can untrusted input leak into the output or do harm, for example through hidden characters?
7. Does it stay fast and small with large input?
8. Is every claim backed by a test?
9. Is the code easy to read and change?

**Input from every platform.** UTF-8 with and without a byte order mark, UTF-16 and UTF-32, Windows line endings (CRLF), and file paths that are not valid UTF-8.

**Tests on every platform.** Every change runs the full test suite on Linux, Windows, macOS on Apple Silicon and macOS on Intel, plus the oldest supported Rust version (1.85).

**Release.** The release is built only after the tests pass again on the tagged commit. Each archive gets a SHA-256 checksum.

**Dependencies.** `cargo audit` on 2026-10-12 against the RustSec advisory database (1296 advisories): no known vulnerabilities or warnings in the 33 crate dependencies. It now runs in CI on every change.

### How we checked

- Independent code review, by reviewers who did not write the code.
- An external automated code review. All its findings are fixed in this version.
- Automated tests in CI. Runs: [release v0.1.1](https://github.com/qwpp7g79y4-maker/log-bill/actions/runs/38164492339), [CI on main](https://github.com/qwpp7g79y4-maker/log-bill/actions/workflows/ci.yml?query=branch%3Amain).
- Dependency audit with `cargo audit`.

We do not name the review services here. The checks and their results are what matters, and every result below can be verified in the code and the changelog.

### What we found and fixed

| Problem | Fix |
|---|---|
| UTF-16 and UTF-32 input was read as UTF-8, so its NUL bytes could split a key such as `password=` and let a secret through unmasked | Such input is refused with exit 2 and a message that says how to convert it |
| NUL bytes inside UTF-8 text could hide a key from masking | They are removed from the message before masking; byte counts do not change |
| UTF-8 text with a NUL every 4 bytes could be taken for UTF-16 | The encoding check no longer mistakes it for UTF-16 |
| A JSON lines or price file starting with a UTF-8 byte order mark was refused | The byte order mark is accepted |
| File names that are not valid UTF-8 were refused | They are read; `--json` shows U+FFFD for invalid bytes |
| On Windows the usage line named `log-bill.exe` | It always names `log-bill` |

All fixes are in v0.1.1, see the [changelog](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/CHANGELOG.md#011---2026-10-11).

### Not covered

- UTF-16 text without a byte order mark that is mostly not ASCII may not be recognised.
- The cost estimate uses prices you supply. It is not an official bill.

## Report a problem

Open an [issue](https://github.com/qwpp7g79y4-maker/log-bill/issues), or for security problems follow [SECURITY.md](SECURITY.md). Contact: pep_ai@icloud.com.
