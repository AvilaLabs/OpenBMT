//! The `KeyApp` object: setup, the Key app's identity and the lock around
//! the one approver.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use bitcoin::bip32::Xpub;
use bitcoin::secp256k1::Secp256k1;
use openbmt_approver::Approver;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::PairOffer;
use openbmt_core::wallet::{Keychain, WalletDescription};
use openbmt_signer::SignerState;

use crate::codec::{wire_record, wire_record_get, wire_unit_enum, Define, Put, Shaped, TypeDef};
use crate::envelope::{self, wipe_string, Material, WalletPart};
use crate::error::{internal, refused, refused_by, store_failed, wrong_state, KeyError};
use crate::keys::{make_material, EntropySource, KeyNetwork};
use crate::lock::{self, LockCell, Sealing};
use crate::store::{Detached, KeyStore};

/// Where the Key app is in its setup (`docs/DESIGN.md` §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupState {
    /// Nothing is stored.
    Empty,
    /// The identity (and key 2, if this device holds it) is stored; no
    /// wallet has been accepted yet.
    HasIdentity {
        /// Key 2 is on this device.
        holds_key2: bool,
        /// The words of the key just made have not been taken yet with
        /// `key_words`. They are not stored and cannot be shown again.
        words_waiting: bool,
    },
    /// The wallet and the owner root are pinned. Sessions can be opened.
    Ready {
        /// Key 2 is on this device.
        holds_key2: bool,
    },
    /// A wallet setup from the owner phone passed every check and is kept,
    /// not yet registered: the key holder compares its words with the other
    /// devices and then confirms or discards it (SR-67). Nothing is signed.
    WalletPending {
        /// Key 2 is on this device.
        holds_key2: bool,
    },
    /// The stored state is behind a PIN that has not been given yet, or is
    /// an older state that has no PIN yet (SR-72, SR-73). Nothing else can
    /// be asked until `lock_status` and then `unlock` or `set_pin` are
    /// called.
    Locked,
}

/// Where the lock of the stored state is (SR-72, SR-73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStatus {
    /// Nothing is stored and no PIN is set: set one before a key is made.
    NoPin,
    /// An older state without a PIN is stored: set a PIN, which locks it,
    /// and then go on.
    Legacy,
    /// The state is stored behind a PIN: unlock it.
    Locked,
    /// A PIN is set and the state is open.
    Open,
}

wire_unit_enum!(LockStatus {
    NoPin = 0,
    Legacy = 1,
    Locked = 2,
    Open = 3,
});

/// What the Key app is given when the owner phone has built the wallet
/// (`docs/DESIGN.md` §2.1 step 5).
#[derive(Debug, Clone)]
pub struct WalletSetup {
    /// The wallet description text, with its checksum.
    pub description: String,
    /// The network it is for.
    pub network: KeyNetwork,
    /// The owner root, 32 bytes, as the owner phone showed it. The Key app
    /// pins it.
    pub owner_root: Vec<u8>,
    /// Key 3's account xpub, from the other publisher's app. With key 2 on
    /// this device the signer checks that exactly one other slot holds it.
    pub key3_xpub: Option<String>,
    /// The fewest addresses a batch may hold, or `None` for the core
    /// default.
    pub batch_min_count: Option<u32>,
}

/// Key 2's public parts, for the owner phone to build the wallet from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key2Public {
    /// The network the key was made for.
    pub network: KeyNetwork,
    /// The master key's fingerprint, 8 hex characters.
    pub master_fingerprint: String,
    /// The origin path of the account key, for example `48h/1h/0h/2h`.
    pub origin_path: String,
    /// The account-level extended public key.
    pub account_xpub: String,
}

/// One key of the wallet, for the check on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotView {
    /// 1, 2 or 3.
    pub index: u32,
    /// The master fingerprint, 8 hex characters.
    pub fingerprint: String,
    /// The account xpub.
    pub xpub: String,
}

/// What the Key app shows to compare with the other devices
/// (`docs/DESIGN.md` §2.1 step 5). The words are for people; the software
/// compares full values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletView {
    /// The four checksum words of the wallet description.
    pub checksum_words: Vec<String>,
    /// The full wallet ID, 32 bytes.
    pub wallet_id: Vec<u8>,
    /// The three keys.
    pub slots: Vec<SlotView>,
    /// The first receive address, to compare with the owner phone.
    pub first_receive_address: String,
    /// The Key app's own four fingerprint words.
    pub own_root_words: Vec<String>,
    /// The pinned owner root's four fingerprint words.
    pub owner_root_words: Vec<String>,
}

wire_record!(Key2Public {
    network,
    master_fingerprint,
    origin_path,
    account_xpub,
});
wire_record!(SlotView {
    index,
    fingerprint,
    xpub,
});
wire_record!(WalletView {
    checksum_words,
    wallet_id,
    slots,
    first_receive_address,
    own_root_words,
    owner_root_words,
});
wire_record!(WalletSetup {
    description,
    network,
    owner_root,
    key3_xpub,
    batch_min_count,
});
wire_record_get!(WalletSetup {
    description,
    network,
    owner_root,
    key3_xpub,
    batch_min_count,
});

const STATE_EMPTY: u8 = 0;
const STATE_HAS_IDENTITY: u8 = 1;
const STATE_READY: u8 = 2;
const STATE_WALLET_PENDING: u8 = 3;
const STATE_LOCKED: u8 = 4;

impl Shaped for SetupState {
    fn shape() -> String {
        "SetupState".to_string()
    }
}

impl Put for SetupState {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Empty => out.push(STATE_EMPTY),
            Self::HasIdentity {
                holds_key2,
                words_waiting,
            } => {
                out.push(STATE_HAS_IDENTITY);
                holds_key2.put(out);
                words_waiting.put(out);
            }
            Self::Ready { holds_key2 } => {
                out.push(STATE_READY);
                holds_key2.put(out);
            }
            Self::WalletPending { holds_key2 } => {
                out.push(STATE_WALLET_PENDING);
                holds_key2.put(out);
            }
            Self::Locked => out.push(STATE_LOCKED),
        }
    }
}

impl Define for SetupState {
    fn define() -> TypeDef {
        TypeDef {
            name: "SetupState",
            body: "enum: Empty | HasIdentity{holds_key2: bool, words_waiting: bool} \
                   | Ready{holds_key2: bool} | WalletPending{holds_key2: bool} \
                   | Locked"
                .to_string(),
            tags: vec![
                ("Empty", STATE_EMPTY),
                ("HasIdentity", STATE_HAS_IDENTITY),
                ("Ready", STATE_READY),
                ("WalletPending", STATE_WALLET_PENDING),
                ("Locked", STATE_LOCKED),
            ],
        }
    }
}

/// The states of the Key app.
pub(crate) enum Phase {
    Empty,
    Identity(Box<Material>),
    Ready(Box<ReadyState>),
    /// A wallet setup that passed every check and waits for confirmation.
    Pending(Box<PendingState>),
}

/// A Key app that holds a checked wallet setup it has not registered
/// (SR-67). The approver is built, so that confirming moves it to
/// [`ReadyState`] without a check that could fail differently.
pub(crate) struct PendingState {
    pub material: Material,
    pub wallet: WalletPart,
    pub approver: Approver,
}

/// A Key app with its wallet accepted.
pub(crate) struct ReadyState {
    pub material: Material,
    pub wallet: WalletPart,
    pub approver: Approver,
}

/// Bytes that hold secrets, overwritten when dropped.
pub(crate) struct Wiped(Vec<u8>);

impl Wiped {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn copy_of(other: &Self) -> Self {
        Self(other.0.clone())
    }
}

impl Drop for Wiped {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

/// Whether the stored state has been opened (SR-72, SR-73).
pub(crate) enum Gate {
    /// The state is open, or nothing is stored.
    Open,
    /// The locked frame as stored, waiting for the PIN.
    Locked(Vec<u8>),
    /// An older state without a PIN, waiting for one.
    Legacy(Wiped),
}

/// Everything behind the lock.
pub(crate) struct Inner {
    /// What the Key app reads and writes through: every write is sealed
    /// under the PIN's key (SR-71).
    pub store: Arc<dyn KeyStore>,
    /// The platform's own store.
    pub raw: Arc<dyn KeyStore>,
    /// The key of the lock, once the PIN is set or given.
    pub lock: Arc<LockCell>,
    pub gate: Gate,
    pub poisoned: bool,
    /// The number of the session that is open, if one is.
    pub session: Option<u64>,
    pub next_session: u64,
    pub phase: Phase,
    /// The words of the key just made, until `key_words` takes them.
    pub words: Option<Vec<String>>,
}

impl Inner {
    fn set_words(&mut self, words: Option<Vec<String>>) {
        if let Some(old) = self.words.take() {
            old.into_iter().for_each(wipe_string);
        }
        self.words = words;
    }

    /// Own-screen and setup calls are refused while a session is open: the
    /// lines on screen would no longer be what the session works on.
    pub(crate) fn no_session(&self) -> Result<(), KeyError> {
        match self.session {
            None => Ok(()),
            Some(_) => Err(wrong_state(
                "session-open",
                "a session is open: finish or cancel it first",
            )),
        }
    }

    /// SR-73: every call but the lock's own is refused until the state is
    /// open.
    pub(crate) fn require_open(&self) -> Result<(), KeyError> {
        match self.gate {
            Gate::Open => Ok(()),
            Gate::Locked(_) => Err(wrong_state(
                "locked",
                "the Key app is locked: give the PIN first",
            )),
            Gate::Legacy(_) => Err(wrong_state(
                "pin-needed",
                "the stored key has no PIN yet: set one first",
            )),
        }
    }

    /// SR-72: refused until a PIN is set, so that nothing is stored without
    /// one.
    pub(crate) fn pin_set(&self) -> Result<(), KeyError> {
        let held = self.lock.lock().map(|g| g.is_some()).unwrap_or(false);
        if held {
            Ok(())
        } else {
            Err(wrong_state(
                "pin-first",
                "set a PIN before a key is made: nothing is stored without one",
            ))
        }
    }

    /// Refused while a wallet waits for confirmation: it is confirmed or
    /// discarded first (SR-67).
    pub(crate) fn not_pending(&self) -> Result<(), KeyError> {
        match &self.phase {
            Phase::Pending(_) => Err(wrong_state(
                "wallet-pending",
                "a wallet is waiting for the key holder: confirm it or discard it first",
            )),
            _ => Ok(()),
        }
    }

    pub(crate) fn ready(&mut self) -> Result<&mut ReadyState, KeyError> {
        match &mut self.phase {
            Phase::Ready(r) => Ok(r),
            Phase::Pending(_) => Err(wrong_state(
                "wallet-pending",
                "a wallet is waiting for the key holder: confirm it or discard it first",
            )),
            _ => Err(wrong_state(
                "not-ready",
                "the Key app has no wallet yet: finish the setup first",
            )),
        }
    }

    /// Key 3 as stored, to keep when the key made here is made again.
    fn key3_kept(&self) -> Option<openbmt_signer::KeyPart> {
        self.material().and_then(|m| m.key3.clone())
    }

    /// Key 2 as taken from its signer, to keep when the identity is made
    /// again with no key 2 of its own (SR-84).
    fn key2_taken_kept(&self) -> Option<openbmt_signer::KeyPart> {
        self.material().and_then(|m| m.key2_taken.clone())
    }

    pub(crate) fn material(&self) -> Option<&Material> {
        match &self.phase {
            Phase::Empty => None,
            Phase::Identity(m) => Some(&**m),
            Phase::Ready(r) => Some(&r.material),
            Phase::Pending(p) => Some(&p.material),
        }
    }
}

/// The Key app. One object holds the one approver behind a lock; every
/// rule is enforced in the existing crates, and every change of state is
/// stored through [`KeyStore::save`] before a call returns.
pub struct KeyApp {
    pub(crate) inner: Mutex<Inner>,
}

/// Store the envelope of a Key app that has its wallet.
pub(crate) fn save_ready(
    store: &Arc<dyn KeyStore>,
    material: &Material,
    wallet: &WalletPart,
    snapshot: &[u8],
) -> Result<(), crate::error::StoreError> {
    store.save(envelope::encode(material, Some((wallet, snapshot))))
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "a panic".to_string()
    }
}

impl KeyApp {
    /// Run `f` with the lock held. A panic inside `f` is caught, becomes
    /// [`KeyError::Internal`] and poisons the object: nothing after it
    /// touches the state again.
    pub(crate) fn run<T>(
        &self,
        f: impl FnOnce(&mut Inner) -> Result<T, KeyError>,
    ) -> Result<T, KeyError> {
        self.run_with(true, f)
    }

    /// [`run`](Self::run) for the calls that work while the state is still
    /// locked: the setup state and the lock's own calls (SR-73).
    pub(crate) fn run_locked<T>(
        &self,
        f: impl FnOnce(&mut Inner) -> Result<T, KeyError>,
    ) -> Result<T, KeyError> {
        self.run_with(false, f)
    }

    fn run_with<T>(
        &self,
        open_only: bool,
        f: impl FnOnce(&mut Inner) -> Result<T, KeyError>,
    ) -> Result<T, KeyError> {
        let mut guard = self.inner.lock().map_err(|_| {
            internal("an earlier panic poisoned the Key app: open it again from its stored bytes")
        })?;
        if guard.poisoned {
            return Err(internal(
                "an earlier panic poisoned the Key app: open it again from its stored bytes",
            ));
        }
        if open_only {
            guard.require_open()?;
        }
        match catch_unwind(AssertUnwindSafe(|| f(&mut guard))) {
            Ok(result) => result,
            Err(p) => {
                guard.poisoned = true;
                Err(internal(format!("caught a panic: {}", panic_text(&*p))))
            }
        }
    }

    /// Use `store` for the calls that follow. The call protocol hands the
    /// store in with every call, so that nothing of the app's side is kept
    /// between two calls.
    pub(crate) fn attach(&self, store: Arc<dyn KeyStore>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.store = Arc::new(Sealing::new(Arc::clone(&store), Arc::clone(&inner.lock)));
            inner.raw = store;
        }
    }

    /// Drop the store that [`attach`](Self::attach) set: every call that
    /// reaches for the store fails until the next `attach`.
    pub(crate) fn detach(&self) {
        self.attach(Arc::new(Detached));
    }

    fn open_inner(store: Arc<dyn KeyStore>) -> Result<Inner, KeyError> {
        let loaded = store.load().map_err(|e| KeyError::Store {
            rule: "load".to_string(),
            text: format!("could not read the stored state: {e}"),
        })?;
        let (phase, gate) = match loaded {
            None => (Phase::Empty, Gate::Open),
            Some(bytes) if lock::is_locked(&bytes) => {
                lock::decode(&bytes).map_err(|e| KeyError::Store {
                    rule: "OBMx".to_string(),
                    text: format!("OBMx: the stored state is not readable: {e}"),
                })?;
                (Phase::Empty, Gate::Locked(bytes))
            }
            Some(bytes) => {
                // An older state, with no PIN: read it now so that bytes
                // that are not a state are refused here, and keep it until
                // a PIN is set.
                phase_of(&bytes)?;
                (Phase::Empty, Gate::Legacy(Wiped(bytes)))
            }
        };
        let lock = Arc::new(LockCell::default());
        Ok(Inner {
            store: Arc::new(Sealing::new(Arc::clone(&store), Arc::clone(&lock))),
            raw: store,
            lock,
            gate,
            poisoned: false,
            session: None,
            next_session: 1,
            phase,
            words: None,
        })
    }
}

/// The state of the Key app that the `OBM7` bytes describe.
pub(crate) fn phase_of(bytes: &[u8]) -> Result<Phase, KeyError> {
    let decoded = envelope::decode(bytes).map_err(|e| KeyError::Store {
        rule: "OBM7".to_string(),
        text: format!("OBM7: the stored state is not readable: {e}"),
    })?;
    if let Some(setup) = decoded.pending {
        let parsed = crate::ceremony::wallet_setup(&setup)?;
        let (wallet, approver) = build_wallet(&decoded.material, parsed, true)?;
        return Ok(Phase::Pending(Box::new(PendingState {
            material: decoded.material,
            wallet,
            approver,
        })));
    }
    Ok(match decoded.wallet {
        None => Phase::Identity(Box::new(decoded.material)),
        Some((wallet, snapshot)) => {
            let description = WalletDescription::parse(&wallet.description, wallet.network)
                .map_err(|e| refused_by("OBM7", e))?;
            let approver = Approver::restore(
                decoded.material.identity.bytes(),
                description,
                wallet.owner,
                BatchPolicy {
                    min_count: wallet.batch_min_count,
                },
                decoded.material.key2.as_ref().map(|k| k.account),
                &snapshot,
            )
            .map_err(refused)?;
            Phase::Ready(Box::new(ReadyState {
                material: decoded.material,
                wallet,
                approver,
            }))
        }
    })
}

fn wipe_source(source: EntropySource) {
    match source {
        EntropySource::DiceHashed { rolls } | EntropySource::DiceSkewCorrected { rolls, .. } => {
            wipe_string(rolls);
        }
        EntropySource::TypedWords { words } => words.into_iter().for_each(wipe_string),
        EntropySource::Platform | EntropySource::CoinFlips { .. } => {}
    }
}

pub(crate) fn parse_xpub(text: &str) -> Result<Xpub, KeyError> {
    text.parse::<Xpub>()
        .map_err(|e| refused_by("xpub", format!("not an extended public key: {e}")))
}

fn words4(words: [&'static str; 4]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

impl KeyApp {
    /// Open the Key app from what `store` holds. An empty store gives an
    /// empty Key app; stored bytes that do not decode strictly, or that the
    /// approver refuses, are an error.
    pub fn open(store: Arc<dyn KeyStore>) -> Result<Arc<Self>, KeyError> {
        let inner = match catch_unwind(AssertUnwindSafe(|| Self::open_inner(store))) {
            Ok(r) => r?,
            Err(p) => return Err(internal(format!("caught a panic: {}", panic_text(&*p)))),
        };
        Ok(Arc::new(Self {
            inner: Mutex::new(inner),
        }))
    }

    /// Where the Key app is in its setup.
    pub fn state(&self) -> Result<SetupState, KeyError> {
        self.run_locked(|inner| {
            if !matches!(inner.gate, Gate::Open) {
                return Ok(SetupState::Locked);
            }
            let words_waiting = inner.words.is_some();
            Ok(match &inner.phase {
                Phase::Empty => SetupState::Empty,
                Phase::Identity(m) => SetupState::HasIdentity {
                    holds_key2: m.key2.is_some(),
                    words_waiting,
                },
                Phase::Ready(r) => SetupState::Ready {
                    holds_key2: r.material.key2.is_some(),
                },
                Phase::Pending(p) => SetupState::WalletPending {
                    holds_key2: p.material.key2.is_some(),
                },
            })
        })
    }

    /// `docs/DESIGN.md` §2.1 step 3: make the identity and key 2 on this
    /// device from `source`, and store them. The words are then waiting
    /// for one call of `key_words`; they are not stored. Allowed while no
    /// wallet has been accepted; making a key again replaces the old one.
    pub fn create_from_entropy(
        &self,
        source: EntropySource,
        network: KeyNetwork,
    ) -> Result<(), KeyError> {
        let result = self.run(|inner| {
            inner.no_session()?;
            inner.not_pending()?;
            inner.pin_set()?;
            if matches!(inner.phase, Phase::Ready(_)) {
                return Err(wrong_state(
                    "wallet-set",
                    "the wallet is accepted already; key 2 cannot be made again",
                ));
            }
            let (mut material, words) =
                make_material(&inner.store, Some((&source, network.into())))?;
            material.key3 = inner.key3_kept();
            inner
                .store
                .save(envelope::encode(&material, None))
                .map_err(|e| store_failed(&e))?;
            inner.set_words(words);
            inner.phase = Phase::Identity(Box::new(material));
            Ok(())
        });
        wipe_source(source);
        result
    }

    /// An approver only: make the identity and no key 2, for a Key app on
    /// a spare device when key 2 is on a hardware wallet. Spends are then
    /// declined with "not on this device" (AS-36).
    pub fn create_approver_only(&self) -> Result<(), KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            inner.not_pending()?;
            inner.pin_set()?;
            if matches!(inner.phase, Phase::Ready(_)) {
                return Err(wrong_state(
                    "wallet-set",
                    "the wallet is accepted already; the identity cannot be made again",
                ));
            }
            let (mut material, _) = make_material(&inner.store, None)?;
            material.key3 = inner.key3_kept();
            material.key2_taken = inner.key2_taken_kept();
            inner
                .store
                .save(envelope::encode(&material, None))
                .map_err(|e| store_failed(&e))?;
            inner.set_words(None);
            inner.phase = Phase::Identity(Box::new(material));
            Ok(())
        })
    }

    /// The words of the key just made, once. They are not stored: after
    /// this call, or after the object is closed, they cannot be shown
    /// again, and a second call is refused.
    pub fn key_words(&self) -> Result<Vec<String>, KeyError> {
        self.run(|inner| {
            inner.words.take().ok_or_else(|| {
                wrong_state(
                    "words-shown-once",
                    "the words were shown once and are not kept; they cannot be shown again",
                )
            })
        })
    }

    /// Key 2's public parts, to give to the owner phone (`docs/DESIGN.md`
    /// §2.1 step 5). `None` for an approver only.
    pub fn key2_public(&self) -> Result<Option<Key2Public>, KeyError> {
        self.run(|inner| {
            let Some(material) = inner.material() else {
                return Err(wrong_state("empty", "no key has been made yet"));
            };
            let Some(k) = &material.key2 else {
                return Ok(None);
            };
            let secp = Secp256k1::new();
            let network = match k.network {
                bitcoin::Network::Bitcoin => KeyNetwork::Mainnet,
                bitcoin::Network::Testnet => KeyNetwork::Testnet,
                bitcoin::Network::Signet => KeyNetwork::Signet,
                _ => KeyNetwork::Regtest,
            };
            let path = format!("48h/{}h/0h/2h", openbmt_core::wallet::coin_type(k.network));
            Ok(Some(Key2Public {
                network,
                master_fingerprint: k.master_fingerprint.to_string(),
                origin_path: path,
                account_xpub: Xpub::from_priv(&secp, &k.account).to_string(),
            }))
        })
    }

    /// The Key app's own root, 32 bytes, for the owner phone.
    pub fn device_root(&self) -> Result<Vec<u8>, KeyError> {
        self.run(|inner| {
            let Some(material) = inner.material() else {
                return Err(wrong_state("empty", "no key has been made yet"));
            };
            let identity =
                Identity::from_secret_bytes(material.identity.bytes()).map_err(internal)?;
            Ok(identity.device_id().to_bytes().to_vec())
        })
    }

    /// The Key app's own root as four words, for a person to compare.
    pub fn device_root_words(&self) -> Result<Vec<String>, KeyError> {
        self.run(|inner| {
            let Some(material) = inner.material() else {
                return Err(wrong_state("empty", "no key has been made yet"));
            };
            let identity =
                Identity::from_secret_bytes(material.identity.bytes()).map_err(internal)?;
            Ok(words4(identity.fingerprint_words()))
        })
    }

    /// `docs/DESIGN.md` §2.1 step 5: accept the wallet description and pin
    /// the owner root. With key 2 on this device the signer checks that its
    /// slot holds exactly this key (and key 3's holds key 3's); the state
    /// is stored before this returns.
    pub fn accept_wallet(&self, setup: WalletSetup) -> Result<WalletView, KeyError> {
        self.accept(setup, None)
    }

    /// The body of `accept_wallet`. With `pending` set (the `OBMw` bytes of
    /// the ceremony), key 3 must have been taken before (SR-67) and the
    /// wallet is kept as pending until it is confirmed (SR-67).
    pub(crate) fn accept(
        &self,
        setup: WalletSetup,
        pending: Option<Vec<u8>>,
    ) -> Result<WalletView, KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            inner.not_pending()?;
            let material =
                match &inner.phase {
                    Phase::Identity(m) => m,
                    Phase::Empty => {
                        return Err(wrong_state(
                            "empty",
                            "make the Key app's key first (create_from_entropy)",
                        ))
                    }
                    Phase::Ready(_) => {
                        return Err(wrong_state("wallet-set", "the wallet is accepted already"))
                    }
                    Phase::Pending(_) => return Err(wrong_state(
                        "wallet-pending",
                        "a wallet is waiting for the key holder: confirm it or discard it first",
                    )),
                };
            let (part, approver) = build_wallet(material, setup, pending.is_some())?;
            // On a device with no key 2 the only accept is the ceremony's, which
            // checks slot 2 against the key 2 taken (SR-88, SR-89).
            if pending.is_some() {
                crate::ceremony::check_key2_taken(material, approver.wallet(), part.network)?;
            } else if material.key2.is_none() {
                return Err(crate::ceremony::key2_missing());
            }
            let view = wallet_view(&approver)?;
            let bytes = match &pending {
                Some(setup_bytes) => envelope::encode_pending(material, setup_bytes),
                None => envelope::encode(material, Some((&part, &approver.snapshot()))),
            };
            inner.store.save(bytes).map_err(|e| store_failed(&e))?;
            let Phase::Identity(material) = std::mem::replace(&mut inner.phase, Phase::Empty)
            else {
                return Err(internal("the phase changed during setup"));
            };
            inner.phase = match pending {
                Some(_) => Phase::Pending(Box::new(PendingState {
                    material: *material,
                    wallet: part,
                    approver,
                })),
                None => Phase::Ready(Box::new(ReadyState {
                    material: *material,
                    wallet: part,
                    approver,
                })),
            };
            Ok(view)
        })
    }

    /// SR-67, `docs/DESIGN.md` §2.1 step 5: the key holder says the words
    /// and fingerprints match. The pending wallet becomes the registered
    /// wallet, stored before this returns; SR-63 applies from then on.
    pub fn confirm_wallet(&self) -> Result<(), KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            let Phase::Pending(p) = &inner.phase else {
                return Err(nothing_pending());
            };
            inner
                .store
                .save(envelope::encode(
                    &p.material,
                    Some((&p.wallet, &p.approver.snapshot())),
                ))
                .map_err(|e| store_failed(&e))?;
            let Phase::Pending(p) = std::mem::replace(&mut inner.phase, Phase::Empty) else {
                return Err(internal("the phase changed during setup"));
            };
            let PendingState {
                material,
                wallet,
                approver,
                ..
            } = *p;
            inner.phase = Phase::Ready(Box::new(ReadyState {
                material,
                wallet,
                approver,
            }));
            Ok(())
        })
    }

    /// SR-67: the key holder says the words or fingerprints differ. The
    /// pending wallet is dropped and the Key app is as it was before the
    /// wallet setup was accepted, with key 2 and key 3 kept; stored before
    /// this returns.
    pub fn discard_wallet(&self) -> Result<(), KeyError> {
        self.run(|inner| {
            inner.no_session()?;
            let Phase::Pending(p) = &inner.phase else {
                return Err(nothing_pending());
            };
            inner
                .store
                .save(envelope::encode(&p.material, None))
                .map_err(|e| store_failed(&e))?;
            let Phase::Pending(p) = std::mem::replace(&mut inner.phase, Phase::Empty) else {
                return Err(internal("the phase changed during setup"));
            };
            inner.phase = Phase::Identity(Box::new(p.material));
            Ok(())
        })
    }

    /// What the Key app shows of the accepted wallet, to compare with the
    /// other devices and the recovery kit.
    pub fn wallet_view(&self) -> Result<WalletView, KeyError> {
        self.run(|inner| match &inner.phase {
            Phase::Pending(p) => wallet_view(&p.approver),
            _ => wallet_view(&inner.ready()?.approver),
        })
    }

    /// The pair offer (`OBMO`) for the counter to scan: the Key app's
    /// own side of the pairing check (`docs/DESIGN.md` §2.9).
    pub fn pair_offer_bytes(&self) -> Result<Vec<u8>, KeyError> {
        self.run(|inner| {
            let a = &inner.ready()?.approver;
            PairOffer::new(
                a.owner(),
                a.device_id(),
                *a.wallet_id(),
                a.wallet().network(),
            )
            .map(|o| o.to_bytes())
            .map_err(refused)
        })
    }
}

fn nothing_pending() -> KeyError {
    wrong_state(
        "nothing-pending",
        "no wallet is waiting for confirmation: scan the owner phone's wallet first",
    )
}

/// Every check of a wallet setup, and the approver built from it, with the
/// signer's state attached when this device holds key 2. Nothing is stored
/// and nothing is changed. With `ceremony` set, key 3 must be held (SR-67).
pub(crate) fn build_wallet(
    material: &Material,
    setup: WalletSetup,
    ceremony: bool,
) -> Result<(WalletPart, Approver), KeyError> {
    let network: bitcoin::Network = setup.network.into();
    if let Some(k) = &material.key2 {
        if k.network != network {
            return Err(refused_by(
                "network",
                "key 2 was made for another network than the wallet's",
            ));
        }
    }
    let description = WalletDescription::parse(&setup.description, network)
        .map_err(|e| refused_by("wallet", e))?;
    let owner_bytes: [u8; 32] = setup
        .owner_root
        .as_slice()
        .try_into()
        .map_err(|_| refused_by("owner", "the owner root is not 32 bytes"))?;
    let owner = DeviceId::from_bytes(&owner_bytes).map_err(|e| refused_by("owner", e))?;
    let policy = setup
        .batch_min_count
        .map_or_else(BatchPolicy::default, |m| BatchPolicy { min_count: m });
    let mut approver = Approver::new(
        material.identity.bytes(),
        description.clone(),
        owner,
        policy,
    )
    .map_err(refused)?;
    let key3 = crate::ceremony::key3_for_accept(
        material,
        &description,
        network,
        setup.key3_xpub.as_deref(),
        ceremony,
    )?;
    if let Some(k) = &material.key2 {
        let xpub = Xpub::from_priv(&Secp256k1::new(), &k.account);
        let state = SignerState::register(
            description.canonical_string(),
            network,
            &xpub,
            key3.as_ref(),
        )
        .map_err(refused)?;
        approver.attach_signer(state, k.account).map_err(refused)?;
    }
    let part = WalletPart {
        network,
        description: description.canonical_string().to_string(),
        owner,
        batch_min_count: policy.min_count,
    };
    Ok((part, approver))
}

fn wallet_view(approver: &Approver) -> Result<WalletView, KeyError> {
    let wallet = approver.wallet();
    let fingerprints = wallet.fingerprints();
    let slots = wallet
        .slots()
        .iter()
        .zip(fingerprints)
        .map(|(s, fp)| SlotView {
            index: u32::from(s.index),
            fingerprint: fp.to_string(),
            xpub: s.xpub.to_string(),
        })
        .collect();
    let first = wallet
        .address(Keychain::Receive, 0)
        .map_err(|e| refused_by("wallet", e))?;
    Ok(WalletView {
        checksum_words: words4(wallet.checksum_words()),
        wallet_id: approver.wallet_id().to_vec(),
        slots,
        first_receive_address: first.to_string(),
        own_root_words: words4(approver.device_id().fingerprint_words()),
        owner_root_words: words4(approver.owner().fingerprint_words()),
    })
}
