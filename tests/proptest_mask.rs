//! Property tests for masking. Modest case counts so these run in a normal
//! `cargo test`, not only under `cargo fuzz`.

use log_bill::mask::mask;
use proptest::prelude::*;

const KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "authorization",
    "bearer",
];

/// Separators `mask` itself recognises: plain, spaced, and the Unicode
/// whitespace it special-cases around `=` or `:` (tab, LF, CR, NBSP,
/// ideographic space, NEL). Fullwidth `＝`/`：` are folded to ASCII by
/// `read::without_nul` before a real line reaches the masker, so they are
/// not part of this function's own contract.
const SEPS: &[&str] = &[
    "=",
    ":",
    " = ",
    " : ",
    "\t=",
    "\n=",
    "\r:",
    "\u{00A0}=\u{00A0}",
    "\u{3000}=",
    "\u{0085}=",
];

const TAGS: &[&str] = &["TS", "UUID", "IP", "EMAIL", "SECRET", "HEX", "NUM", "VAR"];

/// Tokens with no internal spaces, chosen so that joining several with a
/// single space and masking each independently gives a result that does not
/// depend on the order: none of them glue onto a neighbour the way a key
/// glues onto a following IP or e-mail.
const INDEPENDENT_TOKENS: &[&str] = &[
    "404",
    "3",
    "deadbeef01",
    "192.0.2.10",
    "2001:db8::1",
    "550e8400-e29b-41d4-a716-446655440000",
    "pod-7",
    "verbinding",
    "wacht",
    "ok",
];

fn secret_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9]{6,40}".prop_filter("not a mask tag", |text| {
        !TAGS.contains(&text.to_ascii_uppercase().as_str())
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Masking twice gives the same result as masking once: the tags and the
    /// collapsed spaces it introduces are not themselves further masked.
    #[test]
    fn mask_is_idempotent(text in ".{0,200}") {
        let once = mask(&text);
        let twice = mask(&once);
        prop_assert_eq!(once, twice);
    }

    /// Masking a line of independent, space-separated tokens does not depend
    /// on the order those tokens appear in: the same multiset of masked
    /// tokens comes out either way.
    #[test]
    fn mask_is_order_independent(
        indices in prop::collection::vec(0..INDEPENDENT_TOKENS.len(), 1..=6)
    ) {
        let forward: Vec<&str> = indices.iter().map(|&i| INDEPENDENT_TOKENS[i]).collect();
        let mut backward = forward.clone();
        backward.reverse();

        let mask_sorted = |tokens: &[&str]| -> Vec<String> {
            let line = tokens.join(" ");
            let masked = mask(&line);
            let mut parts: Vec<String> = masked.split(' ').map(str::to_owned).collect();
            parts.sort();
            parts
        };

        prop_assert_eq!(mask_sorted(&forward), mask_sorted(&backward));
    }

    /// A secret value placed after a recognised key and any separator the
    /// masker understands never appears in the masked output, whatever
    /// whitespace variant introduces it.
    #[test]
    fn secret_after_key_never_leaks(
        key_index in 0..KEYS.len(),
        sep_index in 0..SEPS.len(),
        secret in secret_strategy(),
    ) {
        let key = KEYS[key_index];
        let sep = SEPS[sep_index];
        let line = format!("{key}{sep}{secret}");
        let masked = mask(&line);
        prop_assert!(
            !masked.contains(&secret),
            "secret leaked: input={line:?} output={masked:?}"
        );
    }

    /// Same as above, but the key carries a `\uXXXX` escape on its first
    /// letter, as a JSON key might after `decode_key_escapes` reads it back.
    #[test]
    fn secret_after_escaped_key_never_leaks(
        key_index in 0..KEYS.len(),
        sep_index in 0..SEPS.len(),
        secret in secret_strategy(),
    ) {
        let key = KEYS[key_index];
        let mut chars = key.chars();
        let first = chars.next().expect("key is not empty");
        let escaped_key = format!("\\u{:04x}{}", first as u32, chars.as_str());
        let sep = SEPS[sep_index];
        let line = format!("{escaped_key}{sep}{secret}");
        let masked = mask(&line);
        prop_assert!(
            !masked.contains(&secret),
            "secret leaked: input={line:?} output={masked:?}"
        );
    }
}
