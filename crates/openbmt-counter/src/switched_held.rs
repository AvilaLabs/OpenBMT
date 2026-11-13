//! The record that tells the owner a counter switched from the starter
//! wallet, or refused to (CR-262).
//!
//! It is a `Setting` record of the counter's own chain, like the
//! confirmation of CR-241. The body is one tag byte (37), one byte (1 for
//! switched, 0 for refused at "Different"), the hash of the new pinned wallet
//! (32 bytes) and the counter's signature with its identity key over the tag,
//! the byte and the hash under `OpenBMT/switched-own/v1` (64 bytes): 98 bytes.
//! The decoder is strict, and re-encoding a decoded body gives the input.
//! A body of 98 bytes is read as this kind only (CR-243).

use openbmt_core::identity::{DeviceId, Identity, Sig, TAG_SWITCHED_OWN};
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};

const TAG_SWITCHED_HELD: u8 = 37;

/// The length of the body, in bytes.
pub const SWITCHED_HELD_LEN: usize = 1 + 1 + 32 + 64;

/// What the counter writes when the manager answers a switch (CR-260,
/// CR-261, CR-262).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchedHeld {
    /// True when the counter switched, false when the manager said
    /// "Different".
    pub switched: bool,
    /// The hash of the new pinned wallet the switch named.
    pub wallet: [u8; 32],
    /// The counter's signature over the tag, the byte and the hash.
    pub sig: Sig,
}

fn message(switched: bool, wallet: &[u8; 32]) -> [u8; 34] {
    let mut msg = [0u8; 34];
    msg[0] = TAG_SWITCHED_HELD;
    msg[1] = u8::from(switched);
    msg[2..].copy_from_slice(wallet);
    msg
}

impl SwitchedHeld {
    /// The record the counter `identity` writes.
    pub fn sign(identity: &Identity, switched: bool, wallet: [u8; 32]) -> Self {
        Self {
            switched,
            wallet,
            sig: identity.sign_tagged(TAG_SWITCHED_OWN, &message(switched, &wallet)),
        }
    }

    /// Whether `counter` signed this record.
    pub fn verify(&self, counter: &DeviceId) -> bool {
        counter.verify_tagged(
            TAG_SWITCHED_OWN,
            &message(self.switched, &self.wallet),
            &self.sig,
        )
    }

    /// The kind of record it is written as: `Setting` (CR-262).
    pub fn kind(&self) -> RecordKind {
        RecordKind::Setting
    }

    /// The record body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SWITCHED_HELD_LEN);
        out.extend_from_slice(&message(self.switched, &self.wallet));
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Read a record body strictly (CR-262): the tag 37, a byte of 0 or 1,
    /// the hash, the signature and no byte more.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        if r.u8()? != TAG_SWITCHED_HELD {
            return Err(DecodeError::NonCanonical(
                "a switched record's tag is not 37",
            ));
        }
        let switched = r.flag()?;
        let wallet = r.array::<32>()?;
        let sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            switched,
            wallet,
            sig,
        })
    }
}
