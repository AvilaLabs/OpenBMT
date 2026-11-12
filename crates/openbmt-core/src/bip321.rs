//! Strict reader for the on-chain and payment-instruction subset of BIP 321.
//!
//! This reader accepts mainnet addresses and preserves payment instructions
//! as URI-encoded text for the application that understands them. It does not
//! decode Lightning or other payment protocol payloads.

use std::str::FromStr;

use bitcoin::address::NetworkChecked;
use bitcoin::{Address, AddressType, Amount, Network};

/// Maximum encoded URI length accepted by [`PaymentRequest::parse`].
pub const MAX_URI_BYTES: usize = 2048;
const MAX_TEXT_BYTES: usize = 512;
const MAX_AMOUNT_SATS: u64 = 2_100_000_000_000_000;

/// A recognized payment instruction, retained exactly as it appeared after `=`.
///
/// The key is lower-case ASCII. `value` keeps percent escapes and `+` exactly
/// as supplied; callers should pass it to a protocol-specific decoder only
/// after choosing that payment method. `required` records a `req-` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentInstruction {
    pub key: PaymentMethod,
    pub value: String,
    pub required: bool,
}

/// Payment methods whose URI keys this module recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMethod {
    Lightning,
    LightningOffer,
    SilentPayment,
}

impl PaymentMethod {
    fn parse(key: &str) -> Option<Self> {
        match key {
            "lightning" => Some(Self::Lightning),
            "lno" => Some(Self::LightningOffer),
            "sp" => Some(Self::SilentPayment),
            _ => None,
        }
    }
}

/// One optional, unrecognized query pair that a caller may safely ignore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalParameter {
    pub key: String,
    pub value: String,
}

/// A standard field explicitly marked mandatory with a `req-` prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredField {
    Amount,
    Label,
    Message,
}

/// A validated Bitcoin payment request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRequest {
    /// Mainnet only. `None` is possible only when a nonempty payment
    /// instruction is present.
    pub address: Option<Address<NetworkChecked>>,
    /// Integer satoshis; no floating point conversion is used.
    pub amount: Option<Amount>,
    /// Percent-decoded UTF-8, with control characters refused.
    pub label: Option<String>,
    /// Percent-decoded UTF-8, with control characters refused.
    pub message: Option<String>,
    /// Raw, URI-encoded values. Required methods retain their required flag.
    pub payment_instructions: Vec<PaymentInstruction>,
    /// Standard non-payment fields explicitly marked with a `req-` prefix.
    pub required_fields: Vec<RequiredField>,
    /// Unknown non-required keys, retained in lower-case form.
    pub optional_parameters: Vec<OptionalParameter>,
}

/// Why a Bitcoin payment URI was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    TooLong,
    BadScheme,
    BadUriShape,
    InvalidAddress,
    TestnetAddress,
    MissingPaymentTarget,
    EmptyPaymentInstruction,
    InvalidQuery,
    DuplicateKey,
    UnsupportedRequiredParameter,
    InvalidAmount,
    AmountTooLarge,
    InvalidText,
    TextTooLong,
}

impl PaymentRequest {
    /// Parse a strict-profile BIP 321 URI for a mainnet payment.
    pub fn parse(uri: &str) -> Result<Self, Refusal> {
        if uri.len() > MAX_URI_BYTES {
            return Err(Refusal::TooLong);
        }
        if !uri.is_ascii() || uri.bytes().any(|b| b.is_ascii_control() || b == b' ') {
            return Err(Refusal::BadUriShape);
        }
        let (scheme, rest) = uri.split_once(':').ok_or(Refusal::BadScheme)?;
        if !scheme.eq_ignore_ascii_case("bitcoin") {
            return Err(Refusal::BadScheme);
        }
        if rest.contains('#') || rest.starts_with("//") {
            return Err(Refusal::BadUriShape);
        }
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };
        if path.contains('%') || path.contains('/') || path.contains('?') {
            return Err(Refusal::BadUriShape);
        }

        let address = if path.is_empty() {
            None
        } else {
            let parsed = Address::from_str(path).map_err(|_| Refusal::InvalidAddress)?;
            match parsed.require_network(Network::Bitcoin) {
                Ok(checked) => {
                    if !matches!(
                        checked.address_type(),
                        Some(
                            AddressType::P2pkh
                                | AddressType::P2sh
                                | AddressType::P2wpkh
                                | AddressType::P2wsh
                                | AddressType::P2tr
                        )
                    ) {
                        return Err(Refusal::InvalidAddress);
                    }
                    Some(checked)
                }
                Err(_) => {
                    // A syntactically valid address for a different network is
                    // reported distinctly to make the mainnet policy explicit.
                    let parsed = Address::from_str(path).map_err(|_| Refusal::InvalidAddress)?;
                    if parsed.is_valid_for_network(Network::Testnet)
                        || parsed.is_valid_for_network(Network::Signet)
                        || parsed.is_valid_for_network(Network::Regtest)
                    {
                        return Err(Refusal::TestnetAddress);
                    }
                    return Err(Refusal::InvalidAddress);
                }
            }
        };

        let mut seen = std::collections::HashSet::new();
        let mut amount = None;
        let mut label = None;
        let mut message = None;
        let mut payment_instructions = Vec::new();
        let mut required_fields = Vec::new();
        let mut optional_parameters = Vec::new();

        if let Some(query) = query {
            if query.is_empty() {
                return Err(Refusal::InvalidQuery);
            }
            for pair in query.split('&') {
                if pair.is_empty() {
                    return Err(Refusal::InvalidQuery);
                }
                let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
                if raw_key.is_empty() || raw_key.contains('=') || raw_value.contains('=') {
                    return Err(Refusal::InvalidQuery);
                }
                validate_percent_escapes(raw_key)?;
                validate_percent_escapes(raw_value)?;
                let decoded_key = percent_decode(raw_key).map_err(|_| Refusal::InvalidQuery)?;
                if decoded_key.iter().any(|byte| byte.is_ascii_control()) {
                    return Err(Refusal::InvalidQuery);
                }
                let key = std::str::from_utf8(&decoded_key)
                    .map_err(|_| Refusal::InvalidQuery)?
                    .to_ascii_lowercase();
                if key.is_empty() || !key.is_ascii() {
                    return Err(Refusal::InvalidQuery);
                }
                let canonical_key = key.strip_prefix("req-").unwrap_or(&key).to_owned();
                if !seen.insert(canonical_key) {
                    return Err(Refusal::DuplicateKey);
                }
                if key == "tb" {
                    return Err(Refusal::TestnetAddress);
                }

                let (required, base_key) = key
                    .strip_prefix("req-")
                    .map_or((false, key.as_str()), |base| (true, base));
                match base_key {
                    "amount" => {
                        amount = Some(parse_amount(raw_value)?);
                        if required {
                            required_fields.push(RequiredField::Amount);
                        }
                    }
                    "label" => {
                        label = Some(parse_text(raw_value)?);
                        if required {
                            required_fields.push(RequiredField::Label);
                        }
                    }
                    "message" => {
                        message = Some(parse_text(raw_value)?);
                        if required {
                            required_fields.push(RequiredField::Message);
                        }
                    }
                    "lightning" | "lno" | "sp" => {
                        if raw_value.is_empty() {
                            return Err(Refusal::EmptyPaymentInstruction);
                        }
                        payment_instructions.push(PaymentInstruction {
                            key: PaymentMethod::parse(base_key).ok_or(Refusal::InvalidQuery)?,
                            value: raw_value.to_owned(),
                            required,
                        });
                    }
                    _ if required => return Err(Refusal::UnsupportedRequiredParameter),
                    _ => optional_parameters.push(OptionalParameter {
                        key,
                        value: raw_value.to_owned(),
                    }),
                }
            }
        }

        if address.is_none() && !payment_instructions.iter().any(|p| !p.value.is_empty()) {
            return Err(Refusal::MissingPaymentTarget);
        }
        Ok(Self {
            address,
            amount,
            label,
            message,
            payment_instructions,
            required_fields,
            optional_parameters,
        })
    }
}

fn validate_percent_escapes(input: &str) -> Result<(), Refusal> {
    let bytes = input.as_bytes();
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let escape = bytes.get(i + 1..i + 3).ok_or(Refusal::InvalidQuery)?;
            let high = escape.first().ok_or(Refusal::InvalidQuery)?;
            let low = escape.get(1).ok_or(Refusal::InvalidQuery)?;
            if !high.is_ascii_hexdigit() || !low.is_ascii_hexdigit() {
                return Err(Refusal::InvalidQuery);
            }
            i += 3;
        } else {
            let unreserved = byte.is_ascii_alphanumeric() || b"-._~".contains(&byte);
            let sub_delim = b"!$'()*+,;".contains(&byte);
            if !unreserved && !sub_delim && !matches!(byte, b':' | b'@' | b'/' | b'?') {
                return Err(Refusal::InvalidQuery);
            }
            i += 1;
        }
    }
    Ok(())
}

fn percent_decode(input: &str) -> Result<Vec<u8>, ()> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let escape = bytes.get(i + 1..i + 3).ok_or(())?;
            let hi = hex_nibble(*escape.first().ok_or(())?).ok_or(())?;
            let lo = hex_nibble(*escape.get(1).ok_or(())?).ok_or(())?;
            decoded.push((hi << 4) | lo);
            i += 3;
        } else {
            decoded.push(byte);
            i += 1;
        }
    }
    Ok(decoded)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_text(raw: &str) -> Result<String, Refusal> {
    let decoded = percent_decode(raw).map_err(|_| Refusal::InvalidText)?;
    if decoded.len() > MAX_TEXT_BYTES {
        return Err(Refusal::TextTooLong);
    }
    let text = String::from_utf8(decoded).map_err(|_| Refusal::InvalidText)?;
    if text.chars().any(char::is_control) {
        return Err(Refusal::InvalidText);
    }
    Ok(text)
}

fn parse_amount(raw: &str) -> Result<Amount, Refusal> {
    if raw.is_empty() || !raw.is_ascii() {
        return Err(Refusal::InvalidAmount);
    }
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || (raw.contains('.')
            && (fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit())))
        || fraction.len() > 8
    {
        return Err(Refusal::InvalidAmount);
    }
    let whole = whole.parse::<u64>().map_err(|_| Refusal::AmountTooLarge)?;
    let fraction_value = if fraction.is_empty() {
        0
    } else {
        let parsed = fraction
            .parse::<u64>()
            .map_err(|_| Refusal::InvalidAmount)?;
        parsed * 10_u64.pow((8 - fraction.len()) as u32)
    };
    let sats = whole
        .checked_mul(100_000_000)
        .and_then(|n| n.checked_add(fraction_value))
        .ok_or(Refusal::AmountTooLarge)?;
    if sats > MAX_AMOUNT_SATS {
        return Err(Refusal::AmountTooLarge);
    }
    Ok(Amount::from_sat(sats))
}

#[cfg(test)]
mod tests {
    use super::*;

    const P2PKH: &str = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT";
    const P2SH: &str = "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy";
    const P2WPKH: &str = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
    const P2WSH: &str = "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3";
    const P2TR: &str = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0";

    #[test]
    fn bu_01_mainnet_address_classes_and_network_are_checked() {
        for address in [P2PKH, P2SH, P2WPKH, P2WSH, P2TR] {
            assert!(PaymentRequest::parse(&format!("bitcoin:{address}"))
                .unwrap()
                .address
                .is_some());
        }
        assert_eq!(
            PaymentRequest::parse("bitcoin:tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx"),
            Err(Refusal::TestnetAddress)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t5"),
            Err(Refusal::InvalidAddress)
        );
        assert_eq!(
            PaymentRequest::parse(
                "bitcoin:bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqh2y7hd"
            ),
            Err(Refusal::InvalidAddress)
        );
        assert_eq!(
            PaymentRequest::parse(
                "bitcoin:bc1z0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqglt7rf"
            ),
            Err(Refusal::InvalidAddress)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:Bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
            Err(Refusal::InvalidAddress)
        );
    }

    #[test]
    fn bu_02_amount_converts_exact_decimal_to_satoshis() {
        let request = PaymentRequest::parse("BITCOIN:1BoatSLRHtKNngkdXEeobR76b53LETtpyT?AMOUNT=1.00000001&Lightning=lnbcabc%2Fxyz&label=hi+there")
            .unwrap();
        assert_eq!(request.amount, Some(Amount::from_sat(100_000_001)));
        assert_eq!(request.label.as_deref(), Some("hi+there"));
        assert_eq!(request.payment_instructions[0].value, "lnbcabc%2Fxyz");
        assert_eq!(
            request.payment_instructions[0].key,
            PaymentMethod::Lightning
        );
    }

    #[test]
    fn bu_03_duplicate_keys_and_unknown_required_are_refused() {
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?amount=1&AMOUNT=2")),
            Err(Refusal::DuplicateKey)
        );
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?amount=1&req-amount=2")),
            Err(Refusal::DuplicateKey)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:?req-future=x"),
            Err(Refusal::UnsupportedRequiredParameter)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:?lightning="),
            Err(Refusal::EmptyPaymentInstruction)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:?label=ok"),
            Err(Refusal::MissingPaymentTarget)
        );
    }

    #[test]
    fn bu_04_amount_and_decoded_text_bounds_are_strict() {
        assert!(PaymentRequest::parse(&format!("bitcoin:{P2PKH}?amount=21000000")).is_ok());
        for value in ["21000000.00000001", "1e2", "1.000000001", ".1", "1.", "-1"] {
            assert!(PaymentRequest::parse(&format!("bitcoin:{P2PKH}?amount={value}")).is_err());
        }
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?label=%00")),
            Err(Refusal::InvalidText)
        );
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?message=%FF")),
            Err(Refusal::InvalidText)
        );
    }

    #[test]
    fn bu_05_uri_shape_and_percent_escapes_are_validated() {
        for uri in [
            "bitcoin://example.com/address",
            "bitcoin:user@host",
            "bitcoin:1BoatSLRHtKNngkdXEeobR76b53LETtpyT#fragment",
            "bitcoin:1BoatSLRHtKNngkdXEeobR76b53LETtpyT?label=%Q0",
            "bitcoin:1BoatSLRHtKNngkdXEeobR76b53LETtpyT?lightning=lnbc%Q0",
            "bitcoin:1BoatSLRHtKNngkdXEeobR76b53LETtpyT?x=unknown%Q0",
        ] {
            assert!(PaymentRequest::parse(uri).is_err(), "{uri}");
        }
    }

    #[test]
    fn bu_06_required_payment_flag_and_uri_size_are_preserved() {
        let request = PaymentRequest::parse("bitcoin:?REQ-Lightning=lnbcraw").unwrap();
        assert_eq!(request.address, None);
        assert_eq!(request.payment_instructions.len(), 1);
        assert!(request.payment_instructions[0].required);
        assert_eq!(request.payment_instructions[0].value, "lnbcraw");

        let long = format!("bitcoin:{P2PKH}?x={}", "a".repeat(MAX_URI_BYTES));
        assert_eq!(PaymentRequest::parse(&long), Err(Refusal::TooLong));
        assert_eq!(
            PaymentRequest::parse("bitcoin:?%72eq-future=x"),
            Err(Refusal::UnsupportedRequiredParameter)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:?tb=tb1qexample"),
            Err(Refusal::TestnetAddress)
        );
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?x=1&%78=2")),
            Err(Refusal::DuplicateKey)
        );
    }

    #[test]
    fn bu_07_encoded_keys_cannot_bypass_canonicalization() {
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?amount=1&%61mount=2")),
            Err(Refusal::DuplicateKey)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:?%72eq-future=x"),
            Err(Refusal::UnsupportedRequiredParameter)
        );
        assert_eq!(
            PaymentRequest::parse("bitcoin:?%00key=x"),
            Err(Refusal::InvalidQuery)
        );
    }

    #[test]
    fn bu_08_supported_methods_keep_raw_values_and_required_flags() {
        let request =
            PaymentRequest::parse("bitcoin:?lightning=lnbc%2Fraw&req-lno=lno1%2Braw&sp=sp1raw")
                .unwrap();
        assert_eq!(request.payment_instructions.len(), 3);
        assert_eq!(request.payment_instructions[0].value, "lnbc%2Fraw");
        assert!(!request.payment_instructions[0].required);
        assert_eq!(request.payment_instructions[1].value, "lno1%2Braw");
        assert!(request.payment_instructions[1].required);
        assert_eq!(request.payment_instructions[2].value, "sp1raw");
        assert!(!request.payment_instructions[2].required);
        assert_eq!(
            PaymentRequest::parse("bitcoin:?label=recipient"),
            Err(Refusal::MissingPaymentTarget)
        );
        let required = PaymentRequest::parse(&format!(
            "bitcoin:{P2PKH}?req-amount=1.23&req-label=recipient%20name&req-message=purpose"
        ))
        .unwrap();
        assert_eq!(required.amount, Some(Amount::from_sat(123_000_000)));
        assert_eq!(required.label.as_deref(), Some("recipient name"));
        assert_eq!(required.message.as_deref(), Some("purpose"));
        assert_eq!(
            required.required_fields,
            vec![
                RequiredField::Amount,
                RequiredField::Label,
                RequiredField::Message
            ]
        );
        assert_eq!(
            PaymentRequest::parse(&format!("bitcoin:{P2PKH}?req-amount=1.000000001")),
            Err(Refusal::InvalidAmount)
        );
    }

    #[test]
    fn bu_00_required_rule_ids_are_complete_and_unique() {
        const RULES: &str = include_str!("../../../docs/URI-RULES.md");
        const SOURCE: &str = include_str!("bip321.rs");
        const RULE_TESTS: &[(&str, &str)] = &[
            (
                "BU-01",
                "bu_01_mainnet_address_classes_and_network_are_checked",
            ),
            ("BU-02", "bu_02_amount_converts_exact_decimal_to_satoshis"),
            (
                "BU-03",
                "bu_03_duplicate_keys_and_unknown_required_are_refused",
            ),
            ("BU-04", "bu_04_amount_and_decoded_text_bounds_are_strict"),
            ("BU-05", "bu_05_uri_shape_and_percent_escapes_are_validated"),
            (
                "BU-06",
                "bu_06_required_payment_flag_and_uri_size_are_preserved",
            ),
            ("BU-07", "bu_07_encoded_keys_cannot_bypass_canonicalization"),
            (
                "BU-08",
                "bu_08_supported_methods_keep_raw_values_and_required_flags",
            ),
        ];
        assert!(RULE_TESTS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (id, test_name) in RULE_TESTS {
            assert!(
                RULES.contains(&format!("**{id} —")),
                "missing doc rule {id}"
            );
            assert!(
                RULES.contains(test_name),
                "missing doc test name {test_name}"
            );
            assert!(
                SOURCE.contains(&format!("fn {test_name}")),
                "missing test {test_name}"
            );
        }
    }
}
