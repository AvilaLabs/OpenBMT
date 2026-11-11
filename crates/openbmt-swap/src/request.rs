//! The request for a reverse swap (SW-01 to SW-03).

use bitcoin::hex::DisplayHex;
use bitcoin::secp256k1::PublicKey;

use crate::error::Refused;
use crate::keys::Secrets;
use crate::{MAX_DESCRIPTION_BYTES, MAX_INVOICE_SATS, MIN_INVOICE_SATS};

/// A request for a reverse swap, made and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    amount_sats: u64,
    preimage_hash: [u8; 32],
    claim_public: PublicKey,
    description: String,
}

impl Request {
    /// SW-01 to SW-04: the request for an invoice of `amount_sats`, for the
    /// preimage hash and the claim key of `secrets`.
    pub fn new(secrets: &Secrets, amount_sats: u64, description: &str) -> Result<Self, Refused> {
        if !(MIN_INVOICE_SATS..=MAX_INVOICE_SATS).contains(&amount_sats) {
            return Err(Refused::AmountOutOfBand);
        }
        if description.is_empty()
            || description.len() > MAX_DESCRIPTION_BYTES
            || description.chars().any(char::is_control)
        {
            return Err(Refused::BadDescription);
        }
        Ok(Self {
            amount_sats,
            preimage_hash: secrets.preimage_hash(),
            claim_public: secrets.claim_public(),
            description: description.to_string(),
        })
    }

    /// The invoice amount asked for, in satoshis.
    pub fn amount_sats(&self) -> u64 {
        self.amount_sats
    }

    /// SW-01: the body of the POST to `/v2/swap/reverse`, with exactly the
    /// members of the API and in a fixed order.
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("{\"from\":\"BTC\",\"to\":\"BTC\",\"invoiceAmount\":");
        out.push_str(&self.amount_sats.to_string());
        out.push_str(",\"preimageHash\":\"");
        out.push_str(&self.preimage_hash.to_lower_hex_string());
        out.push_str("\",\"claimPublicKey\":\"");
        out.push_str(&self.claim_public.serialize().to_lower_hex_string());
        out.push_str("\",\"description\":\"");
        for c in self.description.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c => out.push(c),
            }
        }
        out.push_str("\"}");
        out
    }
}
