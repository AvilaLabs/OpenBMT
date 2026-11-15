//! The payment request of a sale: the BIP 321 text and the QR matrix. The
//! texts are those of the counter's bridge (`openbmt-counter-ffi`, `pay`),
//! and a test holds the two together.

use bitcoin::{Address, Network, ScriptBuf};
use openbmt_qr::{encode_text, Ecc};

/// What the counter's screen shows the customer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentView {
    /// The regtest address.
    pub address: String,
    /// The text the QR code holds.
    pub uri: String,
    /// The same request in lower case, for "copy".
    pub copy_uri: String,
    /// Modules along one side of the code.
    pub qr_size: usize,
    /// One character for each module, row by row: `1` dark, `0` light.
    pub qr: String,
}

/// An amount in satoshis as BIP 21 writes it.
pub fn btc_amount(sats: u64) -> String {
    let whole = sats / 100_000_000;
    let frac = sats % 100_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:08}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// The two forms of the request: the QR text, with the scheme and the
/// address in capitals, and the lower-case text for "copy".
pub fn request_texts(address: &str, sats: u64) -> (String, String) {
    let query = if sats == 0 {
        String::new()
    } else {
        format!("?amount={}", btc_amount(sats))
    };
    let copy = format!("bitcoin:{address}{query}");
    let code = format!("BITCOIN:{}{query}", address.to_ascii_uppercase());
    (code, copy)
}

/// The request for the sale whose address is `script`, asking `sats`.
pub fn payment_for(script: &ScriptBuf, sats: u64) -> Result<PaymentView, String> {
    let address = Address::from_script(script, Network::Regtest)
        .map_err(|e| format!("the script has no address: {e}"))?
        .to_string();
    let (uri, copy_uri) = request_texts(&address, sats);
    let code = encode_text(&uri, Ecc::M).map_err(|e| format!("the code could not be made: {e}"))?;
    let size = code.size();
    let mut qr = String::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            qr.push(if code.get(x, y) { '1' } else { '0' });
        }
    }
    Ok(PaymentView {
        address,
        uri,
        copy_uri,
        qr_size: size,
        qr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texts_are_the_bridges() {
        for sats in [0, 1, 5_000, 123_456_789, 100_000_000] {
            let ours = request_texts("bcrt1qexample", sats);
            let theirs = openbmt_counter_ffi::request_texts("bcrt1qexample", true, sats);
            assert_eq!(ours, theirs, "{sats}");
            assert_eq!(btc_amount(sats), openbmt_counter_ffi::btc_amount(sats));
        }
    }
}
