#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use log_bill::money::{self, Price, Prices};

const UNITS: &[u8] = b"smhd";

#[derive(Debug, Arbitrary)]
struct Case {
    prices_src: Vec<u8>,
    window_digits: Vec<u8>,
    window_unit: u8,
    sample_bytes: u64,
    days_per_month: u32,
    numer: u128,
    denom: u128,
    gb_bytes: u128,
    budget_src: Vec<u8>,
}

// Money and price parsing: a price file, window, and budget of arbitrary
// text never panic to parse, and every amount the calculator produces is a
// non-negative decimal, never NaN, even for adversarial price fractions.
fuzz_target!(|data: &[u8]| {
    let _ = money::parse_prices(data);

    let Ok(case) = Case::arbitrary_take_rest(Unstructured::new(data)) else {
        return;
    };
    let _ = money::parse_prices(&case.prices_src);

    let digits: String = case
        .window_digits
        .iter()
        .filter(|byte| byte.is_ascii_digit())
        .take(6)
        .map(|&byte| byte as char)
        .collect();
    let unit = UNITS[case.window_unit as usize % UNITS.len()] as char;
    let window_text = if digits.is_empty() {
        format!("1{unit}")
    } else {
        format!("{digits}{unit}")
    };
    let Ok(window) = money::parse_window(&window_text) else {
        return;
    };

    let gb_bytes = case.gb_bytes.max(1);
    let _ = money::extrapolate(case.sample_bytes, window, case.days_per_month, gb_bytes);

    let price = Price {
        numer: case.numer,
        denom: case.denom.max(1),
    };
    assert_amount_sane(money::cost(
        case.sample_bytes,
        window,
        case.days_per_month,
        price,
        gb_bytes,
    ));
    if let Ok(steps) =
        money::explain_steps(case.sample_bytes, window, case.days_per_month, price, gb_bytes)
    {
        assert!(
            !steps.to_ascii_lowercase().contains("nan"),
            "NaN in explain_steps: {steps}"
        );
    }

    let budget_text: String = case
        .budget_src
        .iter()
        .filter(|byte| byte.is_ascii_digit() || **byte == b'.')
        .take(20)
        .map(|&byte| byte as char)
        .collect();
    let budget = money::parse_budget(&budget_text)
        .unwrap_or_else(|| money::parse_budget("0").expect("zero budget parses"));
    let prices = Prices {
        currency: "USD".to_owned(),
        source: "fuzz".to_owned(),
        date: "2026-01-01".to_owned(),
        ingest_per_gb: price,
        index_per_gb: Some(Price {
            numer: case.numer,
            denom: case.denom.max(1),
        }),
        gb_bytes,
    };
    let _ = money::month_above_budget(case.sample_bytes, window, case.days_per_month, &prices, budget);
});

fn assert_amount_sane(cost: Result<money::Cost, money::CalcError>) {
    let Ok(cost) = cost else {
        return;
    };
    for text in [cost.per_day.to_string(), cost.per_month.to_string()] {
        assert!(!text.contains('-'), "negative amount: {text}");
        assert!(
            !text.to_ascii_lowercase().contains("nan"),
            "NaN amount: {text}"
        );
        assert!(text.contains('.'), "amount missing a decimal point: {text}");
    }
}
