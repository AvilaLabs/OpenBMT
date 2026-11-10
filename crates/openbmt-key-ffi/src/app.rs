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

use crate::envelope::{self, wipe_string, Material, WalletPart};
use crate::error::{internal, refused, refused_by, store_failed, wrong_state, KeyError};
use crate::keys::{make_material, EntropySource, KeyNetwork};
use crate::store::KeyStore;

/// Where the Key app is in its setup (`docs/DESIGN.md` §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
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
}

/// What the Key app is given when the owner phone has built the wallet
/// (`docs/DESIGN.md` §2.1 step 5).
#[derive(Debug, Clone, uniffi::Record)]
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
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

/// The states of the Key app.
pub(crate) enum Phase {
    Empty,
    Identity(Material),
    Ready(Box<ReadyState>),
}

/// A Key app with its wallet accepted.
pub(crate) struct ReadyState {
    pub material: Material,
    pub wallet: WalletPart,
    pub approver: Approver,
}

/// Everything behind the lock.
pub(crate) struct Inner {
    pub store: Arc<dyn KeyStore>,
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

    pub(crate) fn ready(&mut self) -> Result<&mut ReadyState, KeyError> {
        match &mut self.phase {
            Phase::Ready(r) => Ok(r),
            _ => Err(wrong_state(
                "not-ready",
                "the Key app has no wallet yet: finish the setup first",
            )),
        }
    }

    fn material(&self) -> Option<&Material> {
        match &self.phase {
            Phase::Empty => None,
            Phase::Identity(m) => Some(m),
            Phase::Ready(r) => Some(&r.material),
        }
    }
}

/// The Key app. One object holds the one approver behind a lock; every
/// rule is enforced in the existing crates, and every change of state is
/// stored through [`KeyStore::save`] before a call returns.
#[derive(uniffi::Object)]
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
        let mut guard = self.inner.lock().map_err(|_| {
            internal("an earlier panic poisoned the Key app: open it again from its stored bytes")
        })?;
        if guard.poisoned {
            return Err(internal(
                "an earlier panic poisoned the Key app: open it again from its stored bytes",
            ));
        }
        match catch_unwind(AssertUnwindSafe(|| f(&mut guard))) {
            Ok(result) => result,
            Err(p) => {
                guard.poisoned = true;
                Err(internal(format!("caught a panic: {}", panic_text(&*p))))
            }
        }
    }

    fn open_inner(store: Arc<dyn KeyStore>) -> Result<Inner, KeyError> {
        let loaded = store.load().map_err(|e| KeyError::Store {
            rule: "load".to_string(),
            message: format!("could not read the stored state: {e}"),
        })?;
        let phase = match loaded {
            None => Phase::Empty,
            Some(bytes) => {
                let decoded = envelope::decode(&bytes).map_err(|e| KeyError::Store {
                    rule: "OBM7".to_string(),
                    message: format!("OBM7: the stored state is not readable: {e}"),
                })?;
                match decoded.wallet {
                    None => Phase::Identity(decoded.material),
                    Some((wallet, snapshot)) => {
                        let description =
                            WalletDescription::parse(&wallet.description, wallet.network)
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
                }
            }
        };
        Ok(Inner {
            store,
            poisoned: false,
            session: None,
            next_session: 1,
            phase,
            words: None,
        })
    }
}

fn wipe_source(source: EntropySource) {
    match source {
        EntropySource::DiceHashed { rolls } | EntropySource::DiceSkewCorrected { rolls, .. } => {
            wipe_string(rolls);
        }
        EntropySource::Platform | EntropySource::CoinFlips { .. } => {}
    }
}

fn parse_xpub(text: &str) -> Result<Xpub, KeyError> {
    text.parse::<Xpub>()
        .map_err(|e| refused_by("xpub", format!("not an extended public key: {e}")))
}

fn words4(words: [&'static str; 4]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

#[uniffi::export]
impl KeyApp {
    /// Open the Key app from what `store` holds. An empty store gives an
    /// empty Key app; stored bytes that do not decode strictly, or that the
    /// approver refuses, are an error.
    #[uniffi::constructor]
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
        self.run(|inner| {
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
            if matches!(inner.phase, Phase::Ready(_)) {
                return Err(wrong_state(
                    "wallet-set",
                    "the wallet is accepted already; key 2 cannot be made again",
                ));
            }
            let (material, words) = make_material(&inner.store, Some((&source, network.into())))?;
            inner
                .store
                .save(envelope::encode(&material, None))
                .map_err(|e| store_failed(&e))?;
            inner.set_words(words);
            inner.phase = Phase::Identity(material);
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
            if matches!(inner.phase, Phase::Ready(_)) {
                return Err(wrong_state(
                    "wallet-set",
                    "the wallet is accepted already; the identity cannot be made again",
                ));
            }
            let (material, _) = make_material(&inner.store, None)?;
            inner
                .store
                .save(envelope::encode(&material, None))
                .map_err(|e| store_failed(&e))?;
            inner.set_words(None);
            inner.phase = Phase::Identity(material);
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
        self.run(|inner| {
            inner.no_session()?;
            let material = match &inner.phase {
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
            };
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
            if let Some(k) = &material.key2 {
                let key3 = setup.key3_xpub.as_deref().map(parse_xpub).transpose()?;
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
            let view = wallet_view(&approver)?;
            inner
                .store
                .save(envelope::encode(
                    material,
                    Some((&part, &approver.snapshot())),
                ))
                .map_err(|e| store_failed(&e))?;
            let Phase::Identity(material) = std::mem::replace(&mut inner.phase, Phase::Empty)
            else {
                return Err(internal("the phase changed during setup"));
            };
            inner.phase = Phase::Ready(Box::new(ReadyState {
                material,
                wallet: part,
                approver,
            }));
            Ok(view)
        })
    }

    /// What the Key app shows of the accepted wallet, to compare with the
    /// other devices and the recovery kit.
    pub fn wallet_view(&self) -> Result<WalletView, KeyError> {
        self.run(|inner| wallet_view(&inner.ready()?.approver))
    }

    /// The pair offer (`OBMO`) for the owner phone to read: the Key app's
    /// own side of the pairing check (`docs/DESIGN.md` §2.9).
    pub fn pair_offer_bytes(&self) -> Result<Vec<u8>, KeyError> {
        self.run(|inner| {
            let a = &inner.ready()?.approver;
            PairOffer::new(a.owner(), a.device_id(), *a.wallet_id())
                .map(|o| o.to_bytes())
                .map_err(refused)
        })
    }
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
