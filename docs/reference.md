# log-bill reference

English · [Nederlands](reference.nl.md)

This page has the complete details. For an overview, see the [README](../README.md).

## Options

Run `log-bill --help` or `log-bill scan --help` for the built-in help. Combinations that do not apply are rejected with exit 3 instead of being silently ignored; see [Exit codes and usage errors](#exit-codes-and-usage-errors).

| Option | Applies to | What it does |
| --- | --- | --- |
| `--format` | scan and diff | Input format, `text` or `jsonl`. Default `text`. |
| `--message-field` | scan and diff, only with `--format jsonl` | Take the pattern from this JSON field. |
| `--window` | scan; for diff only with `--prices` or `--json` | Time window of the sample, for example `15m`. Extrapolates to a day and a month. |
| `--days-per-month` | scan and diff, only with `--window` | Days in a month. Default 30. |
| `--prices` | scan and diff, only with `--window` | Your own price file (JSON). |
| `--rules` | scan and diff | Simulate `drop` and `sample` rules. |
| `--top` | scan | Show the N largest patterns; the rest as one `other` line. |
| `--explain` | scan, also with `--json` | Show the masks per pattern and the full calculation. |
| `--json` | scan and diff | JSON output with `schema_version` 1. |
| `--budget-month` | scan, with `--window` and `--prices` | Exit 1 if the monthly ingest cost after rules is above this amount. The indexed-event estimate is not included. |
| `--max-new-share` | diff | Exit 1 if new patterns are more than PCT percent of the new bytes. |
| `--retention` | scan, only with `--window` | Indexed-log retention in days: `3`, `7`, `15` or `30`. Default 15 when an indexed option is set. |
| `--billing` | scan, only with `--window` | Indexed-log plan: `annual`, `monthly` or `ondemand`. Default `ondemand` when an indexed option is set. |
| `--indexed-price` | scan, only with `--window` | USD per 1M indexed events. Overrides the list price. Same decimal rules as a price in the price file. |
| `--index-rules` | scan, only with `--window` | Index exclusion rules, same syntax as `--rules`, applied to the weight left after `--rules`. |
| `--help` | everywhere | Show help and exit 0. |
| `--version` | top level | Show the version (`log-bill 0.1.2`) and exit 0. |

## Price file

Prices come only from a JSON file you pass with `--prices` (together with `--window`). This is the synthetic fixture `tests/fixtures/prices.json`:

```json
{
  "currency": "EUR",
  "source": "manual",
  "date": "2026-10-08",
  "ingest_per_gb": 0.10,
  "index_per_gb": 0.05,
  "gb_bytes": 500000000
}
```

`index_per_gb` and `gb_bytes` are optional. Without `gb_bytes` a GB is 1000000000 bytes. `gb_bytes` must be a whole number greater than 0. A price is that exact decimal, not a binary floating-point value. A fraction that does not fit in the accumulator is not a number: exit 2, `price file has a price that is not a number`. A number outside the range of a binary float, such as `1e400`, `-1e400`, `1E+400` or a `1` followed by 309 zeros, is not invalid JSON. On a price that is the same message. On `gb_bytes` it is `gb_bytes must be a whole number greater than 0`. More than 128 nested arrays or objects is exit 2: `price file is nested too deeply`. The same key twice is exit 2: `price file has a duplicate key`.

## How it counts

- **Bytes of a line:** the raw bytes of the line plus 1 for the newline. A `\r` before the `\n` counts as a byte. A last line without a newline also counts +1. Empty lines do not count.
- **Encoding:** input is UTF-8. Invalid UTF-8 bytes count and are shown as U+FFFD. UTF-16 and UTF-32 are refused with exit 2, see [Exit codes and usage errors](#exit-codes-and-usage-errors). NUL bytes are removed from the message before masking; they still count as bytes. One UTF-8 byte order mark at the start of a text file is skipped for the line text and still counts in the bytes of the first line, so the same line with and without that mark is one pattern. Control characters in a log line and in a price file's currency, source and date are not written raw. Text output shows them as `\u00XX` (ESC is `\u001b`). The same text output escapes bidi controls, so they cannot reverse the line: U+202A–U+202E, U+2066–U+2069, U+200E, U+200F and U+061C. U+202E is `\u202e`. JSON keeps the character (a control is escaped by JSON; a bidi mark is kept as UTF-8). The pattern id is the hash of the masked text, those characters included.
- **JSON lines:** the bytes are those of the whole JSON line. One UTF-8 byte order mark at the start of the file is skipped for parsing and counts in the bytes of the first line; the same BOM is skipped in a price file. The pattern comes from `--message-field`, or by default from the first of `message`, `msg` or `log` that exists. Without a message field the pattern is `<NO-MESSAGE>`. An unknown field is ignored, also when its number does not fit in a binary float (such as `1e400`) or its value is nested 200 times; that line still counts. If `message`, `msg`, `log`, or the field named with `--message-field` appears twice, the later value is the one that is used.
- **Pattern:** the masked message (see [Masking](#masking)). The pattern id is `P` plus the 16 hex characters of the 64-bit FNV-1a hash of the pattern text, so it is stable between runs.
- **Share:** bytes of the pattern divided by the total bytes, shown as a percentage with 1 decimal, rounded half away from zero.
- **Window:** `--window` takes a number and `s`, `m`, `h` or `d`. The daily factor is 86400 divided by the window in seconds. A month is `--days-per-month` days, 30 by default.
- **Cost:** per day = bytes in the sample × factor / bytes per GB × price per GB, separately for ingest and (if given) index. Per month = per day × days per month. All arithmetic uses whole numbers and fractions; rounding only happens on display: amounts to 2 decimals, GB to 3 decimals, half away from zero.
- **Rules (`--rules FILE`):** one per line, `drop <regex>` or `sample <fraction> <regex>`. The fraction is the part that is kept, from 0 up to but not including 1, written with a dot (`0.1`, not `0,1`), and at most 80 characters. The regex is tested against the raw line and the first matching rule wins. Lines that match no rule stay. For sampled lines the pattern keeps the expected value: the sum of `1 × fraction` for the lines and the sum of `bytes × fraction` for the bytes, each rounded down once and independently. With one fraction for the whole pattern that is `floor(count × fraction)` lines and `floor(bytes × fraction)` bytes. If two fractions apply to the same pattern, the expected values are added first and rounded down once. That is why a pattern can end up with 0 lines and still some bytes. The regex and the raw line are never shown, only the rule's line number.
- **Indexed events:** shown only when `--retention`, `--billing`, `--indexed-price` or `--index-rules` is set, and only together with `--window`. Events are the ingested records. An empty line does not count. One JSON line is one event, even when the message contains a newline. The month factor is the same as for the bytes. The count stays an exact fraction until display: millions of events with 3 decimals, amounts with 2 decimals, half away from zero. `--index-rules` uses the same syntax as `--rules` and applies to the weight left after `--rules`. A sample stays a fraction (889 lines × 0.1 = 88.9 events) and is not rounded down to whole lines. Defaults are 15 days and `ondemand`, and only when indexing is on. The built-in USD price per 1M events was read from https://www.datadoghq.com/pricing/list/ on 2026-10-14: 3 days 1.06/1.27/1.59, 7 days 1.27/1.52/1.91, 15 days 1.70/2.04/2.55, 30 days 2.50/3.00/3.75 (annual/monthly/ondemand). `--indexed-price` replaces that cell. The label is `list price, estimate`, or `custom price, estimate` when the price is overridden. Text, `--explain` and `--json` show events and cost per month before and after the index rules, plus the savings. Savings round the cost of the difference once. Per-pattern index costs are rounded separately and can therefore differ by 1 cent from the total. `--budget-month` does not include this estimate. Flex Logs and contract prices are not included.
- **Budget:** `--budget-month` is checked against the bytes after `--rules`. The `total` line and its cost show the bytes before the rules; the savings are shown separately. The indexed-event estimate is not part of that check.
- **Diff:** per pattern id the old and new bytes and share, plus new and removed patterns. The share of new patterns is their bytes divided by the total new bytes.

## Masking

Masks are applied to the message in this order. A secret key is found in the original text, before a later mask can split it:

1. Secret after a key → `<SECRET>`.
2. Timestamp → `<TS>` (ISO 8601 / RFC 3339 including a comma fraction like `2026-10-08T13:55:36,123Z`, `10/Oct/2026:13:55:36 +0200`, `2026-10-08 11:07:44`).
3. UUID → `<UUID>`.
4. IPv4 and IPv6 → `<IP>`.
5. E-mail address → `<EMAIL>`, including an ordinary address, a quoted local part (`"john.doe"@example.com`, and `"a b"@example.com` with a space inside the quotes) and letters outside ASCII (`jöhn@exämple.de`).
6. A word of 8 or more hex characters → `<HEX>`, but only if it contains at least one letter a-f (or A-F) and at least one digit. A word of only digits, such as `1699482000`, becomes `<NUM>`.
7. Any other number → `<NUM>`, including digits with a leading `+` or `-` or a decimal fraction (`-12.5`, `3.14`, `+31612345678`), digits outside ASCII (`١٢٣٤٥`), and a number with a trailing `!`, `?`, `,` or `;` (`7!`). A trailing dot is not part of the number (`1699482000.` stays `<NUM>.`). A time like `13:55:36` is not a number. Any other word containing a digit, and any word of 32 or more characters → `<VAR>`.

Multiple spaces become one space.

**IP addresses.** An IPv4 address is four decimal octets without a leading zero. IPv6 is recognised per word: plain `2001:db8::1`, `::1`, `::` and full eight-group addresses become `<IP>`. `std::fs::read` (two `::`) does not, and neither does a compressed form with text on both sides of `::`, at most two groups, no digit and no IPv4 tail, such as `a::b`. A time like `13:55:36` is not an IP. An IPv4-mapped address (`::ffff:192.0.2.10`) is one `<IP>`, and so is an address with a zone index (`fe80::1%eth0`). `[address]:port` (`[::1]:8080`, `[2001:db8::1]:443`) and `[address]` without a port are one `<IP>` including the brackets and port.

**Secrets.** A key is the word directly before `=`, `:`, a space, a tab, a line break or a carriage return, also in JSON-like notation (`"key": "value"`) and in headers. If that word contains `password`, `passwd`, `secret`, `token`, `api_key`, `apikey`, `authorization` or `bearer`, in any letter case and also with `_` or `-` before or after it, the value always becomes `<SECRET>`, regardless of its length or digits. A `-` counts as `_`, so `x-api-key` contains `api_key`. `monkey=` and `keyboard=` stay, because `key` on its own is not a keyword. `tokenizer=`, `passwordless=` and `secretary=` are masked, because otherwise `sessiontoken=` would leak. A `\uXXXX` escape inside the key is that character, so `pass\u0077ord` contains `password`. Lookalike characters in a key name are recognised too, so a key that only looks like `password` or `secret` still hides the value. Fullwidth forms fold to ASCII, common Cyrillic and Greek lookalikes fold to the Latin letter they imitate, and invisible characters inside the key are ignored, as are an invalid byte (read as U+FFFD) and control characters other than tab, line feed and carriage return. The key is read before UUID, IP and e-mail masks run, so `password-123e4567-e89b-12d3-a456-426614174000=` and `password-10.0.0.1=` still hide the value, and so does the same key with the UUID, IP or e-mail on the other side. An e-mail address used as a key (`a@b.co=`) hides the value too; a space after an address, with no `=` or `:`, does not.

For a key containing `authorization` (`Authorization`, `Proxy-Authorization`, `"authorization":` and so on) the whole value, including the scheme word (`Basic`, `Bearer`, `Digest`, `NTLM`, `Negotiate` or an unknown scheme), becomes one `<SECRET>`, and an unquoted value runs to the end of the message.

Where a secret value ends: a value without quotes ends at the first space. A quoted value (double or single quotes) ends at its closing quote only if a separator follows: the end of the message, a comma, `}` or `]` (also after spaces), or spaces followed by a next field like `name=` or `name:`. A quote without such a separator belongs to the value, and a backslash escape keeps the next character in the value. If no closing is found, the value runs to the end of the message: when in doubt, mask rather than leak.

A tab ends an unquoted value the same way, and a tab is a separator wherever a space is. A line break or a carriage return is too: `password` then a newline then `= rosebud` hides the value, and so does a carriage return before `:`. A backslash right before `"` or `'` is that quote, so escaped JSON in a message (`{\"password\":\"open sesame\"}`) masks the value. Inside a value that opened with a real quote, the backslash still keeps the next character. A value that starts with `[` or `{` runs through the matching `]` or `}`, so the whole array or object is one secret (`password=["alpha", "beta"]`, `secret={"a": "hunter", "b": "two"}`). If that closer never comes, the value runs to the end of the message. A value that contains a keyword is still that one secret (`api_key=tokenHunterWordHere next=ok`, `password=secretPass1`, `password=TokenValueXyzHere`). Masking continues at the value only when the whole value is exactly a keyword and that keyword has its own value (`token: Bearer abc`). An e-mail address does not swallow a trailing keyword that introduces a secret (`id=a@b.co.token=hunterpass`).

With `--explain`, a `<SECRET>` count is marked `possible secret`. In `--json` such a pattern gets an extra `note` key with that text, and the `secret` key in `masks` holds the count.

## Exit codes and usage errors

| Code | Meaning |
| --- | --- |
| 0 | OK |
| 1 | Budget (`--budget-month`) or threshold (`--max-new-share`) exceeded |
| 2 | Input error: log file, JSON, price file or rules file |
| 3 | Usage error, or the output could not be written |

If the output cannot be written, for example because the disk is full, the exit code is 3 and stderr is `could not write output`. `--help` and `--version` use that same code; they do not exit 0 when their text cannot be written.

A log file, price file or rules file must be a regular file. A FIFO, a directory or anything else is refused with exit 2 without waiting for a writer to open it. A `gb_bytes` that is a fraction such as `0.5`, 0 or negative, or a number outside the binary-float range, is exit 2; a price that is text instead of a number, a fraction that does not fit, or a price outside that range, gives `price file has a price that is not a number`. More than 128 nested arrays or objects gives `price file is nested too deeply`. The same key twice gives `price file has a duplicate key`. `sample 0,1` is not a valid fraction: exit 2 with `invalid rule on line 1 of the rules file`, without showing the line. The regular-file messages do not include the path, and no error message contains a raw log line.

Every row below was run against the binary, and a test keeps it that way. A row with exit 3 has empty stdout. The message is the complete stderr; the binary adds a newline. `\n` in the last column is a newline inside the message. `--json` does not change a message. For `diff`, `--explain` is checked before `--top`, `--window`, `--days-per-month` and a valid index option, so its message wins. An invalid amount, an invalid percentage, an invalid retention, billing plan or indexed price, and `--prices` without a window are checked before `--explain`. On `scan`, a valid index option without `--window` is rejected before the index-rules file is opened.

| Option | Applies to | Case | Wrong use | Exit | Message |
| --- | --- | --- | --- | --- | --- |
| `--format` | scan and diff | Invalid value. | `scan --format nope file.log` | 3 | `error: invalid value 'nope' for '--format <FORMAT>'\n  [possible values: text, jsonl]\n\nFor more information, try '--help'.\n` |
| `--message-field` | scan | Without jsonl. | `scan --message-field message file.log` | 3 | `--message-field only works with --format jsonl` |
| `--message-field` | diff | Without jsonl. | `diff --message-field message OLD NEW` | 3 | `--message-field only works with --format jsonl` |
| `--message-field` | scan | `--json` is not the input format jsonl. | `scan --json --message-field message file.log` | 3 | `--message-field only works with --format jsonl` |
| `--window` | diff | Without `--prices` and without `--json`. | `diff --window 1m OLD NEW` | 3 | `--window with diff only works together with --prices or --json` |
| `--window` | diff | `--explain` is checked first. | `diff --explain --window 1m OLD NEW` | 3 | `--explain only works with scan` |
| `--window` | scan | 0m is invalid. | `scan --window 0m file.log` | 3 | `invalid window` |
| `--window` | scan | 5x is invalid. | `scan --window 5x file.log` | 3 | `invalid window` |
| `--window` | scan | The unit is not one of `s`, `m`, `h`, `d`. | `scan --window 1é file.log` | 3 | `invalid window` |
| `--window` | diff | 0m is invalid, before the diff check. | `diff --window 0m OLD NEW` | 3 | `invalid window` |
| `--days-per-month` | scan | Without a window. | `scan --days-per-month 31 file.log` | 3 | `--days-per-month only works together with --window` |
| `--days-per-month` | diff | Without a window. | `diff --days-per-month 31 OLD NEW` | 3 | `--days-per-month only works together with --window` |
| `--days-per-month` | diff | `--json` does not change the message. | `diff --json --days-per-month 31 OLD NEW` | 3 | `--days-per-month only works together with --window` |
| `--days-per-month` | diff | `--explain` wins over this later check. | `diff --explain --days-per-month 31 OLD NEW` | 3 | `--explain only works with scan` |
| `--prices` | scan | Without a window. | `scan --prices no-prices.json file.log` | 3 | `--prices only works together with --window` |
| `--prices` | diff | Without a window. | `diff --prices no-prices.json OLD NEW` | 3 | `--prices only works together with --window` |
| `--prices` | scan | `--json` and `--explain` do not change the message. | `scan --json --explain --prices no-prices.json file.log` | 3 | `--prices only works together with --window` |
| `--prices` | diff | `--explain` does not win: prices are checked first. | `diff --explain --prices no-prices.json OLD NEW` | 3 | `--prices only works together with --window` |
| `--top` | diff | Not for diff. | `diff --top 1 OLD NEW` | 3 | `--top only works with scan` |
| `--top` | diff | `--json` does not change the message. | `diff --json --top 1 OLD NEW` | 3 | `--top only works with scan` |
| `--explain` | diff | Not for diff. | `diff --explain OLD NEW` | 3 | `--explain only works with scan` |
| `--explain` | diff | `--json` does not change the message. | `diff --json --explain OLD NEW` | 3 | `--explain only works with scan` |
| `--budget-month` | diff | Not for diff. | `diff --budget-month 1 OLD NEW` | 3 | `the monthly budget only applies to scan` |
| `--budget-month` | diff | `--json` and `--explain` do not change the message. | `diff --json --explain --budget-month 1 OLD NEW` | 3 | `the monthly budget only applies to scan` |
| `--budget-month` | scan | Without a window. | `scan --budget-month 1 file.log` | 3 | `budget requires a window` |
| `--budget-month` | scan | Window but no prices. | `scan --budget-month 1 --window 1m file.log` | 3 | `budget requires a price file` |
| `--budget-month` | scan | Prices but no window. The window is checked before `--prices`. | `scan --budget-month 1 --prices no-prices.json file.log` | 3 | `budget requires a window` |
| `--budget-month` | scan | 1,5 is not an amount. | `scan --budget-month 1,5 file.log` | 3 | `invalid budget` |
| `--budget-month` | diff | abc is invalid, before the command check. | `diff --budget-month abc OLD NEW` | 3 | `invalid budget` |
| `--max-new-share` | scan | Not for scan. | `scan --max-new-share 10 file.log` | 3 | `--max-new-share only works with diff` |
| `--max-new-share` | scan | `--json` and `--explain` do not change the message. | `scan --json --explain --max-new-share 10 file.log` | 3 | `--max-new-share only works with diff` |
| `--max-new-share` | scan | abc on scan gives the same message, not invalid percentage. | `scan --max-new-share abc file.log` | 3 | `--max-new-share only works with diff` |
| `--max-new-share` | diff | abc is not a percentage. | `diff --max-new-share abc OLD NEW` | 3 | `invalid percentage` |
| `--max-new-share` | diff | `--explain` does not win: the percentage is checked first. | `diff --explain --max-new-share abc OLD NEW` | 3 | `invalid percentage` |
| `--max-new-share` | diff | `--json` does not change the message. | `diff --json --max-new-share abc OLD NEW` | 3 | `invalid percentage` |
| `--retention` | scan | 5 is not 3, 7, 15 or 30. Checked before the window. | `scan --retention 5 file.log` | 3 | `invalid retention` |
| `--retention` | diff | 5 is invalid, before the command check. | `diff --retention 5 OLD NEW` | 3 | `invalid retention` |
| `--retention` | scan | Without a window. | `scan --retention 7 file.log` | 3 | `--retention only works together with --window` |
| `--retention` | diff | Not for diff. | `diff --retention 7 OLD NEW` | 3 | `--retention only works with scan` |
| `--retention` | diff | `--explain` wins over a valid retention. | `diff --explain --retention 7 OLD NEW` | 3 | `--explain only works with scan` |
| `--retention` | diff | `--window` without prices is checked first. | `diff --window 1m --retention 7 OLD NEW` | 3 | `--window with diff only works together with --prices or --json` |
| `--billing` | scan | yearly is not a plan. | `scan --billing yearly file.log` | 3 | `invalid billing` |
| `--billing` | scan | Without a window. | `scan --billing annual file.log` | 3 | `--billing only works together with --window` |
| `--billing` | diff | Not for diff. | `diff --billing monthly OLD NEW` | 3 | `--billing only works with scan` |
| `--indexed-price` | scan | nope is not a decimal. | `scan --indexed-price nope file.log` | 3 | `invalid indexed price` |
| `--indexed-price` | scan | A minus sign is a negative price. | `scan --indexed-price -1 --window 1s file.log` | 3 | `invalid indexed price` |
| `--indexed-price` | scan | Without a window. | `scan --indexed-price 1 file.log` | 3 | `--indexed-price only works together with --window` |
| `--indexed-price` | diff | Not for diff. | `diff --indexed-price 1 OLD NEW` | 3 | `--indexed-price only works with scan` |
| `--index-rules` | scan | Without a window. The file is not opened. | `scan --index-rules rules.txt file.log` | 3 | `--index-rules only works together with --window` |
| `--index-rules` | diff | Not for diff. | `diff --index-rules rules.txt OLD NEW` | 3 | `--index-rules only works with scan` |
| `--index-rules` | diff | `--explain` wins over the index-rules file. | `diff --explain --index-rules rules.txt OLD NEW` | 3 | `--explain only works with scan` |
| `--index-rules` | scan | The file cannot be read. The path is not shown. |  | 2 | `index rules file is not readable` |
| `--index-rules` | scan | Not a regular file. |  | 2 | `index rules file is not a regular file` |
| `--index-rules` | scan | A rule is invalid. The line is not shown. |  | 2 | `invalid rule on line 1 of the index rules file` |
| log file | scan and diff | Not a regular file (FIFO, directory). Refused without waiting. |  | 2 | `file is not a regular file` |
| log file | scan and diff | A JSON line longer than 1 MiB (a text line that long is cut at 1 MiB and still counts every byte). |  | 2 | `line N is longer than 1 MiB; JSON lines that long are not read` |
| log file | scan and diff | UTF-16 or UTF-32, with or without a byte order mark. The name is `UTF-16LE`, `UTF-16BE`, `UTF-32LE` or `UTF-32BE`. |  | 2 | `file is UTF-16LE, not UTF-8; convert it first, for example with iconv -f UTF-16LE -t UTF-8, or in PowerShell with Out-File -Encoding utf8` |
| `--prices` | scan and diff | Price file is not a regular file. |  | 2 | `price file is not a regular file` |
| `--rules` | scan and diff | Rules file is not a regular file. |  | 2 | `rules file is not a regular file` |
| `--prices` | scan and diff | `gb_bytes` is not a whole number greater than 0 (a fraction, 0, negative, or a number outside the binary-float range). |  | 2 | `gb_bytes must be a whole number greater than 0` |
| `--prices` | scan and diff | The same key appears twice. |  | 2 | `price file has a duplicate key` |
| `--prices` | scan and diff | A price fraction does not fit, or the number is outside the binary-float range. |  | 2 | `price file has a price that is not a number` |
| `--prices` | scan and diff | More than 128 nested arrays or objects. |  | 2 | `price file is nested too deeply` |

## Privacy and safety

- Fully offline: no network, no API key, no telemetry. Dependencies are limited to clap, regex and serde_json.
- Lookalike characters in a key name are recognised too, so a key that only looks like `password` or `secret` still hides the value.
- Patterns keep the static words of a line. Timestamps, UUIDs, IP addresses, e-mail addresses, secrets, hex, numbers and other variable words are replaced. A line with nothing to replace, such as `disk full`, is shown in full. Output also includes pattern ids, counts, byte sizes and shares, and with `--prices` the currency, source and date from the price file. An error about a log file, a price file or a rules file does not include the path, the raw log line, the rules line or the price-file contents.
- Tests plant a fake secret (a canary) in plain text, JSON fields, after `password=` and `Authorization: Bearer`, in a broken line and in a rules file, and check that no output (text, JSON, `--explain`, diff, stderr) contains it.
- A privacy test encodes `password=`, `token=` and `Authorization: Bearer` canaries as UTF-16LE, UTF-16BE, UTF-32LE and UTF-32BE, each with and without a byte order mark, and checks that no output contains them, also not after removing NUL bytes.
- The `inputs.paths` field of `--json` shows a file name that is not valid UTF-8 with U+FFFD for the invalid bytes.
- All fixtures are synthetic; see [tests/fixtures/README.md](../tests/fixtures/README.md).
