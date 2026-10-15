# Quality report

This page shows how each release of log-bill was checked, what we found, and what we fixed. It describes checks we did. It is not a guarantee that the software is free of bugs or secure.

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
- Unreleased: the indexed-cost code added after this version (commit [`0c3486d`](https://github.com/qwpp7g79y4-maker/log-bill/commit/0c3486d)) has not yet had a final review.

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
