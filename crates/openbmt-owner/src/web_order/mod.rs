//! The owner phone's side of a web order (`docs/WEB-ORDER-RULES.md`): the
//! key it signs with, the code that pairs it with a store, the ranges of
//! addresses it gives the store, the requests it makes, the notices it
//! signs and the check of the orders a store reports.
//!
//! The library has no network and no clock. The caller passes the time,
//! the random bytes and the addresses of the request chain
//! (`docs/REQUEST-RULES.md`, RQ-03). The plugin in
//! `integrations/woocommerce/openbmt-pay` reads the same bytes; both sides
//! test against the vectors in its `tests_standalone/vectors` folder (WO-50).

mod check;
mod key;
mod notice;
mod pairing;
mod range;
mod request;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;

pub use check::{OrderCheck, OrderRefusal, SentRanges, StoreOrder};
pub use key::{WebOrderKey, FINGERPRINT_BYTES, PUBLIC_BYTES, SIGNATURE_BYTES};
pub use notice::{Notice, NoticeState, MAX_NOTICE_TXIDS, NOTICE_MAGIC};
pub use pairing::{PairingCode, MAX_SHOP_NAME_BYTES, PAIRING_MAGIC, SECRET_BYTES};
pub use range::{Range, MAX_ADDRESS_BYTES, MIN_ADDRESS_BYTES, RANGE_MAGIC, RANGE_SIZE};
pub use request::{
    orders_request_body, SignedRequest, HEADER_MAC, HEADER_NONCE, HEADER_TIME, NONCE_BYTES,
    ROUTE_NOTICE, ROUTE_ORDERS, ROUTE_RANGE, TIME_WINDOW_SECS,
};

/// The version byte of every frame of this module.
pub const VERSION: u8 = 1;

/// Why a web order frame, key or request was refused. Each carries the rule
/// it comes from; the words a person reads are not written here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebOrderRefusal {
    /// WO-43: the kernel gave no bytes, or weak ones.
    Random,
    /// WO-05: the pairing code is not what the format says.
    PairingCode,
    /// WO-05: the shop's name is empty, too long or has a control byte.
    ShopName,
    /// WO-10: a range is not what the format says, or holds no address, too
    /// many, or one that is not a bitcoin address.
    Range,
    /// WO-30: a notice is not what the format says.
    Notice,
    /// WO-10, WO-30: the signature does not match the key.
    Signature,
    /// WO-20: a request's method, route or body is not allowed.
    Request,
    /// WO-22: the text a store sent is not a list of orders.
    Orders,
}

impl WebOrderRefusal {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Random => "WO-43",
            Self::PairingCode | Self::ShopName => "WO-05",
            Self::Range => "WO-10",
            Self::Notice => "WO-30",
            Self::Signature => "WO-30",
            Self::Request => "WO-20",
            Self::Orders => "WO-22",
        }
    }
}

/// Lower-case hex of bytes.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(b & 15), 16).unwrap_or('0'));
    }
    out
}

/// Bytes of lower-case hex, or `None` for any other text.
#[cfg(test)]
pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    bytes
        .chunks(2)
        .map(|pair| {
            let hi = nibble(*pair.first()?)?;
            let lo = nibble(*pair.get(1)?)?;
            Some(hi << 4 | lo)
        })
        .collect()
}

#[cfg(test)]
fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// A reader over bytes that never indexes past the end.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.bytes.split_at_checked(n)?;
        self.bytes = rest;
        Some(head)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}
