//! The counter's side of pairing: the ceremony in memory, the bundle the
//! owner phone hands back, and the record the counter keeps afterwards
//! (`docs/COUNTER-RULES.md`, CR-91 to CR-99).
//!
//! The ceremony of `openbmt_core::pairing` has the counter scan the Key
//! app's offer, show its own request with four words, wait for the person
//! to compare the words, and then take what the owner phone brings back.
//! Here that is one value, [`Ceremony`], that lives in memory only: a
//! restart forgets it, and so does "They differ". Its number and key are
//! the caller's, drawn from the system's random source for this one
//! ceremony.
//!
//! What the counter keeps when it is done is a [`PairingRecord`]: its own
//! identity, the roots, the wallet and the network (all fixed from the
//! offer), the settings floor, the signed settings, the first batch and the
//! peers. The app seals the record and hands it back at every start.
//!
//! Nothing that changes what money may do is taken from the bundle without a
//! check the counter makes itself:
//!
//! - the roots, wallet and network are the offer's (CR-93);
//! - the settings carry both roots' signatures and name this counter
//!   (CR-94, CR-95);
//! - the first batch goes through the same checks as any later batch, with
//!   a smallest size that is the counter's own (CR-96);
//! - the peers only choose whom to ask first (CR-99).
//!
//! The record's wire form (strict, [`openbmt_core::wire`]):
//!
//! ```text
//! "OBMt" | 1 or 2 | identity secret (32) | owner (32) | Key app (32) | wallet (32)
//!   | network u8 | settings floor u32 LE
//!   | settings len u32 | signed settings ("OBMS")
//!   | batch flag u8 | (batch len u32 | signed batch ("OBMB"))
//!   | peer count u8 | per peer: len u8 | "host:port"
//!   | version 2 only: relay hint ([`openbmt_core::relay`])
//! ```
//!
//! Version 2 is the version with a relay (CR-131): a record with no relay is
//! written as version 1, and a version 2 record always has one. The relay
//! comes from the bundle, or later from the owner's signed relay for counters
//! ([`PairingRecord::with_relay`]); like the peers it is a hint and adds no
//! authority (`docs/LINK-RULES.md`, LK-09).

use std::convert::Infallible;
use std::fmt;

use bitcoin::Network;
use openbmt_core::batch::{
    AddError, BatchError, BatchPolicy, CounterBatches, PinnedRoots, SignedBatch, DEFAULT_MIN_COUNT,
};
use openbmt_core::bundle::{peer_is_valid, CounterBundle, MAX_PEERS};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::{
    network_code, network_from_code, CounterPairing, PairError, PAIRING_WINDOW_SECS,
};
use openbmt_core::relay::{RelayHint, SignedRelayHint};
use openbmt_core::settings::{SettingsError, SignedSettings};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_core::words::WORD_COUNT;
use openbmt_counter::Pairing;

/// Magic of a [`PairingRecord`].
pub const MAGIC_PAIRING_RECORD: [u8; 4] = *b"OBMt";

/// Longest signed settings a record holds, in bytes.
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// Longest signed batch a record holds, in bytes.
const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;

/// CR-96: the fewest addresses in a batch this counter takes. It is the
/// counter's own number: no bundle, record, setting or app call can lower
/// it.
pub const BATCH_MIN_COUNT: u32 = DEFAULT_MIN_COUNT;

/// CR-96: the batch rule of every counter.
pub fn batch_policy() -> BatchPolicy {
    BatchPolicy {
        min_count: BATCH_MIN_COUNT,
    }
}

/// Why a step of pairing, or a saved pairing, was not accepted. Each names
/// its rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairRefused {
    /// CR-91: no ceremony is under way (none was begun, it was ended, it was
    /// used, or the app was restarted).
    NoCeremony,
    /// CR-91: the counter could not make a key and a number of its own: the
    /// system's random source failed, or gave a secret that is not a key.
    NoRandomness,
    /// CR-91, CR-92: a step of the ceremony of `openbmt_core::pairing`.
    Pair(PairError),
    /// CR-92: what the owner phone showed is not a bundle.
    Bundle(DecodeError),
    /// CR-94: the settings are not signed by both pinned roots, or are not
    /// valid.
    Settings(SettingsError),
    /// CR-94: the settings are not newer than the pairing's floor.
    SettingsOld {
        /// The settings version the pairing names.
        floor: u32,
        /// The version of the settings in the bundle.
        got: u32,
    },
    /// CR-95: this counter is not on the settings' device list.
    NotOnDevices,
    /// CR-95: this counter is not among the settings' counters.
    NotOnCounters,
    /// CR-96: the first batch was refused by the batch checks.
    Batch(BatchError),
    /// CR-97: the saved pairing record does not read back.
    Record(DecodeError),
    /// CR-97: the saved pairing record holds a secret that is not a key.
    RecordKey,
}

impl PairRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NoCeremony | Self::NoRandomness => "CR-91",
            Self::Pair(_) | Self::Bundle(_) => "CR-92",
            Self::Settings(_) | Self::SettingsOld { .. } => "CR-94",
            Self::NotOnDevices | Self::NotOnCounters => "CR-95",
            Self::Batch(_) => "CR-96",
            Self::Record(_) | Self::RecordKey => "CR-97",
        }
    }
}

impl fmt::Display for PairRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NoCeremony => write!(f, "no pairing is under way"),
            Self::NoRandomness => write!(f, "the system gave no random numbers"),
            Self::Pair(e) => write!(f, "{e}"),
            Self::Bundle(e) => write!(f, "the owner phone's code is not a bundle: {e}"),
            Self::Settings(e) => write!(f, "the bundle's settings are refused: {e}"),
            Self::SettingsOld { floor, got } => write!(
                f,
                "the bundle's settings are version {got}, which does not pass the pairing's \
                 version {floor}"
            ),
            Self::NotOnDevices => write!(f, "this counter is not on the settings' device list"),
            Self::NotOnCounters => write!(f, "this counter is not among the settings' counters"),
            Self::Batch(e) => write!(f, "the first batch is refused: {e}"),
            Self::Record(e) => write!(f, "the saved pairing does not read back: {e}"),
            Self::RecordKey => write!(f, "the saved pairing holds a secret that is not a key"),
        }
    }
}

impl std::error::Error for PairRefused {}

/// Why a relay for counters was not taken (CR-132). Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRefused {
    /// What was shown is not a relay for counters.
    Bytes(DecodeError),
    /// The signature is not the owner root's this counter pinned.
    NotFromOwner,
}

impl RelayRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        "CR-132"
    }
}

impl fmt::Display for RelayRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Bytes(e) => write!(f, "the code is not a relay for counters: {e}"),
            Self::NotFromOwner => write!(
                f,
                "the relay for counters is not signed by the owner this counter was paired with"
            ),
        }
    }
}

impl std::error::Error for RelayRefused {}

impl From<PairError> for PairRefused {
    fn from(e: PairError) -> Self {
        Self::Pair(e)
    }
}

/// What a begun ceremony shows: the request for the owner phone to scan and
/// the four words to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The request (`PairRequest::to_bytes`, magic `OBMJ`).
    pub request: Vec<u8>,
    /// The four session words.
    pub words: [&'static str; WORD_COUNT],
}

/// One pairing in progress (CR-91). It is kept in memory only.
pub struct Ceremony {
    /// The core's ceremony; taken by the one finish that uses it.
    pairing: Option<CounterPairing>,
    /// The identity secret the counter will keep if the ceremony succeeds.
    secret: [u8; 32],
    /// Whether the person said the words match.
    confirmed: bool,
    started_at: u64,
}

impl fmt::Debug for Ceremony {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ceremony")
            .field("confirmed", &self.confirmed)
            .finish_non_exhaustive()
    }
}

impl Ceremony {
    /// CR-91: read the offer, fix its roots, wallet and network, and make
    /// the request. `secret` is the counter's new identity secret and
    /// `nonce` the fresh number; both must come from the system's random
    /// source and be new for this ceremony. `now` is the time in seconds.
    pub fn begin(
        offer_bytes: &[u8],
        name: &str,
        secret: [u8; 32],
        nonce: [u8; 32],
        now: u64,
    ) -> Result<(Self, Started), PairRefused> {
        let identity =
            Identity::from_secret_bytes(&secret).map_err(|_| PairRefused::NoRandomness)?;
        let (pairing, request) = CounterPairing::begin(offer_bytes, &identity, name, nonce, now)?;
        let words = pairing.session_words();
        let started = Started {
            request: request.to_bytes(),
            words,
        };
        Ok((
            Self {
                pairing: Some(pairing),
                secret,
                confirmed: false,
                started_at: now,
            },
            started,
        ))
    }

    /// Whether the ceremony can still take a bundle: it has not been used up
    /// by a finish that got past the first two checks.
    pub fn is_open(&self) -> bool {
        self.pairing.is_some()
    }

    /// The network the offer named.
    pub fn network(&self) -> Option<Network> {
        self.pairing.as_ref().map(CounterPairing::network)
    }

    /// Whether the person has said the words match.
    pub fn confirmed(&self) -> bool {
        self.confirmed
    }

    /// CR-92: the person says the four words match. Refused when the time
    /// window is over, so that a person is not asked to scan the owner phone
    /// when the answer would be refused.
    pub fn confirm(&mut self, now: u64) -> Result<(), PairRefused> {
        if self.pairing.is_none() {
            return Err(PairRefused::NoCeremony);
        }
        if now < self.started_at {
            return Err(PairError::ClockWentBack.into());
        }
        if now - self.started_at > PAIRING_WINDOW_SECS {
            return Err(PairError::Expired.into());
        }
        self.confirmed = true;
        Ok(())
    }

    /// CR-92 to CR-96: take the owner phone's bundle. The checks run in this
    /// order and the first failure wins: "They match" was said, the bundle
    /// reads, the time window, the entry (wallet, counter, number, name, the
    /// Key app's signature, the owner's signature), the settings' two
    /// signatures, their version, the device list, the counters, and the
    /// first batch.
    ///
    /// A call before "They match" and a bundle that does not read change
    /// nothing, and the ceremony goes on. Any other outcome uses the
    /// ceremony up: a new try needs a new ceremony, with a new number.
    pub fn finish(
        &mut self,
        bundle_bytes: &[u8],
        policy: &BatchPolicy,
        now: u64,
    ) -> Result<PairingRecord, PairRefused> {
        if self.pairing.is_none() {
            return Err(PairRefused::NoCeremony);
        }
        if !self.confirmed {
            return Err(PairError::NotConfirmed.into());
        }
        let bundle = CounterBundle::from_bytes(bundle_bytes).map_err(PairRefused::Bundle)?;
        let Some(pairing) = self.pairing.take() else {
            return Err(PairRefused::NoCeremony);
        };
        let paired = pairing.finish(&bundle.pairing, true, now)?;
        let record = PairingRecord {
            identity_secret: self.secret,
            roots: paired.roots(),
            wallet: paired.wallet(),
            network: paired.network(),
            settings_floor: paired.settings_floor(),
            settings: bundle.settings,
            first_batch: bundle.first_batch,
            peers: bundle.peers,
            relay: bundle.relay,
        };
        record.check(policy)?;
        Ok(record)
    }
}

/// What a counter keeps after pairing (CR-97).
#[derive(Clone, PartialEq, Eq)]
pub struct PairingRecord {
    /// The counter's identity secret.
    pub identity_secret: [u8; 32],
    /// The roots fixed from the offer.
    pub roots: PinnedRoots,
    /// The wallet's checksum, from the offer.
    pub wallet: [u8; 32],
    /// The network, from the offer.
    pub network: Network,
    /// The settings version in the pairing entry; the settings must pass it.
    pub settings_floor: u32,
    /// The settings that added this counter.
    pub settings: SignedSettings,
    /// The first batch of addresses, if the bundle had one.
    pub first_batch: Option<SignedBatch>,
    /// Peers to try first.
    pub peers: Vec<String>,
    /// The relay the counter reaches the owner through, if it knows one
    /// (CR-131).
    pub relay: Option<RelayHint>,
}

impl fmt::Debug for PairingRecord {
    // The secret is never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingRecord")
            .field("roots", &self.roots)
            .field("network", &self.network)
            .field("settings_floor", &self.settings_floor)
            .finish_non_exhaustive()
    }
}

impl PairingRecord {
    /// The counter's identity.
    pub fn identity(&self) -> Result<Identity, PairRefused> {
        Identity::from_secret_bytes(&self.identity_secret).map_err(|_| PairRefused::RecordKey)
    }

    /// What the counter library takes as its pairing: the roots, the wallet
    /// and the counter's own batch rule (CR-96).
    pub fn pairing(&self) -> Pairing {
        Pairing {
            roots: self.roots,
            wallet_checksum: self.wallet,
            batch_policy: batch_policy(),
        }
    }

    /// CR-94 to CR-96: the checks the counter makes on the bundle's contents,
    /// and again on a record when it starts. Settings: both pinned roots
    /// signed them and they are valid; their version is above the pairing's
    /// floor; this counter is on the device list and among the counters. The
    /// first batch, if there is one, goes through the counter's batch path
    /// with `policy`, which is the counter's own (see [`batch_policy`]).
    pub fn check(&self, policy: &BatchPolicy) -> Result<(), PairRefused> {
        let own: DeviceId = self.identity()?.device_id();
        // `None` for the settings in force: the first settings count as
        // loosening, so both signatures are required.
        self.settings
            .verify(&self.roots, None)
            .map_err(PairRefused::Settings)?;
        let s = &self.settings.settings;
        if s.version <= self.settings_floor {
            return Err(PairRefused::SettingsOld {
                floor: self.settings_floor,
                got: s.version,
            });
        }
        if !s.devices.contains(&own) {
            return Err(PairRefused::NotOnDevices);
        }
        if !s.counters.contains_key(&own) {
            return Err(PairRefused::NotOnCounters);
        }
        if let Some(batch) = &self.first_batch {
            let mut scratch = CounterBatches::new(own, self.roots, self.wallet, *policy);
            match scratch.add(batch.clone(), |_| Ok::<(), Infallible>(())) {
                Ok(()) => {}
                Err(AddError::Refused(e)) => return Err(PairRefused::Batch(e)),
                Err(AddError::Persist(never)) => match never {},
            }
        }
        Ok(())
    }

    /// CR-132: this record with the relay of a relay for counters, if the
    /// owner root this counter pinned signed it. A relay the record already
    /// has is replaced; nothing else changes, and `self` is not touched.
    pub fn with_relay(&self, signed: &SignedRelayHint) -> Result<Self, RelayRefused> {
        if !signed.verify(&self.roots.owner) {
            return Err(RelayRefused::NotFromOwner);
        }
        let mut next = self.clone();
        next.relay = Some(signed.hint.clone());
        Ok(next)
    }

    /// Wire form: see the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let settings = self.settings.to_bytes();
        let batch = self.first_batch.as_ref().map(SignedBatch::to_bytes);
        let mut out = wire::start(
            &MAGIC_PAIRING_RECORD,
            260 + settings.len() + batch.as_ref().map_or(0, Vec::len) + self.peers.len() * 24,
        );
        if self.relay.is_some() {
            if let Some(version) = out.get_mut(4) {
                *version = 2;
            }
        }
        out.extend_from_slice(&self.identity_secret);
        out.extend_from_slice(&self.roots.owner.to_bytes());
        out.extend_from_slice(&self.roots.approver.to_bytes());
        out.extend_from_slice(&self.wallet);
        out.push(network_code(self.network).unwrap_or(u8::MAX));
        out.extend_from_slice(&self.settings_floor.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(settings.len()).to_le_bytes());
        out.extend_from_slice(&settings);
        match batch {
            Some(b) => {
                out.push(1);
                out.extend_from_slice(&wire::len_u32(b.len()).to_le_bytes());
                out.extend_from_slice(&b);
            }
            None => out.push(0),
        }
        out.push(u8::try_from(self.peers.len()).unwrap_or(u8::MAX));
        for p in &self.peers {
            out.push(u8::try_from(p.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(p.as_bytes());
        }
        if let Some(relay) = &self.relay {
            out.extend_from_slice(&relay.to_bytes());
        }
        out
    }

    /// Parse the wire form strictly. The secret must be a valid key, the
    /// roots different valid keys, the network a code of the offer, every
    /// length within its limit, and the peers valid and ascending. This does
    /// not check signatures: [`PairingRecord::check`] does. Version 1 has no
    /// relay and version 2 has one at the end.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (mut r, version) = Reader::framed_up_to(bytes, &MAGIC_PAIRING_RECORD, 2)?;
        let identity_secret: [u8; 32] = r.array()?;
        if Identity::from_secret_bytes(&identity_secret).is_err() {
            return Err(DecodeError::NonCanonical(
                "the identity secret is not a key",
            ));
        }
        let owner = r.device_id()?;
        let approver = r.device_id()?;
        let roots = PinnedRoots::new(owner, approver)
            .map_err(|_| DecodeError::NonCanonical("owner and Key app roots are equal"))?;
        let wallet = r.array()?;
        let network = network_from_code(r.u8()?)
            .ok_or(DecodeError::NonCanonical("the network code is unknown"))?;
        let settings_floor = r.u32_le()?;
        let n = len32(&mut r, MAX_SETTINGS_BYTES)?;
        let settings = SignedSettings::from_bytes(r.take(n)?)?;
        let first_batch = if r.flag()? {
            let n = len32(&mut r, MAX_BATCH_BYTES)?;
            if n == 0 {
                return Err(DecodeError::NonCanonical("a batch of no bytes"));
            }
            Some(SignedBatch::from_bytes(r.take(n)?)?)
        } else {
            None
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
            identity_secret,
            roots,
            wallet,
            network,
            settings_floor,
            settings,
            first_batch,
            peers,
            relay,
        })
    }
}

/// A u32 length that must be at most `max`, refused before any bytes are
/// taken.
fn len32(r: &mut Reader<'_>, max: usize) -> Result<usize, DecodeError> {
    let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
    if n > max {
        return Err(DecodeError::TooLarge);
    }
    Ok(n)
}
