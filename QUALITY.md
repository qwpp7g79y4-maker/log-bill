# Quality report

This page shows how each release of log-bill was checked, what we found, and what we fixed. It describes checks we did. It is not a guarantee that the software is free of bugs or secure.

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
