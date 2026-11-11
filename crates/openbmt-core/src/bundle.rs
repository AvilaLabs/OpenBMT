//! What the owner phone hands a counter to finish pairing (step 4 of
//! [`crate::pairing`]): the pairing the two roots signed, the settings that
//! add the counter, the first batch of addresses if there is one, and some
//! peers to try.
//!
//! The bundle is a carrier. Nothing in it changes what money may do except
//! through a signature the counter checks against the roots it fixed itself:
//!
//! - the network and the wallet come only from the offer the counter scanned;
//! - the smallest batch the counter takes is the counter's own rule;
//! - the peers are hints from a device the counter does not trust. They only
//!   choose whom to ask first: what a peer says is checked as for any peer.
//!
//! Wire form (strict, see [`crate::wire`]):
//!
//! ```text
//! "OBMs" | 1 or 2
//!   | pairing len u16 | signed pairing ("OBMD")
//!   | settings len u32 | signed settings ("OBMS")
//!   | batch len u32 (0 for none) | signed batch ("OBMB")
//!   | peer count u8 | per peer: len u8 | "host:port" (ascending, no repeats)
//!   | version 2 only: relay hint ([`crate::relay`])
//! ```
//!
//! Version 2 is the version with a relay (CR-130): a bundle with no relay is
//! written as version 1, byte for byte as before, and a version 2 bundle
//! always has one. The relay is where the counter reaches the owner through
//! the shop's Box or relay; it adds no authority (`docs/LINK-RULES.md`,
//! LK-09).
//!
//! Decoding only parses. The counter's own checks of the pairing, the
//! settings and the batch are in the counter crate.

use std::fmt;
use std::net::SocketAddr;

use crate::batch::SignedBatch;
use crate::pairing::SignedPairing;
use crate::relay::RelayHint;
use crate::settings::SignedSettings;
use crate::wire::{self, DecodeError, Reader};

/// Most peers a bundle carries.
pub const MAX_PEERS: usize = 8;
/// Longest peer text, in bytes. The longest IPv6 socket address text is 47.
pub const MAX_PEER_BYTES: usize = 64;
/// Longest signed pairing the bundle reads, in bytes (the longest real one
/// is under 400).
const MAX_PAIRING_BYTES: usize = 512;
/// Longest signed settings the bundle reads, in bytes (the longest real one
/// is under 20,000).
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// Longest signed batch the bundle reads, in bytes: the most addresses a
/// batch holds, 34 bytes each, and room for the rest.
const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;

/// A u32 length that must be at most `max`. A length above it is `TooLarge`
/// before any bytes are taken.
fn len32(r: &mut Reader<'_>, max: usize) -> Result<usize, DecodeError> {
    let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
    if n > max {
        return Err(DecodeError::TooLarge);
    }
    Ok(n)
}

/// Why a bundle could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleError {
    /// More than [`MAX_PEERS`] peers.
    TooManyPeers,
    /// A peer is not `ip:port` in its shortest spelling, or is too long.
    BadPeer,
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPeers => write!(f, "a bundle carries at most {MAX_PEERS} peers"),
            Self::BadPeer => write!(f, "a peer is not an address and a port"),
        }
    }
}

impl std::error::Error for BundleError {}

/// Whether `text` is a peer a bundle may carry: an IP address and a port in
/// the spelling Rust prints it (`1.2.3.4:8333`, `[::1]:8333`), with a port
/// above 0, at most
/// [`MAX_PEER_BYTES`] bytes. Names are not carried: looking one up would go
/// to a resolver, which a counter behind Tor must not do.
pub fn peer_is_valid(text: &str) -> bool {
    text.len() <= MAX_PEER_BYTES
        && text
            .parse::<SocketAddr>()
            .is_ok_and(|addr| addr.port() != 0 && addr.to_string() == text)
}

/// The pairing, the settings, the first batch, the peers and the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterBundle {
    /// The pairing both roots signed.
    pub pairing: SignedPairing,
    /// The settings that add the counter.
    pub settings: SignedSettings,
    /// The first batch of addresses, if the owner phone had one.
    pub first_batch: Option<SignedBatch>,
    /// Peers to try first, in ascending order with no repeats.
    pub peers: Vec<String>,
    /// The relay the counter reaches the owner through, if the shop has one
    /// (CR-130).
    pub relay: Option<RelayHint>,
}

impl CounterBundle {
    /// A bundle. The peers are sorted and a repeat is dropped; each must
    /// pass [`peer_is_valid`], and there may be at most [`MAX_PEERS`].
    pub fn new(
        pairing: SignedPairing,
        settings: SignedSettings,
        first_batch: Option<SignedBatch>,
        mut peers: Vec<String>,
    ) -> Result<Self, BundleError> {
        peers.sort();
        peers.dedup();
        if peers.len() > MAX_PEERS {
            return Err(BundleError::TooManyPeers);
        }
        if !peers.iter().all(|p| peer_is_valid(p)) {
            return Err(BundleError::BadPeer);
        }
        Ok(Self {
            pairing,
            settings,
            first_batch,
            peers,
            relay: None,
        })
    }

    /// The same bundle with this relay (or none).
    pub fn with_relay(mut self, relay: Option<RelayHint>) -> Self {
        self.relay = relay;
        self
    }

    /// Wire form: see the module docs. Call it on a bundle that came from
    /// [`CounterBundle::new`] or from decoding; the parts must be as their
    /// own encoders need them.
    pub fn to_bytes(&self) -> Vec<u8> {
        let pairing = self.pairing.to_bytes();
        let settings = self.settings.to_bytes();
        let batch = self
            .first_batch
            .as_ref()
            .map(SignedBatch::to_bytes)
            .unwrap_or_default();
        let mut out = wire::start(
            &wire::MAGIC_COUNTER_BUNDLE,
            pairing.len() + settings.len() + batch.len() + 16 + self.peers.len() * 24 + 100,
        );
        if self.relay.is_some() {
            if let Some(version) = out.get_mut(4) {
                *version = 2;
            }
        }
        out.extend_from_slice(
            &u16::try_from(pairing.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&pairing);
        out.extend_from_slice(&wire::len_u32(settings.len()).to_le_bytes());
        out.extend_from_slice(&settings);
        out.extend_from_slice(&wire::len_u32(batch.len()).to_le_bytes());
        out.extend_from_slice(&batch);
        out.push(u8::try_from(self.peers.len()).unwrap_or(u8::MAX));
        for p in &self.peers {
            out.push(u8::try_from(p.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(p.as_bytes());
        }
        if let Some(relay) = &self.relay {
            relay.put(&mut out);
        }
        out
    }

    /// Parse the wire form strictly. Every length is checked against its
    /// limit and against the bytes present before anything is read; each
    /// part is decoded by its own strict decoder; peers must be valid, in
    /// ascending order and without repeats. Version 1 has no relay and
    /// version 2 has one at the end.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (mut r, version) = Reader::framed_up_to(bytes, &wire::MAGIC_COUNTER_BUNDLE, 2)?;
        let n = usize::from(r.u16_le()?);
        if n > MAX_PAIRING_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let pairing = SignedPairing::from_bytes(r.take(n)?)?;
        let n = len32(&mut r, MAX_SETTINGS_BYTES)?;
        let settings = SignedSettings::from_bytes(r.take(n)?)?;
        let n = len32(&mut r, MAX_BATCH_BYTES)?;
        let first_batch = if n == 0 {
            None
        } else {
            Some(SignedBatch::from_bytes(r.take(n)?)?)
        };
        let count = usize::from(r.u8()?);
        if count > MAX_PEERS {
            return Err(DecodeError::TooLarge);
        }
        let mut peers: Vec<String> = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::from(r.u8()?);
            let text = std::str::from_utf8(r.take(len)?)
                .map_err(|_| DecodeError::NonCanonical("a peer is not UTF-8"))?;
            if !peer_is_valid(text) {
                return Err(DecodeError::NonCanonical("a peer is not ip:port"));
            }
            if peers.last().is_some_and(|last| last.as_str() >= text) {
                return Err(DecodeError::NonCanonical(
                    "peers are not in ascending order",
                ));
            }
            peers.push(text.to_owned());
        }
        let relay = if version >= 2 {
            Some(RelayHint::read(&mut r)?)
        } else {
            None
        };
        r.finish()?;
        Ok(Self {
            pairing,
            settings,
            first_batch,
            peers,
            relay,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use bitcoin::Network;

    use super::*;
    use crate::batch::{Approver, BatchBuilder, BatchPolicy, HighWater};
    use crate::pairing::{owner_cosign, CounterPairing, PairOffer};
    use crate::relay::RelayRoute;
    use crate::settings::Settings;
    use crate::testutil::{assert_strict_decoding, identity, wallet};

    const SMALL: BatchPolicy = BatchPolicy { min_count: 3 };

    fn signed_pairing() -> SignedPairing {
        let wallet = wallet(Network::Regtest);
        let offer = PairOffer::new(
            identity(1).device_id(),
            identity(2).device_id(),
            wallet.checksum32(),
            Network::Regtest,
        )
        .unwrap();
        let (counter, request) =
            CounterPairing::begin(&offer.to_bytes(), &identity(3), "Front", [9; 32], 100).unwrap();
        let pending = crate::pairing::check_request(&offer, &request).unwrap();
        let (entry, sig) = pending.approve(&identity(2), 1).unwrap();
        let signed = owner_cosign(&offer, entry, sig, &identity(1)).unwrap();
        counter.finish(&signed, true, 110).unwrap();
        signed
    }

    fn signed_settings() -> SignedSettings {
        let mut s = Settings::defaults(1_000);
        s.version = 2;
        let id = identity(3).device_id();
        s.devices.insert(id);
        s.counters.insert(id, 10_000);
        let h = s.hash();
        SignedSettings {
            owner_sig: identity(1).sign_digest(&h),
            approver_sig: Some(identity(2).sign_digest(&h)),
            settings: s,
        }
    }

    fn signed_batch() -> SignedBatch {
        let wallet = wallet(Network::Regtest);
        let counter = identity(3).device_id();
        let batch = BatchBuilder::new(SMALL)
            .derive(&wallet, counter, 0, 3)
            .unwrap();
        let approver = Approver::new(identity(2), SMALL);
        let mut hw = HighWater::new();
        let owner_sig = batch.sign(&identity(1));
        let approver_sig = approver
            .check_and_cosign(
                &wallet,
                &batch,
                &mut hw,
                &BTreeSet::from([counter]),
                &identity(1).device_id(),
                &owner_sig,
                |_| Ok::<(), ()>(()),
            )
            .unwrap();
        SignedBatch {
            owner_sig,
            approver_sig,
            batch,
        }
    }

    fn peers() -> Vec<String> {
        vec!["10.0.0.2:8333".to_string(), "[::1]:48333".to_string()]
    }

    fn bundle() -> CounterBundle {
        CounterBundle::new(
            signed_pairing(),
            signed_settings(),
            Some(signed_batch()),
            peers(),
        )
        .unwrap()
    }

    #[test]
    fn round_trip_with_and_without_a_batch_and_peers() {
        let full = bundle();
        let bytes = full.to_bytes();
        assert_eq!(&bytes[..5], b"OBMs\x01");
        let back = CounterBundle::from_bytes(&bytes).unwrap();
        assert_eq!(back, full);
        assert_eq!(back.to_bytes(), bytes);
        let bare = CounterBundle::new(signed_pairing(), signed_settings(), None, vec![]).unwrap();
        let bytes = bare.to_bytes();
        assert_eq!(CounterBundle::from_bytes(&bytes).unwrap(), bare);
        assert_eq!(CounterBundle::from_bytes(&bytes).unwrap().to_bytes(), bytes);
    }

    #[test]
    fn decoding_is_strict() {
        assert_strict_decoding(
            &bundle().to_bytes(),
            CounterBundle::from_bytes,
            CounterBundle::to_bytes,
        );
        let bare = CounterBundle::new(signed_pairing(), signed_settings(), None, vec![]).unwrap();
        assert_strict_decoding(
            &bare.to_bytes(),
            CounterBundle::from_bytes,
            CounterBundle::to_bytes,
        );
    }

    #[test]
    fn a_relay_stays_with_a_bare_bundle_too() {
        let bare = CounterBundle::new(signed_pairing(), signed_settings(), None, vec![])
            .unwrap()
            .with_relay(Some(relay()));
        assert_strict_decoding(
            &bare.to_bytes(),
            CounterBundle::from_bytes,
            CounterBundle::to_bytes,
        );
    }

    #[test]
    fn wrong_magic_and_version_are_refused() {
        let bytes = bundle().to_bytes();
        let mut m = bytes.clone();
        m[..4].copy_from_slice(b"OBMD");
        assert_eq!(
            CounterBundle::from_bytes(&m).unwrap_err(),
            DecodeError::BadMagic
        );
        for version in [0u8, 3, 255] {
            let mut v = bytes.clone();
            v[4] = version;
            assert_eq!(
                CounterBundle::from_bytes(&v).unwrap_err(),
                DecodeError::UnsupportedVersion(version)
            );
        }
    }

    fn relay() -> RelayHint {
        RelayHint::new(
            RelayRoute::Direct("192.168.1.20:48333".parse().unwrap()),
            identity(40).device_id().to_bytes(),
        )
        .unwrap()
    }

    fn onion_relay() -> RelayHint {
        RelayHint::new(
            RelayRoute::Onion {
                name: "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad".to_string(),
                port: 48333,
            },
            identity(40).device_id().to_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn cr_130_a_bundle_with_a_relay_is_version_2_and_reads_back() {
        for hint in [relay(), onion_relay()] {
            let with = bundle().with_relay(Some(hint.clone()));
            let bytes = with.to_bytes();
            assert_eq!(&bytes[..5], b"OBMs\x02");
            let back = CounterBundle::from_bytes(&bytes).unwrap();
            assert_eq!(back.relay, Some(hint));
            assert_eq!(back, with);
            assert_eq!(back.to_bytes(), bytes);
            assert_strict_decoding(&bytes, CounterBundle::from_bytes, CounterBundle::to_bytes);
        }
        // Without a relay the bundle is the bytes of version 1, as before.
        assert_eq!(&bundle().to_bytes()[..5], b"OBMs\x01");
        assert_eq!(bundle().relay, None);
        assert_eq!(
            bundle().with_relay(Some(relay())).with_relay(None).to_bytes(),
            bundle().to_bytes()
        );
    }

    #[test]
    fn cr_130_version_1_reads_with_no_relay_and_version_2_needs_one() {
        let v1 = bundle().to_bytes();
        assert_eq!(CounterBundle::from_bytes(&v1).unwrap().relay, None);
        // Version 2 with nothing after the peers is cut short, and a version
        // 1 with a relay after the peers has bytes left over.
        let mut v2_bare = v1.clone();
        v2_bare[4] = 2;
        assert_eq!(
            CounterBundle::from_bytes(&v2_bare).unwrap_err(),
            DecodeError::Truncated
        );
        let mut v1_with = bundle().with_relay(Some(relay())).to_bytes();
        v1_with[4] = 1;
        assert_eq!(
            CounterBundle::from_bytes(&v1_with).unwrap_err(),
            DecodeError::TrailingBytes
        );
    }

    #[test]
    fn a_part_in_the_wrong_place_is_refused() {
        // The settings where the pairing goes.
        let mut out = wire::start(&wire::MAGIC_COUNTER_BUNDLE, 0);
        let settings = signed_settings().to_bytes();
        out.extend_from_slice(&(settings.len() as u16).to_le_bytes());
        out.extend_from_slice(&settings);
        assert_eq!(
            CounterBundle::from_bytes(&out).unwrap_err(),
            DecodeError::BadMagic
        );
    }

    #[test]
    fn lengths_above_their_limits_are_refused_before_anything_is_read() {
        let mut out = wire::start(&wire::MAGIC_COUNTER_BUNDLE, 0);
        out.extend_from_slice(&513u16.to_le_bytes());
        assert_eq!(
            CounterBundle::from_bytes(&out).unwrap_err(),
            DecodeError::TooLarge
        );
        // The settings length is huge and there are no bytes.
        let pairing = signed_pairing().to_bytes();
        let mut out = wire::start(&wire::MAGIC_COUNTER_BUNDLE, 0);
        out.extend_from_slice(&(pairing.len() as u16).to_le_bytes());
        out.extend_from_slice(&pairing);
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            CounterBundle::from_bytes(&out).unwrap_err(),
            DecodeError::TooLarge
        );
        // A batch length at its limit and past the bytes present.
        let settings = signed_settings().to_bytes();
        let mut out = wire::start(&wire::MAGIC_COUNTER_BUNDLE, 0);
        out.extend_from_slice(&(pairing.len() as u16).to_le_bytes());
        out.extend_from_slice(&pairing);
        out.extend_from_slice(&(settings.len() as u32).to_le_bytes());
        out.extend_from_slice(&settings);
        out.extend_from_slice(&(4u32 * 1024 * 1024).to_le_bytes());
        assert_eq!(
            CounterBundle::from_bytes(&out).unwrap_err(),
            DecodeError::Truncated
        );
        out.truncate(out.len() - 4);
        out.extend_from_slice(&(4u32 * 1024 * 1024 + 1).to_le_bytes());
        assert_eq!(
            CounterBundle::from_bytes(&out).unwrap_err(),
            DecodeError::TooLarge
        );
    }

    /// The bytes of a bundle with no batch and the given peer bytes.
    fn with_peer_bytes(count: u8, rest: &[u8]) -> Vec<u8> {
        let bare = CounterBundle::new(signed_pairing(), signed_settings(), None, vec![]).unwrap();
        let mut bytes = bare.to_bytes();
        bytes.pop();
        bytes.push(count);
        bytes.extend_from_slice(rest);
        bytes
    }

    fn peer_bytes(list: &[&str]) -> Vec<u8> {
        list.iter()
            .flat_map(|p| {
                let mut v = vec![p.len() as u8];
                v.extend_from_slice(p.as_bytes());
                v
            })
            .collect()
    }

    #[test]
    fn peers_must_be_few_valid_ascending_and_without_repeats() {
        let ok = ["1.1.1.1:1", "1.1.1.2:1"];
        assert!(CounterBundle::from_bytes(&with_peer_bytes(2, &peer_bytes(&ok))).is_ok());
        // Out of order and repeated.
        for list in [["1.1.1.2:1", "1.1.1.1:1"], ["1.1.1.1:1", "1.1.1.1:1"]] {
            assert!(matches!(
                CounterBundle::from_bytes(&with_peer_bytes(2, &peer_bytes(&list))).unwrap_err(),
                DecodeError::NonCanonical(_)
            ));
        }
        // Names, a missing port, a port of zero, another spelling, junk.
        for bad in [
            "example.com:8333",
            "1.2.3.4",
            "1.2.3.4:0",
            "[0:0:0:0:0:0:0:1]:8333",
            "01.2.3.4:8333",
            " 1.2.3.4:8333",
            "",
        ] {
            assert!(
                CounterBundle::from_bytes(&with_peer_bytes(1, &peer_bytes(&[bad]))).is_err(),
                "{bad:?}"
            );
        }
        // Not UTF-8.
        assert!(matches!(
            CounterBundle::from_bytes(&with_peer_bytes(1, &[1, 0xff])).unwrap_err(),
            DecodeError::NonCanonical(_)
        ));
        // More than eight.
        let many: Vec<String> = (1..=9).map(|n| format!("10.0.0.{n}:8333")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            CounterBundle::from_bytes(&with_peer_bytes(9, &peer_bytes(&refs))).unwrap_err(),
            DecodeError::TooLarge
        );
        // The count says two and one follows.
        assert_eq!(
            CounterBundle::from_bytes(&with_peer_bytes(2, &peer_bytes(&["1.1.1.1:1"])))
                .unwrap_err(),
            DecodeError::Truncated
        );
    }

    #[test]
    fn new_sorts_drops_repeats_and_refuses_bad_peers() {
        let make = |peers: Vec<String>| {
            CounterBundle::new(signed_pairing(), signed_settings(), None, peers)
        };
        let b = make(vec![
            "10.0.0.9:1".into(),
            "10.0.0.1:1".into(),
            "10.0.0.9:1".into(),
        ])
        .unwrap();
        assert_eq!(b.peers, vec!["10.0.0.1:1", "10.0.0.9:1"]);
        let nine: Vec<String> = (1..=9).map(|n| format!("10.0.0.{n}:1")).collect();
        assert_eq!(make(nine).unwrap_err(), BundleError::TooManyPeers);
        assert_eq!(
            make(vec!["host:1".into()]).unwrap_err(),
            BundleError::BadPeer
        );
        let long = format!("[{}]:1", "1:".repeat(40));
        assert_eq!(make(vec![long]).unwrap_err(), BundleError::BadPeer);
        assert!(!BundleError::BadPeer.to_string().is_empty());
        assert!(!BundleError::TooManyPeers.to_string().is_empty());
    }

    #[test]
    fn a_batch_length_of_zero_is_no_batch_and_nothing_else_is() {
        let with = bundle().to_bytes();
        let batch_len = signed_batch().to_bytes().len();
        // The same bytes with the batch cut out and its length set to zero
        // are the bundle without one.
        let pairing_len = signed_pairing().to_bytes().len();
        let settings_len = signed_settings().to_bytes().len();
        let at = 5 + 2 + pairing_len + 4 + settings_len;
        let mut cut = with[..at].to_vec();
        cut.extend_from_slice(&0u32.to_le_bytes());
        cut.extend_from_slice(&with[at + 4 + batch_len..]);
        let back = CounterBundle::from_bytes(&cut).unwrap();
        assert!(back.first_batch.is_none());
        assert_eq!(back.to_bytes(), cut);
    }
}
