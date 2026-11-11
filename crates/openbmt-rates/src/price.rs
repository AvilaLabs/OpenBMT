//! The text of a price into minor units per bitcoin (RT-20 to RT-24).

use crate::error::PriceRefused;

/// RT-21: the most digits after the dot.
pub const MAX_DECIMALS: usize = 8;
/// RT-24: the lowest price accepted, in cents per bitcoin (100 USD). **[to
/// tune]**
pub const MIN_CENTS_PER_BTC: u64 = 100 * 100;
/// RT-24: the highest price accepted, in cents per bitcoin (100,000,000
/// USD). **[to tune]**
pub const MAX_CENTS_PER_BTC: u64 = 100_000_000 * 100;

/// A price as the text of a source gives it, in cents per bitcoin: digits,
/// at most one dot with a digit on each side, no sign, no exponent, no
/// leading zero but a lone one, at most 8 digits after the dot. The amount
/// is worked out exactly in integers and rounded half up to a whole cent
/// (RT-22). Zero and a price outside the band are refused.
pub fn parse_price_cents(text: &str) -> Result<u64, PriceRefused> {
    let bytes = text.as_bytes();
    let first = *bytes.first().ok_or(PriceRefused::Empty)?;
    if matches!(first, b'+' | b'-') {
        return Err(PriceRefused::Sign);
    }
    if bytes.iter().any(|b| matches!(b, b'e' | b'E')) {
        return Err(PriceRefused::Exponent);
    }
    if bytes.iter().any(|b| !b.is_ascii_digit() && *b != b'.') {
        return Err(PriceRefused::BadCharacter);
    }
    if bytes.iter().filter(|b| **b == b'.').count() > 1 {
        return Err(PriceRefused::TwoDots);
    }
    let (whole, fraction) = match text.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (text, None),
    };
    if whole.is_empty() || fraction.is_some_and(str::is_empty) {
        return Err(PriceRefused::BadShape);
    }
    if whole.len() > 1 && whole.starts_with('0') {
        return Err(PriceRefused::LeadingZero);
    }
    let fraction = fraction.unwrap_or("");
    if fraction.len() > MAX_DECIMALS {
        return Err(PriceRefused::TooManyDecimals);
    }
    // In units of 10^-8 of a dollar; every step is checked.
    let mut units: u128 = 0;
    for b in whole.bytes() {
        units = units
            .checked_mul(10)
            .and_then(|u| u.checked_add(u128::from(b - b'0')))
            .ok_or(PriceRefused::OutOfBand)?;
        if units > u128::from(MAX_CENTS_PER_BTC) {
            return Err(PriceRefused::OutOfBand);
        }
    }
    units *= 100_000_000;
    let mut scale: u128 = 10_000_000;
    for b in fraction.bytes() {
        units += u128::from(b - b'0') * scale;
        scale /= 10;
    }
    // RT-22: half up, to cents (10^6 units).
    let cents =
        u64::try_from((units + 500_000) / 1_000_000).map_err(|_| PriceRefused::OutOfBand)?;
    if cents == 0 {
        return Err(PriceRefused::Zero);
    }
    if !(MIN_CENTS_PER_BTC..=MAX_CENTS_PER_BTC).contains(&cents) {
        return Err(PriceRefused::OutOfBand);
    }
    Ok(cents)
}
