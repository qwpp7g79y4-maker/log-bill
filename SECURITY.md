# Security policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |

## Reporting a vulnerability

Please report security problems privately by e-mail to pep_ai@icloud.com, not in a public issue. Examples are a secret, token or other sensitive value that log-bill does not mask in its output, a raw log line that ends up in an error message, or a way to make log-bill read, write or hang on something it should refuse.

Please include:

- the log-bill version (`log-bill --version`) and your operating system;
- the exact command;
- a minimal input that reproduces the problem, with made-up values instead of real secrets;
- what log-bill printed and what you expected.

We aim to acknowledge a report and follow up within a reasonable time. Once a fix is released, the report can be credited in the changelog if you wish.
