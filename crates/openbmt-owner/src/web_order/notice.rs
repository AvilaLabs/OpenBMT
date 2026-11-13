//! WO-30: the notice the owner phone signs when a request made from a web
//! order is Paid, Overpaid, Part paid or Paid late.
//!
//! Bytes: the magic, the version, the order number (u64), the address
//! (length byte and ASCII), the state (1 byte), the amount counted in
//! satoshis (u64), the depth (u32), the number of transaction IDs (1 byte)
//! and each ID as 32 bytes in the order a block explorer shows it, and the
//! 64-byte signature of everything before it. All numbers are big endian.

use super::key::{verify, WebOrderKey, PUBLIC_BYTES, SIGNATURE_BYTES};
use super::range::address_is_valid;
use super::{Reader, WebOrderRefusal, VERSION};

/// The first four bytes of a notice.
pub const NOTICE_MAGIC: [u8; 4] = *b"OBM}";
/// The most transaction IDs in one notice.
pub const MAX_NOTICE_TXIDS: usize = 16;

/// The states a notice may carry (RQ-16, RQ-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeState {
    /// Paid in full at the shop's depth.
    Paid,
    /// Paid, and more than asked.
    Overpaid,
    /// Some of the amount, at the shop's depth.
    PartPaid,
    /// A payment after the quote or the request ran out.
    PaidLate,
}

impl NoticeState {
    /// The byte in the notice.
    pub fn byte(self) -> u8 {
        match self {
            Self::Paid => 1,
            Self::Overpaid => 2,
            Self::PartPaid => 3,
            Self::PaidLate => 4,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Paid),
            2 => Some(Self::Overpaid),
            3 => Some(Self::PartPaid),
            4 => Some(Self::PaidLate),
            _ => None,
        }
    }
}

/// What the owner phone tells the store about one web order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The order number the store gave.
    pub order: u64,
    /// The address of the order.
    pub address: String,
    /// The state of the request.
    pub state: NoticeState,
    /// The satoshis counted at the shop's depth.
    pub counted_sats: u64,
    /// The confirmations the payment has.
    pub depth: u32,
    /// The transaction IDs, each as a block explorer shows it.
    pub txids: Vec<[u8; 32]>,
}

impl Notice {
    /// The bytes that are signed. A notice with a bad address or no
    /// transaction ID, or more than [`MAX_NOTICE_TXIDS`], is refused.
    fn body(&self) -> Result<Vec<u8>, WebOrderRefusal> {
        let bad = WebOrderRefusal::Notice;
        if !address_is_valid(&self.address)
            || self.txids.is_empty()
            || self.txids.len() > MAX_NOTICE_TXIDS
        {
            return Err(bad);
        }
        let mut out = Vec::with_capacity(64 + self.txids.len() * 32);
        out.extend_from_slice(&NOTICE_MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&self.order.to_be_bytes());
        out.push(u8::try_from(self.address.len()).map_err(|_| bad)?);
        out.extend_from_slice(self.address.as_bytes());
        out.push(self.state.byte());
        out.extend_from_slice(&self.counted_sats.to_be_bytes());
        out.extend_from_slice(&self.depth.to_be_bytes());
        out.push(u8::try_from(self.txids.len()).map_err(|_| bad)?);
        for t in &self.txids {
            out.extend_from_slice(t);
        }
        Ok(out)
    }

    /// WO-30: the notice and its signature, as the bytes sent to the store.
    pub fn sign(&self, key: &WebOrderKey) -> Result<Vec<u8>, WebOrderRefusal> {
        let mut out = self.body()?;
        let signature = key.sign(&out);
        out.extend_from_slice(&signature);
        Ok(out)
    }

    /// Reads a signed notice, strictly, and checks the signature against
    /// `public`.
    pub fn open(bytes: &[u8], public: &[u8; PUBLIC_BYTES]) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Notice;
        let body_len = bytes.len().checked_sub(SIGNATURE_BYTES).ok_or(bad)?;
        let (body, signature) = bytes.split_at_checked(body_len).ok_or(bad)?;
        let signature: [u8; SIGNATURE_BYTES] = signature.try_into().map_err(|_| bad)?;
        let notice = Self::read_body(body)?;
        if !verify(public, body, &signature) {
            return Err(WebOrderRefusal::Signature);
        }
        Ok(notice)
    }

    fn read_body(body: &[u8]) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::Notice;
        let mut r = Reader::new(body);
        if r.array::<4>().ok_or(bad)? != NOTICE_MAGIC || r.u8().ok_or(bad)? != VERSION {
            return Err(bad);
        }
        let order = r.u64().ok_or(bad)?;
        let len = usize::from(r.u8().ok_or(bad)?);
        let address = std::str::from_utf8(r.take(len).ok_or(bad)?)
            .map_err(|_| bad)?
            .to_string();
        let state = NoticeState::from_byte(r.u8().ok_or(bad)?).ok_or(bad)?;
        let counted_sats = r.u64().ok_or(bad)?;
        let depth = r.u32().ok_or(bad)?;
        let count = usize::from(r.u8().ok_or(bad)?);
        let mut txids = Vec::with_capacity(count);
        for _ in 0..count {
            txids.push(r.array::<32>().ok_or(bad)?);
        }
        if !r.is_empty() {
            return Err(bad);
        }
        let notice = Self {
            order,
            address,
            state,
            counted_sats,
            depth,
            txids,
        };
        // The same checks as on making one.
        notice.body()?;
        Ok(notice)
    }
}
