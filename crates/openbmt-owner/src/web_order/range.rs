//! WO-10: a range of fresh request addresses, signed with the web order key.
//!
//! Bytes: the magic, the version, the id of the pairing (8 bytes, WO-10), the
//! index of the first address (u32, big endian), the number of addresses (1 byte), each address as a length byte
//! and its ASCII bytes, and the 64-byte signature of everything before it.

use std::str::FromStr;

use bitcoin::address::NetworkUnchecked;
use bitcoin::Address;

use super::key::{verify, WebOrderKey, PUBLIC_BYTES, SIGNATURE_BYTES};
use super::pairing::PAIRING_ID_BYTES;
use super::{Reader, WebOrderRefusal, VERSION};

/// The first four bytes of a range.
pub const RANGE_MAGIC: [u8; 4] = *b"OBM{";
/// The most addresses in one range (WO-10).
pub const RANGE_SIZE: usize = 100;
/// The shortest text of an address that is taken.
pub const MIN_ADDRESS_BYTES: usize = 14;
/// The longest text of an address that is taken: the most a bech32 address
/// has.
pub const MAX_ADDRESS_BYTES: usize = 90;

/// A range of addresses: the index of the first in the request chain
/// (RQ-03) and the addresses, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    /// The index of the first address.
    pub start: u32,
    /// The addresses, in the order of the chain.
    pub addresses: Vec<String>,
}

/// WO-10: a lower-case segwit address of a network, as the text gives it.
pub(crate) fn address_is_valid(text: &str) -> bool {
    let known = ["bc1", "tb1", "bcrt1"].iter().any(|p| text.starts_with(p));
    known
        && (MIN_ADDRESS_BYTES..=MAX_ADDRESS_BYTES).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && Address::<NetworkUnchecked>::from_str(text).is_ok()
}

impl Range {
    /// A range, with every address checked.
    pub fn new(start: u32, addresses: Vec<String>) -> Result<Self, WebOrderRefusal> {
        let count = u32::try_from(addresses.len()).map_err(|_| WebOrderRefusal::Range)?;
        if addresses.is_empty()
            || addresses.len() > RANGE_SIZE
            || start.checked_add(count).is_none()
            || !addresses.iter().all(|a| address_is_valid(a))
        {
            return Err(WebOrderRefusal::Range);
        }
        Ok(Self { start, addresses })
    }

    /// The bytes that are signed.
    fn body(&self, id: &[u8; PAIRING_ID_BYTES]) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.addresses.len() * 64);
        out.extend_from_slice(&RANGE_MAGIC);
        out.push(VERSION);
        out.extend_from_slice(id);
        out.extend_from_slice(&self.start.to_be_bytes());
        out.push(u8::try_from(self.addresses.len()).unwrap_or(0));
        for a in &self.addresses {
            out.push(u8::try_from(a.len()).unwrap_or(0));
            out.extend_from_slice(a.as_bytes());
        }
        out
    }

    /// WO-10: the range for the pairing `id` and its signature, as the bytes
    /// sent to the store.
    pub fn sign(&self, key: &WebOrderKey, id: &[u8; PAIRING_ID_BYTES]) -> Vec<u8> {
        let mut out = self.body(id);
        let signature = key.sign(&out);
        out.extend_from_slice(&signature);
        out
    }

    /// Reads a signed range, strictly, checks the signature against `public`
    /// and that it is for the pairing `id`.
    pub fn open(
        bytes: &[u8],
        public: &[u8; PUBLIC_BYTES],
        id: &[u8; PAIRING_ID_BYTES],
    ) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Range;
        let body_len = bytes.len().checked_sub(SIGNATURE_BYTES).ok_or(bad)?;
        let (body, signature) = bytes.split_at_checked(body_len).ok_or(bad)?;
        let signature: [u8; SIGNATURE_BYTES] = signature.try_into().map_err(|_| bad)?;
        let (range, range_id) = Self::read_body(body)?;
        if !verify(public, body, &signature) {
            return Err(WebOrderRefusal::Signature);
        }
        if range_id != *id {
            return Err(WebOrderRefusal::PairingId);
        }
        Ok(range)
    }

    fn read_body(body: &[u8]) -> Result<(Self, [u8; PAIRING_ID_BYTES]), WebOrderRefusal> {
        let bad = WebOrderRefusal::Range;
        let mut r = Reader::new(body);
        if r.array::<4>().ok_or(bad)? != RANGE_MAGIC || r.u8().ok_or(bad)? != VERSION {
            return Err(bad);
        }
        let id = r.array::<PAIRING_ID_BYTES>().ok_or(bad)?;
        let start = r.u32().ok_or(bad)?;
        let count = usize::from(r.u8().ok_or(bad)?);
        let mut addresses = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::from(r.u8().ok_or(bad)?);
            let text = std::str::from_utf8(r.take(len).ok_or(bad)?).map_err(|_| bad)?;
            addresses.push(text.to_string());
        }
        if !r.is_empty() {
            return Err(bad);
        }
        Ok((Self::new(start, addresses)?, id))
    }
}
