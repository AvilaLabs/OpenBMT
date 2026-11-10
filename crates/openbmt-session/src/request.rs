//! The request: owner phone to Key app (AS-01 to AS-05).
//!
//! ```text
//! "OBMF" | 1 | session ID (32) | wallet ID (32) | owner (32)
//!   | item count u32 LE
//!   | for each item: number u32 LE | kind u8 | body length u32 LE | body
//!   | owner signature (64)
//! ```
//!
//! The owner signs everything after the version byte and before the
//! signature, under `OpenBMT/session-request/v1`.

use openbmt_core::identity::{
    tagged_hash, DeviceId, Identity, Sig, TAG_SESSION_HASH, TAG_SESSION_REQUEST,
};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::error::{EncodeError, VerifyError};
use crate::item::{decode_body, encode_body, ItemKind, RequestItem};
use crate::text::put_len;
use crate::{MAGIC_REQUEST, MAX_ITEMS, MAX_SESSION_BYTES};

/// A request. Its fields are read through getters, and its bytes are the
/// ones that were signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    session_id: [u8; 32],
    wallet: [u8; 32],
    owner: DeviceId,
    items: Vec<RequestItem>,
    sig: Sig,
    bytes: Vec<u8>,
}

/// The bytes before the signature.
fn encode_unsigned(
    session_id: &[u8; 32],
    wallet: &[u8; 32],
    owner: &DeviceId,
    items: &[RequestItem],
) -> Result<Vec<u8>, EncodeError> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(EncodeError::ItemCount);
    }
    if items.windows(2).any(|w| match w {
        [a, b] => a.number >= b.number,
        _ => false,
    }) {
        return Err(EncodeError::ItemOrder);
    }
    let mut out = wire::start(&MAGIC_REQUEST, 4096);
    out.extend_from_slice(session_id);
    out.extend_from_slice(wallet);
    out.extend_from_slice(&owner.to_bytes());
    put_len(&mut out, items.len())?;
    for entry in items {
        let body = encode_body(&entry.item)?;
        out.extend_from_slice(&entry.number.to_le_bytes());
        out.push(entry.item.kind().byte());
        put_len(&mut out, body.len())?;
        out.extend_from_slice(&body);
        if out.len() + 64 > MAX_SESSION_BYTES {
            return Err(EncodeError::TooLarge);
        }
    }
    Ok(out)
}

/// The signed part: everything after the 5 header bytes.
fn signed_part(unsigned: &[u8]) -> &[u8] {
    unsigned.get(5..).unwrap_or(&[])
}

impl Request {
    /// AS-05: build a request and sign it with the owner's identity. The
    /// items must follow AS-04, AS-06 to AS-11, and the whole file AS-02.
    pub fn new(
        owner: &Identity,
        session_id: [u8; 32],
        wallet: [u8; 32],
        items: Vec<RequestItem>,
    ) -> Result<Self, EncodeError> {
        let owner_id = owner.device_id();
        let mut bytes = encode_unsigned(&session_id, &wallet, &owner_id, &items)?;
        let sig = owner.sign_tagged(TAG_SESSION_REQUEST, signed_part(&bytes));
        bytes.extend_from_slice(&sig.0);
        Ok(Self {
            session_id,
            wallet,
            owner: owner_id,
            items,
            sig,
            bytes,
        })
    }

    /// AS-01 to AS-04: read a request strictly. This does not check the
    /// signature: [`Request::verify`] does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_SESSION_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let mut r = Reader::framed(bytes, &MAGIC_REQUEST)?;
        let session_id = r.array::<32>()?;
        let wallet = r.array::<32>()?;
        let owner = r.device_id()?;
        let n = r.count(MAX_ITEMS, 9)?;
        if n == 0 {
            return Err(DecodeError::NonCanonical("a request holds no item"));
        }
        let mut items: Vec<RequestItem> = Vec::with_capacity(n);
        for _ in 0..n {
            let number = r.u32_le()?;
            if items.last().is_some_and(|p| p.number >= number) {
                return Err(DecodeError::NonCanonical(
                    "item numbers do not strictly ascend",
                ));
            }
            let k = r.u8()?;
            let kind = ItemKind::from_byte(k).ok_or(DecodeError::UnknownKind(k))?;
            let len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
            let body = r.take(len)?;
            items.push(RequestItem {
                number,
                item: decode_body(kind, body)?,
            });
        }
        let sig = r.sig()?;
        r.finish()?;
        let mut again = encode_unsigned(&session_id, &wallet, &owner, &items)
            .map_err(|_| DecodeError::NonCanonical("not the form encoding gives"))?;
        again.extend_from_slice(&sig.0);
        if again != bytes {
            return Err(DecodeError::NonCanonical("not the form encoding gives"));
        }
        Ok(Self {
            session_id,
            wallet,
            owner,
            items,
            sig,
            bytes: again,
        })
    }

    /// The bytes of the request, as signed.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// AS-05: the tagged hash of the whole request, signature included.
    pub fn hash(&self) -> [u8; 32] {
        tagged_hash(TAG_SESSION_HASH, &self.bytes)
    }

    /// AS-05: the owner key in the file is `pinned_owner` and its signature
    /// verifies.
    pub fn verify(&self, pinned_owner: &DeviceId) -> Result<(), VerifyError> {
        if self.owner != *pinned_owner {
            return Err(VerifyError::WrongOwner);
        }
        let end = self.bytes.len().saturating_sub(64);
        let body = self.bytes.get(5..end).unwrap_or(&[]);
        if self
            .owner
            .verify_tagged(TAG_SESSION_REQUEST, body, &self.sig)
        {
            Ok(())
        } else {
            Err(VerifyError::BadSignature)
        }
    }

    /// The session ID.
    pub fn session_id(&self) -> &[u8; 32] {
        &self.session_id
    }

    /// The wallet's ID.
    pub fn wallet(&self) -> &[u8; 32] {
        &self.wallet
    }

    /// The owner's identity as the file names it.
    pub fn owner(&self) -> DeviceId {
        self.owner
    }

    /// The items, in item order.
    pub fn items(&self) -> &[RequestItem] {
        &self.items
    }

    /// The item with this number.
    pub fn item(&self, number: u32) -> Option<&RequestItem> {
        self.items.iter().find(|i| i.number == number)
    }
}
