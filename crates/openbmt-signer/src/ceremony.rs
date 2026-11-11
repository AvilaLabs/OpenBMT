//! The public parts of the key ceremony (`docs/DESIGN.md` §2.1 steps 2, 4
//! and 5; `docs/SIGNER-RULES.md` SR-64 to SR-69).
//!
//! Three things are read and written here, and nothing else: the text that
//! carries key 3 (or another key made elsewhere) to the Key app, the Key
//! app's public answer for the owner phone, and the wallet setup the owner
//! phone sends back. All of them are public values. No secret is in any
//! of them.
//!
//! ```text
//! OBMv  the Key app's public answer
//!   "OBMv" | 1 | network u8
//!   | key 2 flag u8, then if 1: fingerprint (4) | account key, BIP 32 form (78)
//!   | key 3: fingerprint (4) | account key (78)
//!   | the Key app's root (32)
//!
//! OBMw  the wallet setup
//!   "OBMw" | 1 | network u8
//!   | description u16 length and text (UTF-8, the canonical form)
//!   | the owner root (32) | batch minimum u32
//! ```
//!
//! The networks are 0 mainnet, 1 testnet, 2 signet, 3 regtest. A key's
//! origin path is not carried: it is always `48h/<coin>h/0h/2h` for the
//! network, so there is one way to write it. Decoding is strict, like the
//! other frames: it succeeds only for bytes that `to_bytes` would write.

use std::fmt;
use std::str::FromStr;

use bitcoin::base58;
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpub};
use bitcoin::{Network, NetworkKind};
use openbmt_core::identity::DeviceId;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_core::wire::{self, Reader};

/// SR-68: the magic of the Key app's public answer.
pub const MAGIC_KEY_ANSWER: [u8; 4] = *b"OBMv";
/// SR-68: the magic of the wallet setup the owner phone sends.
pub const MAGIC_WALLET_SETUP: [u8; 4] = *b"OBMw";
/// SR-64: the longest key text read.
pub const MAX_KEY_TEXT: usize = 256;
/// SR-68: the longest wallet description in a setup.
pub const MAX_SETUP_DESCRIPTION: usize = 2048;

/// The version bytes of an extended key, as they start the base58 text.
const XPUB: [u8; 4] = [0x04, 0x88, 0xB2, 0x1E];
const TPUB: [u8; 4] = [0x04, 0x35, 0x87, 0xCF];
/// SLIP 132 multisig P2WSH, the plain form's twin: mainnet and testnet.
const ZPUB_UPPER: [u8; 4] = [0x02, 0xAA, 0x7E, 0xD3];
const VPUB_UPPER: [u8; 4] = [0x02, 0x57, 0x54, 0x83];
/// SLIP 132 single signature: ypub, zpub, upub, vpub.
const SINGLE_SIG: [[u8; 4]; 4] = [
    [0x04, 0x9D, 0x7C, 0xB2],
    [0x04, 0xB2, 0x47, 0x46],
    [0x04, 0x4A, 0x52, 0x62],
    [0x04, 0x5F, 0x1C, 0xF6],
];
/// SLIP 132 multisig P2SH-P2WSH: Ypub, Upub.
const WRAPPED_MULTISIG: [[u8; 4]; 2] = [[0x02, 0x95, 0xB4, 0x3F], [0x02, 0x42, 0x89, 0xEF]];
/// Secret extended keys: xprv, tprv and the secret forms of SLIP-0132
/// (yprv, zprv, uprv, vprv, Yprv, Zprv, Uprv, Vprv).
const SECRET: [[u8; 4]; 10] = [
    [0x04, 0x88, 0xAD, 0xE4],
    [0x04, 0x35, 0x83, 0x94],
    [0x04, 0x9D, 0x78, 0x78],
    [0x04, 0xB2, 0x43, 0x0C],
    [0x04, 0x4A, 0x4E, 0x28],
    [0x04, 0x5F, 0x18, 0xBC],
    [0x02, 0x95, 0xB0, 0x05],
    [0x02, 0xAA, 0x7A, 0x99],
    [0x02, 0x42, 0x85, 0xB5],
    [0x02, 0x57, 0x50, 0x48],
];

/// Why a key text, the answer or the setup was not taken. Every text is
/// fixed, so that words can be looked up from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyRefusal {
    /// SR-64: empty, too long or not text a key is written in.
    NotKeyText,
    /// SR-64: no origin (`[fingerprint/path]`) in front of the key.
    NoOrigin,
    /// SR-64: the origin is not `fingerprint/48h/<coin>h/0h/2h` for the
    /// key's network.
    BadOrigin,
    /// SR-64: the text is not an extended public key.
    NotAKey,
    /// SR-64: the text is an extended secret key.
    SecretKey,
    /// SR-64: the key is not at the account level (depth 4, child 2h).
    BadLevel,
    /// SR-65: a single-signature key (ypub, zpub, upub, vpub).
    SingleSignature,
    /// SR-65: a multisig key of another script type (Ypub, Upub).
    OtherScript,
    /// SR-66: the key is for another kind of network than the wallet's.
    NetworkDiffers,
    /// SR-66: the key is key 2.
    SameAsKey2,
    /// SR-67: the Key app holds no key 3 yet.
    NoKey3,
    /// SR-67: key 3 as given is not the key 3 the Key app holds.
    Key3Differs,
    /// SR-67: no key of the wallet is the key 3 the Key app holds.
    Key3NotInWallet,
    /// SR-68: the Key app's answer is cut short, changed or not in its
    /// form.
    AnswerFrame,
    /// SR-68: the wallet setup is cut short, changed or not in its form.
    SetupFrame,
}

/// Every refusal, once, for the lists of words and for the tests.
pub const ALL_REFUSALS: [CeremonyRefusal; 15] = [
    CeremonyRefusal::NotKeyText,
    CeremonyRefusal::NoOrigin,
    CeremonyRefusal::BadOrigin,
    CeremonyRefusal::NotAKey,
    CeremonyRefusal::SecretKey,
    CeremonyRefusal::BadLevel,
    CeremonyRefusal::SingleSignature,
    CeremonyRefusal::OtherScript,
    CeremonyRefusal::NetworkDiffers,
    CeremonyRefusal::SameAsKey2,
    CeremonyRefusal::NoKey3,
    CeremonyRefusal::Key3Differs,
    CeremonyRefusal::Key3NotInWallet,
    CeremonyRefusal::AnswerFrame,
    CeremonyRefusal::SetupFrame,
];

impl CeremonyRefusal {
    /// The ID of the rule in `docs/SIGNER-RULES.md` that was broken.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NotKeyText
            | Self::NoOrigin
            | Self::BadOrigin
            | Self::NotAKey
            | Self::SecretKey
            | Self::BadLevel => "SR-64",
            Self::SingleSignature | Self::OtherScript => "SR-65",
            Self::NetworkDiffers | Self::SameAsKey2 => "SR-66",
            Self::NoKey3 | Self::Key3Differs | Self::Key3NotInWallet => "SR-67",
            Self::AnswerFrame | Self::SetupFrame => "SR-68",
        }
    }

    fn text(&self) -> &'static str {
        match self {
            Self::NotKeyText => "the text is empty, too long or not a key",
            Self::NoOrigin => "the key has no origin in front of it",
            Self::BadOrigin => "the origin is not a fingerprint and a 48h multisig path",
            Self::NotAKey => "the text is not an extended public key",
            Self::SecretKey => "the text is an extended secret key",
            Self::BadLevel => "the key is not at the account level",
            Self::SingleSignature => "the key is a single-signature key",
            Self::OtherScript => "the key is a multisig key of another script type",
            Self::NetworkDiffers => "the key is for another network",
            Self::SameAsKey2 => "the key is key 2",
            Self::NoKey3 => "no key 3 has been taken yet",
            Self::Key3Differs => "the key 3 given is not the key 3 taken",
            Self::Key3NotInWallet => "no key of the wallet is the key 3 taken",
            Self::AnswerFrame => "the answer is cut short, changed or not in its form",
            Self::SetupFrame => "the setup is cut short, changed or not in its form",
        }
    }

    /// The refusal whose `Display` text is `text`.
    pub fn from_text(text: &str) -> Option<Self> {
        ALL_REFUSALS.into_iter().find(|r| r.to_string() == text)
    }

    /// What the key holder is shown. Nothing was changed.
    pub fn words(&self) -> Words {
        self.words_for(Who::KeyHolder)
    }

    /// What `who` is shown: the key holder (on the Key app) or the owner
    /// (on the owner phone). Only the step to take differs. Nothing was
    /// changed.
    pub fn words_for(&self, who: Who) -> Words {
        let key_holder = who == Who::KeyHolder;
        let (heading, what, key_next, owner_next) = match self {
            Self::NotKeyText => (
                "Not a key",
                "The text that was scanned is empty, too long or is not a key.",
                "Scan the key again from the other wallet app.",
                "Scan the key again from the other wallet app.",
            ),
            Self::NoOrigin => (
                "Key has no origin",
                "The key came without its fingerprint and path, so this app cannot tell which key it is.",
                "In the other wallet app, export the key with its origin, then scan it again.",
                "Export the key with its origin from where it was made, then scan it again.",
            ),
            Self::BadOrigin => (
                "Key on another path",
                "The key does not come from the path that a shared wallet uses.",
                "Make or export the key for a shared wallet of native segwit type, then scan it again.",
                "Make or export the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::NotAKey => (
                "Not a public key",
                "The text that was scanned is not a public key that this app can read.",
                "Scan the key again from the other wallet app.",
                "Scan the key again from where it was made.",
            ),
            Self::SecretKey => (
                "That is a secret key",
                "The text that was scanned is a secret key, which must never leave the app that made it.",
                "Do not use it. Make a new key 3, and tell the owner in person.",
                "Do not use it. Make a new key, and tell the key holder in person.",
            ),
            Self::BadLevel => (
                "Key at the wrong level",
                "The key is not the key of a shared wallet's account.",
                "Export the account key for a shared wallet, then scan it again.",
                "Export the account key for a shared wallet, then scan it again.",
            ),
            Self::SingleSignature => (
                "Single-signature key",
                "This is a key for a wallet that one key spends from, not for a shared wallet.",
                "In the other wallet app, export the key for a shared wallet of native segwit type, then scan it again.",
                "Export the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::OtherScript => (
                "Shared key of another type",
                "This key is for a shared wallet of another type than this one.",
                "In the other wallet app, export the key for a shared wallet of native segwit type, then scan it again.",
                "Export the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::NetworkDiffers => (
                "Key for another network",
                "The key was made for a different network than the one in use.",
                "Make or export the key for the right network, then scan it again.",
                "Start the setup again on the network the Key app was set up for.",
            ),
            Self::SameAsKey2 => (
                "Key 3 is key 2",
                "The key scanned is the same key as the one on this phone.",
                "Do not use it. Make a different key 3 in the other wallet app, then scan it.",
                "Do not use it. Make a different key, then scan it.",
            ),
            Self::NoKey3 => (
                "Key 3 not taken yet",
                "This app has not been given key 3 yet, so it cannot check a wallet.",
                "Scan key 3 from the other wallet app first.",
                "Ask the key holder to scan key 3 into the Key app first.",
            ),
            Self::Key3Differs => (
                "Key 3 not the one taken",
                "The key 3 in this setup is not the key 3 this app was given.",
                "Do not accept the wallet, and tell the owner in person.",
                "Do not go on. Check key 3 on both phones, and start the setup again.",
            ),
            Self::Key3NotInWallet => (
                "Key 3 not in the wallet",
                "None of the wallet's keys is the key 3 this app was given.",
                "Do not accept the wallet, and tell the owner in person.",
                "Do not go on. Check key 3 on both phones, and start the setup again.",
            ),
            Self::AnswerFrame => (
                "Answer not readable",
                "What was scanned from the Key app is cut short, changed or not an answer of its kind.",
                "Show the answer again and scan it again.",
                "Ask the key holder to show the answer again, then scan it again.",
            ),
            Self::SetupFrame => (
                "Wallet code not readable",
                "What was scanned from the owner's phone is cut short, changed or not a wallet code.",
                "Ask the owner to show the wallet code again, then scan it again.",
                "Show the wallet code again.",
            ),
        };
        let next = if key_holder { key_next } else { owner_next };
        let (tone, who) = (
            Tone::Stop,
            if key_holder {
                Who::KeyHolder
            } else {
                Who::Owner
            },
        );
        Words::new(tone, heading, what, who, next).code(self.rule())
    }
}

impl fmt::Display for CeremonyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.text())
    }
}

impl std::error::Error for CeremonyRefusal {}

/// One key of the wallet as the ceremony passes it on: its master
/// fingerprint and its account public key. The origin path is the one of
/// the network (`48h/<coin>h/0h/2h`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPart {
    /// The master key's fingerprint.
    pub fingerprint: Fingerprint,
    /// The account-level public key.
    pub xpub: Xpub,
}

impl KeyPart {
    /// SR-64, SR-65: read key text. See the module of rules: the form is
    /// `[<8 hex>/48h/<coin>h/0h/2h]<key>`, with `'` allowed for `h`, where
    /// the key is an `xpub`, a `tpub`, or the multisig P2WSH `Zpub` or
    /// `Vpub`, read as the plain form of the same key. Surrounding white
    /// space is ignored. Nothing is guessed: a key without its origin is
    /// refused.
    pub fn parse(text: &str) -> Result<Self, CeremonyRefusal> {
        let text = text.trim();
        if text.is_empty() || text.len() > MAX_KEY_TEXT || !text.is_ascii() {
            return Err(CeremonyRefusal::NotKeyText);
        }
        let (origin, key_text) = match text.strip_prefix('[') {
            Some(rest) => {
                let (origin, key) = rest.split_once(']').ok_or(CeremonyRefusal::BadOrigin)?;
                (Some(origin), key)
            }
            None => (None, text),
        };
        let bytes = base58::decode_check(key_text).map_err(|_| CeremonyRefusal::NotAKey)?;
        let version: [u8; 4] = bytes
            .get(..4)
            .and_then(|v| v.try_into().ok())
            .ok_or(CeremonyRefusal::NotAKey)?;
        if bytes.len() != 78 {
            return Err(CeremonyRefusal::NotAKey);
        }
        if SECRET.contains(&version) {
            return Err(CeremonyRefusal::SecretKey);
        }
        if SINGLE_SIG.contains(&version) {
            return Err(CeremonyRefusal::SingleSignature);
        }
        if WRAPPED_MULTISIG.contains(&version) {
            return Err(CeremonyRefusal::OtherScript);
        }
        let plain = match version {
            v if v == XPUB || v == TPUB => version,
            v if v == ZPUB_UPPER => XPUB,
            v if v == VPUB_UPPER => TPUB,
            _ => return Err(CeremonyRefusal::NotAKey),
        };
        let origin = origin.ok_or(CeremonyRefusal::NoOrigin)?;
        let mut plain_bytes = bytes.clone();
        for (dst, src) in plain_bytes.iter_mut().zip(plain) {
            *dst = src;
        }
        let xpub = Xpub::decode(&plain_bytes).map_err(|_| CeremonyRefusal::NotAKey)?;
        if xpub.depth != 4 || xpub.child_number != (ChildNumber::Hardened { index: 2 }) {
            return Err(CeremonyRefusal::BadLevel);
        }
        let fingerprint = parse_origin(origin, xpub.network)?;
        Ok(Self { fingerprint, xpub })
    }

    /// The master fingerprint and the 78 bytes of the account key, as the
    /// frames and the Key app's stored state keep them.
    pub fn to_raw(&self) -> ([u8; 4], [u8; 78]) {
        (*self.fingerprint.as_bytes(), self.xpub.encode())
    }

    /// The key from [`to_raw`](Self::to_raw), strictly: the bytes are a
    /// public key in its one form, at the account level. `None` for any
    /// other bytes.
    pub fn from_raw(fingerprint: [u8; 4], raw: &[u8; 78]) -> Option<Self> {
        let xpub = Xpub::decode(raw).ok()?;
        if xpub.encode() != *raw
            || xpub.depth != 4
            || xpub.child_number != (ChildNumber::Hardened { index: 2 })
        {
            return None;
        }
        Some(Self {
            fingerprint: Fingerprint::from(fingerprint),
            xpub,
        })
    }

    /// The network kind the key is for.
    pub fn kind(&self) -> NetworkKind {
        self.xpub.network
    }

    /// The origin path as written in a description: `48h/0h/0h/2h` on
    /// mainnet and `48h/1h/0h/2h` on the others.
    pub fn origin_path(&self) -> String {
        origin_path(self.xpub.network)
    }

    /// The key as text with its origin, in the one form this crate reads
    /// back: `[fingerprint/48h/<coin>h/0h/2h]xpub`.
    pub fn origin_text(&self) -> String {
        format!("[{}/{}]{}", self.fingerprint, self.origin_path(), self.xpub)
    }

    /// Whether this is the key in `slot`, in full: the account key, the
    /// fingerprint and the path.
    pub fn is_slot(&self, slot: &KeySlot, network: Network) -> bool {
        slot.xpub.encode() == self.xpub.encode()
            && slot.origin_fingerprint == self.fingerprint
            && self.xpub.network == NetworkKind::from(network)
            && DerivationPath::from_str(&format!("m/{}", self.origin_path()))
                .is_ok_and(|p| p == slot.origin_path)
    }

    /// Whether `other` is the same key: the same account key or the same
    /// fingerprint.
    pub fn same_as(&self, other: &KeyPart) -> bool {
        self.xpub.encode() == other.xpub.encode() || self.fingerprint == other.fingerprint
    }
}

fn origin_path(kind: NetworkKind) -> String {
    let coin = if kind == NetworkKind::Main { 0 } else { 1 };
    format!("48h/{coin}h/0h/2h")
}

/// The fingerprint of an origin `ffffffff/48h/<coin>h/0h/2h`, where `h` may
/// be `'`, for a key of `kind`.
fn parse_origin(origin: &str, kind: NetworkKind) -> Result<Fingerprint, CeremonyRefusal> {
    let bad = CeremonyRefusal::BadOrigin;
    let (fp, path) = origin.split_once('/').ok_or(bad)?;
    if fp.len() != 8 || !fp.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad);
    }
    let fingerprint = Fingerprint::from_str(fp).map_err(|_| bad)?;
    let normal = path.replace('\'', "h");
    if normal != origin_path(kind) {
        return Err(bad);
    }
    Ok(fingerprint)
}

fn network_byte(network: Network) -> Option<u8> {
    match network {
        Network::Bitcoin => Some(0),
        Network::Testnet => Some(1),
        Network::Signet => Some(2),
        Network::Regtest => Some(3),
        _ => None,
    }
}

fn network_from(byte: u8) -> Result<Network, CeremonyRefusal> {
    match byte {
        0 => Ok(Network::Bitcoin),
        1 => Ok(Network::Testnet),
        2 => Ok(Network::Signet),
        3 => Ok(Network::Regtest),
        _ => Err(CeremonyRefusal::AnswerFrame),
    }
}

fn put_part(out: &mut Vec<u8>, part: &KeyPart) {
    out.extend_from_slice(part.fingerprint.as_bytes());
    out.extend_from_slice(&part.xpub.encode());
}

fn read_part(r: &mut Reader<'_>, kind: NetworkKind) -> Result<KeyPart, CeremonyRefusal> {
    let bad = CeremonyRefusal::AnswerFrame;
    let fingerprint = r.array::<4>().map_err(|_| bad)?;
    let raw = r.array::<78>().map_err(|_| bad)?;
    let part = KeyPart::from_raw(fingerprint, &raw).ok_or(bad)?;
    if part.xpub.network != kind {
        return Err(bad);
    }
    Ok(part)
}

/// SR-68, SR-69: what the Key app tells the owner phone so that the wallet
/// can be built: public values only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAnswer {
    /// The network. With key 2 on the device it is key 2's own network.
    /// A device with no key 2 knows only the kind of network its key 3 is
    /// for, and writes `Bitcoin` for main and `Testnet` for the others.
    pub network: Network,
    /// Key 2, when the Key app holds it.
    pub key2: Option<KeyPart>,
    /// Key 3, as the Key app took it.
    pub key3: KeyPart,
    /// The Key app's own root.
    pub device_root: DeviceId,
}

impl KeyAnswer {
    /// The bytes of the answer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_KEY_ANSWER, 1 + 1 + 82 + 82 + 32);
        out.push(network_byte(self.network).unwrap_or(u8::MAX));
        match &self.key2 {
            Some(k) => {
                out.push(1);
                put_part(&mut out, k);
            }
            None => out.push(0),
        }
        put_part(&mut out, &self.key3);
        out.extend_from_slice(&self.device_root.to_bytes());
        out
    }

    /// SR-68: read an answer, strictly. Both keys are for the network's
    /// kind, at the account level, and are different keys.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CeremonyRefusal> {
        let bad = CeremonyRefusal::AnswerFrame;
        let mut r = Reader::framed(bytes, &MAGIC_KEY_ANSWER).map_err(|_| bad)?;
        let network = network_from(r.u8().map_err(|_| bad)?)?;
        let kind = NetworkKind::from(network);
        let key2 = if r.flag().map_err(|_| bad)? {
            Some(read_part(&mut r, kind)?)
        } else {
            None
        };
        let key3 = read_part(&mut r, kind)?;
        let device_root = r.device_id().map_err(|_| bad)?;
        r.finish().map_err(|_| bad)?;
        if key2.as_ref().is_some_and(|k| k.same_as(&key3)) {
            return Err(bad);
        }
        Ok(Self {
            network,
            key2,
            key3,
            device_root,
        })
    }
}

/// SR-68: the wallet the owner phone built, with what the Key app pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletSetupBytes {
    /// The network the wallet is for.
    pub network: Network,
    /// The wallet, in its canonical form.
    pub description: WalletDescription,
    /// The owner root the Key app pins.
    pub owner_root: DeviceId,
    /// The fewest addresses a batch may hold.
    pub batch_min_count: u32,
}

impl WalletSetupBytes {
    /// The bytes of the setup.
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = self.description.canonical_string();
        let mut out = wire::start(&MAGIC_WALLET_SETUP, 3 + text.len() + 36);
        out.push(network_byte(self.network).unwrap_or(u8::MAX));
        out.extend_from_slice(&u16::try_from(text.len()).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&self.owner_root.to_bytes());
        out.extend_from_slice(&self.batch_min_count.to_le_bytes());
        out
    }

    /// SR-68: read a setup, strictly. The description must be the
    /// canonical text of a wallet that is valid for the network.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CeremonyRefusal> {
        let bad = CeremonyRefusal::SetupFrame;
        let mut r = Reader::framed(bytes, &MAGIC_WALLET_SETUP).map_err(|_| bad)?;
        let network = match r.u8().map_err(|_| bad)? {
            0 => Network::Bitcoin,
            1 => Network::Testnet,
            2 => Network::Signet,
            3 => Network::Regtest,
            _ => return Err(bad),
        };
        let n = usize::from(r.u16_le().map_err(|_| bad)?);
        if n > MAX_SETUP_DESCRIPTION {
            return Err(bad);
        }
        let text = std::str::from_utf8(r.take(n).map_err(|_| bad)?).map_err(|_| bad)?;
        let owner_root = r.device_id().map_err(|_| bad)?;
        let batch_min_count = r.u32_le().map_err(|_| bad)?;
        r.finish().map_err(|_| bad)?;
        let description = WalletDescription::parse(text, network).map_err(|_| bad)?;
        if description.canonical_string() != text {
            return Err(bad);
        }
        Ok(Self {
            network,
            description,
            owner_root,
            batch_min_count,
        })
    }
}

/// SR-66: whether a key of `kind` may go in a wallet of `network`, and is
/// not key 2.
pub fn check_key3(
    key3: &KeyPart,
    key2: Option<&KeyPart>,
    network: Network,
) -> Result<(), CeremonyRefusal> {
    if key3.kind() != NetworkKind::from(network) {
        return Err(CeremonyRefusal::NetworkDiffers);
    }
    if key2.is_some_and(|k| k.same_as(key3)) {
        return Err(CeremonyRefusal::SameAsKey2);
    }
    Ok(())
}

/// SR-67: exactly one key of the wallet is `key3`, in full (account key,
/// fingerprint and path).
pub fn check_key3_in_wallet(
    key3: &KeyPart,
    wallet: &WalletDescription,
) -> Result<(), CeremonyRefusal> {
    let holders = wallet
        .slots()
        .iter()
        .filter(|s| key3.is_slot(s, wallet.network()))
        .count();
    if holders == 1 {
        Ok(())
    } else {
        Err(CeremonyRefusal::Key3NotInWallet)
    }
}

/// The words of a key taken at step 2: shown so that the owner can compare
/// the fingerprint on both screens.
pub fn key3_taken_words() -> Words {
    Words::new(
        Tone::Good,
        "Key 3 taken",
        "This app now holds the public part of key 3, and nothing else of it.",
        Who::KeyHolder,
        "Compare the short code shown here with the one in the other wallet app, then go on.",
    )
    .code("SR-67")
}

/// The words of the Key app's answer, once shown.
pub fn answer_ready_words() -> Words {
    Words::new(
        Tone::Info,
        "Answer ready for the owner",
        "This app shows its public details, which the owner's phone needs to build the wallet.",
        Who::Owner,
        "Scan the answer with the owner's phone, then wait for the wallet code.",
    )
    .code("SR-69")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_text_is_found_again_from_its_display() {
        for r in ALL_REFUSALS {
            assert_eq!(CeremonyRefusal::from_text(&r.to_string()), Some(r));
        }
    }
}
