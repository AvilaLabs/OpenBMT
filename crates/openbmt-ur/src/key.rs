//! A cosigner key from a code (UR-23 to UR-30): the text that the key
//! ceremony takes, `[<8 hex>/48h/<coin>h/0h/2h]<xpub or tpub>`, made from
//! a `crypto-account`, a `crypto-hdkey` or a Coldcard-style JSON export.
//! Nothing is guessed: a code that leaves out a part of the key, or that
//! holds more than one possible key, is refused with words that say what to
//! do about it.

use std::fmt;

use crate::base58::{decode_check, encode_check};
use crate::cbor::decode_bytes;
use crate::cbor_read::{self, Item};
use crate::json_read::{read_object, Value};
use crate::point::is_compressed_point;

/// The longest message read, in bytes. A key export is under 2 KiB.
pub const MAX_KEY_CBOR: usize = 64 * 1024;

/// The version bytes of an extended public key on the main network.
const XPUB: [u8; 4] = [0x04, 0x88, 0xB2, 0x1E];
/// The version bytes of an extended public key on a test network.
const TPUB: [u8; 4] = [0x04, 0x35, 0x87, 0xCF];
/// Version bytes of the other public forms of SLIP 132: ypub, zpub, Ypub,
/// Zpub, upub, vpub, Upub, Vpub.
const SLIP_PUBLIC: [[u8; 4]; 8] = [
    [0x04, 0x9D, 0x7C, 0xB2],
    [0x04, 0xB2, 0x47, 0x46],
    [0x02, 0x95, 0xB4, 0x3F],
    [0x02, 0xAA, 0x7E, 0xD3],
    [0x04, 0x4A, 0x52, 0x62],
    [0x04, 0x5F, 0x1C, 0xF6],
    [0x02, 0x42, 0x89, 0xEF],
    [0x02, 0x57, 0x54, 0x83],
];
/// Version bytes of extended secret keys: xprv, tprv and the SLIP 132 forms.
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

/// The child number of the last step of the path, `2h`.
const CHILD: u32 = 0x8000_0002;

/// The key text the key ceremony takes: `[<fingerprint>/48h/<coin>h/0h/2h]`
/// and the extended public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyText(String);

impl KeyText {
    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text, owned.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Words for a person: a heading and two sentences, as `Words` of the
/// screen rules has them (`docs/SCREEN-RULES.md`). The app gives the colour
/// and says who does the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyWords {
    pub heading: &'static str,
    pub what: &'static str,
    pub next: &'static str,
}

/// Why a code was not taken as a key. Nothing was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// UR-23: not in the strict form that is read, or too long.
    Malformed,
    /// UR-24: a code of the newer kinds, which are not read.
    NewKind,
    /// UR-24: a code of a kind that is not a key.
    OtherKind,
    /// UR-24: a key laid out in a way that is not read.
    Shape,
    /// UR-25: no key for a shared wallet of native segwit type at the
    /// first account.
    NoSharedKey,
    /// UR-25: more than one such key.
    SeveralSharedKeys,
    /// UR-26: a secret key.
    SecretKey,
    /// UR-26: a key without its chain code.
    NoChainCode,
    /// UR-26: key bytes that are not a public key of the curve.
    BadKeyBytes,
    /// UR-26: coin or network other than bitcoin on main or test.
    BadCoinInfo,
    /// UR-27: no origin.
    NoOrigin,
    /// UR-27: an origin that is not `48h/<coin>h/0h/2h` for the network.
    BadPath,
    /// UR-27: no master fingerprint anywhere.
    NoFingerprint,
    /// UR-27: two master fingerprints that differ.
    FingerprintsDiffer,
    /// UR-28: no parent fingerprint, so the key cannot be written out.
    NoParent,
    /// UR-29: text that is not a strict JSON object.
    JsonNotStrict,
    /// UR-29: a JSON export without the fingerprint, the path or the key,
    /// or with one that is not text of the right form.
    JsonFields,
    /// UR-29: a key written in one of the special forms of one wallet type.
    JsonSpecialForm,
    /// UR-29: a key that is not a public account key.
    JsonKey,
}

/// Every refusal, once, for the lists of words and for the tests.
pub const ALL_KEY_REFUSALS: [KeyRefusal; 19] = [
    KeyRefusal::Malformed,
    KeyRefusal::NewKind,
    KeyRefusal::OtherKind,
    KeyRefusal::Shape,
    KeyRefusal::NoSharedKey,
    KeyRefusal::SeveralSharedKeys,
    KeyRefusal::SecretKey,
    KeyRefusal::NoChainCode,
    KeyRefusal::BadKeyBytes,
    KeyRefusal::BadCoinInfo,
    KeyRefusal::NoOrigin,
    KeyRefusal::BadPath,
    KeyRefusal::NoFingerprint,
    KeyRefusal::FingerprintsDiffer,
    KeyRefusal::NoParent,
    KeyRefusal::JsonNotStrict,
    KeyRefusal::JsonFields,
    KeyRefusal::JsonSpecialForm,
    KeyRefusal::JsonKey,
];

impl KeyRefusal {
    /// The rule's code.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Malformed => "UR-23",
            Self::NewKind | Self::OtherKind | Self::Shape => "UR-24",
            Self::NoSharedKey | Self::SeveralSharedKeys => "UR-25",
            Self::SecretKey | Self::NoChainCode | Self::BadKeyBytes | Self::BadCoinInfo => "UR-26",
            Self::NoOrigin | Self::BadPath | Self::NoFingerprint | Self::FingerprintsDiffer => {
                "UR-27"
            }
            Self::NoParent => "UR-28",
            Self::JsonNotStrict | Self::JsonFields | Self::JsonSpecialForm | Self::JsonKey => {
                "UR-29"
            }
        }
    }

    fn text(&self) -> &'static str {
        match self {
            Self::Malformed => "the code is cut short, changed, too long or not in the form read",
            Self::NewKind => "the code is of a newer kind that is not read",
            Self::OtherKind => "the code is not a key",
            Self::Shape => "the key is laid out in a way that is not read",
            Self::NoSharedKey => "the code has no key for a shared wallet at the first account",
            Self::SeveralSharedKeys => "the code has more than one such key",
            Self::SecretKey => "the code holds a secret key",
            Self::NoChainCode => "the key has no chain code",
            Self::BadKeyBytes => "the key bytes are not a public key",
            Self::BadCoinInfo => "the coin or network is not bitcoin on main or test",
            Self::NoOrigin => "the key has no origin",
            Self::BadPath => "the origin is not the path of a shared wallet for the network",
            Self::NoFingerprint => "the code has no master fingerprint",
            Self::FingerprintsDiffer => "the code has two master fingerprints that differ",
            Self::NoParent => "the key has no parent fingerprint",
            Self::JsonNotStrict => "the text is not a strict JSON object",
            Self::JsonFields => "the export lacks a field or has one that is not in form",
            Self::JsonSpecialForm => "the key is written in a special form of one wallet type",
            Self::JsonKey => "the key is not a public key at the account level",
        }
    }

    /// What a person is told. Every set passes the checks of
    /// `docs/SCREEN-RULES.md` (UR-30).
    pub fn words(&self) -> KeyWords {
        let (heading, what, next) = match self {
            Self::Malformed => (
                "Key code not readable",
                "The code was read in full, but what it holds is cut short, changed or not in a form this app takes.",
                "Show the key code again, or show the key as text in the other wallet app and scan that.",
            ),
            Self::NewKind => (
                "Code type not read",
                "This code is of a newer kind that this app does not read, so no key was taken from it.",
                "In the other wallet app, show the key as text, then scan that instead.",
            ),
            Self::OtherKind => (
                "Not a key code",
                "This code carries something other than one key, so nothing was taken from it.",
                "Scan the code that shows the key, or show the key as text and scan that.",
            ),
            Self::Shape => (
                "Key code in an odd shape",
                "The code holds a key, but not laid out in a way this app reads, so nothing was taken.",
                "In the other wallet app, show the key as text, then scan that instead.",
            ),
            Self::NoSharedKey => (
                "No key for a shared wallet",
                "The code has no key for a shared wallet of native segwit type at the first account.",
                "In the other wallet app, export the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::SeveralSharedKeys => (
                "Several keys in the code",
                "The code holds more than one key for a shared wallet, so this app cannot tell which one to take.",
                "In the other wallet app, export only the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::SecretKey => (
                "Code holds a secret key",
                "The code carries a secret key, which must never leave the app that made it, so nothing was taken.",
                "Do not use it. Make a new key in the other wallet app, and show only its public key.",
            ),
            Self::NoChainCode => (
                "Key code is incomplete",
                "The key in the code comes without the part that lets this app work out the wallet's addresses.",
                "In the other wallet app, export the full key with its origin, then scan it again.",
            ),
            Self::BadKeyBytes => (
                "Key in the code is damaged",
                "The key in the code is not a valid public key, so nothing was taken.",
                "Show the code again and scan it again. If it fails again, export the key again.",
            ),
            Self::BadCoinInfo => (
                "Code is for another coin",
                "The code says the key is for something other than bitcoin on the main or the test network.",
                "In the other wallet app, make or export the key for bitcoin, then scan it again.",
            ),
            Self::NoOrigin => (
                "Key has no origin",
                "The key came without its fingerprint and path, so this app cannot tell which key it is.",
                "In the other wallet app, export the key with its origin, then scan it again.",
            ),
            Self::BadPath => (
                "Key on another path",
                "The key does not come from the path that a shared wallet uses, or not for the network in use.",
                "Make or export the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::NoFingerprint => (
                "Key has no fingerprint",
                "The code does not say which key it came from, so this app cannot tell it from other keys.",
                "In the other wallet app, export the key with its fingerprint, then scan it again.",
            ),
            Self::FingerprintsDiffer => (
                "Fingerprints do not match",
                "The code names two different keys as the one it came from, so it cannot be relied on.",
                "Do not use it. Export the key again in the other wallet app, then scan it again.",
            ),
            Self::NoParent => (
                "Key code lacks a detail",
                "The code leaves out a detail this app needs to write the key out in full, so nothing was taken.",
                "In the other wallet app, export the key again, or show it as text, then scan that.",
            ),
            Self::JsonNotStrict => (
                "Key text not readable",
                "The code holds text that is not a plain, well-formed key export, so nothing was taken.",
                "Export the key again in the other wallet app, or show it as text, then scan it again.",
            ),
            Self::JsonFields => (
                "Key export incomplete",
                "The export lacks the fingerprint, the path or the key for a shared wallet of native segwit type.",
                "In the other wallet app, export the key for a shared wallet of native segwit type, then scan it again.",
            ),
            Self::JsonSpecialForm => (
                "Key in a special form",
                "The key in the export is written in a special form for one wallet type, not as a plain public key.",
                "In the other wallet app, export the plain public key for a shared wallet, then scan it again.",
            ),
            Self::JsonKey => (
                "Key in the export is wrong",
                "The key in the export is not a valid public key at the first account level of a shared wallet.",
                "Export the key again in the other wallet app, then scan it again.",
            ),
        };
        KeyWords {
            heading,
            what,
            next,
        }
    }
}

impl fmt::Display for KeyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule(), self.text())
    }
}

impl std::error::Error for KeyRefusal {}

type Refused<T> = Result<T, KeyRefusal>;

/// The key text in a finished UR of `ur_type` whose message is `cbor`
/// (UR-24): `crypto-account`, `crypto-hdkey`, or `bytes` holding the JSON
/// of a Coldcard-style export. `cbor` is the message as the decoder gives
/// it, so for `bytes` it is the CBOR byte string around the JSON.
pub fn ur_to_key_text(ur_type: &str, cbor: &[u8]) -> Result<KeyText, KeyRefusal> {
    match ur_type {
        "crypto-account" | "crypto-hdkey" | "bytes" => {}
        "hdkey" | "account-descriptor" | "output-descriptor" => return Err(KeyRefusal::NewKind),
        _ => return Err(KeyRefusal::OtherKind),
    }
    if cbor.len() > MAX_KEY_CBOR {
        return Err(KeyRefusal::Malformed);
    }
    if ur_type == "bytes" {
        let json = decode_bytes(cbor).map_err(|_| KeyRefusal::Malformed)?;
        return from_json(json).map(KeyText);
    }
    let item = cbor_read::read(cbor).map_err(|_| KeyRefusal::Malformed)?;
    let text = if ur_type == "crypto-account" {
        from_account(&item)?
    } else {
        let inner = untag(&item, &[303, 40303]);
        read_hdkey(inner, None)?
    };
    Ok(KeyText(text))
}

/// The item inside one tag of `tags`, if it has one of them, else itself.
fn untag<'x, 'a>(item: &'x Item<'a>, tags: &[u64]) -> &'x Item<'a> {
    match item {
        Item::Tag(t, inner) if tags.contains(t) => inner.as_ref(),
        _ => item,
    }
}

/// The item inside a tag of `tags`; no tag, or another, is refused.
fn tagged<'x, 'a>(item: &'x Item<'a>, tags: &[u64]) -> Refused<&'x Item<'a>> {
    match item {
        Item::Tag(t, inner) if tags.contains(t) => Ok(inner.as_ref()),
        _ => Err(KeyRefusal::Shape),
    }
}

/// An unsigned integer that is a nonzero 32-bit fingerprint.
fn fingerprint(item: &Item<'_>) -> Refused<u32> {
    item.uint()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n != 0)
        .ok_or(KeyRefusal::Shape)
}

/// UR-25: the one key of the account that is a cosigner key of the first
/// account for a shared wallet of native segwit type.
fn from_account(item: &Item<'_>) -> Refused<String> {
    let map = untag(item, &[311]);
    let Item::Map(entries) = map else {
        return Err(KeyRefusal::Shape);
    };
    if entries.iter().any(|(k, _)| *k != 1 && *k != 2) {
        return Err(KeyRefusal::Shape);
    }
    let account_fp = fingerprint(map.get(1).ok_or(KeyRefusal::Shape)?)?;
    let Some(Item::Array(outputs)) = map.get(2) else {
        return Err(KeyRefusal::Shape);
    };
    if outputs.is_empty() {
        return Err(KeyRefusal::Shape);
    }
    let mut chosen: Vec<&Item<'_>> = Vec::new();
    for output in outputs {
        if let Some(key) = wsh_single_key(output)? {
            let origin = read_origin(key)?;
            if origin.coin().is_some() {
                chosen.push(key);
            }
        }
    }
    match chosen.as_slice() {
        [] => Err(KeyRefusal::NoSharedKey),
        [key] => read_hdkey(key, Some(account_fp)),
        _ => Err(KeyRefusal::SeveralSharedKeys),
    }
}

/// The hdkey of an output that is `wsh` over a single key: `401(303(key))`
/// or `401(410(303(key)))`, each with or without an outer `308`. Another
/// output of the old kind (`wpkh`, `sh(wsh)`, `tr`, a multisig) is `None`;
/// one that is not of the old kind is refused.
fn wsh_single_key<'x, 'a>(output: &'x Item<'a>) -> Refused<Option<&'x Item<'a>>> {
    let mut chain: Vec<u64> = Vec::new();
    let mut item = output;
    while let Item::Tag(t, inner) = item {
        chain.push(*t);
        item = inner.as_ref();
    }
    let chain = match chain.split_first() {
        Some((308, rest)) => rest,
        _ => chain.as_slice(),
    };
    let known = !chain.is_empty() && chain.iter().all(|t| matches!(*t, 303 | 400..=410 | 40303));
    if !known || !matches!(item, Item::Map(_)) {
        return Err(KeyRefusal::Shape);
    }
    Ok(match chain {
        [401, hd] | [401, 410, hd] if *hd == 303 || *hd == 40303 => Some(item),
        _ => None,
    })
}

/// The origin of an hdkey, as written.
struct Origin {
    path: Vec<(u64, bool)>,
    fingerprint: Option<u32>,
    depth: Option<u64>,
}

impl Origin {
    /// The coin of the path `48h/<coin>h/0h/2h`, when it is one.
    fn coin(&self) -> Option<u8> {
        match self.path.as_slice() {
            [(48, true), (coin @ (0 | 1), true), (0, true), (2, true)] => u8::try_from(*coin).ok(),
            _ => None,
        }
    }
}

/// UR-27: key 6 of an hdkey.
fn read_origin(hdkey: &Item<'_>) -> Refused<Origin> {
    let origin = hdkey.get(6).ok_or(KeyRefusal::NoOrigin)?;
    let keypath = tagged(origin, &[304, 40304])?;
    let Item::Map(entries) = keypath else {
        return Err(KeyRefusal::Shape);
    };
    if entries.iter().any(|(k, _)| !(1..=3).contains(k)) {
        return Err(KeyRefusal::Shape);
    }
    let Some(Item::Array(parts)) = keypath.get(1) else {
        return Err(KeyRefusal::NoOrigin);
    };
    if !parts.len().is_multiple_of(2) {
        return Err(KeyRefusal::Shape);
    }
    let mut path = Vec::new();
    for pair in parts.as_chunks::<2>().0 {
        match pair {
            [Item::Uint(index), Item::Bool(hardened)] if *index < 0x8000_0000 => {
                path.push((*index, *hardened));
            }
            _ => return Err(KeyRefusal::Shape),
        }
    }
    let fingerprint = keypath.get(2).map(self::fingerprint).transpose()?;
    let depth = match keypath.get(3) {
        None => None,
        Some(item) => Some(item.uint().filter(|d| *d <= 255).ok_or(KeyRefusal::Shape)?),
    };
    Ok(Origin {
        path,
        fingerprint,
        depth,
    })
}

/// UR-26: key 5 of an hdkey: 0 for the main network, 1 for a test one.
fn read_network(hdkey: &Item<'_>) -> Refused<u8> {
    let Some(info) = hdkey.get(5) else {
        return Ok(0);
    };
    let info = tagged(info, &[305, 40305])?;
    let Item::Map(entries) = info else {
        return Err(KeyRefusal::Shape);
    };
    if entries.iter().any(|(k, _)| *k != 1 && *k != 2) {
        return Err(KeyRefusal::BadCoinInfo);
    }
    if let Some(kind) = info.get(1) {
        if kind.uint() != Some(0) {
            return Err(KeyRefusal::BadCoinInfo);
        }
    }
    match info.get(2) {
        None => Ok(0),
        Some(net) => match net.uint() {
            Some(0) => Ok(0),
            Some(1) => Ok(1),
            _ => Err(KeyRefusal::BadCoinInfo),
        },
    }
}

/// UR-26 to UR-28: the key text of one hdkey. `account_fp` is the master
/// fingerprint of the account around it, if there is one.
fn read_hdkey(hdkey: &Item<'_>, account_fp: Option<u32>) -> Refused<String> {
    let Item::Map(entries) = hdkey else {
        return Err(KeyRefusal::Shape);
    };
    // Key 1 is a master key and 7 is for children; neither is a cosigner
    // key, and nothing else is defined above 10.
    if entries.iter().any(|(k, _)| !(2..=10).contains(k)) {
        return Err(KeyRefusal::Shape);
    }
    match hdkey.get(2) {
        None | Some(Item::Bool(false)) => {}
        Some(Item::Bool(true)) => return Err(KeyRefusal::SecretKey),
        Some(_) => return Err(KeyRefusal::Shape),
    }
    let key = match hdkey.get(3) {
        Some(Item::Bytes(b)) if b.len() == 33 && is_compressed_point(b) => *b,
        _ => return Err(KeyRefusal::BadKeyBytes),
    };
    let chain = match hdkey.get(4) {
        None => return Err(KeyRefusal::NoChainCode),
        Some(Item::Bytes(b)) if b.len() == 32 => *b,
        Some(_) => return Err(KeyRefusal::BadKeyBytes),
    };
    let network = read_network(hdkey)?;
    let origin = read_origin(hdkey)?;
    if origin.coin() != Some(network) || origin.depth.is_some_and(|d| d != 4) {
        return Err(KeyRefusal::BadPath);
    }
    let master = match (account_fp, origin.fingerprint) {
        (Some(a), Some(o)) if a != o => return Err(KeyRefusal::FingerprintsDiffer),
        (Some(f), _) | (None, Some(f)) => f,
        (None, None) => return Err(KeyRefusal::NoFingerprint),
    };
    let parent = fingerprint(hdkey.get(8).ok_or(KeyRefusal::NoParent)?)?;
    let mut raw = Vec::with_capacity(78);
    raw.extend_from_slice(if network == 0 { &XPUB } else { &TPUB });
    raw.push(4);
    raw.extend_from_slice(&parent.to_be_bytes());
    raw.extend_from_slice(&CHILD.to_be_bytes());
    raw.extend_from_slice(chain);
    raw.extend_from_slice(key);
    let key = encode_check(&raw).ok_or(KeyRefusal::Shape)?;
    Ok(origin_text(master, network, &key))
}

fn origin_text(master: u32, coin: u8, key: &str) -> String {
    format!("[{master:08x}/48h/{coin}h/0h/2h]{key}")
}

/// UR-29: the key text of a Coldcard-style JSON export.
fn from_json(json: &[u8]) -> Refused<String> {
    let entries = read_object(json).ok_or(KeyRefusal::JsonNotStrict)?;
    let field = |name: &str| -> Refused<&str> {
        match entries.iter().find(|(k, _)| k == name) {
            Some((_, Value::Text(t))) => Ok(t.as_str()),
            _ => Err(KeyRefusal::JsonFields),
        }
    };
    let xfp = field("xfp")?;
    let deriv = field("p2wsh_deriv")?;
    let key = field("p2wsh")?;
    if xfp.len() != 8 || !xfp.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(KeyRefusal::JsonFields);
    }
    let master = u32::from_str_radix(xfp, 16).map_err(|_| KeyRefusal::JsonFields)?;
    if master == 0 {
        return Err(KeyRefusal::JsonFields);
    }
    let coin = json_coin(deriv)?;
    let raw = decode_check(key).ok_or(KeyRefusal::JsonKey)?;
    let version: [u8; 4] = raw
        .get(..4)
        .and_then(|v| v.try_into().ok())
        .ok_or(KeyRefusal::JsonKey)?;
    if SECRET.contains(&version) {
        return Err(KeyRefusal::SecretKey);
    }
    if SLIP_PUBLIC.contains(&version) {
        return Err(KeyRefusal::JsonSpecialForm);
    }
    let network = match version {
        XPUB => 0,
        TPUB => 1,
        _ => return Err(KeyRefusal::JsonKey),
    };
    if network != coin {
        return Err(KeyRefusal::BadPath);
    }
    let level_ok = raw.len() == 78
        && raw.get(4) == Some(&4)
        && raw.get(9..13) == Some(CHILD.to_be_bytes().as_slice())
        && raw.get(45..).is_some_and(is_compressed_point);
    if !level_ok {
        return Err(KeyRefusal::JsonKey);
    }
    let key = encode_check(&raw).ok_or(KeyRefusal::JsonKey)?;
    Ok(origin_text(master, coin, &key))
}

/// The coin of `m/48'/<coin>'/0'/2'` (`'` or `h`), 0 or 1.
fn json_coin(deriv: &str) -> Refused<u8> {
    let rest = deriv.strip_prefix("m/").ok_or(KeyRefusal::BadPath)?;
    let mut numbers = Vec::new();
    for part in rest.split('/') {
        let digits = part
            .strip_suffix('\'')
            .or_else(|| part.strip_suffix('h'))
            .ok_or(KeyRefusal::BadPath)?;
        if digits.is_empty()
            || digits.len() > 3
            || (digits.len() > 1 && digits.starts_with('0'))
            || !digits.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(KeyRefusal::BadPath);
        }
        numbers.push(digits.parse::<u32>().map_err(|_| KeyRefusal::BadPath)?);
    }
    match numbers.as_slice() {
        [48, coin @ (0 | 1), 0, 2] => u8::try_from(*coin).map_err(|_| KeyRefusal::BadPath),
        _ => Err(KeyRefusal::BadPath),
    }
}
