//! The response: Key app to owner phone (AS-12 to AS-14).
//!
//! ```text
//! "OBM2" | 1 | session ID (32) | request hash (32) | approver (32)
//!   | result count u32 LE
//!   | for each result: item number u32 LE | status u8
//!       status 1, approved: kind u8 | payload length u32 LE | payload
//!       status 2, declined: code u8 | text length u8 | text
//!   | approver signature (64)
//! ```
//!
//! The approver signs everything after the version byte and before the
//! signature, under `OpenBMT/session-response/v1`.

use openbmt_core::identity::{DeviceId, Identity, Sig, TAG_SESSION_HEALTH, TAG_SESSION_RESPONSE};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::error::{BindError, EncodeError};
use crate::item::{Item, ItemKind};
use crate::request::Request;
use crate::text::{put_len, put_reason_text, read_reason_text};
use crate::{MAGIC_RESPONSE, MAX_ITEMS, MAX_SESSION_BYTES};

/// AS-12: why a line was declined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineCode {
    /// The person on the Key app declined it.
    ByPerson = 1,
    /// The Key app's checks failed; the text names the rule.
    ChecksFailed = 2,
    /// Spends are not answered on a device without key 2.
    NotOnThisDevice = 3,
    /// Nobody decided the line.
    NotDecided = 4,
}

impl DeclineCode {
    /// The code a byte stands for.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::ByPerson),
            2 => Some(Self::ChecksFailed),
            3 => Some(Self::NotOnThisDevice),
            4 => Some(Self::NotDecided),
            _ => None,
        }
    }
}

/// AS-12: what an approved result carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// The PSBT the signer returned, with its signatures.
    Spend {
        /// The PSBT bytes.
        psbt: Vec<u8>,
    },
    /// The approver's signature over the batch hash.
    Batch {
        /// The signature.
        sig: Sig,
    },
    /// The approver's signature over the settings hash.
    Settings {
        /// The signature.
        sig: Sig,
    },
    /// The settings version the Key app signed and its signature over the
    /// pairing entry.
    Pairing {
        /// The version in the entry.
        settings_version: u32,
        /// The signature.
        sig: Sig,
    },
    /// The signed answer to a health check (AS-14).
    Health {
        /// The signature.
        sig: Sig,
    },
}

impl Payload {
    /// The kind of item this payload answers.
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Spend { .. } => ItemKind::Spend,
            Self::Batch { .. } => ItemKind::BatchRefill,
            Self::Settings { .. } => ItemKind::Settings,
            Self::Pairing { .. } => ItemKind::Pairing,
            Self::Health { .. } => ItemKind::HealthCheck,
        }
    }

    fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        match self {
            Self::Spend { psbt } => {
                if psbt.len() > MAX_SESSION_BYTES {
                    return Err(EncodeError::TooLarge);
                }
                Ok(psbt.clone())
            }
            Self::Batch { sig } | Self::Settings { sig } | Self::Health { sig } => {
                Ok(sig.0.to_vec())
            }
            Self::Pairing {
                settings_version,
                sig,
            } => {
                let mut out = settings_version.to_le_bytes().to_vec();
                out.extend_from_slice(&sig.0);
                Ok(out)
            }
        }
    }

    fn decode(kind: ItemKind, bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        let payload = match kind {
            ItemKind::Spend => {
                return Ok(Self::Spend {
                    psbt: bytes.to_vec(),
                })
            }
            ItemKind::BatchRefill => Self::Batch { sig: r.sig()? },
            ItemKind::Settings => Self::Settings { sig: r.sig()? },
            ItemKind::HealthCheck => Self::Health { sig: r.sig()? },
            ItemKind::Pairing => Self::Pairing {
                settings_version: r.u32_le()?,
                sig: r.sig()?,
            },
        };
        r.finish()?;
        Ok(payload)
    }
}

/// AS-12: the answer to one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Approved, with its payload.
    Approved(Payload),
    /// Declined, with a code and a short text.
    Declined {
        /// The code.
        code: DeclineCode,
        /// The text (1 to 160 bytes).
        text: String,
    },
}

/// One result of a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemResult {
    /// The item's number.
    pub number: u32,
    /// The answer.
    pub verdict: Verdict,
}

/// A response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    session_id: [u8; 32],
    request_hash: [u8; 32],
    approver: DeviceId,
    results: Vec<ItemResult>,
    sig: Sig,
    bytes: Vec<u8>,
}

fn encode_unsigned(
    session_id: &[u8; 32],
    request_hash: &[u8; 32],
    approver: &DeviceId,
    results: &[ItemResult],
) -> Result<Vec<u8>, EncodeError> {
    if results.is_empty() || results.len() > MAX_ITEMS {
        return Err(EncodeError::ItemCount);
    }
    if results.windows(2).any(|w| match w {
        [a, b] => a.number >= b.number,
        _ => false,
    }) {
        return Err(EncodeError::ItemOrder);
    }
    let mut out = wire::start(&MAGIC_RESPONSE, 1024);
    out.extend_from_slice(session_id);
    out.extend_from_slice(request_hash);
    out.extend_from_slice(&approver.to_bytes());
    put_len(&mut out, results.len())?;
    for result in results {
        out.extend_from_slice(&result.number.to_le_bytes());
        match &result.verdict {
            Verdict::Approved(payload) => {
                out.push(1);
                out.push(payload.kind().byte());
                let bytes = payload.encode()?;
                put_len(&mut out, bytes.len())?;
                out.extend_from_slice(&bytes);
            }
            Verdict::Declined { code, text } => {
                out.push(2);
                out.push(*code as u8);
                put_reason_text(&mut out, text)?;
            }
        }
        if out.len() + 64 > MAX_SESSION_BYTES {
            return Err(EncodeError::TooLarge);
        }
    }
    Ok(out)
}

impl Response {
    /// AS-12: build a response and sign it with the approver's identity.
    pub fn new(
        approver: &Identity,
        session_id: [u8; 32],
        request_hash: [u8; 32],
        results: Vec<ItemResult>,
    ) -> Result<Self, EncodeError> {
        let approver_id = approver.device_id();
        let mut bytes = encode_unsigned(&session_id, &request_hash, &approver_id, &results)?;
        let body = bytes.get(5..).unwrap_or(&[]);
        let sig = approver.sign_tagged(TAG_SESSION_RESPONSE, body);
        bytes.extend_from_slice(&sig.0);
        Ok(Self {
            session_id,
            request_hash,
            approver: approver_id,
            results,
            sig,
            bytes,
        })
    }

    /// AS-01 to AS-03: read a response strictly. This does not check the
    /// signature or the binding: [`Response::check_against`] does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_SESSION_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let mut r = Reader::framed(bytes, &MAGIC_RESPONSE)?;
        let session_id = r.array::<32>()?;
        let request_hash = r.array::<32>()?;
        let approver = r.device_id()?;
        let n = r.count(MAX_ITEMS, 7)?;
        if n == 0 {
            return Err(DecodeError::NonCanonical("a response holds no result"));
        }
        let mut results: Vec<ItemResult> = Vec::with_capacity(n);
        for _ in 0..n {
            let number = r.u32_le()?;
            if results.last().is_some_and(|p| p.number >= number) {
                return Err(DecodeError::NonCanonical(
                    "result numbers do not strictly ascend",
                ));
            }
            let verdict = match r.u8()? {
                1 => {
                    let k = r.u8()?;
                    let kind = ItemKind::from_byte(k).ok_or(DecodeError::UnknownKind(k))?;
                    let len = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
                    Verdict::Approved(Payload::decode(kind, r.take(len)?)?)
                }
                2 => {
                    let code = DeclineCode::from_byte(r.u8()?)
                        .ok_or(DecodeError::NonCanonical("unknown decline code"))?;
                    Verdict::Declined {
                        code,
                        text: read_reason_text(&mut r)?,
                    }
                }
                _ => return Err(DecodeError::NonCanonical("unknown result status")),
            };
            results.push(ItemResult { number, verdict });
        }
        let sig = r.sig()?;
        r.finish()?;
        let mut again = encode_unsigned(&session_id, &request_hash, &approver, &results)
            .map_err(|_| DecodeError::NonCanonical("not the form encoding gives"))?;
        again.extend_from_slice(&sig.0);
        if again != bytes {
            return Err(DecodeError::NonCanonical("not the form encoding gives"));
        }
        Ok(Self {
            session_id,
            request_hash,
            approver,
            results,
            sig,
            bytes: again,
        })
    }

    /// The bytes of the response, as signed.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The session ID.
    pub fn session_id(&self) -> &[u8; 32] {
        &self.session_id
    }

    /// The hash of the request this response answers.
    pub fn request_hash(&self) -> &[u8; 32] {
        &self.request_hash
    }

    /// The approver's identity as the file names it.
    pub fn approver(&self) -> DeviceId {
        self.approver
    }

    /// The results, in item order.
    pub fn results(&self) -> &[ItemResult] {
        &self.results
    }

    /// AS-13, AS-14: check that this response is bound to `request` and was
    /// made by the pinned approver root.
    pub fn check_against(
        &self,
        request: &Request,
        approver_root: &DeviceId,
    ) -> Result<(), BindError> {
        if self.approver != *approver_root {
            return Err(BindError::WrongApprover);
        }
        let end = self.bytes.len().saturating_sub(64);
        let body = self.bytes.get(5..end).unwrap_or(&[]);
        if !self
            .approver
            .verify_tagged(TAG_SESSION_RESPONSE, body, &self.sig)
        {
            return Err(BindError::BadSignature);
        }
        if self.session_id != *request.session_id() {
            return Err(BindError::WrongSession);
        }
        if self.request_hash != request.hash() {
            return Err(BindError::WrongRequest);
        }
        for result in &self.results {
            let asked = request.item(result.number).ok_or(BindError::UnknownItem {
                item: result.number,
            })?;
            if let Verdict::Approved(payload) = &result.verdict {
                if payload.kind() != asked.item.kind() {
                    return Err(BindError::KindMismatch {
                        item: result.number,
                    });
                }
                if let (Payload::Health { sig }, Item::HealthCheck { nonce }) =
                    (payload, &asked.item)
                {
                    let message = health_message(&self.session_id, result.number, nonce);
                    if !approver_root.verify_tagged(TAG_SESSION_HEALTH, &message, sig) {
                        return Err(BindError::BadHealthProof {
                            item: result.number,
                        });
                    }
                }
            }
        }
        for asked in request.items() {
            if !self.results.iter().any(|r| r.number == asked.number) {
                return Err(BindError::MissingItem { item: asked.number });
            }
        }
        Ok(())
    }
}

/// AS-14: the bytes the approver signs for a health check.
pub fn health_message(session_id: &[u8; 32], item: u32, nonce: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(68);
    out.extend_from_slice(session_id);
    out.extend_from_slice(&item.to_le_bytes());
    out.extend_from_slice(nonce);
    out
}

/// AS-14: the approver's signed answer to a health check.
pub fn health_proof(
    approver: &Identity,
    session_id: &[u8; 32],
    item: u32,
    nonce: &[u8; 32],
) -> Sig {
    approver.sign_tagged(TAG_SESSION_HEALTH, &health_message(session_id, item, nonce))
}
