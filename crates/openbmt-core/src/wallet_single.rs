//! The single-key wallet description, and what the owner may bring in
//! (`docs/WALLET-RULES.md`, WL-01 to WL-15).
//!
//! An owner who already holds their own keys gives OpenBMT only the public
//! description of one account. The accepted single-key form is exactly
//!
//! ```text
//! wpkh([fp/84h/0h/Nh]xpub/<0;1>/*)#checksum
//! ```
//!
//! on mainnet, with the checksum of BIP380. [`ImportedWallet::read`] is the
//! one entry point for what the owner scans or opens: it returns this
//! wallet or the 2-of-3 wallet of [`crate::wallet`], or says in one word why
//! the text was refused. A refusal never carries the text, because the text
//! may hold a private key.

use std::fmt;

use bitcoin::bip32::{ChildNumber, Fingerprint, Xpub};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::{Address, CompressedPublicKey, Network, ScriptBuf};

use crate::identity::secp;
use crate::wallet::{
    descriptor_checksum, split_and_verify, Keychain, WalletDescription, WalletError,
};

/// The most bytes of text read (WL-14).
pub const MAX_IMPORT_BYTES: usize = 4096;

/// Why a single-key description was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingleKeyError {
    /// The text has no `#checksum` (or a character no description has).
    ChecksumMissing,
    /// The checksum does not match the text.
    ChecksumMismatch,
    /// The key or its path is for a test network.
    TestNetwork,
    /// The text is another kind of wallet, or not in the accepted form.
    Unsupported(&'static str),
    /// The index is not a valid non-hardened child index.
    IndexOutOfRange,
    /// Key derivation failed.
    Derive,
}

impl fmt::Display for SingleKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChecksumMissing => write!(f, "WL-04: the description has no checksum"),
            Self::ChecksumMismatch => write!(f, "WL-04: the checksum does not match"),
            Self::TestNetwork => write!(f, "WL-11: the description is for a test network"),
            Self::Unsupported(why) => write!(f, "WL-02: not an accepted description: {why}"),
            Self::IndexOutOfRange => write!(f, "WL-05: address index out of range"),
            Self::Derive => write!(f, "WL-05: key derivation failed"),
        }
    }
}

impl std::error::Error for SingleKeyError {}

fn checksum_error(e: WalletError) -> SingleKeyError {
    match e {
        WalletError::ChecksumMissing => SingleKeyError::ChecksumMissing,
        // A character outside the descriptor alphabet cannot be in a
        // description whose checksum holds.
        _ => SingleKeyError::ChecksumMismatch,
    }
}

fn unsupported(why: &'static str) -> SingleKeyError {
    SingleKeyError::Unsupported(why)
}

/// Which branches a description names.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Branches {
    Both,
    Receive,
    Change,
}

struct Part {
    fingerprint: Fingerprint,
    account: u32,
    xpub: Xpub,
    branches: Branches,
}

/// A hardened index written `Nh`, `NH` or `N'`, below 2^31, with no sign and
/// no leading zero.
fn hardened_index(text: &str) -> Option<u32> {
    let digits = text
        .strip_suffix(['h', 'H', '\''])
        .filter(|d| !d.is_empty())?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits.parse::<u32>().ok().filter(|n| *n < 1 << 31)
}

fn parse_part(text: &str) -> Result<Part, SingleKeyError> {
    let body = split_and_verify(text).map_err(checksum_error)?;
    let inner = body
        .strip_prefix("wpkh(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or(unsupported("not a native SegWit single-key description"))?;
    let rest = inner
        .strip_prefix('[')
        .ok_or(unsupported("the key has no origin"))?;
    let (origin, after) = rest
        .split_once(']')
        .ok_or(unsupported("the origin is not closed"))?;
    let (xpub_text, suffix) = after
        .split_once('/')
        .ok_or(unsupported("the key has no derivation after it"))?;
    let branches = match suffix {
        "<0;1>/*" => Branches::Both,
        "0/*" => Branches::Receive,
        "1/*" => Branches::Change,
        _ => return Err(unsupported("the key is not followed by /<0;1>/*")),
    };
    let comps: Vec<&str> = origin.split('/').collect();
    let [fp, purpose, coin, account] = comps.as_slice() else {
        return Err(unsupported("the origin path is not 84h/0h/Nh"));
    };
    if fp.len() != 8 || !fp.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(unsupported("the origin fingerprint is not 8 hex digits"));
    }
    let fingerprint = fp
        .parse::<Fingerprint>()
        .map_err(|_| unsupported("the origin fingerprint is not 8 hex digits"))?;
    if hardened_index(purpose) != Some(84) {
        return Err(unsupported("the origin path is not 84h/0h/Nh"));
    }
    match hardened_index(coin) {
        Some(0) => {}
        Some(1) => return Err(SingleKeyError::TestNetwork),
        _ => return Err(unsupported("the origin path is not 84h/0h/Nh")),
    }
    let account = hardened_index(account).ok_or(unsupported("the account is not hardened"))?;
    let xpub = xpub_text
        .parse::<Xpub>()
        .map_err(|_| unsupported("the key is not an xpub"))?;
    Ok(Part {
        fingerprint,
        account,
        xpub,
        branches,
    })
}

/// The validated single-key wallet: one BIP84 account, mainnet.
#[derive(Debug, Clone)]
pub struct SingleKeyWallet {
    fingerprint: Fingerprint,
    account: u32,
    xpub: Xpub,
    canonical: String,
}

impl PartialEq for SingleKeyWallet {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}
impl Eq for SingleKeyWallet {}

impl SingleKeyWallet {
    /// Validate the account key `xpub`, found at `84h/0h/<account>h` under
    /// the master key with `fingerprint`.
    pub fn new(fingerprint: Fingerprint, account: u32, xpub: Xpub) -> Result<Self, SingleKeyError> {
        if account >= 1 << 31 {
            return Err(unsupported("the account is not below 2^31"));
        }
        if xpub.network != bitcoin::NetworkKind::Main {
            return Err(SingleKeyError::TestNetwork);
        }
        if xpub.depth != 3 || xpub.child_number != (ChildNumber::Hardened { index: account }) {
            return Err(unsupported("the key is not an account-level xpub"));
        }
        let body = Self::body(fingerprint, account, &xpub, "<0;1>/*");
        let canonical = format!("{body}#{}", checksum_of(&body)?);
        Ok(Self {
            fingerprint,
            account,
            xpub,
            canonical,
        })
    }

    fn body(fingerprint: Fingerprint, account: u32, xpub: &Xpub, suffix: &str) -> String {
        format!("wpkh([{fingerprint}/84h/0h/{account}h]{xpub}/{suffix})")
    }

    /// Read the accepted form back: the multipath description, or the two
    /// single-path descriptions (receive and change, in either order) given
    /// together and separated by white space. Every checksum must be present
    /// and correct, and the two must name the same key and origin.
    pub fn parse(text: &str) -> Result<Self, SingleKeyError> {
        let texts: Vec<&str> = text.split_whitespace().collect();
        let parts = texts
            .iter()
            .map(|t| parse_part(t))
            .collect::<Result<Vec<_>, _>>()?;
        let key = |p: &Part| (p.fingerprint, p.account, p.xpub.encode());
        let first = match parts.as_slice() {
            [one] if one.branches == Branches::Both => one,
            [_] => return Err(unsupported("only one of the two branches was given")),
            [a, b] => {
                let pair = (a.branches, b.branches);
                if pair != (Branches::Receive, Branches::Change)
                    && pair != (Branches::Change, Branches::Receive)
                {
                    return Err(unsupported(
                        "the two descriptions are not receive and change",
                    ));
                }
                if key(a) != key(b) {
                    return Err(unsupported("the two descriptions name different keys"));
                }
                a
            }
            _ => return Err(unsupported("expected one description or two")),
        };
        Self::new(first.fingerprint, first.account, first.xpub)
    }

    /// The master fingerprint of the key.
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// The hardened account number N of `84h/0h/Nh`.
    pub fn account(&self) -> u32 {
        self.account
    }

    /// The account-level extended public key.
    pub fn xpub(&self) -> &Xpub {
        &self.xpub
    }

    /// The pinned multipath description with its checksum.
    pub fn canonical_string(&self) -> &str {
        &self.canonical
    }

    /// SHA-256 of the canonical string (including `#checksum`), as for
    /// [`WalletDescription::checksum32`].
    pub fn checksum32(&self) -> [u8; 32] {
        sha256::Hash::hash(self.canonical.as_bytes()).to_byte_array()
    }

    /// The two single-path descriptions (receive `/0/*`, change `/1/*`),
    /// each with its own checksum.
    pub fn single_path_strings(&self) -> [String; 2] {
        let make = |suffix: &str| {
            let body = Self::body(self.fingerprint, self.account, &self.xpub, suffix);
            // The body is made of the descriptor alphabet only.
            let sum = descriptor_checksum(&body).unwrap_or_default();
            format!("{body}#{sum}")
        };
        [make("0/*"), make("1/*")]
    }

    fn key_at(&self, chain: Keychain, index: u32) -> Result<CompressedPublicKey, SingleKeyError> {
        if index >= 1 << 31 {
            return Err(SingleKeyError::IndexOutOfRange);
        }
        let branch = match chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        };
        let path = [
            ChildNumber::Normal { index: branch },
            ChildNumber::Normal { index },
        ];
        let child = self
            .xpub
            .derive_pub(secp(), &path)
            .map_err(|_| SingleKeyError::Derive)?;
        Ok(CompressedPublicKey(child.public_key))
    }

    /// The script at `index` on `chain`: P2WPKH, 22 bytes.
    pub fn script(&self, chain: Keychain, index: u32) -> Result<ScriptBuf, SingleKeyError> {
        let key = self.key_at(chain, index)?;
        Ok(ScriptBuf::new_p2wpkh(&key.wpubkey_hash()))
    }

    /// Receive script at `index`.
    pub fn receive_script(&self, index: u32) -> Result<ScriptBuf, SingleKeyError> {
        self.script(Keychain::Receive, index)
    }

    /// Change script at `index`.
    pub fn change_script(&self, index: u32) -> Result<ScriptBuf, SingleKeyError> {
        self.script(Keychain::Change, index)
    }

    /// The address at `index` on `chain`.
    pub fn address(&self, chain: Keychain, index: u32) -> Result<Address, SingleKeyError> {
        let key = self.key_at(chain, index)?;
        Ok(Address::p2wpkh(&key, Network::Bitcoin))
    }

    /// The first receive address, the one the owner compares on the wallet's
    /// own screen.
    pub fn first_receive_address(&self) -> Result<Address, SingleKeyError> {
        self.address(Keychain::Receive, 0)
    }
}

fn checksum_of(body: &str) -> Result<String, SingleKeyError> {
    descriptor_checksum(body).ok_or(SingleKeyError::Derive)
}

/// Why text the owner brought in was refused. It carries nothing of the
/// text (WL-09).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportRefused {
    /// The text holds a private key, a seed or its words.
    HasPrivateKey,
    /// The checksum is missing or wrong.
    Damaged,
    /// The text is for a test network.
    TestNetwork,
    /// Any other kind of wallet, or text that is not a wallet.
    Unsupported,
}

impl fmt::Display for ImportRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HasPrivateKey => write!(f, "WL-08: the text holds a private key"),
            Self::Damaged => write!(f, "WL-10: the description is damaged"),
            Self::TestNetwork => write!(f, "WL-11: the description is for a test network"),
            Self::Unsupported => write!(f, "WL-12: the wallet type is not supported"),
        }
    }
}

impl std::error::Error for ImportRefused {}

/// A wallet the owner brought in. It is held once and read once, so the size
/// of the larger variant costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedWallet {
    /// A native SegWit single-key account.
    Single(SingleKeyWallet),
    /// The 2-of-3 wallet of [`WalletDescription`], on mainnet.
    Multisig(WalletDescription),
}

const EXTENDED_PRIVATE: [&str; 10] = [
    "xprv", "tprv", "yprv", "zprv", "uprv", "vprv", "Yprv", "Zprv", "Uprv", "Vprv",
];
const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
}

fn looks_like_wif(token: &str) -> bool {
    let first = token.as_bytes().first().copied().unwrap_or(0);
    let shape = match token.len() {
        51 => matches!(first, b'5' | b'9'),
        52 => matches!(first, b'K' | b'L' | b'c'),
        _ => false,
    };
    shape && token.chars().all(|c| BASE58.contains(c))
}

/// Twelve to twenty-four words that are all on the English list, or a
/// count a seed has made of lower-case words of 3 to 8 letters (a seed
/// with a misspelt word still holds the key).
fn looks_like_seed_words(text: &str) -> bool {
    let words: Vec<&str> = text
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .collect();
    let n = words.len();
    if !(12..=24).contains(&n)
        || !words
            .iter()
            .all(|w| w.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return false;
    }
    let list = bip39::Language::English;
    let on_list = words
        .iter()
        .all(|w| list.find_word(&w.to_lowercase()).is_some());
    let shaped = n.is_multiple_of(3)
        && words
            .iter()
            .all(|w| (3..=8).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_lowercase()));
    on_list || shaped
}

fn holds_private_key(text: &str) -> bool {
    tokens(text).any(|t| EXTENDED_PRIVATE.iter().any(|p| t.starts_with(p)) || looks_like_wif(t))
        || looks_like_seed_words(text)
}

fn is_test_network(text: &str) -> bool {
    let key = tokens(text).any(|t| {
        ["tpub", "upub", "vpub"].iter().any(|p| t.starts_with(p))
            || t.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("tb1"))
            || t.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("bcrt1"))
    });
    // Inside an origin `[fp/purpose/coin/...]`, a coin type of 1.
    let path = text.split('[').skip(1).any(|rest| {
        let origin = rest.split(']').next().unwrap_or("");
        origin
            .split('/')
            .nth(2)
            .is_some_and(|coin| hardened_index(coin) == Some(1))
    });
    key || path
}

impl ImportedWallet {
    /// Read what the owner scanned or opened. The checks run in this order
    /// (WL-13): a private key, a test network, the checksum, the type.
    pub fn read(text: &str) -> Result<Self, ImportRefused> {
        if text.len() > MAX_IMPORT_BYTES {
            return Err(ImportRefused::Unsupported);
        }
        if holds_private_key(text) {
            return Err(ImportRefused::HasPrivateKey);
        }
        if is_test_network(text) {
            return Err(ImportRefused::TestNetwork);
        }
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.is_empty() {
            return Err(ImportRefused::Damaged);
        }
        for part in &parts {
            split_and_verify(part).map_err(|_| ImportRefused::Damaged)?;
        }
        if let [one] = parts.as_slice() {
            if one.starts_with("wsh(") {
                return match WalletDescription::parse(one, Network::Bitcoin) {
                    Ok(w) => Ok(Self::Multisig(w)),
                    Err(WalletError::NetworkMismatch { .. }) => Err(ImportRefused::TestNetwork),
                    Err(_) => Err(ImportRefused::Unsupported),
                };
            }
        }
        match SingleKeyWallet::parse(text) {
            Ok(w) => Ok(Self::Single(w)),
            Err(SingleKeyError::TestNetwork) => Err(ImportRefused::TestNetwork),
            Err(_) => Err(ImportRefused::Unsupported),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{wallet, MNEMONICS};
    use bitcoin::base58;
    use bitcoin::bip32::{DerivationPath, Xpriv};
    use miniscript::descriptor::DescriptorPublicKey;
    use miniscript::Descriptor;
    use std::str::FromStr;

    const RULES: &str = include_str!("../../../docs/WALLET-RULES.md");
    const SOURCE: &str = include_str!("wallet_single.rs");
    const SAY: &str = include_str!("say/import.rs");
    const ELECTRUM: &str = include_str!("../testdata/keymaker-electrum-expected.txt");

    fn master(mnemonic: &str, network: Network) -> Xpriv {
        let seed = bip39::Mnemonic::parse(mnemonic).unwrap().to_seed("");
        Xpriv::new_master(network, &seed).unwrap()
    }

    /// The account key at `84h/<coin>h/<account>h` and its master fingerprint.
    fn account_key(
        mnemonic: &str,
        network: Network,
        coin: u32,
        account: u32,
    ) -> (Fingerprint, Xpub) {
        let m = master(mnemonic, network);
        let path = DerivationPath::from_str(&format!("m/84h/{coin}h/{account}h")).unwrap();
        let x = m.derive_priv(secp(), &path).unwrap();
        (m.fingerprint(secp()), Xpub::from_priv(secp(), &x))
    }

    fn mainnet(i: usize, account: u32) -> SingleKeyWallet {
        let (fp, xpub) = account_key(MNEMONICS[i], Network::Bitcoin, 0, account);
        SingleKeyWallet::new(fp, account, xpub).unwrap()
    }

    fn with_sum(body: &str) -> String {
        format!("{body}#{}", descriptor_checksum(body).unwrap())
    }

    fn key_text(i: usize, account: u32) -> (String, String) {
        let (fp, xpub) = account_key(MNEMONICS[i], Network::Bitcoin, 0, account);
        (fp.to_string(), xpub.to_string())
    }

    fn desc(i: usize, origin_path: &str, suffix: &str) -> String {
        let (fp, xpub) = key_text(i, 0);
        with_sum(&format!("wpkh([{fp}/{origin_path}]{xpub}/{suffix})"))
    }

    fn zpub(xpub: &Xpub) -> String {
        let mut raw = base58::decode_check(&xpub.to_string()).unwrap();
        raw[..4].copy_from_slice(&[0x04, 0xb2, 0x47, 0x46]);
        base58::encode_check(&raw)
    }

    #[test]
    fn wl_01_the_canonical_form_is_pinned_and_reads_back() {
        let w = mainnet(0, 0);
        let c = w.canonical_string();
        assert!(c.starts_with("wpkh([73c5da0a/84h/0h/0h]xpub"));
        assert!(c.contains("/<0;1>/*)#") && !c.contains('\''));
        let (body, sum) = c.rsplit_once('#').unwrap();
        assert_eq!(descriptor_checksum(body).unwrap(), sum);
        assert_eq!(SingleKeyWallet::parse(c).unwrap(), w);
        // The apostrophe marker and an upper-case fingerprint read as the
        // same wallet, written back in the pinned form.
        let (fp, xpub) = key_text(0, 0);
        let loose = with_sum(&format!(
            "wpkh([{}/84'/0'/0']{xpub}/<0;1>/*)",
            fp.to_uppercase()
        ));
        let again = SingleKeyWallet::parse(&loose).unwrap();
        assert_eq!(again.canonical_string(), c);
        assert_eq!(
            w.checksum32(),
            sha256::Hash::hash(c.as_bytes()).to_byte_array()
        );
        assert_eq!(w.fingerprint().to_string(), fp);
        assert_eq!(w.account(), 0);
    }

    #[test]
    fn wl_02_the_origin_and_the_key_are_checked() {
        let (fp, xpub) = key_text(0, 0);
        let bad = |origin: &str, key: &str| {
            let text = with_sum(&format!("wpkh([{origin}]{key}/<0;1>/*)"));
            SingleKeyWallet::parse(&text).is_err()
        };
        assert!(!bad(&format!("{fp}/84h/0h/0h"), &xpub));
        assert!(bad(&format!("{fp}/49h/0h/0h"), &xpub));
        assert!(bad(&format!("{fp}/84h/2h/0h"), &xpub));
        assert!(bad(&format!("{fp}/84h/0h/0"), &xpub));
        assert!(bad(&format!("{fp}/84h/0h/00h"), &xpub));
        assert!(bad(&format!("{fp}/84h/0h/+0h"), &xpub));
        assert!(bad(&format!("{fp}/84h/0h/2147483648h"), &xpub));
        assert!(bad(&format!("{fp}/84h/0h"), &xpub));
        assert!(bad(&format!("{fp}/84h/0h/0h/0h"), &xpub));
        assert!(bad(&format!("{}/84h/0h/0h", &fp[..6]), &xpub));
        // The account in the origin must be the key's own child number.
        assert!(bad(&format!("{fp}/84h/0h/1h"), &xpub));
        // A zpub is refused, and so is a key that is not at depth 3.
        let (_, x) = account_key(MNEMONICS[0], Network::Bitcoin, 0, 0);
        assert!(bad(&format!("{fp}/84h/0h/0h"), &zpub(&x)));
        let m = master(MNEMONICS[0], Network::Bitcoin);
        let at4 = Xpub::from_priv(
            secp(),
            &m.derive_priv(secp(), &DerivationPath::from_str("m/84h/0h/0h/0h").unwrap())
                .unwrap(),
        );
        assert!(bad(&format!("{fp}/84h/0h/0h"), &at4.to_string()));
        let at2 = Xpub::from_priv(
            secp(),
            &m.derive_priv(secp(), &DerivationPath::from_str("m/84h/0h").unwrap())
                .unwrap(),
        );
        assert!(bad(&format!("{fp}/84h/0h/0h"), &at2.to_string()));
        // No origin, and a key that is not a key.
        assert!(SingleKeyWallet::parse(&with_sum(&format!("wpkh({xpub}/<0;1>/*)"))).is_err());
        assert!(bad(&format!("{fp}/84h/0h/0h"), "xpub661MyMwAqRbcFake"));
        // The largest account is accepted.
        let top = master(MNEMONICS[1], Network::Bitcoin)
            .derive_priv(
                secp(),
                &DerivationPath::from_str("m/84h/0h/2147483647h").unwrap(),
            )
            .unwrap();
        let w = SingleKeyWallet::new(
            Fingerprint::from_str("00000000").unwrap(),
            2_147_483_647,
            Xpub::from_priv(secp(), &top),
        )
        .unwrap();
        assert!(w.canonical_string().contains("/84h/0h/2147483647h]"));
        assert_eq!(SingleKeyWallet::parse(w.canonical_string()).unwrap(), w);
        assert!(SingleKeyWallet::new(
            Fingerprint::from_str("00000000").unwrap(),
            1 << 31,
            Xpub::from_priv(secp(), &top)
        )
        .is_err());
    }

    #[test]
    fn wl_03_the_two_single_path_descriptions_are_accepted_together() {
        let w = mainnet(0, 0);
        let [r, c] = w.single_path_strings();
        for text in [
            format!("{r}\n{c}"),
            format!("{c} {r}\n"),
            format!("  {r}\r\n{c}  "),
        ] {
            assert_eq!(SingleKeyWallet::parse(&text).unwrap(), w);
        }
        // One alone, the same twice, three, or two different keys.
        assert!(SingleKeyWallet::parse(&r).is_err());
        assert!(SingleKeyWallet::parse(&format!("{r} {r}")).is_err());
        assert!(SingleKeyWallet::parse(&format!("{r} {c} {r}")).is_err());
        let other = mainnet(1, 0).single_path_strings();
        assert!(SingleKeyWallet::parse(&format!("{r} {}", other[1])).is_err());
        // The same key under another account.
        let acct = mainnet(0, 1).single_path_strings();
        assert!(SingleKeyWallet::parse(&format!("{r} {}", acct[1])).is_err());
        // A multipath description with another description.
        assert!(SingleKeyWallet::parse(&format!("{} {c}", w.canonical_string())).is_err());
        // Other suffixes.
        for suffix in ["<0;1>/0/*", "2/*", "<1;0>/*", "0/0", "*"] {
            assert!(
                SingleKeyWallet::parse(&desc(0, "84h/0h/0h", suffix)).is_err(),
                "{suffix}"
            );
        }
    }

    #[test]
    fn wl_04_the_checksum_is_required_and_checked() {
        let w = mainnet(0, 0);
        let c = w.canonical_string();
        let (body, sum) = c.rsplit_once('#').unwrap();
        assert_eq!(
            SingleKeyWallet::parse(body),
            Err(SingleKeyError::ChecksumMissing)
        );
        let mut wrong = sum.to_string();
        wrong.replace_range(..1, if sum.starts_with('q') { "p" } else { "q" });
        assert_eq!(
            SingleKeyWallet::parse(&format!("{body}#{wrong}")),
            Err(SingleKeyError::ChecksumMismatch)
        );
        assert_eq!(
            SingleKeyWallet::parse(&format!("{body}#{}", &sum[1..])),
            Err(SingleKeyError::ChecksumMismatch)
        );
        assert_eq!(
            SingleKeyWallet::parse("wpkh(é)#qqqqqqqq"),
            Err(SingleKeyError::ChecksumMismatch)
        );
        // One of two single-path descriptions with a wrong checksum.
        let [r, ch] = w.single_path_strings();
        let bad = format!("{}#{wrong}", r.rsplit_once('#').unwrap().0);
        assert!(SingleKeyWallet::parse(&format!("{bad} {ch}")).is_err());
    }

    fn miniscript_scripts(text: &str, index: u32) -> ScriptBuf {
        let d = Descriptor::<DescriptorPublicKey>::from_str(text).unwrap();
        d.at_derivation_index(index)
            .unwrap()
            .derived_descriptor(secp())
            .unwrap()
            .script_pubkey()
    }

    #[test]
    fn wl_05_scripts_and_addresses_agree_with_miniscript() {
        for (i, account) in [(0, 0), (1, 7), (2, 2_147_483_647)] {
            let (fp, xpub) = if account == 2_147_483_647 {
                let m = master(MNEMONICS[i], Network::Bitcoin);
                let p = DerivationPath::from_str("m/84h/0h/2147483647h").unwrap();
                (
                    m.fingerprint(secp()),
                    Xpub::from_priv(secp(), &m.derive_priv(secp(), &p).unwrap()),
                )
            } else {
                account_key(MNEMONICS[i], Network::Bitcoin, 0, account)
            };
            let w = SingleKeyWallet::new(fp, account, xpub).unwrap();
            let multi = Descriptor::<DescriptorPublicKey>::from_str(w.canonical_string()).unwrap();
            let chains = multi.into_single_descriptors().unwrap();
            let [r, c] = w.single_path_strings();
            assert!(r.contains("/0/*)#") && c.contains("/1/*)#"));
            for index in 0..1000u32 {
                for (k, (chain, text)) in [(Keychain::Receive, &r), (Keychain::Change, &c)]
                    .into_iter()
                    .enumerate()
                {
                    let ours = w.script(chain, index).unwrap();
                    assert_eq!(ours.len(), 22);
                    assert!(ours.is_p2wpkh());
                    assert_eq!(ours, miniscript_scripts(text, index));
                    let via_multi = chains[k]
                        .at_derivation_index(index)
                        .unwrap()
                        .derived_descriptor(secp())
                        .unwrap();
                    assert_eq!(ours, via_multi.script_pubkey());
                    assert_eq!(
                        w.address(chain, index).unwrap(),
                        via_multi.address(Network::Bitcoin).unwrap()
                    );
                }
                assert_eq!(
                    w.receive_script(index).unwrap(),
                    w.script(Keychain::Receive, index).unwrap()
                );
                assert_eq!(
                    w.change_script(index).unwrap(),
                    w.script(Keychain::Change, index).unwrap()
                );
            }
            assert_ne!(w.receive_script(0).unwrap(), w.change_script(0).unwrap());
            assert_eq!(
                w.first_receive_address().unwrap(),
                w.address(Keychain::Receive, 0).unwrap()
            );
            assert!(w
                .first_receive_address()
                .unwrap()
                .to_string()
                .starts_with("bc1q"));
            assert_eq!(
                w.receive_script(1 << 31),
                Err(SingleKeyError::IndexOutOfRange)
            );
            assert_eq!(
                w.address(Keychain::Change, u32::MAX),
                Err(SingleKeyError::IndexOutOfRange)
            );
            assert!(w.receive_script((1 << 31) - 1).is_ok());
            // The single-path strings read back as the same wallet.
            assert_eq!(SingleKeyWallet::parse(&format!("{r}\n{c}")).unwrap(), w);
        }
    }

    #[test]
    fn wl_06_the_key_makers_test_seed_gives_its_first_address() {
        let xpub = Xpub::from_str(
            "xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8",
        )
        .unwrap();
        let w = SingleKeyWallet::new(Fingerprint::from_str("818bc9b6").unwrap(), 0, xpub).unwrap();
        assert_eq!(
            w.first_receive_address().unwrap().to_string(),
            "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj"
        );
    }

    #[test]
    fn wl_06_the_twenty_electrum_results_agree() {
        let mut rows = 0;
        for line in ELECTRUM
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let f: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(f.len(), 16, "{line}");
            let (words, fp, xpub, address) = (f[1..13].join(" "), f[13], f[14], f[15]);
            let w = SingleKeyWallet::new(
                Fingerprint::from_str(fp).unwrap(),
                0,
                Xpub::from_str(xpub).unwrap(),
            )
            .unwrap();
            assert_eq!(
                w.first_receive_address().unwrap().to_string(),
                address,
                "{line}"
            );
            // The keys that the words themselves give are the same.
            let (own_fp, own_xpub) = account_key(&words, Network::Bitcoin, 0, 0);
            assert_eq!(own_fp.to_string(), fp);
            assert_eq!(own_xpub.to_string(), xpub);
            // And the description reads through the one entry point.
            assert_eq!(
                ImportedWallet::read(w.canonical_string()),
                Ok(ImportedWallet::Single(w))
            );
            rows += 1;
        }
        assert_eq!(rows, 20);
    }

    #[test]
    fn wl_07_read_returns_either_wallet() {
        let single = mainnet(0, 0);
        let [r, c] = single.single_path_strings();
        for text in [
            single.canonical_string().to_string(),
            format!("{r}\n{c}\n"),
            format!("\n  {}  \n", single.canonical_string()),
        ] {
            assert_eq!(
                ImportedWallet::read(&text),
                Ok(ImportedWallet::Single(single.clone()))
            );
        }
        let multi = wallet(Network::Bitcoin);
        assert_eq!(
            ImportedWallet::read(multi.canonical_string()),
            Ok(ImportedWallet::Multisig(multi.clone()))
        );
        assert!(ImportedWallet::read("").is_err());
    }

    fn seed_text(words: usize) -> String {
        let list = bip39::Language::English.word_list();
        (0..words)
            .map(|i| list[i * 37 % 2048])
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn wl_08_a_private_key_is_refused() {
        let m = master(MNEMONICS[0], Network::Bitcoin);
        let xprv = m.to_string();
        let tprv = master(MNEMONICS[0], Network::Testnet).to_string();
        let wif = bitcoin::PrivateKey::new(m.private_key, Network::Bitcoin);
        let wif_c = wif.to_wif();
        let wif_u = bitcoin::PrivateKey {
            compressed: false,
            ..wif
        }
        .to_wif();
        let mut cases = vec![
            xprv.clone(),
            tprv.clone(),
            format!("  {xprv}\n"),
            wif_c.clone(),
            wif_u,
            format!("wpkh({wif_c})"),
            format!("wpkh([73c5da0a/84h/0h/0h]{xprv}/<0;1>/*)"),
            with_sum(&format!("wpkh([73c5da0a/84h/0h/0h]{xprv}/<0;1>/*)")),
            MNEMONICS[0].to_string(),
            MNEMONICS[0].to_uppercase(),
            MNEMONICS[0].replace(' ', ", "),
            format!("{}\n", seed_text(24)),
            // Not on the list, but the shape of a seed.
            "zzzzzz qqqqqq zzzzzz qqqqqq zzzzzz qqqqqq zzzzzz qqqqqq zzzzzz qqqqqq zzzzzz qqqqqq"
                .to_string(),
        ];
        for n in [12, 13, 15, 18, 21, 24] {
            cases.push(seed_text(n));
        }
        for text in cases {
            assert_eq!(
                ImportedWallet::read(&text),
                Err(ImportRefused::HasPrivateKey),
                "{text}"
            );
        }
        // Eleven words and twenty-five are not a seed.
        assert_ne!(
            ImportedWallet::read(&seed_text(11)),
            Err(ImportRefused::HasPrivateKey)
        );
        assert_ne!(
            ImportedWallet::read(&seed_text(25)),
            Err(ImportRefused::HasPrivateKey)
        );
        // A public description is never taken for a key.
        for i in 0..3 {
            let w = mainnet(i, 0);
            assert!(matches!(
                ImportedWallet::read(w.canonical_string()),
                Ok(ImportedWallet::Single(_))
            ));
        }
    }

    #[test]
    fn wl_09_a_refusal_does_not_repeat_the_text() {
        let m = master(MNEMONICS[0], Network::Bitcoin);
        let secret = m.to_string();
        let texts = [
            secret.clone(),
            MNEMONICS[0].to_string(),
            with_sum(&format!("wpkh([73c5da0a/84h/0h/0h]{secret}/<0;1>/*)")),
        ];
        for text in &texts {
            let err = ImportedWallet::read(text).unwrap_err();
            let shown = format!("{err} {err:?} {:?}", err.words());
            assert!(!shown.contains(&secret) && !shown.contains("abandon"));
            assert!(!shown.contains(&text[..20.min(text.len())]), "{shown}");
        }
        for r in crate::say::import::ALL_REFUSED {
            assert!(format!("{r:?}").len() < 20);
            assert!(!format!("{r}").contains("xprv"));
        }
        // The key text is not in the error of the single-key parser either.
        let err = SingleKeyWallet::parse(&with_sum("wpkh(secretsecretsecret)")).unwrap_err();
        assert!(!format!("{err} {err:?}").contains("secretsecretsecret"));
    }

    #[test]
    fn wl_10_a_damaged_description_is_refused() {
        let c = mainnet(0, 0).canonical_string().to_string();
        let (body, sum) = c.rsplit_once('#').unwrap();
        let wrong = format!("{body}#{}", sum.replace(&sum[..1], "x"));
        let [r, ch] = mainnet(0, 0).single_path_strings();
        let multi = wallet(Network::Bitcoin);
        let (mbody, _) = multi.canonical_string().rsplit_once('#').unwrap();
        for text in [
            body.to_string(),
            format!("{body}#"),
            wrong,
            c[..c.len() - 1].to_string(),
            c.replacen("84h", "84", 1),
            format!("{r} {}", ch.rsplit_once('#').unwrap().0),
            mbody.to_string(),
            format!("{mbody}#qqqqqqqq"),
            "hello".to_string(),
            String::new(),
            "   \n".to_string(),
            "wpkh(é)#qqqqqqqq".to_string(),
        ] {
            assert_eq!(
                ImportedWallet::read(&text),
                Err(ImportRefused::Damaged),
                "{text}"
            );
        }
    }

    #[test]
    fn wl_11_a_test_network_is_refused() {
        let (fp, xpub) = account_key(MNEMONICS[0], Network::Testnet, 1, 0);
        let tpub = xpub.to_string();
        assert!(tpub.starts_with("tpub"));
        let vpub = {
            let mut raw = base58::decode_check(&tpub).unwrap();
            raw[..4].copy_from_slice(&[0x04, 0x5f, 0x1c, 0xf6]);
            base58::encode_check(&raw)
        };
        let mut cases = vec![
            with_sum(&format!("wpkh([{fp}/84h/1h/0h]{tpub}/<0;1>/*)")),
            with_sum(&format!("wpkh([{fp}/84h/0h/0h]{tpub}/<0;1>/*)")),
            with_sum(&format!("wpkh([{fp}/84'/1'/0']{tpub}/<0;1>/*)")),
            with_sum(&format!("wpkh([{fp}/84h/0h/0h]{vpub}/<0;1>/*)")),
            // A coin type of 1 under an xpub.
            desc(0, "84h/1h/0h", "<0;1>/*"),
            desc(0, "48h/1h/0h/2h", "<0;1>/*"),
            "tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx".to_string(),
            "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080".to_string(),
            "TB1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KXPJZSX".to_string(),
            // The checksum is wrong as well: the network is named first.
            format!("wpkh([{fp}/84h/1h/0h]{tpub}/<0;1>/*)#qqqqqqqq"),
        ];
        let test_multi = {
            let w = {
                use crate::testutil::slot_from_mnemonic;
                WalletDescription::new(
                    Network::Testnet,
                    [
                        slot_from_mnemonic(MNEMONICS[0], 1, Network::Testnet),
                        slot_from_mnemonic(MNEMONICS[1], 2, Network::Testnet),
                        slot_from_mnemonic(MNEMONICS[2], 3, Network::Testnet),
                    ],
                )
                .unwrap()
            };
            w.canonical_string().to_string()
        };
        cases.push(test_multi);
        for text in cases {
            assert_eq!(
                ImportedWallet::read(&text),
                Err(ImportRefused::TestNetwork),
                "{text}"
            );
        }
        // The single-key parser says the same for a tpub.
        assert_eq!(
            SingleKeyWallet::new(fp, 0, xpub),
            Err(SingleKeyError::TestNetwork)
        );
    }

    #[test]
    fn wl_12_another_type_is_refused_as_unsupported() {
        let (fp, xpub) = key_text(0, 0);
        let k = format!("[{fp}/84h/0h/0h]{xpub}");
        let (_, z) = account_key(MNEMONICS[0], Network::Bitcoin, 0, 0);
        let (_, x1) = key_text(1, 0);
        let (_, x2) = key_text(2, 0);
        let bare = xpub.clone();
        let cases = vec![
            with_sum(&format!("tr([{fp}/86h/0h/0h]{xpub}/<0;1>/*)")),
            with_sum(&format!("sh(wpkh({k}/<0;1>/*))")),
            with_sum(&format!("pkh({k}/<0;1>/*)")),
            with_sum(&format!("wpkh({bare}/<0;1>/*)")),
            with_sum(&format!("wpkh([{fp}/84h/0h/0h]{}/<0;1>/*)", zpub(&z))),
            with_sum(&format!("wpkh({k}/0/*)")),
            with_sum(&format!("wpkh({k}/<0;1>/*,{k}/<0;1>/*)")),
            with_sum(&format!("wsh(pk({k}/<0;1>/*))")),
            with_sum(&format!("wsh(sortedmulti(2,{k}/<0;1>/*,[{fp}/48h/0h/0h/2h]{x1}/<0;1>/*))")),
            with_sum(&format!(
                "wsh(sortedmulti(1,{k}/<0;1>/*,[{fp}/48h/0h/0h/2h]{x1}/<0;1>/*,[{fp}/48h/0h/0h/2h]{x2}/<0;1>/*))"
            )),
            with_sum(&format!(
                "wsh(and_v(v:pk({k}/<0;1>/*),older(144)))"
            )),
            with_sum("addr(bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj)"),
            with_sum("raw(deadbeef)"),
            with_sum("wpkh(02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5)"),
            // One branch alone.
            mainnet(0, 0).single_path_strings()[0].clone(),
        ];
        for text in cases {
            assert_eq!(
                ImportedWallet::read(&text),
                Err(ImportRefused::Unsupported),
                "{text}"
            );
        }
        // Text that is not a description at all has no checksum.
        assert_eq!(
            ImportedWallet::read("hello world, this is not a wallet #"),
            Err(ImportRefused::Damaged)
        );
    }

    #[test]
    fn wl_13_the_checks_run_in_order() {
        let (fp, xpub) = key_text(0, 0);
        let m = master(MNEMONICS[0], Network::Bitcoin);
        let xprv = m.to_string();
        let tpub = account_key(MNEMONICS[0], Network::Testnet, 1, 0).1;
        // A private key beats a test network, a checksum and a type.
        assert_eq!(
            ImportedWallet::read(&format!("tr([{fp}/86h/1h/0h]{xprv}/*)#qqqqqqqq {tpub}")),
            Err(ImportRefused::HasPrivateKey)
        );
        // A test network beats a bad checksum and a type.
        assert_eq!(
            ImportedWallet::read(&format!("tr([{fp}/86h/0h/0h]{tpub}/*)#qqqqqqqq")),
            Err(ImportRefused::TestNetwork)
        );
        // A bad checksum beats a type.
        assert_eq!(
            ImportedWallet::read(&format!("tr([{fp}/86h/0h/0h]{xpub}/*)#qqqqqqqq")),
            Err(ImportRefused::Damaged)
        );
        assert_eq!(
            ImportedWallet::read(&with_sum(&format!("tr([{fp}/86h/0h/0h]{xpub}/*)"))),
            Err(ImportRefused::Unsupported)
        );
    }

    #[test]
    fn wl_14_long_text_is_refused_before_it_is_read() {
        let c = mainnet(0, 0).canonical_string().to_string();
        let pad = |n: usize| format!("{c}{}", " ".repeat(n - c.len()));
        assert!(matches!(
            ImportedWallet::read(&pad(MAX_IMPORT_BYTES)),
            Ok(ImportedWallet::Single(_))
        ));
        assert_eq!(
            ImportedWallet::read(&pad(MAX_IMPORT_BYTES + 1)),
            Err(ImportRefused::Unsupported)
        );
        // Over the size even a private key is not looked for.
        let big = format!(
            "{} {}",
            master(MNEMONICS[0], Network::Bitcoin),
            "a".repeat(5000)
        );
        assert_eq!(ImportedWallet::read(&big), Err(ImportRefused::Unsupported));
        assert_eq!(
            ImportedWallet::read(&"x".repeat(1 << 20)),
            Err(ImportRefused::Unsupported)
        );
    }

    #[test]
    fn wl_15_the_words_are_those_of_the_screen_list() {
        let rows = [
            (
                ImportRefused::HasPrivateKey,
                "This holds a private key",
                "What you scanned can spend the money, not just watch it. Anyone who has seen it can take the bitcoin.",
                "Do not use this wallet for the shop. Move any bitcoin in it to a new wallet, then scan the new wallet's public description.",
            ),
            (
                ImportRefused::Damaged,
                "This description is damaged",
                "Its checksum does not match, so part of it was misread or changed.",
                "Scan or export it again from your wallet.",
            ),
            (
                ImportRefused::TestNetwork,
                "Test-network wallet",
                "It is for testnet or signet, not real bitcoin.",
                "Export the description of your real bitcoin account and scan that.",
            ),
            (
                ImportRefused::Unsupported,
                "Wallet type not supported",
                "OpenBMT can check native SegWit single-key wallets and its own 2-of-3 multisig. This is a different kind.",
                "In your wallet, export the public description of a native SegWit account (it starts with wpkh) and scan that.",
            ),
        ];
        for (r, heading, what, next) in rows {
            let w = r.words();
            assert_eq!(
                (w.heading.as_str(), w.what.as_str(), w.next.as_str()),
                (heading, what, next)
            );
            assert_eq!(w.who, crate::say::Who::Owner);
            assert!(crate::say::check(&w).is_ok());
        }
        assert!(SAY.contains("WL-15"));
    }

    /// Every rule ID that the rules file defines.
    fn rule_ids() -> Vec<String> {
        RULES
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("- **WL-"))
            .map(|rest| format!("WL-{}", rest.chars().take(2).collect::<String>()))
            .collect()
    }

    #[test]
    fn the_rules_file_defines_the_rules_the_tests_name() {
        let ids = rule_ids();
        assert_eq!(ids.len(), 15);
        assert_eq!(ids.first().map(String::as_str), Some("WL-01"));
        assert_eq!(ids.last().map(String::as_str), Some("WL-15"));
    }

    #[test]
    fn every_rule_has_at_least_one_test_named_for_it() {
        let missing: Vec<_> = rule_ids()
            .into_iter()
            .filter(|id| !SOURCE.contains(&format!("fn wl_{}_", &id[3..])))
            .collect();
        assert!(missing.is_empty(), "rules without a test: {missing:?}");
    }

    #[test]
    fn every_refusal_names_a_rule_of_the_file() {
        let ids = rule_ids();
        for r in crate::say::import::ALL_REFUSED {
            let code = r.words().code.unwrap();
            assert!(ids.iter().any(|i| i == code), "{code}");
            assert!(format!("{r}").starts_with(code));
        }
        for e in [
            SingleKeyError::ChecksumMissing,
            SingleKeyError::ChecksumMismatch,
            SingleKeyError::TestNetwork,
            SingleKeyError::Unsupported("x"),
            SingleKeyError::IndexOutOfRange,
            SingleKeyError::Derive,
        ] {
            let shown = e.to_string();
            assert!(ids.iter().any(|i| shown.starts_with(i.as_str())), "{shown}");
        }
    }
}
