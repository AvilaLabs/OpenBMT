//! A deposit address from the text a camera read: a bare address or a
//! BIP21 `bitcoin:` URI (`docs/SIGNER-RULES.md` SR-30).
//!
//! The parser is small and strict, and it only reads: it enrols nothing.
//! The app shows the result and then calls `enroll_destination`, which
//! applies the signer's own rules again.

use std::str::FromStr;

use bitcoin::{Address, Network};

use crate::codec::wire_record;
use crate::error::{refused_by, KeyError};
use crate::keys::KeyNetwork;

/// The longest text that is read, in bytes.
const MAX_BYTES: usize = 2_048;

/// The most that exists, in satoshis.
const MAX_SATS: u64 = 21_000_000 * 100_000_000;

/// What a scanned deposit code holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositCode {
    /// The address, for the wallet's network, in its usual spelling.
    pub address: String,
    /// The amount asked for, in satoshis, if the code has one.
    pub amount_sats: Option<u64>,
    /// The label the code carries, if any. It is not the label the signer
    /// stores: the person types that on this device (SR-30).
    pub label: Option<String>,
    /// The names of the parameters that were read and not acted on, such
    /// as `lightning` or `pj`, in the order they came. The `message`
    /// parameter is read and dropped, and is not listed.
    pub ignored: Vec<String>,
}

wire_record!(DepositCode {
    address,
    amount_sats,
    label,
    ignored,
});

fn bad(rule: &str, text: &str) -> KeyError {
    refused_by(rule, text)
}

/// Read `text` as a bare address or a BIP21 URI for `network`.
///
/// - The scheme `bitcoin:` is accepted in any case; parameter names are
///   compared in ASCII lower case.
/// - `amount` is a decimal number of bitcoin with at most 8 places, no
///   sign and no exponent, from 1 satoshi to 21,000,000 bitcoin.
/// - `label` and `message` are percent-decoded; the label may not hold a
///   control character.
/// - A parameter that starts with `req-` is not understood by this
///   parser, so the whole code is refused, as BIP21 says. Any other
///   unknown parameter is listed by name in `ignored` and never acted on.
/// - A parameter given twice, a bad percent escape, a space or other
///   character that should have been escaped, and a text over 2,048 bytes
///   are refused.
/// - The address must be for `network` and of a kind the signer's
///   allowlist takes (SR-22, SR-30): P2WPKH, P2WSH, P2TR, P2PKH or P2SH.
pub fn parse_deposit_code(text: String, network: KeyNetwork) -> Result<DepositCode, KeyError> {
    if text.len() > MAX_BYTES {
        return Err(bad(
            "deposit-too-long",
            "the scanned text is too long for a payment code",
        ));
    }
    let text = text.trim_ascii();
    if !text.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(bad(
            "deposit-chars",
            "the code holds a space, a control or a non-ASCII character that a payment code \
             does not carry unescaped",
        ));
    }
    let has_scheme = text
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("bitcoin:"));
    let (address, query) = match text.get(8..).filter(|_| has_scheme) {
        None => (text, None),
        Some(rest) => match rest.split_once('?') {
            Some((a, q)) => (a, Some(q)),
            None => (rest, None),
        },
    };
    let address = address_for(address, network.into())?;
    let mut code = DepositCode {
        address,
        amount_sats: None,
        label: None,
        ignored: Vec::new(),
    };
    if let Some(query) = query {
        read_query(query, &mut code)?;
    }
    Ok(code)
}

/// The address, for `network`, of a kind the allowlist takes.
fn address_for(text: &str, network: Network) -> Result<String, KeyError> {
    let parsed = Address::from_str(text)
        .map_err(|e| refused_by("address", format!("not an address: {e}")))?;
    let checked = parsed.require_network(network).map_err(|e| {
        refused_by(
            "address",
            format!("the address is for another network: {e}"),
        )
    })?;
    // The kinds of SR-22, which SR-30 takes for the allowlist.
    let script = checked.script_pubkey();
    let kind_ok = script.is_p2wpkh()
        || script.is_p2wsh()
        || script.is_p2tr()
        || script.is_p2pkh()
        || script.is_p2sh();
    if !kind_ok {
        return Err(refused_by(
            "address-kind",
            "SR-30: the allowlist takes only P2WPKH, P2WSH, P2TR, P2PKH and P2SH addresses",
        ));
    }
    Ok(checked.to_string())
}

fn read_query(query: &str, code: &mut DepositCode) -> Result<(), KeyError> {
    if query.is_empty() {
        return Err(bad(
            "deposit-syntax",
            "the code has a '?' and no parameters",
        ));
    }
    let mut seen: Vec<String> = Vec::new();
    for part in query.split('&') {
        let (raw_name, raw_value) = part
            .split_once('=')
            .ok_or_else(|| bad("deposit-syntax", "a parameter has no '='"))?;
        let name = percent_decode(raw_name)?.to_ascii_lowercase();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(bad(
                "deposit-syntax",
                "a parameter has an empty or unreadable name",
            ));
        }
        let value = percent_decode(raw_value)?;
        if seen.contains(&name) {
            return Err(bad(
                "deposit-duplicate",
                "a parameter is given more than once",
            ));
        }
        seen.push(name.clone());
        if name.starts_with("req-") {
            return Err(bad(
                "deposit-required",
                "the code requires something this app does not understand, so it is refused",
            ));
        }
        match name.as_str() {
            "amount" => code.amount_sats = Some(amount_sats(&value)?),
            "label" => {
                if value.chars().any(char::is_control) {
                    return Err(bad("deposit-label", "the label holds a control character"));
                }
                code.label = Some(value);
            }
            "message" => {}
            _ => code.ignored.push(name),
        }
    }
    Ok(())
}

/// A decimal number of bitcoin as satoshis, in integer arithmetic.
fn amount_sats(text: &str) -> Result<u64, KeyError> {
    let wrong = || {
        bad(
            "deposit-amount",
            "the amount is not a number of bitcoin with at most 8 decimal places, \
             between 1 satoshi and 21,000,000 bitcoin",
        )
    };
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) if !f.is_empty() => (w, f),
        Some(_) => return Err(wrong()),
        None => (text, ""),
    };
    let digits = |s: &str, most: usize| s.len() <= most && s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole, 8) || !digits(frac, 8) {
        return Err(wrong());
    }
    let whole: u64 = whole.parse().map_err(|_| wrong())?;
    let frac: u64 = format!("{frac:0<8}").parse().map_err(|_| wrong())?;
    let sats = whole
        .checked_mul(100_000_000)
        .and_then(|w| w.checked_add(frac))
        .filter(|s| (1..=MAX_SATS).contains(s))
        .ok_or_else(wrong)?;
    Ok(sats)
}

/// RFC 3986 percent-decoding to UTF-8. A `+` stays a plus.
fn percent_decode(text: &str) -> Result<String, KeyError> {
    let wrong = || bad("deposit-escape", "the code has a bad percent escape");
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(b) = bytes.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }
        let hi = bytes.next().and_then(hex_value).ok_or_else(wrong)?;
        let lo = bytes.next().and_then(hex_value).ok_or_else(wrong)?;
        out.push(hi * 16 + lo);
    }
    String::from_utf8(out).map_err(|_| wrong())
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
