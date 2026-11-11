//! The payment code with both ways to pay: an on-chain address and a
//! Lightning invoice, in the form of BIP 321 (LN-18, LN-19).
//!
//! `bitcoin:<ADDRESS>?amount=<BTC>&lightning=<BOLT11>`.
//! Text of the BIP: <https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki>.

use std::str::FromStr;

use bitcoin::address::{Address, AddressType, NetworkUnchecked};
use bitcoin::Network;

use crate::bolt11::Checked;
use crate::error::UriRefused;
use crate::invoice::MSAT_PER_SAT;

const SATS_PER_BTC: u64 = 100_000_000;

/// An amount of satoshis as the BTC decimal of BIP 321: digits, a period and
/// the fraction with no trailing zeros, or only the digits for a whole number
/// of bitcoin (`0.00001`, `20.3`, `1`).
pub fn btc_decimal(sats: u64) -> String {
    let (whole, frac) = (sats / SATS_PER_BTC, sats % SATS_PER_BTC);
    if frac == 0 {
        return whole.to_string();
    }
    let frac = format!("{frac:08}");
    format!("{whole}.{}", frac.trim_end_matches('0'))
}

/// The code of a sale: the text of the BIP and, when the address allows it,
/// the same in capitals for a QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentUri {
    text: String,
    bech32: bool,
}

impl PaymentUri {
    /// LN-18: the code for an address of the shop's network, an amount in
    /// satoshis and an invoice that has passed `bolt11::check`. The amount of
    /// the code is the amount of the invoice, so the invoice must name exactly
    /// `sats` satoshis. The address is written the way the `bitcoin` crate
    /// writes it, which is lower case for a bech32 address.
    pub fn new(
        address: &str,
        network: Network,
        sats: u64,
        invoice: &Checked,
    ) -> Result<Self, UriRefused> {
        let parsed = Address::<NetworkUnchecked>::from_str(address)
            .and_then(|a| a.require_network(network))
            .map_err(|_| UriRefused::BadAddress)?;
        let bech32 = match parsed.address_type() {
            Some(AddressType::P2wpkh | AddressType::P2wsh | AddressType::P2tr) => true,
            Some(_) => false,
            // A witness version this crate does not know: not an address the
            // counter makes.
            None => return Err(UriRefused::BadAddress),
        };
        if sats == 0 || sats.checked_mul(MSAT_PER_SAT) != Some(invoice.amount_msat) {
            return Err(UriRefused::AmountMismatch);
        }
        Ok(Self {
            text: format!(
                "bitcoin:{parsed}?amount={}&lightning={}",
                btc_decimal(sats),
                invoice.bolt11
            ),
            bech32,
        })
    }

    /// The code as text, as the BIP gives it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// LN-19: the code in capitals, for a QR code, which then uses the
    /// smaller alphanumeric alphabet. Capitals are only safe when the address
    /// is bech32 (case-insensitive when it is all one case) and the invoice
    /// is bech32 as well; a base58 address would change meaning, so for one
    /// the answer is a refusal and the code goes in as it is.
    pub fn qr_text(&self) -> Result<String, UriRefused> {
        if self.bech32 {
            Ok(self.text.to_ascii_uppercase())
        } else {
            Err(UriRefused::NotBech32)
        }
    }
}
