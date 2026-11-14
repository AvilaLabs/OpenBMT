//! The starter line (`docs/LINK-RULES.md` LK-79).
//!
//! The owner phone tells each counter paired with the starter wallet what
//! that wallet holds, the limit, whether the owner asked to stop bitcoin
//! sales above it, and the last payments out of it
//! (`docs/START-SIMPLE-RULES.md` SS-32, SS-36, SS-38). One frame, signed by
//! the owner root:
//!
//! ```text
//! line: "OBM" 0x1b | 1 | number u64 LE | time u64 LE | held sats u64 LE
//!         | limit sats u64 LE | stop flag u8 (0 or 1) | payment count u8 (0..=16)
//!         | per payment: txid (32) | sats u64 LE | time u64 LE | made by the owner phone u8 (0 or 1)
//!         | owner sig (64)
//! ```
//!
//! The signature is over everything between the version byte and itself,
//! under [`TAG_STARTER_LINE`]. Decoding only parses (exact length, at most
//! 16 payments, no transaction twice, flags 0 or 1); the signature, the
//! number against the last one taken and the age are judged by the counter
//! node (`docs/COUNTER-RULES.md` CR-272, CR-273).

use crate::identity::{DeviceId, Identity, Sig, TAG_STARTER_LINE};
use crate::wire::{self, DecodeError, Reader};

/// Most payments one line carries (LK-79).
pub const MAX_LINE_PAYMENTS: usize = 16;

/// Bytes of one payment on the wire.
const PAYMENT_BYTES: usize = 32 + 8 + 8 + 1;

/// One payment out of the starter wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinePayment {
    /// The transaction that spent the wallet's coins.
    pub txid: [u8; 32],
    /// What it paid out of the wallet, in sats.
    pub sats: u64,
    /// The time the owner phone first saw it, in seconds.
    pub time: u64,
    /// The owner phone made it (its outbox took this transaction).
    pub by_owner: bool,
}

/// What the owner phone says of the starter wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterLine {
    /// Rises with each line the owner node makes.
    pub number: u64,
    /// The owner phone's clock when it signed, in seconds.
    pub time: u64,
    /// What the starter wallet holds as SS-36 counts it, in sats.
    pub held_sats: u64,
    /// The owner's limit, in sats.
    pub limit_sats: u64,
    /// The owner turned on "Stop bitcoin sales above my limit" (SS-38).
    pub stop: bool,
    /// The last payments, at most [`MAX_LINE_PAYMENTS`].
    pub payments: Vec<LinePayment>,
    /// The owner root's signature under [`TAG_STARTER_LINE`].
    pub sig: Sig,
}

impl StarterLine {
    fn body(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(34 + self.payments.len() * PAYMENT_BYTES);
        out.extend_from_slice(&self.number.to_le_bytes());
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.held_sats.to_le_bytes());
        out.extend_from_slice(&self.limit_sats.to_le_bytes());
        out.push(u8::from(self.stop));
        out.push(u8::try_from(self.payments.len()).unwrap_or(u8::MAX));
        for p in &self.payments {
            out.extend_from_slice(&p.txid);
            out.extend_from_slice(&p.sats.to_le_bytes());
            out.extend_from_slice(&p.time.to_le_bytes());
            out.push(u8::from(p.by_owner));
        }
        out
    }

    /// Make a line and sign it with the owner root. More than
    /// [`MAX_LINE_PAYMENTS`] payments keep the last ones.
    pub fn sign(
        owner: &Identity,
        number: u64,
        time: u64,
        held_sats: u64,
        limit_sats: u64,
        stop: bool,
        payments: &[LinePayment],
    ) -> Self {
        let from = payments.len().saturating_sub(MAX_LINE_PAYMENTS);
        let mut line = Self {
            number,
            time,
            held_sats,
            limit_sats,
            stop,
            payments: payments[from..].to_vec(),
            sig: Sig([0; 64]),
        };
        line.sig = owner.sign_tagged(TAG_STARTER_LINE, &line.body());
        line
    }

    /// Whether the signature is `owner`'s over every part.
    pub fn verify(&self, owner: &DeviceId) -> bool {
        owner.verify_tagged(TAG_STARTER_LINE, &self.body(), &self.sig)
    }

    /// Wire form: see the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.body();
        let mut out = wire::start(&wire::MAGIC_STARTER_LINE, body.len() + 64);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Parse the wire form strictly (LK-79). The signature is not checked.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_STARTER_LINE)?;
        let number = r.u64_le()?;
        let time = r.u64_le()?;
        let held_sats = r.u64_le()?;
        let limit_sats = r.u64_le()?;
        let stop = r.flag()?;
        let n = usize::from(r.u8()?);
        if n > MAX_LINE_PAYMENTS {
            return Err(DecodeError::TooLarge);
        }
        let mut payments: Vec<LinePayment> = Vec::with_capacity(n);
        for _ in 0..n {
            let txid: [u8; 32] = r.array()?;
            if payments.iter().any(|p| p.txid == txid) {
                return Err(DecodeError::NonCanonical("a payment is listed twice"));
            }
            payments.push(LinePayment {
                txid,
                sats: r.u64_le()?,
                time: r.u64_le()?,
                by_owner: r.flag()?,
            });
        }
        let sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            number,
            time,
            held_sats,
            limit_sats,
            stop,
            payments,
            sig,
        })
    }
}

#[cfg(test)]
mod tests;
