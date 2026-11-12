//! The owner's watcher heartbeat.
//!
//! Instant acceptance needs a second watcher that does not share the
//! counter's network view or its process (DESIGN §5.1, §5.2). The owner's
//! device runs one and tells the counter, every so often, that it is alive,
//! what time it is there, which chain tip it sees and whether its own check
//! is healthy. The owner root signs that message; the counter checks it
//! against the owner root it pinned at pairing. The Key app's root has no
//! part in it, and neither has the Box.
//!
//! This module only encodes, signs and checks the message. Whether the tip it
//! names agrees with the counter's own header chain, and how old it is, are
//! judged by the counter (`docs/COUNTER-RULES.md`, CR-50 to CR-56).
//!
//! The owner phone may also run a watcher while its owner app is closed. That
//! watcher holds no owner key: it signs with a watch key of its own, whose
//! public key the signed settings name (`Settings::watch_key`), under a
//! tag of its own ([`TAG_WATCH_HEARTBEAT`]). The frame is the same; the
//! counter tries the owner root and then the named watch key.
//!
//! The sequence number stops an old heartbeat from being played again. The
//! counter stores the last number it took and passes it to
//! [`SignedHeartbeat::verify`], which refuses any heartbeat that is not
//! higher. Storing the new number is the caller's job.
//!
//! A heartbeat is a claim by the same software family on another device. It
//! guards against a dead or cut-off owner watcher, not against a bad build
//! of the app (DESIGN §5.2).

use std::fmt;

use crate::batch::PinnedRoots;
use crate::identity::{DeviceId, Identity, Sig, TAG_HEARTBEAT, TAG_WATCH_HEARTBEAT};
use crate::wire::{self, DecodeError, Reader};

/// What the owner's watcher says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatcherHeartbeat {
    /// The counter this heartbeat is for.
    pub counter: DeviceId,
    /// Must be higher than the last number the counter took.
    pub seq: u64,
    /// The owner device's clock when it signed, in seconds.
    pub time: u64,
    /// Height of the tip the owner's watcher sees.
    pub tip_height: u32,
    /// Hash of that tip, as the 32 bytes of the block hash in memory order.
    pub tip_hash: [u8; 32],
    /// The owner's own health check says it is working.
    pub healthy: bool,
}

/// Why a heartbeat was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatError {
    /// The heartbeat names a different counter.
    WrongCounter,
    /// The number is not above the last one taken: an old heartbeat played
    /// again, or one that was already taken.
    Stale { last: u64, got: u64 },
    /// The signature is neither the owner root's nor the watch key's named
    /// in the settings in force, over this heartbeat.
    OwnerSignatureInvalid,
}

impl fmt::Display for HeartbeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongCounter => write!(f, "heartbeat is for another counter"),
            Self::Stale { last, got } => {
                write!(
                    f,
                    "heartbeat number {got} is not above the last one, {last}"
                )
            }
            Self::OwnerSignatureInvalid => write!(f, "heartbeat is not signed by the owner"),
        }
    }
}

impl std::error::Error for HeartbeatError {}

impl WatcherHeartbeat {
    /// Canonical bytes: the counter key, the number as u64 LE, the time as
    /// u64 LE, the tip height as u32 LE, the 32-byte tip hash and the health
    /// flag (0 or 1). This is what the owner signs.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(85);
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.time.to_le_bytes());
        out.extend_from_slice(&self.tip_height.to_le_bytes());
        out.extend_from_slice(&self.tip_hash);
        out.push(u8::from(self.healthy));
        out
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            counter: r.device_id()?,
            seq: r.u64_le()?,
            time: r.u64_le()?,
            tip_height: r.u32_le()?,
            tip_hash: r.array::<32>()?,
            healthy: r.flag()?,
        })
    }

    /// Sign with the owner root's identity.
    pub fn sign(&self, owner: &Identity) -> SignedHeartbeat {
        SignedHeartbeat {
            owner_sig: owner.sign_tagged(TAG_HEARTBEAT, &self.encode()),
            heartbeat: *self,
        }
    }

    /// Sign with the shop's watch key, under the tag of its own.
    pub fn sign_watch(&self, watch: &Identity) -> SignedHeartbeat {
        SignedHeartbeat {
            owner_sig: watch.sign_tagged(TAG_WATCH_HEARTBEAT, &self.encode()),
            heartbeat: *self,
        }
    }
}

/// A heartbeat and the owner's signature on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedHeartbeat {
    /// The heartbeat.
    pub heartbeat: WatcherHeartbeat,
    /// The signature over [`WatcherHeartbeat::encode`]: the owner root's, or
    /// the watch key's under its own tag. Which one is decided by checking.
    pub owner_sig: Sig,
}

impl SignedHeartbeat {
    /// Check the heartbeat is for `counter`, is numbered above `last_seq`,
    /// and is signed by the owner root or by `watch`, the watch key the
    /// settings in force name, if any. Checked in that order. On success
    /// the heartbeat is handed back.
    pub fn verify(
        &self,
        roots: &PinnedRoots,
        watch: Option<&DeviceId>,
        counter: &DeviceId,
        last_seq: u64,
    ) -> Result<&WatcherHeartbeat, HeartbeatError> {
        self.verify_owner(&roots.owner, watch, counter, last_seq)
    }

    /// [`SignedHeartbeat::verify`] against the owner root alone, for a
    /// counter that was paired with the shop's own wallet and pinned no Key
    /// app.
    pub fn verify_owner(
        &self,
        owner: &DeviceId,
        watch: Option<&DeviceId>,
        counter: &DeviceId,
        last_seq: u64,
    ) -> Result<&WatcherHeartbeat, HeartbeatError> {
        if self.heartbeat.counter != *counter {
            return Err(HeartbeatError::WrongCounter);
        }
        if self.heartbeat.seq <= last_seq {
            return Err(HeartbeatError::Stale {
                last: last_seq,
                got: self.heartbeat.seq,
            });
        }
        let body = self.heartbeat.encode();
        let by_owner = owner.verify_tagged(TAG_HEARTBEAT, &body, &self.owner_sig);
        let by_watch =
            watch.is_some_and(|w| w.verify_tagged(TAG_WATCH_HEARTBEAT, &body, &self.owner_sig));
        if !by_owner && !by_watch {
            return Err(HeartbeatError::OwnerSignatureInvalid);
        }
        Ok(&self.heartbeat)
    }

    /// Wire form: `OBMT`, version 1, the bytes of
    /// [`WatcherHeartbeat::encode`], then the signature. 154 bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.heartbeat.encode();
        let mut out = wire::start(&wire::MAGIC_HEARTBEAT, body.len() + 64);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.owner_sig.0);
        out
    }

    /// Parse the wire form strictly (see [`crate::wire`]). The health flag
    /// must be 0 or 1. This does not check the signature: call
    /// [`SignedHeartbeat::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_HEARTBEAT)?;
        let heartbeat = WatcherHeartbeat::read(&mut r)?;
        let owner_sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            heartbeat,
            owner_sig,
        })
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

    fn heartbeat(seq: u64) -> WatcherHeartbeat {
        WatcherHeartbeat {
            counter: counter(),
            seq,
            time: 1_700_000_000,
            tip_height: 150,
            tip_hash: [7; 32],
            healthy: true,
        }
    }

    fn signed(seq: u64) -> SignedHeartbeat {
        heartbeat(seq).sign(&identity(1))
    }

    #[test]
    fn a_heartbeat_round_trips_and_verifies() {
        for healthy in [true, false] {
            let hb = WatcherHeartbeat {
                healthy,
                ..heartbeat(5)
            };
            let s = hb.sign(&identity(1));
            assert_eq!(s.verify(&roots(), None, &counter(), 4), Ok(&hb));
            let bytes = s.to_bytes();
            assert_eq!(&bytes[..5], b"OBMT\x01");
            assert_eq!(bytes.len(), 154);
            let back = SignedHeartbeat::from_bytes(&bytes).unwrap();
            assert_eq!(back, s);
            assert_eq!(back.to_bytes(), bytes);
        }
    }

    #[test]
    fn encoding_is_fixed() {
        let mut want = counter().to_bytes().to_vec();
        want.extend_from_slice(&9u64.to_le_bytes());
        want.extend_from_slice(&1_700_000_000u64.to_le_bytes());
        want.extend_from_slice(&150u32.to_le_bytes());
        want.extend_from_slice(&[7; 32]);
        want.push(1);
        assert_eq!(heartbeat(9).encode(), want);
        assert_eq!(want.len(), 85);
    }

    #[test]
    fn a_heartbeat_for_another_counter_is_refused() {
        assert_eq!(
            signed(5).verify(&roots(), None, &identity(11).device_id(), 0),
            Err(HeartbeatError::WrongCounter)
        );
    }

    #[test]
    fn the_number_must_be_above_the_last_one() {
        let s = signed(5);
        assert_eq!(
            s.verify(&roots(), None, &counter(), 5),
            Err(HeartbeatError::Stale { last: 5, got: 5 })
        );
        assert_eq!(
            s.verify(&roots(), None, &counter(), 9),
            Err(HeartbeatError::Stale { last: 9, got: 5 })
        );
        assert!(s.verify(&roots(), None, &counter(), 4).is_ok());
        assert_eq!(
            signed(0).verify(&roots(), None, &counter(), 0),
            Err(HeartbeatError::Stale { last: 0, got: 0 })
        );
    }

    #[test]
    fn only_the_owner_root_signs() {
        let hb = heartbeat(5);
        for who in [2u8, 3, 10] {
            let s = hb.sign(&identity(who));
            assert_eq!(
                s.verify(&roots(), None, &counter(), 0),
                Err(HeartbeatError::OwnerSignatureInvalid),
                "signer {who}"
            );
        }
    }

    #[test]
    fn a_signature_for_another_heartbeat_or_tag_is_refused() {
        let a = heartbeat(5);
        let s = SignedHeartbeat {
            heartbeat: heartbeat(6),
            owner_sig: a.sign(&identity(1)).owner_sig,
        };
        assert_eq!(
            s.verify(&roots(), None, &counter(), 0),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
        // the command tag over the same bytes is not a heartbeat
        let other_tag = SignedHeartbeat {
            heartbeat: a,
            owner_sig: identity(1).sign_tagged(crate::identity::TAG_COMMAND, &a.encode()),
        };
        assert_eq!(
            other_tag.verify(&roots(), None, &counter(), 0),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn checks_run_in_the_documented_order() {
        let mut s = signed(5);
        s.owner_sig = Sig([0; 64]);
        let other = identity(11).device_id();
        assert_eq!(
            s.verify(&roots(), None, &other, 9),
            Err(HeartbeatError::WrongCounter)
        );
        assert_eq!(
            s.verify(&roots(), None, &counter(), 9),
            Err(HeartbeatError::Stale { last: 9, got: 5 })
        );
        assert_eq!(
            s.verify(&roots(), None, &counter(), 1),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn truncated_input_is_refused_at_every_length() {
        let bytes = signed(5).to_bytes();
        for n in 0..bytes.len() {
            assert!(SignedHeartbeat::from_bytes(&bytes[..n]).is_err(), "{n}");
        }
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = signed(5).to_bytes();
        bytes.push(0);
        assert_eq!(
            SignedHeartbeat::from_bytes(&bytes),
            Err(DecodeError::TrailingBytes)
        );
    }

    #[test]
    fn bad_magic_and_version_are_refused() {
        let bytes = signed(5).to_bytes();
        let mut m = bytes.clone();
        m[0] = b'Z';
        assert_eq!(SignedHeartbeat::from_bytes(&m), Err(DecodeError::BadMagic));
        for v in [0u8, 2, 255] {
            let mut m = bytes.clone();
            m[4] = v;
            assert_eq!(
                SignedHeartbeat::from_bytes(&m),
                Err(DecodeError::UnsupportedVersion(v))
            );
        }
        // a command is not a heartbeat
        let mut m = bytes;
        m[3] = b'X';
        assert_eq!(SignedHeartbeat::from_bytes(&m), Err(DecodeError::BadMagic));
    }

    #[test]
    fn the_health_flag_is_zero_or_one() {
        let bytes = signed(5).to_bytes();
        // magic and version (5), counter (32), number, time (16), height (4),
        // tip hash (32), then the flag
        let flag_at = 5 + 32 + 8 + 8 + 4 + 32;
        assert_eq!(bytes[flag_at], 1);
        for bad in [2u8, 3, 255] {
            let mut m = bytes.clone();
            m[flag_at] = bad;
            assert!(matches!(
                SignedHeartbeat::from_bytes(&m),
                Err(DecodeError::NonCanonical(_))
            ));
        }
    }

    #[test]
    fn a_counter_that_is_not_a_key_is_refused() {
        let mut bytes = signed(5).to_bytes();
        bytes[5..37].fill(0);
        assert_eq!(
            SignedHeartbeat::from_bytes(&bytes),
            Err(DecodeError::InvalidKey)
        );
    }

    #[test]
    fn no_single_flipped_byte_gives_a_heartbeat_that_verifies() {
        let bytes = signed(5).to_bytes();
        assert!(SignedHeartbeat::from_bytes(&bytes)
            .unwrap()
            .verify(&roots(), None, &counter(), 4)
            .is_ok());
        for i in 0..bytes.len() {
            for bit in 0..8 {
                let mut m = bytes.clone();
                m[i] ^= 1 << bit;
                if let Ok(s) = SignedHeartbeat::from_bytes(&m) {
                    assert!(
                        s.verify(&roots(), None, &counter(), 4).is_err(),
                        "byte {i} bit {bit}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_watch_key_heartbeat_counts_only_when_the_settings_name_the_key() {
        let watch = identity(20);
        let hb = heartbeat(5);
        let s = hb.sign_watch(&watch);
        let named = watch.device_id();
        assert_eq!(s.verify(&roots(), Some(&named), &counter(), 4), Ok(&hb));
        // the same bytes survive the wire form
        let back = SignedHeartbeat::from_bytes(&s.to_bytes()).unwrap();
        assert_eq!(back.verify(&roots(), Some(&named), &counter(), 4), Ok(&hb));
        // no key named, or another key named: refused
        assert_eq!(
            s.verify(&roots(), None, &counter(), 4),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
        let other = identity(21).device_id();
        assert_eq!(
            s.verify(&roots(), Some(&other), &counter(), 4),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
        // numbers are shared: the owner's and the watch key's use one line
        assert_eq!(
            s.verify(&roots(), Some(&named), &counter(), 5),
            Err(HeartbeatError::Stale { last: 5, got: 5 })
        );
    }

    #[test]
    fn a_watch_signature_is_not_an_owner_signature_and_the_reverse() {
        let watch = identity(20);
        let named = watch.device_id();
        let hb = heartbeat(5);
        // the watch key signing under the owner's heartbeat tag
        let wrong_tag = SignedHeartbeat {
            heartbeat: hb,
            owner_sig: watch.sign_tagged(TAG_HEARTBEAT, &hb.encode()),
        };
        assert_eq!(
            wrong_tag.verify(&roots(), Some(&named), &counter(), 0),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
        // the owner root signing under the watch tag
        let owner_as_watch = SignedHeartbeat {
            heartbeat: hb,
            owner_sig: identity(1).sign_tagged(TAG_WATCH_HEARTBEAT, &hb.encode()),
        };
        assert_eq!(
            owner_as_watch.verify(&roots(), Some(&named), &counter(), 0),
            Err(HeartbeatError::OwnerSignatureInvalid)
        );
        // the tags differ
        assert_ne!(TAG_HEARTBEAT, TAG_WATCH_HEARTBEAT);
    }

    #[test]
    fn error_text_is_plain() {
        assert_eq!(
            HeartbeatError::Stale { last: 3, got: 2 }.to_string(),
            "heartbeat number 2 is not above the last one, 3"
        );
        assert_eq!(
            HeartbeatError::WrongCounter.to_string(),
            "heartbeat is for another counter"
        );
        assert_eq!(
            HeartbeatError::OwnerSignatureInvalid.to_string(),
            "heartbeat is not signed by the owner"
        );
    }
}
