//! The record that carries a counter's confirmation of a change to the
//! shop's settings (CR-241, CR-243).
//!
//! When the manager's PIN was right, the counter signs the confirmation of
//! AP-04 for the hash of the settings with its identity key and writes it as
//! a `Setting` record of its own chain. The owner phone reads it from the
//! counter's records, as it reads every record; nothing else carries it. The
//! body is one tag byte (36; 34 is a held batch and 35 a held range), the
//! settings hash (32 bytes) and the signature (64 bytes): 97 bytes. The
//! decoder is strict, and re-encoding a decoded body gives the input.

use openbmt_core::approval::CounterConfirmation;
use openbmt_core::identity::{DeviceId, Sig};
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};

const TAG_CONFIRM_HELD: u8 = 36;

/// The length of the body, in bytes.
pub const CONFIRM_HELD_LEN: usize = 1 + 32 + 64;

/// What the counter writes when the manager confirms a change (CR-241).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmHeld {
    /// [`openbmt_core::settings::Settings::hash`] of the settings confirmed.
    pub settings_hash: [u8; 32],
    /// The counter's signature of AP-04 over the hash and its own id.
    pub sig: Sig,
}

impl ConfirmHeld {
    /// The kind of record it is written as: `Setting` (CR-241).
    pub fn kind(&self) -> RecordKind {
        RecordKind::Setting
    }

    /// The confirmation this record holds, for the counter that wrote it.
    pub fn confirmation(&self, counter: DeviceId) -> CounterConfirmation {
        CounterConfirmation {
            counter,
            sig: self.sig,
        }
    }

    /// The record body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(CONFIRM_HELD_LEN);
        out.push(TAG_CONFIRM_HELD);
        out.extend_from_slice(&self.settings_hash);
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Read a record body strictly (CR-243): the tag 36, the hash, the
    /// signature and no byte more.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        if r.u8()? != TAG_CONFIRM_HELD {
            return Err(DecodeError::NonCanonical(
                "a confirmation record's tag is not 36",
            ));
        }
        let settings_hash = r.array::<32>()?;
        let sig = r.sig()?;
        r.finish()?;
        Ok(Self { settings_hash, sig })
    }
}
