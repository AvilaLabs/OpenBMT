//! Amounts as decimals with the currency's minor digits (BK-22).

use openbmt_counter::Currency;

/// Currencies with no minor unit.
const ZERO_DIGITS: [&str; 16] = [
    "BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX", "UYI", "VND",
    "VUV", "XAF", "XOF",
];
/// Currencies with three minor digits.
const THREE_DIGITS: [&str; 7] = ["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"];
/// Currencies with four minor digits.
const FOUR_DIGITS: [&str; 2] = ["CLF", "UYW"];

/// BK-22: how many digits follow the point: two unless the currency is
/// listed. `XPF` has none as well.
pub fn minor_digits(currency: Currency) -> u32 {
    let code = currency.to_string();
    let is = |list: &[&str]| list.contains(&code.as_str());
    if is(&ZERO_DIGITS) || code == "XPF" {
        0
    } else if is(&THREE_DIGITS) {
        3
    } else if is(&FOUR_DIGITS) {
        4
    } else {
        2
    }
}

/// BK-22: minor units as a decimal, no separators: `1850` dollars-cents is
/// `18.50`.
pub fn format_minor(minor: u64, currency: Currency) -> String {
    let digits = minor_digits(currency);
    if digits == 0 {
        return minor.to_string();
    }
    let scale = 10u64.pow(digits);
    format!(
        "{}.{:0width$}",
        minor / scale,
        minor % scale,
        width = usize::try_from(digits).unwrap_or(2)
    )
}

/// BK-21, BK-22: a signed amount; a negative one has a leading `-`.
pub fn format_signed(negative: bool, minor: u64, currency: Currency) -> String {
    let body = format_minor(minor, currency);
    if negative && minor > 0 {
        format!("-{body}")
    } else {
        body
    }
}
