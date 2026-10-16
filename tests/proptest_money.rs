//! Property tests for money: parsers never panic, and the rounded cost
//! matches a plain (non-bignum) reference calculation.

use log_bill::money::{self, Price, Window};
use proptest::prelude::*;

/// `numer / denom`, rounded half away from zero, in plain `u128` arithmetic.
/// A reference that does not share any code with `money`'s hand-rolled
/// 512-bit arithmetic, so the two can be compared.
fn round_half_away(numer: u128, denom: u128) -> u128 {
    let mut whole = numer / denom;
    if (numer % denom) * 2 >= denom {
        whole += 1;
    }
    whole
}

/// Reference cost in cents, for `day_scale` days (1 for a day, `days_per_month`
/// for a month): `sample_bytes * 86400 * day_scale * price / window_seconds / gb_bytes`,
/// in cents.
fn reference_cents(
    sample_bytes: u64,
    window_seconds: u64,
    day_scale: u64,
    price: Price,
    gb_bytes: u128,
) -> u128 {
    let numer = u128::from(sample_bytes)
        .checked_mul(86_400)
        .and_then(|value| value.checked_mul(u128::from(day_scale)))
        .and_then(|value| value.checked_mul(price.numer))
        .and_then(|value| value.checked_mul(100))
        .expect("bounded by the proptest ranges below");
    let denom = u128::from(window_seconds)
        .checked_mul(gb_bytes)
        .and_then(|value| value.checked_mul(price.denom))
        .expect("bounded by the proptest ranges below");
    round_half_away(numer, denom)
}

/// `"12.34"` to `1234` cents. `money::Amount` always shows exactly 2 decimals.
fn parse_cents(text: &str) -> u128 {
    let (whole, frac) = text.split_once('.').expect("amount has a decimal point");
    assert_eq!(frac.len(), 2, "amount does not have 2 decimals: {text}");
    let whole: u128 = whole.parse().expect("whole part is digits");
    let frac: u128 = frac.parse().expect("fraction part is digits");
    whole * 100 + frac
}

fn window_of(seconds: u64) -> Window {
    money::parse_window(&format!("{seconds}s")).expect("1..=100_000s is a valid window")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `money::cost` matches the plain reference, for both the daily and the
    /// monthly amount, over a range too small for either side to overflow.
    #[test]
    fn cost_matches_reference(
        sample_bytes in 0u64..=1_000_000,
        window_seconds in 1u64..=100_000,
        days_per_month in 1u32..=31,
        price_numer in 1u128..=1_000,
        price_denom in 1u128..=1_000,
        gb_bytes in 1u128..=2_000_000_000,
    ) {
        let window = window_of(window_seconds);
        let price = Price { numer: price_numer, denom: price_denom };
        let cost = money::cost(sample_bytes, window, days_per_month, price, gb_bytes)
            .expect("bounded inputs do not overflow");

        let day_cents = parse_cents(&cost.per_day.to_string());
        let month_cents = parse_cents(&cost.per_month.to_string());
        let expected_day = reference_cents(sample_bytes, window_seconds, 1, price, gb_bytes);
        let expected_month =
            reference_cents(sample_bytes, window_seconds, u64::from(days_per_month), price, gb_bytes);

        prop_assert_eq!(day_cents, expected_day);
        prop_assert_eq!(month_cents, expected_month);
    }

    /// `money::extrapolate` never panics on bounded, non-overflowing input.
    #[test]
    fn extrapolate_never_panics(
        sample_bytes in 0u64..=u64::MAX,
        window_seconds in 1u64..=100_000,
        days_per_month in 0u32..=1000,
        gb_bytes in 1u128..=u128::MAX,
    ) {
        let window = window_of(window_seconds);
        let _ = money::extrapolate(sample_bytes, window, days_per_month, gb_bytes);
    }

    /// The price-file parser never panics on arbitrary bytes.
    #[test]
    fn parse_prices_never_panics(data in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = money::parse_prices(&data);
    }

    /// The window parser never panics on arbitrary text.
    #[test]
    fn parse_window_never_panics(text in ".{0,64}") {
        let _ = money::parse_window(&text);
    }

    /// The budget parser never panics on arbitrary text.
    #[test]
    fn parse_budget_never_panics(text in ".{0,100}") {
        let _ = money::parse_budget(&text);
    }
}
