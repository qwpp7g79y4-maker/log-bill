# Security policy

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |

Security fixes go into the latest release only.

## Reporting a vulnerability

Please report security problems privately, not in a public issue: open the **Security** tab of this repository and choose **Report a vulnerability**. If that is not possible, e-mail pep_ai@icloud.com. Examples are a secret, token or other sensitive value that log-bill does not mask in its output, a raw log line that ends up in an error message, or a way to make log-bill read, write or hang on something it should refuse.

Please include:

- the log-bill version (`log-bill --version`) and your operating system;
- the exact command;
- a minimal input that reproduces the problem, with made-up values instead of real secrets;
- what log-bill printed and what you expected.

We aim to respond within 7 days (best effort). Once a fix is released, the report can be credited in the changelog if you wish.
