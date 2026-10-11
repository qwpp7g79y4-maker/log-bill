# Fixtures

Every file in this directory is synthetic: written by hand for a test, not taken from a real system. None of them contains a real secret. The canary is a fixed fake string used by the leak tests.

## canary-authorization.jsonl

Synthetic JSON lines file. The message is an Authorization or Proxy-Authorization header, or a JSON key of that name. The canary is a word of letters only, without a digit, or a base64 word without a digit.

## canary-authorization.log

Synthetic text file. Authorization and Proxy-Authorization, also with a prefix, with Basic, Bearer, Digest, NTLM, Negotiate and an unknown scheme. The canary is a word of letters only, without a digit, or a base64 word without a digit.

## canary-broken.jsonl

Synthetic JSON lines file. The first line is valid JSON, the second is not. The broken line contains the canary.

## canary-jsonl.jsonl

Synthetic JSON lines file. The canary is in `message` and in other fields, also after a key. One line has no message field.

## canary-keys.log

Synthetic text file. The canary is a word of letters only, without a digit, after keys with a prefix or suffix (`db_password`, `SECRET_KEY`, `x_token`, `my_secret_value`, `x-api-key` and upper-case variants).

## canary-nested.jsonl

Synthetic JSON lines file: a nested object and a bare array, with the canary in fields.

## canary-new.log

Synthetic text file for a diff. The canary appears after keys and as a standalone word.

## canary-rules-invalid.txt

Synthetic rules file. Rule 1 is a drop, rule 2 an unknown command that contains the canary.

## canary-rules.txt

Synthetic rules file with a drop and a sample. The canary is in the commands. The file starts with a BOM.

## canary-text.log

Synthetic text file. The canary appears on its own and after keys.

## diff-new.log

Synthetic text file: the lines `alpha` and `gamma`.

## diff-old.log

Synthetic text file: two lines `alpha` and one line `beta`.

## prices.json

Synthetic price file with made-up amounts and a `gb_bytes` that is not 1000000000. Not an official rate.

## sample-0.1.txt

Synthetic rules file with one command: `sample 0.1` and a regex on the letter x. The fraction uses a dot.

## sample-zero.log

Synthetic text file. One line of nine letters x plus a newline, 10 bytes in total.

## scan-authorization.jsonl

Synthetic JSON lines file. The message is an Authorization header, a Proxy-Authorization header or a JSON key of that name, with a value of letters only or base64. No real secret.

## scan-authorization.log

Synthetic text file. Authorization and Proxy-Authorization, also with a prefix and as a JSON key, with Basic, Bearer, Digest, NTLM, Negotiate and an unknown scheme. The values are letters only or base64. No real secret.

## scan-ipv6.log

Synthetic text file. Four lines with an IPv6 address: in brackets with a port, IPv4-mapped, and with a zone index. Not a real address of any system.

## scan-jsonl.jsonl

Synthetic JSON lines file with `message`, `msg`, `log` and a line without a message field.

## scan-keys.log

Synthetic text file. Keys with a prefix or suffix (`db_password`, `x-api-key` and upper-case variants) have a value of letters only. One line is JSON-like. One line has an ISO 8601 timestamp with a comma fraction. No real secret.

## scan-text.log

Synthetic text file: two lines with a user, a timestamp and a test address, an empty line, and the line `disk full`.
