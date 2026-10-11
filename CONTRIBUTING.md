# Contributing

Thanks for your interest in log-bill.

## Building and testing

```sh
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

These checks must pass before a change is merged. Test fixtures must be synthetic: never add real logs or real secrets.

## Issues

Use the [issue forms](https://github.com/qwpp7g79y4-maker/log-bill/issues/new/choose) for bugs and feature requests. Remove secrets and personal data from any sample input before you paste it. Report security problems privately as described in [SECURITY.md](SECURITY.md).

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) in English, for example `fix: refuse a FIFO as price file` or `docs: clarify the sample rounding`.

## Contact

PEPAI, pep_ai@icloud.com
