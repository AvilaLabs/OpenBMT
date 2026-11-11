//! The owner phone's side of the key ceremony (`docs/DESIGN.md` §2.1 steps
//! 4 and 5, `docs/OWNER-COINS-RULES.md` OC-40 to OC-46).
//!
//! The steps are:
//!
//! 1. [`OwnerSetup::make_key1`] makes key 1 and the owner's root from the
//!    random bytes the caller passes.
//! 2. [`OwnerSetup::build_wallet`] reads the Key app's answer and builds the
//!    2-of-3 wallet from the three public keys. It returns the wallet setup
//!    for the Key app and the facts the owner compares.
//! 3. [`OwnerSetup::finish`] takes the owner's confirmation and gives the
//!    setup the values the node starts from ([`OwnerSetup::start_fields`]).
//!
//! The setup between the steps is the sealed record `OBMu`, which holds
//! secrets. This crate does not protect it: the app seals it, as it seals
//! its setup text today. No PIN or passphrase is modelled here (`docs/
//! DESIGN.md` §5.4 says key 1 has one): the record is as protected as the
//! app makes it.
//!
//! ```text
//! "OBMu" | 1 | network u8 | owner secret (32) | key 1 master fingerprint (4)
//!   | key 1 account key, BIP 32 secret form (78) | stage u8
//!   stage 1  key 1 made: nothing more
//!   stage 2  wallet built, not yet confirmed:
//!       | the Key app's root (32) | description u16 length and text
//!       | key 3 fingerprint (4) | key 3 account key (78) | batch minimum u32
//!   stage 3  finished: stage 2, then | refund depth u32 | birthday u32
//! ```

use std::fmt;
use std::str::FromStr;

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use openbmt_core::entropy::{words_from_bits, EntropyError, WordCount};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet::{coin_type, KeySlot, Keychain, WalletDescription, WalletError};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_signer::{CeremonyRefusal, KeyAnswer, KeyPart, WalletSetupBytes};

/// OC-44: the magic of the sealed record of the owner's setup.
pub const MAGIC_OWNER_SETUP: [u8; 4] = *b"OBMu";
/// OC-44: the longest wallet description in the record.
const MAX_DESCRIPTION: usize = 2048;
/// OC-45: the most confirmations a payment may be given to count as done.
pub const MAX_REFUND_DEPTH: u32 = 100;

/// Where the owner phone is in its setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStage {
    /// Key 1 is made. No wallet yet.
    KeyMade,
    /// The wallet is built and waits for the owner's confirmation.
    WalletBuilt,
    /// The owner confirmed. The node can start.
    Finished,
}

/// Why a step of the ceremony was refused. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeremonyError {
    /// SR-64 to SR-68: a key text, the answer or the setup was refused.
    Key(CeremonyRefusal),
    /// OC-40: the random bytes are not usable.
    Random,
    /// OC-41: key 2 is neither in the answer nor given as text.
    NoKey2,
    /// OC-41: key 2 is in the answer and was also given as text.
    Key2Twice,
    /// OC-41: the Key app's root is this phone's root.
    SameRoot,
    /// OC-41: the three keys do not make a valid wallet.
    Wallet(WalletError),
    /// OC-44: the sealed record is cut short, changed or inconsistent.
    Record,
    /// OC-45: the step is not allowed at this stage of the setup.
    Stage(SetupStage),
    /// OC-45: the refund depth is not 1 to 100.
    RefundDepth,
}

impl CeremonyError {
    /// The ID of the rule that was broken.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Key(r) => r.rule(),
            Self::Random => "OC-40",
            Self::NoKey2 | Self::Key2Twice | Self::SameRoot | Self::Wallet(_) => "OC-41",
            Self::Record => "OC-44",
            Self::Stage(_) | Self::RefundDepth => "OC-45",
        }
    }
}

impl fmt::Display for CeremonyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(r) => write!(f, "{r}"),
            Self::Wallet(e) => write!(f, "OC-41: the three keys do not make a wallet: {e}"),
            Self::Random => write!(f, "OC-40: the random bytes are not usable"),
            Self::NoKey2 => write!(f, "OC-41: key 2 is neither in the answer nor given"),
            Self::Key2Twice => write!(f, "OC-41: key 2 is in the answer and was also given"),
            Self::SameRoot => write!(f, "OC-41: the Key app's root is this phone's root"),
            Self::Record => write!(f, "OC-44: the sealed record is not in its form"),
            Self::Stage(_) => write!(f, "OC-45: this step is not allowed at this stage"),
            Self::RefundDepth => write!(f, "OC-45: the refund depth is not 1 to 100"),
        }
    }
}

impl std::error::Error for CeremonyError {}

impl From<CeremonyRefusal> for CeremonyError {
    fn from(r: CeremonyRefusal) -> Self {
        Self::Key(r)
    }
}

impl CeremonyError {
    /// What the owner is shown. Nothing was changed.
    pub fn words(&self) -> Words {
        let stop = |heading: &str, what: &str, next: &str| {
            Words::new(Tone::Stop, heading, what, Who::Owner, next).code(self.rule())
        };
        match self {
            Self::Key(r) => r.words_for(Who::Owner),
            Self::Random => stop(
                "No random numbers",
                "The phone gave no random numbers that can make a key, so no key was made.",
                "Try again. If it happens again, report it to the OpenBMT project.",
            ),
            Self::NoKey2 => stop(
                "Key 2 missing",
                "The Key app has no key 2, and no key 2 was given from another device.",
                "Scan key 2 from the device that holds it, then build the wallet again.",
            ),
            Self::Key2Twice => stop(
                "Key 2 given twice",
                "The Key app's answer holds key 2, so a second key 2 was not expected.",
                "Build the wallet again without the second key.",
            ),
            Self::SameRoot => stop(
                "Same phone twice",
                "The Key app and this phone show the same identity, which means one phone was used for both.",
                "Use two different phones, and start the setup again.",
            ),
            Self::Wallet(e) => wallet_words(e, self.rule()),
            Self::Record => Words::new(
                Tone::Stop,
                "Setup record not readable",
                "The saved setup of this phone is cut short or changed, so it cannot be used.",
                Who::Owner,
                "Do not use it. Start the setup again, and report it if it happens again.",
            )
            .code(self.rule()),
            Self::Stage(SetupStage::KeyMade) => stop(
                "Not at this step yet",
                "This step needs a wallet, and none has been built yet.",
                "Scan the Key app's answer and build the wallet first.",
            ),
            Self::Stage(SetupStage::WalletBuilt) => stop(
                "Not at this step now",
                "The wallet is built but not confirmed, so this step is not open.",
                "Compare the words and fingerprints on both phones, then confirm.",
            ),
            Self::Stage(SetupStage::Finished) => Words::new(
                Tone::Ended,
                "Setup already finished",
                "This phone is already set up, so this step cannot be done again.",
                Who::Nobody,
                "Nothing to do. For a different wallet, use \"Move to a new wallet\".",
            )
            .code(self.rule()),
            Self::RefundDepth => stop(
                "Refund depth not usable",
                "The number of blocks after which a payment counts as done must be between 1 and 100.",
                "Choose a number from 1 to 100.",
            ),
        }
    }
}

fn wallet_words(e: &WalletError, code: &'static str) -> Words {
    let what = match e {
        WalletError::DuplicateFingerprint | WalletError::DuplicateXpub => {
            "Two of the three keys are the same key, so they cannot make a shared wallet."
        }
        WalletError::NetworkMismatch { .. } => {
            "One of the three keys was made for another network than the wallet."
        }
        _ => "The three keys do not make a wallet of the kind this app builds.",
    };
    Words::new(
        Tone::Stop,
        "Wallet cannot be built",
        what,
        Who::Owner,
        "Check each key on its own device, and start the setup again.",
    )
    .code(code)
}

impl SetupStage {
    /// What the owner is shown at this stage of the setup.
    pub fn words(&self) -> Words {
        match self {
            Self::KeyMade => Words::new(
                Tone::Info,
                "Key 1 made on this phone",
                "This phone made key 1 and keeps it. The wallet needs the other two keys.",
                Who::Owner,
                "Scan the Key app's answer with this phone, then build the wallet.",
            ),
            Self::WalletBuilt => Words::new(
                Tone::Info,
                "Compare on every device",
                "The wallet is built. Every device must show the same four words and the same three short codes.",
                Who::Owner,
                "Compare them on the Key app and on this phone. Confirm only if they match.",
            ),
            Self::Finished => Words::new(
                Tone::Good,
                "Owner phone is set up",
                "This phone has its key, the wallet and the Key app's identity, and can start.",
                Who::Nobody,
                "Nothing to do. Go on with the recovery kit.",
            ),
        }
    }
}

/// The secret 32 bytes of the record, overwritten when dropped. Best
/// effort: the compiler and the system may keep other copies.
struct Secret32([u8; 32]);

impl Drop for Secret32 {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

/// What the record holds once the wallet is built.
#[derive(Clone, PartialEq, Eq)]
struct WalletPart {
    approver_root: DeviceId,
    description: WalletDescription,
    key3: KeyPart,
    batch_min_count: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Finish {
    refund_depth: u32,
    birthday: u32,
}

/// The owner phone's setup, at one of its three stages. It holds the
/// owner's secret and key 1's account key.
pub struct OwnerSetup {
    network: Network,
    owner_secret: Secret32,
    key1_fingerprint: Fingerprint,
    key1_account: Xpriv,
    wallet: Option<WalletPart>,
    finish: Option<Finish>,
}

impl Drop for OwnerSetup {
    fn drop(&mut self) {
        self.key1_account.private_key.non_secure_erase();
    }
}

impl fmt::Debug for OwnerSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OwnerSetup({:?}, <hidden>)", self.stage())
    }
}

impl Clone for OwnerSetup {
    fn clone(&self) -> Self {
        Self {
            network: self.network,
            owner_secret: Secret32(self.owner_secret.0),
            key1_fingerprint: self.key1_fingerprint,
            key1_account: self.key1_account,
            wallet: self.wallet.clone(),
            finish: self.finish,
        }
    }
}

/// Who holds a key, for the pairs screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// The owner's phone, in owner mode.
    OwnerPhone,
    /// The Key app on the spare phone.
    KeyApp,
    /// A signer made by someone else (a hardware wallet, or another
    /// wallet app) that holds key 2.
    OtherSigner,
    /// The wallet app of another publisher that made key 3.
    OtherWalletApp,
}

/// One pair of keys that can move the money together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    /// The two keys, 1 to 3, the lower first.
    pub keys: [u8; 2],
    /// Who holds each of the two.
    pub holders: [Holder; 2],
}

/// What the owner compares across devices after the wallet is built, and
/// the pairs screen (`docs/DESIGN.md` §2.1 step 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletFacts {
    /// The four checksum words of the whole description.
    pub checksum_words: [&'static str; 4],
    /// The master fingerprints of keys 1, 2 and 3.
    pub fingerprints: [Fingerprint; 3],
    /// The first receive address.
    pub first_receive_address: String,
    /// Every pair of keys and who holds each: (1, 2), (1, 3), (2, 3).
    pub pairs: [Pair; 3],
}

/// The result of [`OwnerSetup::build_wallet`].
pub struct WalletBuilt {
    /// The setup at stage 2, to keep until the owner confirms.
    pub setup: OwnerSetup,
    /// The wallet setup for the Key app.
    pub for_key_app: WalletSetupBytes,
    /// What to compare, and the pairs screen.
    pub facts: WalletFacts,
}

/// The values the node starts from, as the hand-made setup text carries
/// them. They hold the owner's secret and key 1's account key.
pub struct StartFields {
    /// The network.
    pub network: Network,
    /// The owner root's secret.
    pub owner_secret: [u8; 32],
    /// The Key app's root.
    pub approver_root: [u8; 32],
    /// The wallet description, canonical, with its checksum.
    pub wallet: String,
    /// Key 1's account key, 78 bytes.
    pub key1_account: [u8; 78],
    /// Key 3's account public key as text.
    pub key3_xpub: String,
    /// The fewest addresses a batch may hold.
    pub batch_min_count: u32,
    /// Confirmations at which a payment is done.
    pub refund_depth: u32,
    /// The wallet's first block.
    pub birthday: u32,
}

impl Drop for StartFields {
    fn drop(&mut self) {
        self.owner_secret.fill(0);
        self.key1_account.fill(0);
        std::hint::black_box(&mut self.key1_account);
    }
}

fn account_path(network: Network) -> DerivationPath {
    DerivationPath::from(vec![
        ChildNumber::Hardened { index: 48 },
        ChildNumber::Hardened {
            index: coin_type(network),
        },
        ChildNumber::Hardened { index: 0 },
        ChildNumber::Hardened { index: 2 },
    ])
}

fn bits_of(bytes: &[u8]) -> Vec<bool> {
    bytes
        .iter()
        .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1 == 1))
        .collect()
}

fn slot(index: u8, part: &KeyPart) -> Result<KeySlot, CeremonyError> {
    let origin_path = DerivationPath::from_str(&format!("m/{}", part.origin_path()))
        .map_err(|_| CeremonyError::Record)?;
    Ok(KeySlot {
        index,
        origin_fingerprint: part.fingerprint,
        origin_path,
        xpub: part.xpub,
    })
}

impl OwnerSetup {
    /// OC-40: make key 1 and the owner's root from `random`, 64 bytes of
    /// the system's random source. The first 32 are the entropy of 24 words
    /// and key 1's account key is at `m/48'/coin'/0'/2'` of them; the last
    /// 32 are the owner root's secret. Key 1 has no words by default: they
    /// are returned, once and not kept in the record, only when
    /// `show_words` is set. Refused when either half is all zero (a
    /// failed random source) or is not a usable secret.
    pub fn make_key1(
        random: &[u8; 64],
        network: Network,
        show_words: bool,
    ) -> Result<(Self, Option<Vec<String>>), CeremonyError> {
        let (entropy, secret) = random.split_at(32);
        if entropy.iter().all(|b| *b == 0) || secret.iter().all(|b| *b == 0) {
            return Err(CeremonyError::Random);
        }
        let secret: [u8; 32] = secret.try_into().map_err(|_| CeremonyError::Random)?;
        Identity::from_secret_bytes(&secret).map_err(|_| CeremonyError::Random)?;
        let words = words_from_bits(&bits_of(entropy), WordCount::TwentyFour)
            .map_err(|_: EntropyError| CeremonyError::Random)?;
        let shown = show_words.then(|| words.words().map(str::to_string).collect::<Vec<_>>());
        let mnemonic = words.into_mnemonic();
        let mut seed = mnemonic.to_seed_normalized("");
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(network, &seed);
        seed.fill(0);
        std::hint::black_box(&mut seed);
        let master = master.map_err(|_| CeremonyError::Random)?;
        let account = master
            .derive_priv(&secp, &account_path(network))
            .map_err(|_| CeremonyError::Random)?;
        Ok((
            Self {
                network,
                owner_secret: Secret32(secret),
                key1_fingerprint: master.fingerprint(&secp),
                key1_account: account,
                wallet: None,
                finish: None,
            },
            shown,
        ))
    }

    /// Where the setup is.
    pub fn stage(&self) -> SetupStage {
        match (&self.wallet, &self.finish) {
            (None, _) => SetupStage::KeyMade,
            (Some(_), None) => SetupStage::WalletBuilt,
            (Some(_), Some(_)) => SetupStage::Finished,
        }
    }

    /// The network.
    pub fn network(&self) -> Network {
        self.network
    }

    /// Key 1's public parts.
    pub fn key1_public(&self) -> KeyPart {
        KeyPart {
            fingerprint: self.key1_fingerprint,
            xpub: Xpub::from_priv(&Secp256k1::new(), &self.key1_account),
        }
    }

    /// The owner's root.
    pub fn owner_root(&self) -> Result<DeviceId, CeremonyError> {
        Identity::from_secret_bytes(&self.owner_secret.0)
            .map(|i| i.device_id())
            .map_err(|_| CeremonyError::Record)
    }

    /// The wallet's description, once built.
    pub fn description(&self) -> Option<&WalletDescription> {
        self.wallet.as_ref().map(|w| &w.description)
    }

    /// OC-41, OC-42, OC-43: read the Key app's answer and build the wallet
    /// from key 1, key 2 and key 3, in that order of slots. Key 2 is the
    /// answer's, or, when the Key app holds none (key 2 is on a hardware
    /// wallet), `key2_text`, read under SR-64 and SR-65. Allowed at stage 1
    /// and again at stage 2, not once finished. Nothing in `self` changes:
    /// the new setup is in the result.
    pub fn build_wallet(
        &self,
        answer: &KeyAnswer,
        key2_text: Option<&str>,
        batch_min_count: u32,
    ) -> Result<WalletBuilt, CeremonyError> {
        if self.stage() == SetupStage::Finished {
            return Err(CeremonyError::Stage(SetupStage::Finished));
        }
        let kind = bitcoin::NetworkKind::from(self.network);
        if bitcoin::NetworkKind::from(answer.network) != kind
            || (answer.key2.is_some() && answer.network != self.network)
        {
            return Err(CeremonyRefusal::NetworkDiffers.into());
        }
        let owner_root = self.owner_root()?;
        if answer.device_root == owner_root {
            return Err(CeremonyError::SameRoot);
        }
        let (key2, in_key_app) = match (&answer.key2, key2_text) {
            (Some(k), None) => (k.clone(), true),
            (None, Some(text)) => (KeyPart::parse(text)?, false),
            (None, None) => return Err(CeremonyError::NoKey2),
            (Some(_), Some(_)) => return Err(CeremonyError::Key2Twice),
        };
        if key2.kind() != kind {
            return Err(CeremonyRefusal::NetworkDiffers.into());
        }
        let key1 = self.key1_public();
        let slots = [slot(1, &key1)?, slot(2, &key2)?, slot(3, &answer.key3)?];
        let description =
            WalletDescription::new(self.network, slots).map_err(CeremonyError::Wallet)?;
        let part = WalletPart {
            approver_root: answer.device_root,
            description: description.clone(),
            key3: answer.key3.clone(),
            batch_min_count,
        };
        let mut setup = self.clone();
        setup.wallet = Some(part);
        setup.finish = None;
        let first_receive_address = description
            .address(Keychain::Receive, 0)
            .map_err(CeremonyError::Wallet)?
            .to_string();
        let k2_holder = if in_key_app {
            Holder::KeyApp
        } else {
            Holder::OtherSigner
        };
        let (h1, h2, h3) = (Holder::OwnerPhone, k2_holder, Holder::OtherWalletApp);
        let facts = WalletFacts {
            checksum_words: description.checksum_words(),
            fingerprints: description.fingerprints(),
            first_receive_address,
            pairs: [
                Pair {
                    keys: [1, 2],
                    holders: [h1, h2],
                },
                Pair {
                    keys: [1, 3],
                    holders: [h1, h3],
                },
                Pair {
                    keys: [2, 3],
                    holders: [h2, h3],
                },
            ],
        };
        Ok(WalletBuilt {
            setup,
            for_key_app: WalletSetupBytes {
                network: self.network,
                description,
                owner_root,
                batch_min_count,
            },
            facts,
        })
    }

    /// OC-45: the owner confirmed. Only a built wallet can be finished. The
    /// refund depth is 1 to 100. `birthday` is the wallet's first block.
    pub fn finish(&self, refund_depth: u32, birthday: u32) -> Result<Self, CeremonyError> {
        match self.stage() {
            SetupStage::WalletBuilt => {}
            stage => return Err(CeremonyError::Stage(stage)),
        }
        if !(1..=MAX_REFUND_DEPTH).contains(&refund_depth) {
            return Err(CeremonyError::RefundDepth);
        }
        let mut done = self.clone();
        done.finish = Some(Finish {
            refund_depth,
            birthday,
        });
        Ok(done)
    }

    /// OC-45: the values the node starts from, from a finished setup.
    pub fn start_fields(&self) -> Result<StartFields, CeremonyError> {
        let (Some(w), Some(f)) = (&self.wallet, &self.finish) else {
            return Err(CeremonyError::Stage(self.stage()));
        };
        Ok(StartFields {
            network: self.network,
            owner_secret: self.owner_secret.0,
            approver_root: w.approver_root.to_bytes(),
            wallet: w.description.canonical_string().to_string(),
            key1_account: self.key1_account.encode(),
            key3_xpub: w.key3.xpub.to_string(),
            batch_min_count: w.batch_min_count,
            refund_depth: f.refund_depth,
            birthday: f.birthday,
        })
    }

    /// The bytes of the sealed record. They hold secrets.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_OWNER_SETUP, 256);
        out.push(network_byte(self.network));
        out.extend_from_slice(&self.owner_secret.0);
        out.extend_from_slice(self.key1_fingerprint.as_bytes());
        out.extend_from_slice(&self.key1_account.encode());
        let stage: u8 = match self.stage() {
            SetupStage::KeyMade => 1,
            SetupStage::WalletBuilt => 2,
            SetupStage::Finished => 3,
        };
        out.push(stage);
        if let Some(w) = &self.wallet {
            out.extend_from_slice(&w.approver_root.to_bytes());
            let text = w.description.canonical_string();
            out.extend_from_slice(&u16::try_from(text.len()).unwrap_or(u16::MAX).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
            out.extend_from_slice(w.key3.fingerprint.as_bytes());
            out.extend_from_slice(&w.key3.xpub.encode());
            out.extend_from_slice(&w.batch_min_count.to_le_bytes());
        }
        if let Some(f) = &self.finish {
            out.extend_from_slice(&f.refund_depth.to_le_bytes());
            out.extend_from_slice(&f.birthday.to_le_bytes());
        }
        out
    }

    /// OC-44: read a sealed record, strictly: it decodes only for bytes
    /// that `to_bytes` writes, and only when it is consistent: key 1 is the
    /// wallet's first key, key 3 its third, the roots differ and the
    /// refund depth is in range.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CeremonyError> {
        Self::read(bytes).map_err(|_| CeremonyError::Record)
    }

    fn read(bytes: &[u8]) -> Result<Self, DecodeError> {
        let bad = DecodeError::NonCanonical("owner setup");
        let mut r = Reader::framed(bytes, &MAGIC_OWNER_SETUP)?;
        let network = network_from(r.u8()?)?;
        let owner_secret = Secret32(r.array::<32>()?);
        let key1_fingerprint = Fingerprint::from(r.array::<4>()?);
        let raw = r.array::<78>()?;
        let key1_account = Xpriv::decode(&raw).map_err(|_| bad.clone())?;
        // `Xpriv::decode` ignores the padding byte before the key.
        if key1_account.encode() != raw
            || key1_account.depth != 4
            || key1_account.child_number != (ChildNumber::Hardened { index: 2 })
            || key1_account.network != bitcoin::NetworkKind::from(network)
        {
            return Err(bad);
        }
        let identity = Identity::from_secret_bytes(&owner_secret.0).map_err(|_| bad.clone())?;
        let stage = r.u8()?;
        let (wallet, finish) = match stage {
            1 => (None, None),
            2 | 3 => {
                let approver_root = r.device_id()?;
                let n = usize::from(r.u16_le()?);
                if n > MAX_DESCRIPTION {
                    return Err(DecodeError::TooLarge);
                }
                let text = std::str::from_utf8(r.take(n)?).map_err(|_| bad.clone())?;
                let key3_fp = Fingerprint::from(r.array::<4>()?);
                let key3_raw = r.array::<78>()?;
                let batch_min_count = r.u32_le()?;
                let description =
                    WalletDescription::parse(text, network).map_err(|_| bad.clone())?;
                if description.canonical_string() != text {
                    return Err(bad);
                }
                let xpub = Xpub::decode(&key3_raw).map_err(|_| bad.clone())?;
                let key3 = KeyPart {
                    fingerprint: key3_fp,
                    xpub,
                };
                let key1 = KeyPart {
                    fingerprint: key1_fingerprint,
                    xpub: Xpub::from_priv(&Secp256k1::new(), &key1_account),
                };
                let slots = description.slots();
                let first = slots.first().ok_or_else(|| bad.clone())?;
                let third = slots.get(2).ok_or_else(|| bad.clone())?;
                if xpub.encode() != key3_raw
                    || !key1.is_slot(first, network)
                    || !key3.is_slot(third, network)
                    || approver_root == identity.device_id()
                {
                    return Err(bad);
                }
                let finish = if stage == 3 {
                    let refund_depth = r.u32_le()?;
                    let birthday = r.u32_le()?;
                    if !(1..=MAX_REFUND_DEPTH).contains(&refund_depth) {
                        return Err(bad);
                    }
                    Some(Finish {
                        refund_depth,
                        birthday,
                    })
                } else {
                    None
                };
                (
                    Some(WalletPart {
                        approver_root,
                        description,
                        key3,
                        batch_min_count,
                    }),
                    finish,
                )
            }
            _ => return Err(bad),
        };
        r.finish()?;
        Ok(Self {
            network,
            owner_secret,
            key1_fingerprint,
            key1_account,
            wallet,
            finish,
        })
    }
}

fn network_byte(network: Network) -> u8 {
    match network {
        Network::Bitcoin => 0,
        Network::Testnet => 1,
        Network::Signet => 2,
        _ => 3,
    }
}

fn network_from(byte: u8) -> Result<Network, DecodeError> {
    match byte {
        0 => Ok(Network::Bitcoin),
        1 => Ok(Network::Testnet),
        2 => Ok(Network::Signet),
        3 => Ok(Network::Regtest),
        _ => Err(DecodeError::NonCanonical("network byte")),
    }
}

/// The words of the pairs screen (`docs/DESIGN.md` §2.1 step 5): every
/// pair of the three keys can move all the money.
pub fn pairs_words() -> Words {
    Words::new(
        Tone::Info,
        "Any two keys can spend",
        "Any two of the three keys can move all of your money, and no single key can.",
        Who::Owner,
        "Check who holds each key. Confirm if it is right, or go back.",
    )
    .code("OC-43")
}

/// The words of every refusal and every stage of the ceremony, for the
/// lists of SC-12. Built by hand, one entry for each variant and each
/// variant inside it.
pub fn all_words() -> Vec<(String, Words)> {
    let mut all: Vec<(String, Words)> = Vec::new();
    for stage in [
        SetupStage::KeyMade,
        SetupStage::WalletBuilt,
        SetupStage::Finished,
    ] {
        all.push((format!("Setup::{stage:?}"), stage.words()));
    }
    all.push(("Setup::Pairs".to_string(), pairs_words()));
    for r in openbmt_signer::ALL_REFUSALS {
        all.push((
            format!("CeremonyError::Key({r:?})"),
            CeremonyError::Key(r).words(),
        ));
    }
    let others = [
        ("Random", CeremonyError::Random),
        ("NoKey2", CeremonyError::NoKey2),
        ("Key2Twice", CeremonyError::Key2Twice),
        ("SameRoot", CeremonyError::SameRoot),
        (
            "Wallet(DuplicateFingerprint)",
            CeremonyError::Wallet(WalletError::DuplicateFingerprint),
        ),
        (
            "Wallet(NetworkMismatch)",
            CeremonyError::Wallet(WalletError::NetworkMismatch { slot: 1 }),
        ),
        (
            "Wallet(Parse)",
            CeremonyError::Wallet(WalletError::Parse(String::new())),
        ),
        ("Record", CeremonyError::Record),
        ("Stage(KeyMade)", CeremonyError::Stage(SetupStage::KeyMade)),
        (
            "Stage(WalletBuilt)",
            CeremonyError::Stage(SetupStage::WalletBuilt),
        ),
        (
            "Stage(Finished)",
            CeremonyError::Stage(SetupStage::Finished),
        ),
        ("RefundDepth", CeremonyError::RefundDepth),
    ];
    for (name, e) in others {
        all.push((format!("CeremonyError::{name}"), e.words()));
    }
    all
}

#[cfg(test)]
#[path = "ceremony_tests.rs"]
mod tests;
