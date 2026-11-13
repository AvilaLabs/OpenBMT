//! The payment request the customer scans (CR-83).
//!
//! A sale's request is its address and what it needs now (CR-08) as a BIP 21
//! URI. The text the QR code holds has the scheme and a bech32 address in
//! upper case, so that the address is in the alphanumeric set; the
//! parameter name `amount` is lower case, as BIP 21 has it, so the code as a
//! whole is in byte mode. The same URI in lower case is returned for the
//! app's "copy". The code is made by `openbmt-qr` at level M, in one
//! segment.

use bitcoin::{Address, Network, ScriptBuf};
use openbmt_lightning::bip321::PaymentUri;
use openbmt_lightning::bolt11::Checked;
use openbmt_qr::{encode_text, Ecc};

use crate::codec::wire_record;
use crate::error::{refused_by, CounterError};
use crate::tap::tag_content;

/// What the cashier's screen shows the customer for one sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    /// The address, as a wallet shows it (lower case for bech32).
    pub address: String,
    /// The text the QR code holds.
    pub uri: String,
    /// The same request in lower case, for the app's "copy".
    pub copy_uri: String,
    /// Modules along one side of the code, without the quiet zone.
    pub qr_size: u32,
    /// The modules, dark as 1, packed most significant bit first as one
    /// stream, row by row: module `(x, y)` is bit `7 - (n % 8)` of byte
    /// `n / 8` for `n = y * qr_size + x`. The last byte is padded with 0.
    pub qr_bits: Vec<u8>,
    /// The NDEF file of the NFC tag for the same request (TP-01), made from
    /// `copy_uri`; empty when it does not fit (TP-03).
    pub tag: Vec<u8>,
}

wire_record!(Payment {
    address,
    uri,
    copy_uri,
    qr_size,
    qr_bits,
    tag
});

/// An amount in satoshis as BIP 21 writes it: bitcoin, a decimal point and
/// at most 8 decimals, no trailing zero, no exponent. Zero is `0`.
pub fn btc_amount(sats: u64) -> String {
    let whole = sats / 100_000_000;
    let frac = sats % 100_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:08}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// The two forms of the request for an address: the one the QR code holds
/// and the one the app copies. A bech32 address is upper-cased in the first
/// only; a base58 address is case-sensitive and is left as it is.
pub fn request_texts(address: &str, bech32: bool, sats: u64) -> (String, String) {
    let query = if sats == 0 {
        String::new()
    } else {
        format!("?amount={}", btc_amount(sats))
    };
    let copy = format!("bitcoin:{address}{query}");
    let code = if bech32 {
        format!("BITCOIN:{}{query}", address.to_ascii_uppercase())
    } else {
        copy.clone()
    };
    (code, copy)
}

/// CR-214: the payment link of a request to send, for the app to share: the
/// request of CR-83 in lower case, with a `label` (BIP 21) that is the text
/// percent-encoded. Every byte outside `A-Z a-z 0-9 - . _ ~` is written as
/// `%` and two hexadecimal digits.
pub fn request_link(address: &str, sats: u64, label: &str) -> String {
    let (_, copy) = request_texts(address, false, sats);
    let mut out = copy;
    out.push(if sats == 0 { '?' } else { '&' });
    out.push_str("label=");
    for b in label.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// CR-113: the code of a sale that has a Lightning invoice that passed the
/// checks (LN-17): `bitcoin:<address>?amount=<btc>&lightning=<invoice>`
/// (LN-18), in capitals for the QR code when LN-19 allows. The text for
/// "copy" is the code as the BIP gives it.
pub(crate) fn payment_with_invoice(
    script: &ScriptBuf,
    network: Network,
    sats: u64,
    invoice: &Checked,
) -> Result<Payment, CounterError> {
    let address = Address::from_script(script, network)
        .map_err(|e| refused_by("pay-address", format!("the script has no address: {e}")))?
        .to_string();
    let code = PaymentUri::new(&address, network, sats, invoice)
        .map_err(|e| refused_by(e.rule(), e.to_string()))?;
    let copy_uri = code.text().to_string();
    // LN-19: capitals only when the address allows them.
    let uri = code.qr_text().unwrap_or_else(|_| copy_uri.clone());
    finish(address, uri, copy_uri)
}

/// The payment request of the sale whose address is `script`, for `sats`.
pub(crate) fn payment_for(
    script: &ScriptBuf,
    network: Network,
    sats: u64,
) -> Result<Payment, CounterError> {
    let address = Address::from_script(script, network)
        .map_err(|e| refused_by("pay-address", format!("the script has no address: {e}")))?;
    let address = address.to_string();
    let (uri, copy_uri) = request_texts(&address, script.is_witness_program(), sats);
    finish(address, uri, copy_uri)
}

/// The modules of the code the text makes, with the texts.
fn finish(address: String, uri: String, copy_uri: String) -> Result<Payment, CounterError> {
    let code = encode_text(&uri, Ecc::M)
        .map_err(|e| refused_by("pay-code", format!("the code could not be made: {e}")))?;
    let size = code.size();
    let mut qr_bits = vec![0u8; (size * size).div_ceil(8)];
    for y in 0..size {
        for x in 0..size {
            if code.get(x, y) {
                let n = y * size + x;
                if let Some(byte) = qr_bits.get_mut(n / 8) {
                    *byte |= 0x80 >> (n % 8);
                }
            }
        }
    }
    let tag = tag_content(&copy_uri).unwrap_or_default();
    Ok(Payment {
        address,
        uri,
        copy_uri,
        qr_size: u32::try_from(size).map_err(|_| refused_by("pay-code", "the code is too big"))?,
        qr_bits,
        tag,
    })
}
