//! The counter's side of pairing with the shop's own wallet: the ceremony in
//! memory, the bundle the owner phone hands back, and the record the counter
//! keeps afterwards (`docs/COUNTER-RULES.md` section 23, CR-227 to CR-233).
//!
//! The ceremony of `openbmt_core::pairing_own` has the counter read the
//! owner phone's offer and show the wallet's first receive address with the
//! kind of wallet. The counter makes no key and no number until the person
//! says "Same address" (CR-228). Then it shows its request with four words,
//! waits for "They match", and takes the owner phone's bundle. Like
//! [`crate::paired::Ceremony`] it lives in memory only: a restart forgets it,
//! and so does "Different".
//!
//! What the counter keeps when it is done is an [`OwnPairingRecord`]
//! (CR-233). [`PairedRecord`] is either kind of record, found by its magic,
//! for the start.
//!
//! The record's wire form (strict, [`openbmt_core::wire`]):
//!
//! ```text
//! "OBM:" | 1 or 2 | identity secret (32) | owner (32) | network u8 | settings floor u32 LE
//!   | wallet len u16 | pinned wallet ("OBM)")
//!   | pairing len u16 | pairing (entry and owner signature)
//!   | settings len u32 | signed settings ("OBMS")
//!   | first range ("OBM*", 109 bytes)
//!   | peer count u8 | per peer: len u8 | "host:port"
//!   | version 2 only: relay hint ([`openbmt_core::relay`])
//!   | version 3 only: flags u8 (1: the wallet is a starter wallet), then the relay
//!       hint if any bytes are left
//! ```
//!
//! Version 2 is the version with a relay, as for the record of a counter
//! paired with the Key app. Version 3 is the version of a starter wallet
//! (CR-256): the flags byte, then the relay if there is one. A record with no
//! flag is written as version 1 or 2, so a counter that switched
//! (CR-261) writes its record as before.

use std::fmt;

use bitcoin::Network;
use openbmt_core::batch::{BatchPolicy, PinnedRoots};
use openbmt_core::bundle::{peer_is_valid, MAX_PEERS};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::{network_code, network_from_code, PAIRING_WINDOW_SECS};
use openbmt_core::pairing_own::{
    check_range, OwnBundle, OwnCounterPairing, OwnOffer, OwnPairError, OwnSignedPairing,
    RangeLedger,
};
use openbmt_core::pinned_wallet::{PinnedWallet, SignedRange, WalletKind};
use openbmt_core::relay::{RelayHint, SignedRelayHint};
use openbmt_core::settings::{Authority, SettingsError, SignedSettings};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_core::words::WORD_COUNT;
use openbmt_counter::{OwnPairing, Pairing};

use crate::paired::{PairRefused, PairingRecord, Started};

/// Longest signed settings a record holds, in bytes.
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// Longest pairing a record holds, in bytes.
const MAX_PAIRING_BYTES: usize = 512;
/// Bytes of a signed range.
const RANGE_BYTES: usize = 109;

/// Why a step of an own-wallet pairing, or a saved one, was not accepted.
/// Each names its rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnPairRefused {
    /// CR-228, CR-229: no ceremony is under way, or it is not at the step
    /// the call is for (none was begun, it was ended, it was used, the app
    /// was restarted, or "Same address" has not been said).
    NoCeremony,
    /// CR-229: the counter could not make a key and a number of its own.
    NoRandomness,
    /// CR-227 to CR-230: a step of the ceremony of
    /// `openbmt_core::pairing_own`.
    Pair(OwnPairError),
    /// CR-230: what the owner phone showed is not a bundle.
    Bundle(DecodeError),
    /// CR-231: the settings are not signed by the owner root, or are not
    /// valid.
    Settings(SettingsError),
    /// CR-231: the settings are not newer than the offer's floor.
    SettingsOld {
        /// The floor the offer names.
        floor: u32,
        /// The version of the settings in the bundle.
        got: u32,
    },
    /// CR-231: this counter is not on the settings' device list.
    NotOnDevices,
    /// CR-231: this counter is not among the settings' counters.
    NotOnCounters,
    /// CR-130: the relay in the bundle is not signed by the owner root.
    RelayNotSigned,
    /// CR-233: the saved record does not read back.
    Record(DecodeError),
    /// CR-233: the saved record holds a secret that is not a key.
    RecordKey,
}

impl OwnPairRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NoCeremony => "CR-228",
            Self::NoRandomness => "CR-229",
            Self::Pair(OwnPairError::NotMainNetwork | OwnPairError::Decode(_)) => "CR-227",
            Self::Pair(
                OwnPairError::InvalidName
                | OwnPairError::CounterIsOwner
                | OwnPairError::BindingMismatch
                | OwnPairError::RequestSignatureInvalid,
            ) => "CR-229",
            Self::Pair(OwnPairError::Range(_) | OwnPairError::RangeTooSmall { .. }) => "CR-232",
            Self::Pair(_) | Self::Bundle(_) | Self::RelayNotSigned => "CR-230",
            Self::Settings(_)
            | Self::SettingsOld { .. }
            | Self::NotOnDevices
            | Self::NotOnCounters => "CR-231",
            Self::Record(_) | Self::RecordKey => "CR-233",
        }
    }
}

impl fmt::Display for OwnPairRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NoCeremony => write!(f, "no pairing is under way at this step"),
            Self::NoRandomness => write!(f, "the system gave no random numbers"),
            Self::Pair(e) => write!(f, "{e}"),
            Self::Bundle(e) => write!(f, "the owner phone's code is not a bundle: {e}"),
            Self::Settings(e) => write!(f, "the bundle's settings are refused: {e}"),
            Self::SettingsOld { floor, got } => write!(
                f,
                "the bundle's settings are version {got}, which does not pass the offer's \
                 version {floor}"
            ),
            Self::NotOnDevices => write!(f, "this counter is not on the settings' device list"),
            Self::NotOnCounters => write!(f, "this counter is not among the settings' counters"),
            Self::RelayNotSigned => write!(
                f,
                "the bundle's relay is not signed by the owner root this pairing pinned"
            ),
            Self::Record(e) => write!(f, "the saved pairing does not read back: {e}"),
            Self::RecordKey => write!(f, "the saved pairing holds a secret that is not a key"),
        }
    }
}

impl std::error::Error for OwnPairRefused {}

impl From<OwnPairError> for OwnPairRefused {
    fn from(e: OwnPairError) -> Self {
        Self::Pair(e)
    }
}

/// What a read offer shows (CR-228): the wallet's first receive address and
/// the kind of wallet, before anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnFirstAddress {
    /// The first receive address (receive chain, index 0), as text.
    pub address: String,
    /// The kind of wallet.
    pub kind: WalletKind,
}

/// Where an own-wallet ceremony is.
enum Phase {
    /// The offer is read and "Same address" is not said yet. No key, no
    /// number.
    Address(OwnOffer),
    /// The request is made; the pairing waits for the bundle.
    Request {
        pairing: OwnCounterPairing,
        secret: [u8; 32],
    },
    /// Used up.
    Done,
}

/// One own-wallet pairing in progress (CR-228 to CR-230). Memory only.
pub struct OwnCeremony {
    phase: Phase,
    /// Whether the person said the words match.
    confirmed: bool,
    started_at: u64,
}

impl fmt::Debug for OwnCeremony {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnCeremony")
            .field("confirmed", &self.confirmed)
            .finish_non_exhaustive()
    }
}

impl OwnCeremony {
    /// CR-227, CR-228: read the offer and derive the first receive address
    /// and the kind of wallet. No key and no number are made. `now` is the
    /// time in seconds; the ten minutes run from it (CR-229).
    pub fn begin(offer_bytes: &[u8], now: u64) -> Result<(Self, OwnFirstAddress), OwnPairRefused> {
        let offer = OwnOffer::from_bytes(offer_bytes)
            .map_err(|e| OwnPairRefused::Pair(OwnPairError::Decode(e)))?;
        let address = offer
            .first_address()
            .map_err(|e| OwnPairRefused::Pair(OwnPairError::Decode(pinned_error(&e))))?;
        let shown = OwnFirstAddress {
            address: address.to_string(),
            kind: offer.kind(),
        };
        Ok((
            Self {
                phase: Phase::Address(offer),
                confirmed: false,
                started_at: now,
            },
            shown,
        ))
    }

    /// Whether the ceremony can still take a step.
    pub fn is_open(&self) -> bool {
        !matches!(self.phase, Phase::Done)
    }

    /// Whether "Same address" was said and the request made.
    pub fn has_request(&self) -> bool {
        matches!(self.phase, Phase::Request { .. })
    }

    /// Whether the person has said the words match.
    pub fn confirmed(&self) -> bool {
        self.confirmed
    }

    /// CR-229: the person says the address is the same. Only now are the
    /// key and the number used: `secret` is the counter's new identity secret
    /// and `nonce` the fresh number, both from the system's random source,
    /// made after this call and new for this ceremony. A ceremony that is
    /// past its ten minutes, or is not waiting at the address, refuses.
    pub fn address_same(
        &mut self,
        name: &str,
        secret: [u8; 32],
        nonce: [u8; 32],
        now: u64,
    ) -> Result<Started, OwnPairRefused> {
        let Phase::Address(offer) = std::mem::replace(&mut self.phase, Phase::Done) else {
            return Err(OwnPairRefused::NoCeremony);
        };
        if now < self.started_at {
            return Err(OwnPairError::ClockWentBack.into());
        }
        if now - self.started_at > PAIRING_WINDOW_SECS {
            return Err(OwnPairError::Expired.into());
        }
        let identity =
            Identity::from_secret_bytes(&secret).map_err(|_| OwnPairRefused::NoRandomness)?;
        let (pairing, request) =
            OwnCounterPairing::begin(offer, &identity, name, nonce, self.started_at)?;
        let words = pairing.session_words();
        self.phase = Phase::Request { pairing, secret };
        Ok(Started {
            request: request.to_bytes(),
            words,
        })
    }

    /// The four words of the request, once it is made.
    pub fn words(&self) -> Option<[&'static str; WORD_COUNT]> {
        match &self.phase {
            Phase::Request { pairing, .. } => Some(pairing.session_words()),
            _ => None,
        }
    }

    /// CR-229: the person says the four words match. Refused before "Same
    /// address", and when the time window is over.
    pub fn confirm(&mut self, now: u64) -> Result<(), OwnPairRefused> {
        if !self.has_request() {
            return Err(OwnPairRefused::NoCeremony);
        }
        if now < self.started_at {
            return Err(OwnPairError::ClockWentBack.into());
        }
        if now - self.started_at > PAIRING_WINDOW_SECS {
            return Err(OwnPairError::Expired.into());
        }
        self.confirmed = true;
        Ok(())
    }

    /// CR-230 to CR-232: take the owner phone's bundle. The checks run in
    /// this order and the first failure wins: "They match" was said, the
    /// bundle reads, the time window, the pairing (this counter, this number,
    /// the hash of the offer's wallet, the name, the owner root's signature),
    /// the settings (CR-231), the range (CR-232).
    ///
    /// A call before "They match" and a bundle that does not read change
    /// nothing, and the ceremony goes on. Any other outcome uses the ceremony
    /// up: a new try needs a new ceremony, with a new number.
    pub fn finish(
        &mut self,
        bundle_bytes: &[u8],
        policy: &BatchPolicy,
        now: u64,
    ) -> Result<OwnPairingRecord, OwnPairRefused> {
        if !self.has_request() {
            return Err(OwnPairRefused::NoCeremony);
        }
        if !self.confirmed {
            return Err(OwnPairError::NotConfirmed.into());
        }
        let bundle = OwnBundle::from_bytes(bundle_bytes).map_err(OwnPairRefused::Bundle)?;
        let Phase::Request { pairing, secret } = std::mem::replace(&mut self.phase, Phase::Done)
        else {
            return Err(OwnPairRefused::NoCeremony);
        };
        let floor = pairing.offer().floor;
        let paired = pairing.finish(&bundle.pairing, true, now)?;
        // CR-130: the relay is kept only with the owner root's signature.
        if let Some(signed) = &bundle.relay {
            if !signed.verify(&paired.offer.owner) {
                return Err(OwnPairRefused::RelayNotSigned);
            }
        }
        let record = OwnPairingRecord {
            identity_secret: secret,
            owner: paired.offer.owner,
            network: paired.offer.network(),
            wallet: paired.offer.wallet,
            settings_floor: floor,
            starter: paired.offer.starter,
            pairing: paired.pairing,
            settings: bundle.settings,
            first_range: bundle.first_range,
            peers: bundle.peers,
            relay: bundle.relay.map(|signed| signed.hint),
        };
        record.check(policy)?;
        Ok(record)
    }
}

/// The decode error a failure to derive a pinned wallet's address reports.
fn pinned_error(_: &openbmt_core::pinned_wallet::PinnedError) -> DecodeError {
    DecodeError::NonCanonical("the wallet has no first address")
}

/// What a counter paired with the shop's own wallet keeps (CR-233).
#[derive(Clone, PartialEq, Eq)]
pub struct OwnPairingRecord {
    /// The counter's identity secret.
    pub identity_secret: [u8; 32],
    /// The owner root, from the offer.
    pub owner: DeviceId,
    /// The network, from the offer: the main network.
    pub network: Network,
    /// The pinned wallet, from the offer.
    pub wallet: PinnedWallet,
    /// The settings floor of the offer; the settings must pass it.
    pub settings_floor: u32,
    /// CR-256: the pinned wallet is a starter wallet, so the counter may take
    /// one switch (CR-257 to CR-263).
    pub starter: bool,
    /// The pairing the owner root signed.
    pub pairing: OwnSignedPairing,
    /// The counter's first settings.
    pub settings: SignedSettings,
    /// The first signed range.
    pub first_range: SignedRange,
    /// Peers to try first.
    pub peers: Vec<String>,
    /// The relay the counter reaches the owner through, if it knows one.
    pub relay: Option<RelayHint>,
}

impl fmt::Debug for OwnPairingRecord {
    // The secret is never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnPairingRecord")
            .field("owner", &self.owner)
            .field("network", &self.network)
            .field("settings_floor", &self.settings_floor)
            .finish_non_exhaustive()
    }
}

impl OwnPairingRecord {
    /// The counter's identity.
    pub fn identity(&self) -> Result<Identity, OwnPairRefused> {
        Identity::from_secret_bytes(&self.identity_secret).map_err(|_| OwnPairRefused::RecordKey)
    }

    /// What the counter library takes as its pairing (CR-234): the owner
    /// root, the wallet, the first range and the counter's own smallest
    /// range.
    pub fn own_pairing(&self, policy: BatchPolicy) -> OwnPairing {
        OwnPairing {
            owner: self.owner,
            wallet: self.wallet.clone(),
            first_range: self.first_range,
            batch_policy: policy,
        }
    }

    /// CR-230 to CR-232 again, on a record when it starts, the match and the
    /// time aside: the pairing names this counter and the pinned wallet and
    /// is signed by the owner root; the settings are signed by the owner root
    /// alone (no Key app signature), are valid, newer than the floor and list
    /// this counter on the device list and among the counters; the first
    /// range is signed by the owner root, names this counter and holds at
    /// least `policy`'s smallest range, which is the counter's own.
    pub fn check(&self, policy: &BatchPolicy) -> Result<(), OwnPairRefused> {
        self.check_for(policy, self.wallet.checksum32())
    }

    /// CR-261: [`OwnPairingRecord::check`] for a record that switched. Its
    /// pairing still names the wallet the counter was paired with (it was
    /// kept as it was), so the record is accepted with another wallet than
    /// the pairing's only when the counter's own range book proves the
    /// switch: the book is for the wallet the record pins and holds closed
    /// ranges of the wallet the pairing names. A record with the starter
    /// flag has not switched and is not accepted this way.
    pub fn check_switched(
        &self,
        policy: &BatchPolicy,
        ranges: &[u8],
    ) -> Result<(), OwnPairRefused> {
        let own: DeviceId = self.identity()?.device_id();
        let proven = !self.starter
            && RangeLedger::from_bytes(ranges, self.owner, own).is_ok_and(|l| {
                l.switched_to() == Some(self.wallet.checksum32())
                    && l.closed()
                        .iter()
                        .any(|c| c.wallet == self.pairing.entry.wallet)
            });
        if !proven {
            return Err(OwnPairError::WrongWallet.into());
        }
        self.check_for(policy, self.pairing.entry.wallet)
    }

    fn check_for(&self, policy: &BatchPolicy, paired: [u8; 32]) -> Result<(), OwnPairRefused> {
        let own: DeviceId = self.identity()?.device_id();
        if self.network != Network::Bitcoin {
            return Err(OwnPairError::NotMainNetwork.into());
        }
        self.pairing.check(&self.owner, &paired, &own)?;
        // `None` for the settings in force: the first settings are not a
        // change, so nothing is classified (CR-231).
        self.settings
            .verify_with(&Authority::OwnWallet { owner: self.owner }, None)
            .map_err(OwnPairRefused::Settings)?;
        let s = &self.settings.settings;
        if s.version <= self.settings_floor {
            return Err(OwnPairRefused::SettingsOld {
                floor: self.settings_floor,
                got: s.version,
            });
        }
        if !s.devices.contains(&own) {
            return Err(OwnPairRefused::NotOnDevices);
        }
        if !s.counters.contains_key(&own) {
            return Err(OwnPairRefused::NotOnCounters);
        }
        check_range(&self.first_range, &self.owner, &own, policy.min_count)?;
        Ok(())
    }

    /// CR-132: this record with the relay of a relay for counters, if the
    /// owner root this counter pinned signed it.
    pub fn with_relay(
        &self,
        signed: &SignedRelayHint,
    ) -> Result<Self, crate::paired::RelayRefused> {
        if !signed.verify(&self.owner) {
            return Err(crate::paired::RelayRefused::NotFromOwner);
        }
        let mut next = self.clone();
        next.relay = Some(signed.hint.clone());
        Ok(next)
    }

    /// Wire form: see the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let wallet = self.wallet.to_bytes();
        let pairing = self.pairing.to_bytes();
        let settings = self.settings.to_bytes();
        let mut out = wire::start(
            &wire::MAGIC_OWN_PAIRING_RECORD,
            200 + wallet.len() + pairing.len() + settings.len() + self.peers.len() * 24,
        );
        let version = match (self.starter, self.relay.is_some()) {
            (true, _) => 3,
            (false, true) => 2,
            (false, false) => 1,
        };
        if let Some(v) = out.get_mut(4) {
            *v = version;
        }
        out.extend_from_slice(&self.identity_secret);
        out.extend_from_slice(&self.owner.to_bytes());
        out.push(network_code(self.network).unwrap_or(u8::MAX));
        out.extend_from_slice(&self.settings_floor.to_le_bytes());
        out.extend_from_slice(
            &u16::try_from(wallet.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&wallet);
        out.extend_from_slice(
            &u16::try_from(pairing.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&pairing);
        out.extend_from_slice(&wire::len_u32(settings.len()).to_le_bytes());
        out.extend_from_slice(&settings);
        out.extend_from_slice(&self.first_range.to_bytes());
        out.push(u8::try_from(self.peers.len()).unwrap_or(u8::MAX));
        for p in &self.peers {
            out.push(u8::try_from(p.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(p.as_bytes());
        }
        if self.starter {
            out.push(1);
        }
        if let Some(relay) = &self.relay {
            out.extend_from_slice(&relay.to_bytes());
        }
        out
    }

    /// Parse the wire form strictly. The secret must be a valid key, the
    /// owner a valid key, the network a code of the offer, the wallet decoded
    /// as WL-21 says, every length within its limit, and the peers valid and
    /// ascending. This does not check signatures: [`OwnPairingRecord::check`]
    /// does. Version 1 has no relay and version 2 has one at the end.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (mut r, version) = Reader::framed_up_to(bytes, &wire::MAGIC_OWN_PAIRING_RECORD, 3)?;
        let identity_secret: [u8; 32] = r.array()?;
        if Identity::from_secret_bytes(&identity_secret).is_err() {
            return Err(DecodeError::NonCanonical(
                "the identity secret is not a key",
            ));
        }
        let owner = r.device_id()?;
        let network = network_from_code(r.u8()?)
            .ok_or(DecodeError::NonCanonical("the network code is unknown"))?;
        let settings_floor = r.u32_le()?;
        let n = usize::from(r.u16_le()?);
        let wallet = PinnedWallet::from_bytes(r.take(n)?).map_err(|e| match e {
            openbmt_core::pinned_wallet::PinnedError::Decode(d) => d,
            _ => DecodeError::NonCanonical("the wallet is refused"),
        })?;
        let n = usize::from(r.u16_le()?);
        if n > MAX_PAIRING_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let pairing = OwnSignedPairing::from_bytes(r.take(n)?)?;
        let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if n > MAX_SETTINGS_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let settings = SignedSettings::from_bytes(r.take(n)?)?;
        let first_range = SignedRange::from_bytes(r.take(RANGE_BYTES)?)?;
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
        let mut starter = false;
        if version >= 3 {
            // CR-256: only bit 0 is defined, and a version 3 record has it.
            if r.u8()? != 1 {
                return Err(DecodeError::NonCanonical(
                    "the flags of a record are 1 or the record is version 1 or 2",
                ));
            }
            starter = true;
        }
        let relay = if version == 2 || (version >= 3 && !r.is_empty()) {
            Some(RelayHint::read(&mut r)?)
        } else {
            None
        };
        r.finish()?;
        Ok(Self {
            identity_secret,
            owner,
            network,
            wallet,
            settings_floor,
            starter,
            pairing,
            settings,
            first_range,
            peers,
            relay,
        })
    }
}

/// A saved pairing of either kind, told apart by its magic (CR-233): what a
/// start reads.
#[derive(Clone, PartialEq, Eq)]
pub enum PairedRecord {
    /// A counter paired with the Key app (`OBMt`).
    KeyApp(Box<PairingRecord>),
    /// A counter paired with the shop's own wallet (`OBM:`).
    OwnWallet(Box<OwnPairingRecord>),
}

impl fmt::Debug for PairedRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyApp(r) => r.fmt(f),
            Self::OwnWallet(r) => r.fmt(f),
        }
    }
}

/// Why a saved pairing was not read or checked, of whichever kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRefused {
    /// A record of the Key app's kind.
    KeyApp(PairRefused),
    /// A record of the own wallet's kind.
    OwnWallet(OwnPairRefused),
}

impl RecordRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::KeyApp(e) => e.rule(),
            Self::OwnWallet(e) => e.rule(),
        }
    }
}

impl fmt::Display for RecordRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyApp(e) => e.fmt(f),
            Self::OwnWallet(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RecordRefused {}

impl PairedRecord {
    /// Read a saved pairing: `OBM:` is an own-wallet record, anything else is
    /// read as the Key app's (which refuses what is not its own).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RecordRefused> {
        if bytes.get(..4) == Some(&wire::MAGIC_OWN_PAIRING_RECORD[..]) {
            OwnPairingRecord::from_bytes(bytes)
                .map(|r| Self::OwnWallet(Box::new(r)))
                .map_err(|e| RecordRefused::OwnWallet(OwnPairRefused::Record(e)))
        } else {
            PairingRecord::from_bytes(bytes)
                .map(|r| Self::KeyApp(Box::new(r)))
                .map_err(|e| RecordRefused::KeyApp(PairRefused::Record(e)))
        }
    }

    /// The checks of the record's own kind, again at start.
    pub fn check(&self, policy: &BatchPolicy) -> Result<(), RecordRefused> {
        self.check_with_ranges(policy, None)
    }

    /// [`PairedRecord::check`], given the counter's range book as the
    /// folder holds it: an own-wallet record that pins another wallet than
    /// its pairing names is accepted when the book proves the switch
    /// (CR-261).
    pub fn check_with_ranges(
        &self,
        policy: &BatchPolicy,
        ranges: Option<&[u8]>,
    ) -> Result<(), RecordRefused> {
        match self {
            Self::KeyApp(r) => r.check(policy).map_err(RecordRefused::KeyApp),
            Self::OwnWallet(r) => match r.check(policy) {
                Err(OwnPairRefused::Pair(OwnPairError::WrongWallet)) if ranges.is_some() => r
                    .check_switched(policy, ranges.unwrap_or_default())
                    .map_err(RecordRefused::OwnWallet),
                other => other.map_err(RecordRefused::OwnWallet),
            },
        }
    }

    /// The counter's identity.
    pub fn identity(&self) -> Result<Identity, RecordRefused> {
        match self {
            Self::KeyApp(r) => r.identity().map_err(RecordRefused::KeyApp),
            Self::OwnWallet(r) => r.identity().map_err(RecordRefused::OwnWallet),
        }
    }

    /// Who may sign settings for this counter.
    pub fn authority(&self) -> Authority {
        match self {
            Self::KeyApp(r) => Authority::KeyApp(r.roots),
            Self::OwnWallet(r) => Authority::OwnWallet { owner: r.owner },
        }
    }

    /// The owner root.
    pub fn owner(&self) -> DeviceId {
        match self {
            Self::KeyApp(r) => r.roots.owner,
            Self::OwnWallet(r) => r.owner,
        }
    }

    /// The network.
    pub fn network(&self) -> Network {
        match self {
            Self::KeyApp(r) => r.network,
            Self::OwnWallet(r) => r.network,
        }
    }

    /// The settings the counter was paired with.
    pub fn settings(&self) -> &SignedSettings {
        match self {
            Self::KeyApp(r) => &r.settings,
            Self::OwnWallet(r) => &r.settings,
        }
    }

    /// The peers to try first.
    pub fn peers(&self) -> &[String] {
        match self {
            Self::KeyApp(r) => &r.peers,
            Self::OwnWallet(r) => &r.peers,
        }
    }

    /// The relay the counter reaches the owner through, if it knows one.
    pub fn relay(&self) -> Option<&RelayHint> {
        match self {
            Self::KeyApp(r) => r.relay.as_ref(),
            Self::OwnWallet(r) => r.relay.as_ref(),
        }
    }

    /// The record with a relay for counters, if the owner root signed it.
    pub fn with_relay(
        &self,
        signed: &SignedRelayHint,
    ) -> Result<Self, crate::paired::RelayRefused> {
        match self {
            Self::KeyApp(r) => r.with_relay(signed).map(|r| Self::KeyApp(Box::new(r))),
            Self::OwnWallet(r) => r.with_relay(signed).map(|r| Self::OwnWallet(Box::new(r))),
        }
    }

    /// The bytes the app keeps.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::KeyApp(r) => r.to_bytes(),
            Self::OwnWallet(r) => r.to_bytes(),
        }
    }

    /// What the counter library takes as its pairing, for a counter paired
    /// with the Key app.
    pub fn key_app_pairing(&self) -> Option<Pairing> {
        match self {
            Self::KeyApp(r) => Some(r.pairing()),
            Self::OwnWallet(_) => None,
        }
    }

    /// The roots a counter paired with the Key app pinned.
    pub fn roots(&self) -> Option<PinnedRoots> {
        match self {
            Self::KeyApp(r) => Some(r.roots),
            Self::OwnWallet(_) => None,
        }
    }
}
