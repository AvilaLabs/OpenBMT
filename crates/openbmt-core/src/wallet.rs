//! The 2-of-3 wallet description.
//!
//! The wallet is P2WSH `sortedmulti` 2-of-3 with BIP48 script type 2' paths
//! and BIP389 `<0;1>/*` multipath for receive and change. Its stored form is
//! pinned: one hardened marker (`h`), full key origins, keys in slot order and
//! a BIP380 checksum:
//!
//! ```text
//! wsh(sortedmulti(2,[fp1/48h/0h/0h/2h]xpub1/<0;1>/*,[fp2/48h/0h/0h/2h]xpub2/<0;1>/*,[fp3/48h/0h/0h/2h]xpub3/<0;1>/*))#checksum
//! ```
//!
//! Slot order is part of the pinned string (so the string is the same on every
//! device), but the wallet's scripts do not depend on it: `sortedmulti` sorts
//! the derived public keys (BIP67).

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpub};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::opcodes::all::{OP_CHECKMULTISIG, OP_PUSHNUM_2, OP_PUSHNUM_3};
use bitcoin::script::Builder;
use bitcoin::{Address, Network, NetworkKind, PublicKey, ScriptBuf};
use miniscript::descriptor::DescriptorPublicKey;
use miniscript::Descriptor;

use crate::identity::secp;
use crate::words::{fingerprint_words, WORD_COUNT};

const INPUT_CHARSET: &str =
    "0123456789()[],'/*abcdefgh@:$%{}IJKLMNOPQRSTUVWXYZ&+-.;<=>?!^_|~ijklmnopqrstuvwxyzABCDEFGH`#\"\\ ";
const CHECKSUM_CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const GENERATOR: [u64; 5] = [
    0xf5dee51989,
    0xa9fdca3312,
    0x1bab10e32d,
    0x3706b1677a,
    0x644d626ffd,
];

fn polymod(symbols: &[u8]) -> u64 {
    let mut chk: u64 = 1;
    for v in symbols {
        let top = chk >> 35;
        chk = ((chk & 0x7_ffff_ffff) << 5) ^ u64::from(*v);
        for (i, g) in GENERATOR.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

fn expand(s: &str) -> Option<Vec<u8>> {
    let mut symbols = Vec::with_capacity(s.len() + s.len() / 3 + 1);
    let mut groups: Vec<u8> = Vec::with_capacity(3);
    for c in s.chars() {
        let v = INPUT_CHARSET.find(c)? as u8;
        symbols.push(v & 31);
        groups.push(v >> 5);
        if groups.len() == 3 {
            symbols.push(groups[0] * 9 + groups[1] * 3 + groups[2]);
            groups.clear();
        }
    }
    match groups.len() {
        1 => symbols.push(groups[0]),
        2 => symbols.push(groups[0] * 3 + groups[1]),
        _ => {}
    }
    Some(symbols)
}

/// The 8-character BIP380 checksum of a descriptor body (no `#`).
/// `None` if the body has a character outside the descriptor alphabet.
pub fn descriptor_checksum(body: &str) -> Option<String> {
    let mut symbols = expand(body)?;
    symbols.extend_from_slice(&[0; 8]);
    let chk = polymod(&symbols) ^ 1;
    Some(
        (0..8)
            .map(|i| CHECKSUM_CHARSET[((chk >> (5 * (7 - i))) & 31) as usize] as char)
            .collect(),
    )
}

/// Split `body#checksum` and verify the checksum. Returns the body.
pub(crate) fn split_and_verify(text: &str) -> Result<&str, WalletError> {
    let (body, sum) = text.rsplit_once('#').ok_or(WalletError::ChecksumMissing)?;
    let want = descriptor_checksum(body)
        .ok_or_else(|| WalletError::Parse("character outside the descriptor alphabet".into()))?;
    if sum != want {
        return Err(WalletError::ChecksumMismatch);
    }
    Ok(body)
}

/// Why a wallet description was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletError {
    /// A slot's index is not 1, 2, 3 in order.
    BadSlotIndex,
    /// A slot's origin path is not BIP48 script type 2' for this network.
    BadPath { slot: u8 },
    /// A slot's xpub is not an account-level key (depth 4, child 2').
    BadXpubDepth { slot: u8 },
    /// An xpub was made for a different network than the wallet.
    NetworkMismatch { slot: u8 },
    /// Two slots share a master fingerprint.
    DuplicateFingerprint,
    /// Two slots share an xpub.
    DuplicateXpub,
    /// The text has no `#checksum`.
    ChecksumMissing,
    /// The checksum does not match the text.
    ChecksumMismatch,
    /// The text is not in the pinned form.
    NonCanonical,
    /// The text could not be read.
    Parse(String),
    /// The index is not a valid non-hardened child index.
    IndexOutOfRange,
    /// Key derivation failed.
    Derive(String),
}

impl fmt::Display for WalletError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSlotIndex => write!(f, "key slots must be numbered 1, 2, 3 in order"),
            Self::BadPath { slot } => write!(f, "key {slot} is not on a BIP48 script type 2' path"),
            Self::BadXpubDepth { slot } => write!(f, "key {slot} is not an account-level xpub"),
            Self::NetworkMismatch { slot } => write!(f, "key {slot} is for a different network"),
            Self::DuplicateFingerprint => write!(f, "two keys share a master fingerprint"),
            Self::DuplicateXpub => write!(f, "two keys are the same xpub"),
            Self::ChecksumMissing => write!(f, "descriptor has no checksum"),
            Self::ChecksumMismatch => write!(f, "descriptor checksum does not match"),
            Self::NonCanonical => write!(f, "descriptor is not in the pinned form"),
            Self::Parse(s) => write!(f, "cannot read descriptor: {s}"),
            Self::IndexOutOfRange => write!(f, "address index out of range"),
            Self::Derive(s) => write!(f, "key derivation failed: {s}"),
        }
    }
}

impl std::error::Error for WalletError {}

/// Receive or change branch of the wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Keychain {
    /// `/0/*`
    Receive,
    /// `/1/*`
    Change,
}

impl Keychain {
    fn child(self) -> u32 {
        match self {
            Self::Receive => 0,
            Self::Change => 1,
        }
    }
}

/// BIP44-style coin type for a network: 0 for mainnet, 1 for everything else.
pub fn coin_type(network: Network) -> u32 {
    if NetworkKind::from(network) == NetworkKind::Main {
        0
    } else {
        1
    }
}

/// One of the three keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySlot {
    /// 1, 2 or 3.
    pub index: u8,
    /// Master fingerprint of the key.
    pub origin_fingerprint: Fingerprint,
    /// Path from the master key to `xpub`.
    pub origin_path: DerivationPath,
    /// Account-level extended public key.
    pub xpub: Xpub,
}

impl KeySlot {
    fn expected_path(network: Network) -> DerivationPath {
        DerivationPath::from(vec![
            ChildNumber::Hardened { index: 48 },
            ChildNumber::Hardened {
                index: coin_type(network),
            },
            ChildNumber::Hardened { index: 0 },
            ChildNumber::Hardened { index: 2 },
        ])
    }

    fn validate(&self, network: Network) -> Result<(), WalletError> {
        if self.origin_path != Self::expected_path(network) {
            return Err(WalletError::BadPath { slot: self.index });
        }
        if self.xpub.network != NetworkKind::from(network) {
            return Err(WalletError::NetworkMismatch { slot: self.index });
        }
        if self.xpub.depth != 4 || self.xpub.child_number != (ChildNumber::Hardened { index: 2 }) {
            return Err(WalletError::BadXpubDepth { slot: self.index });
        }
        Ok(())
    }

    fn key_expression(&self, network: Network, suffix: &str) -> String {
        format!(
            "[{}/48h/{}h/0h/2h]{}/{}",
            self.origin_fingerprint,
            coin_type(network),
            self.xpub,
            suffix
        )
    }
}

/// The validated 2-of-3 wallet description.
#[derive(Debug, Clone)]
pub struct WalletDescription {
    network: Network,
    slots: [KeySlot; 3],
    canonical: String,
    chains: [Descriptor<DescriptorPublicKey>; 2],
}

impl PartialEq for WalletDescription {
    fn eq(&self, other: &Self) -> bool {
        self.network == other.network && self.canonical == other.canonical
    }
}
impl Eq for WalletDescription {}

impl WalletDescription {
    /// Validate three slots (in order 1, 2, 3) for `network`.
    pub fn new(network: Network, slots: [KeySlot; 3]) -> Result<Self, WalletError> {
        for (i, s) in slots.iter().enumerate() {
            if usize::from(s.index) != i + 1 {
                return Err(WalletError::BadSlotIndex);
            }
        }
        for s in &slots {
            s.validate(network)?;
        }
        let fps: HashSet<_> = slots.iter().map(|s| s.origin_fingerprint).collect();
        if fps.len() != 3 {
            return Err(WalletError::DuplicateFingerprint);
        }
        let xps: HashSet<_> = slots.iter().map(|s| s.xpub.encode()).collect();
        if xps.len() != 3 {
            return Err(WalletError::DuplicateXpub);
        }
        let body = Self::body(network, &slots, "<0;1>/*");
        let canonical = format!("{body}#{}", descriptor_checksum(&body).expect("alphabet"));
        let multi = Descriptor::<DescriptorPublicKey>::from_str(&canonical)
            .map_err(|e| WalletError::Parse(e.to_string()))?;
        let singles = multi
            .into_single_descriptors()
            .map_err(|e| WalletError::Parse(e.to_string()))?;
        let chains: [Descriptor<DescriptorPublicKey>; 2] = singles
            .try_into()
            .map_err(|_| WalletError::Parse("expected two chains".into()))?;
        Ok(Self {
            network,
            slots,
            canonical,
            chains,
        })
    }

    fn body(network: Network, slots: &[KeySlot; 3], suffix: &str) -> String {
        format!(
            "wsh(sortedmulti(2,{},{},{}))",
            slots[0].key_expression(network, suffix),
            slots[1].key_expression(network, suffix),
            slots[2].key_expression(network, suffix)
        )
    }

    /// Read the pinned form back (with `h` or `'` markers). The checksum must
    /// be present and correct, and the text must describe the same wallet as
    /// its canonical form, which is what `canonical_string` then returns.
    pub fn parse(text: &str, network: Network) -> Result<Self, WalletError> {
        let body = split_and_verify(text)?;
        let inner = body
            .strip_prefix("wsh(sortedmulti(2,")
            .and_then(|s| s.strip_suffix("))"))
            .ok_or(WalletError::NonCanonical)?;
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 3 {
            return Err(WalletError::NonCanonical);
        }
        let mut slots = Vec::with_capacity(3);
        for (i, p) in parts.iter().enumerate() {
            slots.push(Self::parse_key(p, (i + 1) as u8)?);
        }
        let slots: [KeySlot; 3] = slots.try_into().expect("three");
        let wallet = Self::new(network, slots)?;
        if wallet.canonical.split('#').next() != Some(&body.replace('\'', "h")) {
            return Err(WalletError::NonCanonical);
        }
        Ok(wallet)
    }

    fn parse_key(expr: &str, index: u8) -> Result<KeySlot, WalletError> {
        let perr = |s: &str| WalletError::Parse(s.to_string());
        let rest = expr
            .strip_prefix('[')
            .ok_or_else(|| perr("missing origin"))?;
        let (origin, after) = rest
            .split_once(']')
            .ok_or_else(|| perr("unclosed origin"))?;
        let xpub_text = after
            .strip_suffix("/<0;1>/*")
            .ok_or(WalletError::NonCanonical)?;
        let (fp, path) = origin
            .split_once('/')
            .ok_or_else(|| perr("origin has no path"))?;
        let origin_fingerprint = Fingerprint::from_str(fp).map_err(|e| perr(&e.to_string()))?;
        let origin_path =
            DerivationPath::from_str(&format!("m/{path}")).map_err(|e| perr(&e.to_string()))?;
        let xpub = Xpub::from_str(xpub_text).map_err(|e| perr(&e.to_string()))?;
        Ok(KeySlot {
            index,
            origin_fingerprint,
            origin_path,
            xpub,
        })
    }

    /// The network this wallet is for.
    pub fn network(&self) -> Network {
        self.network
    }

    /// The three slots in order.
    pub fn slots(&self) -> &[KeySlot; 3] {
        &self.slots
    }

    /// The pinned multipath descriptor with its checksum.
    pub fn canonical_string(&self) -> &str {
        &self.canonical
    }

    /// SHA-256 of the canonical string (including `#checksum`). Batches name
    /// the wallet they belong to by this value.
    pub fn checksum32(&self) -> [u8; 32] {
        sha256::Hash::hash(self.canonical.as_bytes()).to_byte_array()
    }

    /// Four stretched BIP39 words (see `words`) of `checksum32`, which is the
    /// SHA-256 of the full canonical string, `#checksum` included. Every
    /// device shows the same four words for the same wallet; the owner
    /// compares them across devices (setup, and after every app update). A
    /// human cross-check only; software compares the full 32-byte hash.
    pub fn checksum_words(&self) -> [&'static str; WORD_COUNT] {
        fingerprint_words(&self.checksum32())
    }

    /// The two single-path descriptors (receive `/0/*`, change `/1/*`), each
    /// with its own checksum, for software without `<0;1>` support.
    pub fn single_path_strings(&self) -> [String; 2] {
        let make = |suffix: &str| {
            let body = Self::body(self.network, &self.slots, suffix);
            format!("{body}#{}", descriptor_checksum(&body).expect("alphabet"))
        };
        [make("0/*"), make("1/*")]
    }

    /// Master fingerprints in slot order.
    pub fn fingerprints(&self) -> [Fingerprint; 3] {
        [
            self.slots[0].origin_fingerprint,
            self.slots[1].origin_fingerprint,
            self.slots[2].origin_fingerprint,
        ]
    }

    /// Whether `xpub` is exactly the key in slot `slot` (1..=3). Compares the
    /// serialized xpub byte for byte, not just the fingerprint.
    pub fn slot_matches(&self, slot: u8, xpub: &Xpub) -> bool {
        match slot {
            1..=3 => self.slots[usize::from(slot) - 1].xpub.encode() == xpub.encode(),
            _ => false,
        }
    }

    /// The script at `index` on `chain`.
    pub fn script(&self, chain: Keychain, index: u32) -> Result<ScriptBuf, WalletError> {
        if index >= 1 << 31 {
            return Err(WalletError::IndexOutOfRange);
        }
        let d = &self.chains[chain.child() as usize];
        let definite = d
            .at_derivation_index(index)
            .map_err(|e| WalletError::Derive(e.to_string()))?;
        let derived = definite
            .derived_descriptor(secp())
            .map_err(|e| WalletError::Derive(e.to_string()))?;
        Ok(derived.script_pubkey())
    }

    /// The three compressed public keys at `<chain>/<index>`, in slot order
    /// (slot 1, 2, 3), each derived from its slot's account xpub with
    /// unhardened derivation. The index must be below 2^31.
    pub fn derived_keys(&self, chain: Keychain, index: u32) -> Result<[PublicKey; 3], WalletError> {
        if index >= 1 << 31 {
            return Err(WalletError::IndexOutOfRange);
        }
        let path = [
            ChildNumber::Normal {
                index: chain.child(),
            },
            ChildNumber::Normal { index },
        ];
        let mut keys = Vec::with_capacity(3);
        for slot in &self.slots {
            let child = slot
                .xpub
                .derive_pub(secp(), &path)
                .map_err(|e| WalletError::Derive(e.to_string()))?;
            keys.push(PublicKey::new(child.public_key));
        }
        Ok(keys.try_into().expect("three keys"))
    }

    /// The witness script at `index` on `chain`: `OP_2 <k> <k> <k> OP_3
    /// OP_CHECKMULTISIG` with the keys sorted by their compressed bytes
    /// (BIP67, as `sortedmulti` sorts them). It is 105 bytes long.
    pub fn witness_script(&self, chain: Keychain, index: u32) -> Result<ScriptBuf, WalletError> {
        let mut keys = self.derived_keys(chain, index)?;
        keys.sort_by_key(|k| k.inner.serialize());
        Ok(Builder::new()
            .push_opcode(OP_PUSHNUM_2)
            .push_key(&keys[0])
            .push_key(&keys[1])
            .push_key(&keys[2])
            .push_opcode(OP_PUSHNUM_3)
            .push_opcode(OP_CHECKMULTISIG)
            .into_script())
    }

    /// Receive script at `index`.
    pub fn receive_script(&self, index: u32) -> Result<ScriptBuf, WalletError> {
        self.script(Keychain::Receive, index)
    }

    /// Change script at `index`.
    pub fn change_script(&self, index: u32) -> Result<ScriptBuf, WalletError> {
        self.script(Keychain::Change, index)
    }

    /// The address at `index` on `chain`.
    pub fn address(&self, chain: Keychain, index: u32) -> Result<Address, WalletError> {
        let script = self.script(chain, index)?;
        Address::from_script(&script, self.network).map_err(|e| WalletError::Derive(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{slot_from_mnemonic, wallet, MNEMONICS};
    use bitcoin::bip32::Xpriv;

    #[test]
    fn bip380_checksum_test_vector() {
        assert_eq!(descriptor_checksum("raw(deadbeef)").unwrap(), "89f8spxm");
    }

    #[test]
    fn checksum_rejects_foreign_characters() {
        assert!(descriptor_checksum("raw(déadbeef)").is_none());
    }

    #[test]
    fn checksum_agrees_with_miniscript() {
        // Miniscript prints the apostrophe marker, so compare its own
        // checksum against the project value over the printed text, and the bodies after
        // normalising the marker.
        let w = wallet(Network::Testnet);
        let mut all = vec![w.canonical_string().to_string()];
        all.extend(w.single_path_strings());
        for ours in all {
            let d = Descriptor::<DescriptorPublicKey>::from_str(&ours).unwrap();
            let printed = d.to_string();
            let (mbody, msum) = printed.rsplit_once('#').unwrap();
            assert_eq!(descriptor_checksum(mbody).unwrap(), msum);
            let (obody, _) = ours.rsplit_once('#').unwrap();
            assert_eq!(mbody.replace('\'', "h"), obody);
        }
    }

    #[test]
    fn mnemonic_fixture_matches_known_master_fingerprint() {
        // BIP39 "abandon ... about" with an empty passphrase.
        let s = slot_from_mnemonic(MNEMONICS[0], 1, Network::Bitcoin);
        assert_eq!(s.origin_fingerprint.to_string(), "73c5da0a");
    }

    #[test]
    fn canonical_form_is_pinned() {
        let w = wallet(Network::Testnet);
        let c = w.canonical_string();
        assert!(c.starts_with("wsh(sortedmulti(2,[") && c.contains("/48h/1h/0h/2h]tpub"));
        assert!(c.contains("/<0;1>/*,["));
        assert!(!c.contains('\''));
        let (body, sum) = c.rsplit_once('#').unwrap();
        assert_eq!(sum.len(), 8);
        assert_eq!(descriptor_checksum(body).unwrap(), sum);
        // keys appear in slot order
        let fps = w.fingerprints();
        let pos: Vec<_> = fps
            .iter()
            .map(|f| c.find(&f.to_string()).unwrap())
            .collect();
        assert!(pos[0] < pos[1] && pos[1] < pos[2]);
    }

    #[test]
    fn mainnet_uses_coin_zero() {
        let w = wallet(Network::Bitcoin);
        assert!(w.canonical_string().contains("/48h/0h/0h/2h]xpub"));
        assert!(w
            .address(Keychain::Receive, 0)
            .unwrap()
            .to_string()
            .starts_with("bc1q"));
    }

    #[test]
    fn single_path_strings_are_receive_then_change() {
        let w = wallet(Network::Regtest);
        let [r, c] = w.single_path_strings();
        assert!(r.contains("/0/*") && !r.contains("<0;1>") && r.rsplit_once('#').is_some());
        assert!(c.contains("/1/*") && !c.contains("<0;1>"));
        for (s, chain) in [(&r, Keychain::Receive), (&c, Keychain::Change)] {
            let d = Descriptor::<DescriptorPublicKey>::from_str(s).unwrap();
            let got = d
                .at_derivation_index(5)
                .unwrap()
                .derived_descriptor(secp())
                .unwrap()
                .script_pubkey();
            assert_eq!(got, w.script(chain, 5).unwrap());
        }
    }

    #[test]
    fn checksum_words_follow_the_documented_rule() {
        let w = wallet(Network::Testnet);
        let digest = sha256::Hash::hash(w.canonical_string().as_bytes()).to_byte_array();
        // Documented rule: stretch the digest, then take its first 44 bits.
        let digest = crate::words::stretch(&digest);
        let list = bip39::Language::English.word_list();
        let mut bits = 0u64;
        for b in &digest[..6] {
            bits = (bits << 8) | u64::from(*b);
        }
        bits >>= 4;
        let expect = [
            list[((bits >> 33) & 0x7ff) as usize],
            list[((bits >> 22) & 0x7ff) as usize],
            list[((bits >> 11) & 0x7ff) as usize],
            list[(bits & 0x7ff) as usize],
        ];
        assert_eq!(w.checksum_words(), expect);
    }

    #[test]
    fn checksum_words_change_with_any_key() {
        let a = wallet(Network::Testnet);
        let slots = [
            slot_from_mnemonic(MNEMONICS[0], 1, Network::Testnet),
            slot_from_mnemonic(MNEMONICS[1], 2, Network::Testnet),
            slot_from_mnemonic(MNEMONICS[3], 3, Network::Testnet),
        ];
        let b = WalletDescription::new(Network::Testnet, slots).unwrap();
        assert_ne!(a.checksum32(), b.checksum32());
    }

    fn three(net: Network) -> [KeySlot; 3] {
        [
            slot_from_mnemonic(MNEMONICS[0], 1, net),
            slot_from_mnemonic(MNEMONICS[1], 2, net),
            slot_from_mnemonic(MNEMONICS[2], 3, net),
        ]
    }

    #[test]
    fn rejects_duplicate_fingerprints() {
        let mut s = three(Network::Testnet);
        s[2].origin_fingerprint = s[0].origin_fingerprint;
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::DuplicateFingerprint
        );
    }

    #[test]
    fn rejects_same_seed_in_two_slots() {
        let s = [
            slot_from_mnemonic(MNEMONICS[0], 1, Network::Testnet),
            slot_from_mnemonic(MNEMONICS[1], 2, Network::Testnet),
            slot_from_mnemonic(MNEMONICS[0], 3, Network::Testnet),
        ];
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::DuplicateFingerprint
        );
    }

    #[test]
    fn rejects_duplicate_xpubs() {
        let mut s = three(Network::Testnet);
        s[2].xpub = s[1].xpub;
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::DuplicateXpub
        );
    }

    #[test]
    fn rejects_network_mismatch_both_ways() {
        let s = three(Network::Bitcoin);
        assert_eq!(
            WalletDescription::new(Network::Testnet, s.clone()).unwrap_err(),
            WalletError::BadPath { slot: 1 }
        );
        // Path says testnet but the xpub is mainnet.
        let mut s = three(Network::Testnet);
        s[1].xpub = slot_from_mnemonic(MNEMONICS[1], 2, Network::Bitcoin).xpub;
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::NetworkMismatch { slot: 2 }
        );
        // Mainnet wallet given testnet xpubs.
        let mut s = three(Network::Bitcoin);
        s[0].xpub = slot_from_mnemonic(MNEMONICS[0], 1, Network::Testnet).xpub;
        assert_eq!(
            WalletDescription::new(Network::Bitcoin, s).unwrap_err(),
            WalletError::NetworkMismatch { slot: 1 }
        );
    }

    #[test]
    fn rejects_wrong_paths() {
        let bad = [
            "m/48h/1h/0h/1h",   // script type 1' (P2SH-P2WSH)
            "m/84h/1h/0h",      // single-sig path
            "m/48h/1h/1h/2h",   // other account
            "m/48h/0h/0h/2h",   // mainnet coin on a testnet wallet
            "m/48/1h/0h/2h",    // non-hardened purpose
            "m/48h/1h/0h/2h/0", // extra level
        ];
        for p in bad {
            let mut s = three(Network::Testnet);
            s[1].origin_path = DerivationPath::from_str(p).unwrap();
            assert_eq!(
                WalletDescription::new(Network::Testnet, s).unwrap_err(),
                WalletError::BadPath { slot: 2 },
                "{p}"
            );
        }
    }

    #[test]
    fn rejects_non_account_level_xpub() {
        let seed = bip39::Mnemonic::parse(MNEMONICS[1]).unwrap().to_seed("");
        let master = Xpriv::new_master(Network::Testnet, &seed).unwrap();
        let mut s = three(Network::Testnet);
        s[1].xpub = Xpub::from_priv(secp(), &master);
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::BadXpubDepth { slot: 2 }
        );
    }

    #[test]
    fn rejects_out_of_order_slot_indexes() {
        let mut s = three(Network::Testnet);
        s.swap(0, 1);
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::BadSlotIndex
        );
        let mut s = three(Network::Testnet);
        s[2].index = 4;
        assert_eq!(
            WalletDescription::new(Network::Testnet, s).unwrap_err(),
            WalletError::BadSlotIndex
        );
    }

    #[test]
    fn parse_round_trips() {
        for net in [
            Network::Bitcoin,
            Network::Testnet,
            Network::Signet,
            Network::Regtest,
        ] {
            let w = wallet(net);
            let p = WalletDescription::parse(w.canonical_string(), net).unwrap();
            assert_eq!(p, w);
            assert_eq!(p.canonical_string(), w.canonical_string());
        }
    }

    #[test]
    fn parse_accepts_apostrophe_marker_and_normalises() {
        let w = wallet(Network::Testnet);
        let body = w.canonical_string().rsplit_once('#').unwrap().0;
        // Only the path markers change; base58 xpubs may contain 'h'.
        let mut apos = body.to_string();
        for m in ["/48h", "/1h", "/0h", "/2h"] {
            apos = apos.replace(m, &m.replace('h', "'"));
        }
        assert!(apos.contains("48'"));
        let text = format!("{apos}#{}", descriptor_checksum(&apos).unwrap());
        let p = WalletDescription::parse(&text, Network::Testnet).unwrap();
        assert_eq!(p.canonical_string(), w.canonical_string());
    }

    #[test]
    fn parse_rejects_bad_checksum_and_missing_checksum() {
        let w = wallet(Network::Testnet);
        let c = w.canonical_string();
        let mut bad = c.to_string();
        let last = bad.pop().unwrap();
        bad.push(if last == 'q' { 'p' } else { 'q' });
        assert_eq!(
            WalletDescription::parse(&bad, Network::Testnet).unwrap_err(),
            WalletError::ChecksumMismatch
        );
        let body = c.rsplit_once('#').unwrap().0;
        assert_eq!(
            WalletDescription::parse(body, Network::Testnet).unwrap_err(),
            WalletError::ChecksumMissing
        );
    }

    #[test]
    fn parse_rejects_edited_text_even_with_fixed_checksum() {
        let w = wallet(Network::Testnet);
        let body = w.canonical_string().rsplit_once('#').unwrap().0;
        let resum = |b: &str| format!("{b}#{}", descriptor_checksum(b).unwrap());
        // 3-of-3 instead of 2-of-3
        let t = resum(&body.replacen("sortedmulti(2", "sortedmulti(3", 1));
        assert_eq!(
            WalletDescription::parse(&t, Network::Testnet).unwrap_err(),
            WalletError::NonCanonical
        );
        // plain multi instead of sortedmulti
        let t = resum(&body.replacen("sortedmulti", "multi", 1));
        assert_eq!(
            WalletDescription::parse(&t, Network::Testnet).unwrap_err(),
            WalletError::NonCanonical
        );
        // single-path suffix
        let t = resum(&body.replace("<0;1>", "0"));
        assert_eq!(
            WalletDescription::parse(&t, Network::Testnet).unwrap_err(),
            WalletError::NonCanonical
        );
        // uppercase fingerprint is a different text for the same wallet
        let fp = w.fingerprints()[0].to_string();
        let t = resum(&body.replacen(&fp, &fp.to_uppercase(), 1));
        if fp != fp.to_uppercase() {
            assert!(WalletDescription::parse(&t, Network::Testnet).is_err());
        }
        // swapped slot order is another pinned string, so it parses but differs
        let slots = w.slots();
        let swapped = [
            KeySlot {
                index: 1,
                ..slots[1].clone()
            },
            KeySlot {
                index: 2,
                ..slots[0].clone()
            },
            slots[2].clone(),
        ];
        let sw = WalletDescription::new(Network::Testnet, swapped).unwrap();
        assert_ne!(sw.canonical_string(), w.canonical_string());
        // wrong network on parse
        assert!(WalletDescription::parse(w.canonical_string(), Network::Bitcoin).is_err());
    }

    #[test]
    fn slot_matches_is_exact_on_xpub() {
        let w = wallet(Network::Testnet);
        let s = w.slots();
        assert!(w.slot_matches(1, &s[0].xpub));
        assert!(w.slot_matches(3, &s[2].xpub));
        assert!(!w.slot_matches(1, &s[1].xpub));
        assert!(!w.slot_matches(0, &s[0].xpub));
        assert!(!w.slot_matches(4, &s[0].xpub));
        // Same fingerprint-level origin but a sibling account key must not match.
        let seed = bip39::Mnemonic::parse(MNEMONICS[0]).unwrap().to_seed("");
        let master = Xpriv::new_master(Network::Testnet, &seed).unwrap();
        let other = master
            .derive_priv(secp(), &DerivationPath::from_str("m/48h/1h/1h/2h").unwrap())
            .unwrap();
        let other = Xpub::from_priv(secp(), &other);
        assert!(!w.slot_matches(1, &other));
    }

    /// Independent derivation: BIP32 child keys, BIP67 sort, hand-built script.
    fn reference_script(w: &WalletDescription, chain: u32, index: u32) -> ScriptBuf {
        let mut keys: Vec<PublicKey> = w
            .slots()
            .iter()
            .map(|s| {
                let path = [
                    ChildNumber::Normal { index: chain },
                    ChildNumber::Normal { index },
                ];
                let child = s.xpub.derive_pub(secp(), &path).unwrap();
                PublicKey::new(child.public_key)
            })
            .collect();
        keys.sort_by_key(|k| k.inner.serialize());
        let ws = Builder::new()
            .push_opcode(OP_PUSHNUM_2)
            .push_key(&keys[0])
            .push_key(&keys[1])
            .push_key(&keys[2])
            .push_opcode(OP_PUSHNUM_3)
            .push_opcode(OP_CHECKMULTISIG)
            .into_script();
        ScriptBuf::new_p2wsh(&ws.wscript_hash())
    }

    #[test]
    fn miniscript_and_reference_derivation_agree() {
        for net in [Network::Bitcoin, Network::Testnet, Network::Regtest] {
            let w = wallet(net);
            for i in 0..50 {
                assert_eq!(
                    w.receive_script(i).unwrap(),
                    reference_script(&w, 0, i),
                    "recv {i}"
                );
                assert_eq!(
                    w.change_script(i).unwrap(),
                    reference_script(&w, 1, i),
                    "chg {i}"
                );
                let addr = w.address(Keychain::Receive, i).unwrap();
                assert_eq!(addr.script_pubkey(), w.receive_script(i).unwrap());
            }
        }
    }

    #[test]
    fn receive_and_change_scripts_all_differ() {
        let w = wallet(Network::Testnet);
        let mut seen = HashSet::new();
        for i in 0..50 {
            assert!(seen.insert(w.receive_script(i).unwrap()));
            assert!(seen.insert(w.change_script(i).unwrap()));
        }
    }

    #[test]
    fn sorting_makes_scripts_independent_of_slot_order() {
        let w = wallet(Network::Testnet);
        let s = w.slots();
        let reordered = WalletDescription::new(
            Network::Testnet,
            [
                KeySlot {
                    index: 1,
                    ..s[2].clone()
                },
                KeySlot {
                    index: 2,
                    ..s[0].clone()
                },
                KeySlot {
                    index: 3,
                    ..s[1].clone()
                },
            ],
        )
        .unwrap();
        assert_ne!(reordered.canonical_string(), w.canonical_string());
        for i in 0..5 {
            assert_eq!(
                reordered.receive_script(i).unwrap(),
                w.receive_script(i).unwrap()
            );
        }
    }

    #[test]
    fn hardened_index_is_refused() {
        let w = wallet(Network::Testnet);
        assert_eq!(
            w.receive_script(1 << 31).unwrap_err(),
            WalletError::IndexOutOfRange
        );
        assert!(w.receive_script((1 << 31) - 1).is_ok());
    }

    #[test]
    fn witness_script_hashes_to_the_wallet_script() {
        for net in [Network::Bitcoin, Network::Regtest] {
            let w = wallet(net);
            for chain in [Keychain::Receive, Keychain::Change] {
                for i in [0, 1, 2, 7, 19, 20, 1000, (1 << 31) - 1] {
                    let ws = w.witness_script(chain, i).unwrap();
                    assert_eq!(ws.len(), 105, "{chain:?} {i}");
                    assert_eq!(
                        ScriptBuf::new_p2wsh(&ws.wscript_hash()),
                        w.script(chain, i).unwrap(),
                        "{chain:?} {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn witness_script_matches_the_descriptors_explicit_script() {
        let w = wallet(Network::Testnet);
        for (chain, d) in [(Keychain::Receive, 0), (Keychain::Change, 1)] {
            for i in [0, 1, 5, 40] {
                let explicit = w.chains[d]
                    .at_derivation_index(i)
                    .unwrap()
                    .derived_descriptor(secp())
                    .unwrap()
                    .explicit_script()
                    .unwrap();
                assert_eq!(w.witness_script(chain, i).unwrap(), explicit);
            }
        }
    }

    #[test]
    fn derived_keys_are_the_keys_of_the_witness_script_in_slot_order() {
        let w = wallet(Network::Regtest);
        for chain in [Keychain::Receive, Keychain::Change] {
            for i in [0, 3, 99] {
                let keys = w.derived_keys(chain, i).unwrap();
                let ws = w.witness_script(chain, i).unwrap();
                let from_script: HashSet<Vec<u8>> = ws
                    .instructions()
                    .filter_map(|ins| match ins.unwrap() {
                        bitcoin::script::Instruction::PushBytes(b) if b.len() == 33 => {
                            Some(b.as_bytes().to_vec())
                        }
                        _ => None,
                    })
                    .collect();
                let from_keys: HashSet<Vec<u8>> =
                    keys.iter().map(|k| k.inner.serialize().to_vec()).collect();
                assert_eq!(from_script.len(), 3);
                assert_eq!(from_script, from_keys);
                // Slot order: key n comes from slot n's xpub.
                let path = [
                    ChildNumber::Normal {
                        index: chain.child(),
                    },
                    ChildNumber::Normal { index: i },
                ];
                for (k, s) in keys.iter().zip(w.slots()) {
                    let want = s.xpub.derive_pub(secp(), &path).unwrap().public_key;
                    assert_eq!(k.inner, want);
                    assert!(k.compressed);
                }
            }
        }
    }

    #[test]
    fn witness_script_and_keys_refuse_a_hardened_index() {
        let w = wallet(Network::Testnet);
        assert_eq!(
            w.derived_keys(Keychain::Change, 1 << 31).unwrap_err(),
            WalletError::IndexOutOfRange
        );
        assert_eq!(
            w.witness_script(Keychain::Receive, 1 << 31).unwrap_err(),
            WalletError::IndexOutOfRange
        );
        assert!(w.witness_script(Keychain::Receive, (1 << 31) - 1).is_ok());
    }

    #[test]
    fn address_network_prefixes() {
        assert!(wallet(Network::Testnet)
            .address(Keychain::Receive, 0)
            .unwrap()
            .to_string()
            .starts_with("tb1q"));
        assert!(wallet(Network::Regtest)
            .address(Keychain::Change, 0)
            .unwrap()
            .to_string()
            .starts_with("bcrt1q"));
    }
}
