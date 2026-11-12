//! What the Box tells a counter when a till asks for a sale (BX-76): a
//! request to open one sale, and a request to drop it.
//!
//! Both are the content of a sealed message of kind POS notice from the Box
//! to one counter (LK-08). The seal names the Box as the sender and the
//! counter checks that key against its settings, so neither body carries a
//! signature of its own. Neither holds an amount in satoshis, a rate, an
//! address or a transaction ID: the counter makes the quote.

use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::{Amount, Currency};

/// The magic of a till sale body.
pub const MAGIC_TILL_SALE: [u8; 4] = *b"OBM(";
/// Longest reference a request carries, in bytes (BX-76).
pub const MAX_TILL_REFERENCE: usize = 40;

const KIND_REQUEST: u8 = 1;
const KIND_CANCEL: u8 = 2;

/// The till's wish to have one sale opened on a counter (BX-76).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleRequest {
    /// The id the Box gave the sale. The counter puts it in every paid
    /// notice about the sale (BX-77).
    pub sale: [u8; 16],
    /// The amount in minor units of the currency.
    pub amount: Amount,
    /// The currency, the shop's.
    pub currency: Currency,
    /// The till's own words for the sale: printable ASCII, at most
    /// [`MAX_TILL_REFERENCE`] bytes.
    pub reference: String,
    /// When the request lapses, on the Box's clock.
    pub expires_at: u64,
}

/// The till's wish to drop a sale it asked for (BX-76).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaleCancel {
    /// The sale.
    pub sale: [u8; 16],
}

/// What the Box can say to a counter about the till.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TillMessage {
    /// Open a sale.
    Request(SaleRequest),
    /// Drop a sale.
    Cancel(SaleCancel),
}

/// Whether `text` may be a reference: at most [`MAX_TILL_REFERENCE`] bytes of
/// printable ASCII.
pub fn reference_ok(text: &str) -> bool {
    text.len() <= MAX_TILL_REFERENCE && text.bytes().all(|b| (0x20..0x7f).contains(&b))
}

impl TillMessage {
    /// The message as sent.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_TILL_SALE, 80);
        match self {
            Self::Request(r) => {
                out.push(KIND_REQUEST);
                out.extend_from_slice(&r.sale);
                out.extend_from_slice(&r.amount.0.to_le_bytes());
                out.extend_from_slice(&r.currency.code());
                out.extend_from_slice(&r.expires_at.to_le_bytes());
                // The reference is at most 40 bytes, so the length is a byte.
                let bytes = r.reference.as_bytes();
                out.push(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
                out.extend_from_slice(bytes);
            }
            Self::Cancel(c) => {
                out.push(KIND_CANCEL);
                out.extend_from_slice(&c.sale);
            }
        }
        out
    }

    /// Read a message strictly: the kind is known, the amount is above zero,
    /// the currency is valid, the reference is printable ASCII within its
    /// bound, and no byte is left over.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_TILL_SALE)?;
        let message = match r.u8()? {
            KIND_REQUEST => {
                let sale = r.array::<16>()?;
                let amount = r.u64_le()?;
                if amount == 0 {
                    return Err(DecodeError::NonCanonical("a till sale's amount is zero"));
                }
                let currency = Currency::new(r.array::<3>()?)
                    .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?;
                let expires_at = r.u64_le()?;
                let len = usize::from(r.u8()?);
                if len > MAX_TILL_REFERENCE {
                    return Err(DecodeError::NonCanonical("a till reference is too long"));
                }
                let text = std::str::from_utf8(r.take(len)?)
                    .map_err(|_| DecodeError::NonCanonical("a till reference is not text"))?;
                if !reference_ok(text) {
                    return Err(DecodeError::NonCanonical(
                        "a till reference is not printable ASCII",
                    ));
                }
                Self::Request(SaleRequest {
                    sale,
                    amount: Amount(amount),
                    currency,
                    reference: text.to_string(),
                    expires_at,
                })
            }
            KIND_CANCEL => Self::Cancel(SaleCancel {
                sale: r.array::<16>()?,
            }),
            _ => {
                return Err(DecodeError::NonCanonical(
                    "a till message kind is not known",
                ))
            }
        };
        r.finish()?;
        Ok(message)
    }
}
