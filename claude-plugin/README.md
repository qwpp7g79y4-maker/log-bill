# Log Bill

This plugin helps Claude use the `log-bill` command-line tool. The tool groups a local log file into patterns and can extrapolate volume and cost for a day and a month. It works offline, before you send the logs to a vendor.

## Use it

Install the tool with Rust 1.85 or newer: `cargo install --locked log-bill`. Give Claude a log sample and, if you want a cost, a price file and a `--window` that matches how long that sample covers. You can also compare two samples with `diff`, or simulate drop and sample rules.

## Data

The tool is fully offline: no network, no API key, and no telemetry. It reads the log file and optional price or rules files you pass. Patterns replace timestamps, IP addresses, e-mail addresses, secrets, and other variable parts. A line with nothing to replace is shown in full. Error messages do not include raw log lines or secrets. This plugin stores nothing and sends nothing to any service. A cost figure is an extrapolation of one sample with your own prices, not an official rate.
