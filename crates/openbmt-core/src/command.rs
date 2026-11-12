//! Owner-signed commands.
//!
//! These are the owner's actions that the counter must not be able to do by
//! itself (DESIGN §2.9, §6): clearing the alarm a cancel raised, ending a
//! pause that waits for the owner, and releasing one payment's exposure from
//! the ledger. The owner phone signs the command; the counter checks it
//! against the owner root it pinned at pairing. The Key app's root has no part
//! in it.
//!
//! The sequence number stops an old command from being played again. The
//! counter stores the last number it accepted and passes it to
//! [`SignedCommand::verify`], which refuses any command that is not higher.
//! Storing the new number once the command has been carried out is the
//! caller's job.

use std::fmt;

use crate::batch::PinnedRoots;
use crate::identity::{DeviceId, Identity, Sig, TAG_COMMAND};
use crate::policy::PaymentId;
use crate::wire::{self, DecodeError, Reader};

const TAG_CLEAR_CANCEL_ALARM: u8 = 1;
const TAG_RESET_PAUSE: u8 = 2;
const TAG_RELEASE_EXPOSURE: u8 = 3;

/// What the owner asks the counter to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Clear the cancel alarm.
    ClearCancelAlarm,
    /// End a pause that lasts until the owner resets it.
    ResetPause,
    /// Take one payment out of the exposure ledger.
    ReleaseExposure(PaymentId),
}

/// A command for one counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerCommand {
    /// The counter this command is for.
    pub counter: DeviceId,
    /// Must be higher than the last number the counter accepted.
    pub seq: u64,
    /// What to do.
    pub action: Action,
}

/// Why a command was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// The command names a different counter.
    WrongCounter,
    /// The number is not above the last one accepted: an old command played
    /// again, or one that was already carried out.
    Stale { last: u64, got: u64 },
    /// The signature is not the owner root's over this command.
    OwnerSignatureInvalid,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongCounter => write!(f, "command is for another counter"),
            Self::Stale { last, got } => {
                write!(f, "command number {got} is not above the last one, {last}")
            }
            Self::OwnerSignatureInvalid => write!(f, "command is not signed by the owner"),
        }
    }
}

impl std::error::Error for CommandError {}

impl OwnerCommand {
    /// Canonical bytes: the counter key, the number as u64 LE, the action tag
    /// (1 clear alarm, 2 reset pause, 3 release exposure) and, for a release,
    /// the 32-byte payment id. This is what the owner signs.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(73);
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        match &self.action {
            Action::ClearCancelAlarm => out.push(TAG_CLEAR_CANCEL_ALARM),
            Action::ResetPause => out.push(TAG_RESET_PAUSE),
            Action::ReleaseExposure(id) => {
                out.push(TAG_RELEASE_EXPOSURE);
                out.extend_from_slice(&id.0);
            }
        }
        out
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let counter = r.device_id()?;
        let seq = r.u64_le()?;
        let action = match r.u8()? {
            TAG_CLEAR_CANCEL_ALARM => Action::ClearCancelAlarm,
            TAG_RESET_PAUSE => Action::ResetPause,
            TAG_RELEASE_EXPOSURE => Action::ReleaseExposure(PaymentId(r.array::<32>()?)),
            _ => return Err(DecodeError::NonCanonical("unknown action tag")),
        };
        Ok(Self {
            counter,
            seq,
            action,
        })
    }

    /// Sign with the owner root's identity.
    pub fn sign(&self, owner: &Identity) -> Sig {
        owner.sign_tagged(TAG_COMMAND, &self.encode())
    }
}

/// A command and the owner's signature on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedCommand {
    /// The command.
    pub command: OwnerCommand,
    /// The owner root's signature over [`OwnerCommand::encode`].
    pub owner_sig: Sig,
}

impl SignedCommand {
    /// Check the command is for `counter`, is numbered above `last_seq`, and
    /// is signed by the owner root. Checked in that order.
    pub fn verify(
        &self,
        roots: &PinnedRoots,
        counter: &DeviceId,
        last_seq: u64,
    ) -> Result<(), CommandError> {
        self.verify_owner(&roots.owner, counter, last_seq)
    }

    /// [`SignedCommand::verify`] against the owner root alone, for a counter
    /// that was paired with the shop's own wallet and pinned no Key app.
    pub fn verify_owner(
        &self,
        owner: &DeviceId,
        counter: &DeviceId,
        last_seq: u64,
    ) -> Result<(), CommandError> {
        if self.command.counter != *counter {
            return Err(CommandError::WrongCounter);
        }
        if self.command.seq <= last_seq {
            return Err(CommandError::Stale {
                last: last_seq,
                got: self.command.seq,
            });
        }
        if !owner.verify_tagged(TAG_COMMAND, &self.command.encode(), &self.owner_sig) {
            return Err(CommandError::OwnerSignatureInvalid);
        }
        Ok(())
    }

    /// Wire form: `OBMX`, version 1, the bytes of [`OwnerCommand::encode`],
    /// then the signature.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.command.encode();
        let mut out = wire::start(&wire::MAGIC_OWNER_COMMAND, body.len() + 64);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.owner_sig.0);
        out
    }

    /// Parse the wire form strictly (see [`crate::wire`]). An unknown action
    /// tag is `NonCanonical`. This does not check the signature: call
    /// [`SignedCommand::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_OWNER_COMMAND)?;
        let command = OwnerCommand::read(&mut r)?;
        let owner_sig = r.sig()?;
        r.finish()?;
        Ok(Self { command, owner_sig })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::identity;

    fn roots() -> PinnedRoots {
        PinnedRoots::new(identity(1).device_id(), identity(2).device_id()).unwrap()
    }

    fn counter() -> DeviceId {
        identity(10).device_id()
    }

    fn actions() -> [Action; 3] {
        [
            Action::ClearCancelAlarm,
            Action::ResetPause,
            Action::ReleaseExposure(PaymentId([7; 32])),
        ]
    }

    fn command(seq: u64, action: Action) -> OwnerCommand {
        OwnerCommand {
            counter: counter(),
            seq,
            action,
        }
    }

    fn signed(seq: u64, action: Action) -> SignedCommand {
        let command = command(seq, action);
        SignedCommand {
            owner_sig: command.sign(&identity(1)),
            command,
        }
    }

    #[test]
    fn each_action_round_trips_and_verifies() {
        for action in actions() {
            let s = signed(5, action);
            assert_eq!(s.verify(&roots(), &counter(), 4), Ok(()));
            let bytes = s.to_bytes();
            assert_eq!(&bytes[..5], b"OBMX\x01");
            let back = SignedCommand::from_bytes(&bytes).unwrap();
            assert_eq!(back, s);
            assert_eq!(back.to_bytes(), bytes);
        }
    }

    #[test]
    fn encoding_is_fixed_and_tags_are_one_two_three() {
        let c = counter().to_bytes();
        let mut want = c.to_vec();
        want.extend_from_slice(&9u64.to_le_bytes());
        want.push(1);
        assert_eq!(command(9, Action::ClearCancelAlarm).encode(), want);
        *want.last_mut().unwrap() = 2;
        assert_eq!(command(9, Action::ResetPause).encode(), want);
        *want.last_mut().unwrap() = 3;
        want.extend_from_slice(&[7; 32]);
        assert_eq!(
            command(9, Action::ReleaseExposure(PaymentId([7; 32]))).encode(),
            want
        );
    }

    #[test]
    fn a_command_for_another_counter_is_refused() {
        let s = signed(5, Action::ResetPause);
        assert_eq!(
            s.verify(&roots(), &identity(11).device_id(), 0),
            Err(CommandError::WrongCounter)
        );
    }

    #[test]
    fn the_number_must_be_above_the_last_one() {
        let s = signed(5, Action::ResetPause);
        assert_eq!(
            s.verify(&roots(), &counter(), 5),
            Err(CommandError::Stale { last: 5, got: 5 })
        );
        assert_eq!(
            s.verify(&roots(), &counter(), 9),
            Err(CommandError::Stale { last: 9, got: 5 })
        );
        assert_eq!(s.verify(&roots(), &counter(), 4), Ok(()));
        let zero = signed(0, Action::ResetPause);
        assert_eq!(
            zero.verify(&roots(), &counter(), 0),
            Err(CommandError::Stale { last: 0, got: 0 })
        );
    }

    #[test]
    fn only_the_owner_root_signs() {
        let c = command(5, Action::ClearCancelAlarm);
        for who in [2u8, 3, 10] {
            let s = SignedCommand {
                command: c,
                owner_sig: c.sign(&identity(who)),
            };
            assert_eq!(
                s.verify(&roots(), &counter(), 0),
                Err(CommandError::OwnerSignatureInvalid),
                "signer {who}"
            );
        }
    }

    #[test]
    fn a_signature_for_another_command_or_tag_is_refused() {
        let a = command(5, Action::ClearCancelAlarm);
        let b = command(6, Action::ClearCancelAlarm);
        let s = SignedCommand {
            command: b,
            owner_sig: a.sign(&identity(1)),
        };
        assert_eq!(
            s.verify(&roots(), &counter(), 0),
            Err(CommandError::OwnerSignatureInvalid)
        );
        let other_tag = SignedCommand {
            command: a,
            owner_sig: identity(1).sign_tagged(crate::identity::TAG_SETTINGS, &a.encode()),
        };
        assert_eq!(
            other_tag.verify(&roots(), &counter(), 0),
            Err(CommandError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn checks_run_in_the_documented_order() {
        // wrong counter, stale number and a bad signature all at once
        let mut s = signed(5, Action::ResetPause);
        s.owner_sig = Sig([0; 64]);
        let other = identity(11).device_id();
        assert_eq!(
            s.verify(&roots(), &other, 9),
            Err(CommandError::WrongCounter)
        );
        assert_eq!(
            s.verify(&roots(), &counter(), 9),
            Err(CommandError::Stale { last: 9, got: 5 })
        );
        assert_eq!(
            s.verify(&roots(), &counter(), 1),
            Err(CommandError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn truncated_input_is_refused_at_every_length() {
        for action in actions() {
            let bytes = signed(5, action).to_bytes();
            for n in 0..bytes.len() {
                assert!(SignedCommand::from_bytes(&bytes[..n]).is_err(), "{n}");
            }
        }
    }

    #[test]
    fn trailing_bytes_are_refused() {
        for action in actions() {
            let mut bytes = signed(5, action).to_bytes();
            bytes.push(0);
            assert_eq!(
                SignedCommand::from_bytes(&bytes),
                Err(DecodeError::TrailingBytes)
            );
        }
    }

    #[test]
    fn bad_magic_and_version_are_refused() {
        let bytes = signed(5, Action::ResetPause).to_bytes();
        let mut m = bytes.clone();
        m[0] = b'Z';
        assert_eq!(SignedCommand::from_bytes(&m), Err(DecodeError::BadMagic));
        for v in [0u8, 2, 255] {
            let mut m = bytes.clone();
            m[4] = v;
            assert_eq!(
                SignedCommand::from_bytes(&m),
                Err(DecodeError::UnsupportedVersion(v))
            );
        }
    }

    #[test]
    fn an_unknown_action_tag_is_refused() {
        let bytes = signed(5, Action::ResetPause).to_bytes();
        // magic and version (5), counter (32), number (8), then the tag
        let tag_at = 5 + 32 + 8;
        assert_eq!(bytes[tag_at], 2);
        for bad in [0u8, 4, 255] {
            let mut m = bytes.clone();
            m[tag_at] = bad;
            assert!(matches!(
                SignedCommand::from_bytes(&m),
                Err(DecodeError::NonCanonical(_)) | Err(DecodeError::Truncated)
            ));
        }
    }

    #[test]
    fn a_counter_that_is_not_a_key_is_refused() {
        let mut bytes = signed(5, Action::ResetPause).to_bytes();
        bytes[5..37].fill(0);
        assert_eq!(
            SignedCommand::from_bytes(&bytes),
            Err(DecodeError::InvalidKey)
        );
    }

    #[test]
    fn no_single_flipped_byte_gives_a_command_that_verifies() {
        for action in actions() {
            let bytes = signed(5, action).to_bytes();
            assert_eq!(
                SignedCommand::from_bytes(&bytes)
                    .unwrap()
                    .verify(&roots(), &counter(), 4),
                Ok(())
            );
            for i in 0..bytes.len() {
                for bit in 0..8 {
                    let mut m = bytes.clone();
                    m[i] ^= 1 << bit;
                    if let Ok(s) = SignedCommand::from_bytes(&m) {
                        assert!(
                            s.verify(&roots(), &counter(), 4).is_err(),
                            "byte {i} bit {bit}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn error_text_is_plain() {
        assert_eq!(
            CommandError::Stale { last: 3, got: 2 }.to_string(),
            "command number 2 is not above the last one, 3"
        );
        assert_eq!(
            CommandError::WrongCounter.to_string(),
            "command is for another counter"
        );
        assert_eq!(
            CommandError::OwnerSignatureInvalid.to_string(),
            "command is not signed by the owner"
        );
    }
}
