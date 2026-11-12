//! Read the bounded single-signature and 2-of-3 wallet descriptors carried
//! by the Blockchain Commons registry types.

use std::fmt;

use crate::base58::encode_check;
use crate::cbor::decode_bytes;
use crate::cbor_read::{self, Item};
use crate::error::UrError;
use crate::point::is_compressed_point;

/// Largest wallet payload accepted by this reader.
pub const MAX_WALLET_CBOR: usize = 64 * 1024;

/// A canonical output descriptor suitable for importing as a watch-only
/// wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletText(String);

impl WalletText {
    /// The descriptor text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The descriptor text, owned.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Why a payload could not safely be read as one of the supported wallets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletRefusal {
    /// The registry type is not one of the wallet types this reader takes.
    Kind,
    /// The payload is not strict CBOR or has malformed fields.
    Malformed,
    /// The wallet uses a script, key path, or shape not imported here.
    Unsupported,
    /// A key belongs to a test network.
    TestNetwork,
    /// A secret or master key is present.
    SecretKey,
    /// A payload, descriptor or scan exceeds its size bound.
    TooLarge,
    /// A required descriptor component is absent.
    Incomplete,
    /// The scan exhausted its finite part-attempt budget.
    SequenceLimit,
    /// An error from the UR transport.
    Transport(UrError),
}

impl WalletRefusal {
    /// The screen-rule code for this refusal.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Kind => "UR-34",
            Self::Malformed => "UR-35",
            Self::Unsupported => "UR-36",
            Self::TestNetwork => "UR-37",
            Self::SecretKey => "UR-38",
            Self::TooLarge => "UR-39",
            Self::Incomplete | Self::SequenceLimit => "UR-40",
            Self::Transport(error) => error.rule(),
        }
    }
}

impl fmt::Display for WalletRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Kind => "the code type is not a wallet type this app reads",
            Self::Malformed => "the wallet code is changed or not in the form read",
            Self::Unsupported => "the wallet code uses a wallet type this app does not read",
            Self::TestNetwork => "the wallet code is for a test network",
            Self::SecretKey => "the wallet code contains a secret key",
            Self::TooLarge => "the wallet code exceeds the supported size",
            Self::Incomplete => "the wallet code does not contain enough information to import",
            Self::SequenceLimit => "the wallet code sequence is outside the supported limit",
            // Do not echo transport parser details: they can contain input.
            Self::Transport(_) => "the wallet code could not be read",
        };
        write!(f, "{}: {message}", self.rule())
    }
}

impl std::error::Error for WalletRefusal {}

type Refused<T> = Result<T, WalletRefusal>;

const XPUB: [u8; 4] = [0x04, 0x88, 0xB2, 0x1E];

/// Read a `crypto-output`, `output`, `output-descriptor`, or `crypto-account`
/// message into one supported Bitcoin mainnet descriptor.
pub fn ur_to_wallet_text(ur_type: &str, cbor: &[u8]) -> Result<WalletText, WalletRefusal> {
    if !matches!(
        ur_type,
        "crypto-output" | "output" | "output-descriptor" | "crypto-account" | "bytes"
    ) {
        return Err(WalletRefusal::Kind);
    }
    if cbor.len() > MAX_WALLET_CBOR {
        return Err(WalletRefusal::TooLarge);
    }
    let item = cbor_read::read(cbor).map_err(|_| WalletRefusal::Malformed)?;
    let descriptor = match ur_type {
        "crypto-output" => from_crypto_output(&item)?,
        "output" | "output-descriptor" => from_output_descriptor(&item)?,
        "crypto-account" => from_account(&item)?,
        "bytes" => {
            let bytes = decode_bytes(cbor).map_err(|_| WalletRefusal::Malformed)?;
            let text = std::str::from_utf8(bytes).map_err(|_| WalletRefusal::Malformed)?;
            if text.len() > MAX_DESCRIPTOR_LEN {
                return Err(WalletRefusal::TooLarge);
            }
            text.to_owned()
        }
        _ => return Err(WalletRefusal::Kind),
    };
    if descriptor.len() > MAX_DESCRIPTOR_LEN {
        return Err(WalletRefusal::TooLarge);
    }
    let canonical = crate::wallet_text::canonicalize_descriptor(&descriptor)?;
    if canonical.len() > MAX_DESCRIPTOR_LEN {
        return Err(WalletRefusal::TooLarge);
    }
    Ok(WalletText(canonical))
}

const MAX_DESCRIPTOR_LEN: usize = 4096;

fn tags_and_inner<'x, 'a>(item: &'x Item<'a>) -> (Vec<u64>, &'x Item<'a>) {
    let mut tags = Vec::new();
    let mut inner = item;
    while let Item::Tag(tag, next) = inner {
        tags.push(*tag);
        inner = next;
    }
    (tags, inner)
}

fn from_crypto_output(item: &Item<'_>) -> Refused<String> {
    let (tags, inner) = tags_and_inner(item);
    let tags = match tags.as_slice() {
        [308, rest @ ..] => rest,
        _ => tags.as_slice(),
    };
    match tags {
        [404, 303] | [404, 40303] => {
            let key = parse_hdkey(inner, &PathKind::Single, None)?;
            Ok(format!("wpkh({})", key.expression))
        }
        [401, 407] => from_sortedmulti(inner, 2),
        [] => Err(WalletRefusal::Unsupported),
        _ => Err(WalletRefusal::Unsupported),
    }
}

fn from_sortedmulti(item: &Item<'_>, threshold: usize) -> Refused<String> {
    let Item::Map(entries) = item else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| !matches!(*key, 1 | 2)) {
        return Err(WalletRefusal::Malformed);
    }
    if item.get(1).and_then(Item::uint) != Some(threshold as u64) {
        return Err(WalletRefusal::Unsupported);
    }
    let Some(Item::Array(keys)) = item.get(2) else {
        return Err(WalletRefusal::Incomplete);
    };
    if keys.len() != 3 {
        return Err(WalletRefusal::Unsupported);
    }
    let mut expressions = Vec::with_capacity(keys.len());
    for item in keys {
        let hdkey = tagged_hdkey(item)?;
        expressions.push(parse_hdkey(hdkey, &PathKind::Multi, None)?.expression);
    }
    Ok(format!("wsh(sortedmulti(2,{}))", expressions.join(",")))
}

#[derive(Clone, Copy)]
enum PathKind {
    Single,
    Multi,
}

struct ParsedKey {
    expression: String,
}

fn tagged_hdkey<'x, 'a>(item: &'x Item<'a>) -> Refused<&'x Item<'a>> {
    match item {
        Item::Tag(303 | 40303, inner) => Ok(inner),
        _ => Err(WalletRefusal::Malformed),
    }
}

fn public_key_data<'a>(item: &Item<'a>) -> Refused<&'a [u8]> {
    match item.get(3) {
        Some(Item::Bytes(bytes))
            if bytes.len() == 32 || (bytes.len() == 33 && bytes.first() == Some(&0)) =>
        {
            Err(WalletRefusal::SecretKey)
        }
        Some(Item::Bytes(bytes)) if bytes.len() == 33 && is_compressed_point(bytes) => Ok(bytes),
        _ => Err(WalletRefusal::Malformed),
    }
}

fn parse_hdkey(item: &Item<'_>, kind: &PathKind, account_fp: Option<u32>) -> Refused<ParsedKey> {
    let Item::Map(entries) = item else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| !(1..=10).contains(key)) {
        return Err(WalletRefusal::Malformed);
    }
    for key in [9, 10] {
        if item
            .get(key)
            .is_some_and(|value| !matches!(value, Item::Text(_)))
        {
            return Err(WalletRefusal::Malformed);
        }
    }
    if item.get(1) == Some(&Item::Bool(true)) || item.get(2) == Some(&Item::Bool(true)) {
        return Err(WalletRefusal::SecretKey);
    }
    if item.get(1).is_some_and(|v| v != &Item::Bool(false)) {
        return Err(WalletRefusal::Malformed);
    }
    if item.get(2).is_some_and(|v| v != &Item::Bool(false)) {
        return Err(WalletRefusal::Malformed);
    }
    let key = public_key_data(item)?;
    let chain = match item.get(4) {
        Some(Item::Bytes(bytes)) if bytes.len() == 32 => bytes,
        _ => return Err(WalletRefusal::Incomplete),
    };
    let testnet = read_network(item)?;
    if testnet {
        return Err(WalletRefusal::TestNetwork);
    }
    let Origin {
        fingerprint: origin_fp,
        path,
    } = read_origin(item)?;
    if path.get(1) == Some(&(1, true)) {
        return Err(WalletRefusal::TestNetwork);
    }
    let fingerprint = match (origin_fp, account_fp) {
        (Some(origin), Some(account)) if origin != account => return Err(WalletRefusal::Malformed),
        (Some(origin), _) => origin,
        (None, Some(account)) => account,
        (None, None) => return Err(WalletRefusal::Incomplete),
    };
    let expected_path: &[(u32, bool)] = match kind {
        PathKind::Single => &[(84, true), (0, true), (0, true)],
        PathKind::Multi => &[(48, true), (0, true), (0, true), (2, true)],
    };
    if path.as_slice() != expected_path {
        return Err(WalletRefusal::Unsupported);
    }
    let depth = expected_path.len();
    let parent = item
        .get(8)
        .and_then(Item::uint)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(WalletRefusal::Incomplete)?;
    let mut payload = Vec::with_capacity(78);
    payload.extend_from_slice(&XPUB);
    payload.push(u8::try_from(depth).map_err(|_| WalletRefusal::Malformed)?);
    payload.extend_from_slice(&parent.to_be_bytes());
    let child = 0x8000_0000 | path.last().ok_or(WalletRefusal::Malformed)?.0;
    payload.extend_from_slice(&child.to_be_bytes());
    payload.extend_from_slice(chain);
    payload.extend_from_slice(key);
    let xpub = encode_check(&payload).ok_or(WalletRefusal::Malformed)?;
    read_children(item.get(7))?;
    Ok(ParsedKey {
        expression: format!("[{fingerprint:08x}/{}]{xpub}/<0;1>/*", path_text(&path)),
    })
}

fn read_network(hdkey: &Item<'_>) -> Refused<bool> {
    let Some(info) = hdkey.get(5) else {
        return Ok(false);
    };
    let info = match info {
        Item::Tag(305 | 40305, inner) => inner.as_ref(),
        _ => return Err(WalletRefusal::Malformed),
    };
    let Item::Map(entries) = info else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| !matches!(*key, 1 | 2)) {
        return Err(WalletRefusal::Malformed);
    }
    if let Some(coin) = info.get(1) {
        match coin.uint() {
            Some(0) => {}
            Some(1) => return Err(WalletRefusal::TestNetwork),
            _ => return Err(WalletRefusal::Unsupported),
        }
    }
    match info.get(2) {
        None | Some(Item::Uint(0)) => Ok(false),
        Some(Item::Uint(1)) => Ok(true),
        Some(_) => Err(WalletRefusal::Malformed),
    }
}

struct Origin {
    fingerprint: Option<u32>,
    path: Vec<(u32, bool)>,
}

fn read_origin(hdkey: &Item<'_>) -> Refused<Origin> {
    let Some(Item::Tag(304 | 40304, inner)) = hdkey.get(6) else {
        return Err(WalletRefusal::Incomplete);
    };
    let Item::Map(entries) = inner.as_ref() else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| !matches!(*key, 1..=3)) {
        return Err(WalletRefusal::Malformed);
    }
    let Some(Item::Array(parts)) = inner.get(1) else {
        return Err(WalletRefusal::Incomplete);
    };
    if parts.is_empty() || !parts.len().is_multiple_of(2) {
        return Err(WalletRefusal::Malformed);
    }
    let mut path = Vec::with_capacity(parts.len() / 2);
    for pair in parts.as_chunks::<2>().0 {
        match pair {
            [Item::Uint(index), Item::Bool(hardened)] if *index < 0x8000_0000 => {
                path.push((
                    u32::try_from(*index).map_err(|_| WalletRefusal::Malformed)?,
                    *hardened,
                ));
            }
            _ => return Err(WalletRefusal::Malformed),
        }
    }
    let fingerprint = match inner.get(2) {
        None => None,
        Some(item) => Some(
            item.uint()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or(WalletRefusal::Malformed)?,
        ),
    };
    match inner.get(3) {
        None => {}
        Some(Item::Uint(depth)) if *depth == path.len() as u64 => {}
        Some(_) => return Err(WalletRefusal::Malformed),
    }
    Ok(Origin { fingerprint, path })
}

fn path_text(path: &[(u32, bool)]) -> String {
    let mut out = String::new();
    for (offset, (index, hardened)) in path.iter().enumerate() {
        use fmt::Write as _;
        if offset > 0 {
            out.push('/');
        }
        let _ = if *hardened {
            write!(out, "{index}'")
        } else {
            write!(out, "{index}")
        };
    }
    out
}

fn read_children(item: Option<&Item<'_>>) -> Refused<()> {
    let Some(item) = item else {
        return Ok(());
    };
    let Item::Tag(304 | 40304, inner) = item else {
        return Err(WalletRefusal::Malformed);
    };
    let Item::Map(entries) = inner.as_ref() else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| *key != 1) {
        return Err(WalletRefusal::Malformed);
    }
    let Some(Item::Array(parts)) = inner.get(1) else {
        return Err(WalletRefusal::Malformed);
    };
    match parts.as_slice() {
        [Item::Array(pair), Item::Array(wildcard), Item::Bool(false)]
            if pair.as_slice()
                == [
                    Item::Uint(0),
                    Item::Bool(false),
                    Item::Uint(1),
                    Item::Bool(false),
                ]
                && wildcard.is_empty() =>
        {
            Ok(())
        }
        [Item::Uint(0), Item::Bool(false), Item::Array(wildcard), Item::Bool(false)]
            if wildcard.is_empty() =>
        {
            Err(WalletRefusal::Unsupported)
        }
        _ => Err(WalletRefusal::Unsupported),
    }
}

fn from_account(item: &Item<'_>) -> Refused<String> {
    let (tags, inner) = tags_and_inner(item);
    if !matches!(tags.as_slice(), [] | [311]) {
        return Err(WalletRefusal::Malformed);
    }
    let Item::Map(entries) = inner else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| !matches!(*key, 1 | 2)) {
        return Err(WalletRefusal::Malformed);
    }
    let fingerprint = inner
        .get(1)
        .and_then(Item::uint)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(WalletRefusal::Malformed)?;
    let Some(Item::Array(outputs)) = inner.get(2) else {
        return Err(WalletRefusal::Incomplete);
    };
    let mut candidates = Vec::new();
    for output in outputs {
        scan_account_output(output)?;
        if let Some(key) = account_wpkh(output)? {
            let parsed = parse_hdkey(key, &PathKind::Single, Some(fingerprint))?;
            candidates.push(parsed.expression);
        }
    }
    if candidates.is_empty() {
        return Err(WalletRefusal::Incomplete);
    }
    if candidates.len() != 1 {
        return Err(WalletRefusal::Unsupported);
    }
    let expr = candidates.pop().ok_or(WalletRefusal::Incomplete)?;
    Ok(format!("wpkh({expr})"))
}

fn account_wpkh<'x, 'a>(output: &'x Item<'a>) -> Refused<Option<&'x Item<'a>>> {
    let (tags, inner) = tags_and_inner(output);
    let tags = match tags.as_slice() {
        [308, rest @ ..] => rest,
        _ => tags.as_slice(),
    };
    let Some((hd, script)) = tags.split_last() else {
        return Err(WalletRefusal::Unsupported);
    };
    if !matches!(*hd, 303 | 40303) || !matches!(inner, Item::Map(_)) {
        return Err(WalletRefusal::Malformed);
    }
    match script {
        [404] => Ok(Some(inner)),
        // Other standard account exports: BIP44, BIP49, BIP45, BIP48
        // nested/native cosigners, and BIP86. All keys are still checked
        // for private material and a test network before selection.
        [403] | [400, 404] | [400, 410] | [400, 401, 410] | [401, 410] | [401] | [409] => Ok(None),
        _ => Err(WalletRefusal::Unsupported),
    }
}

fn scan_account_output(item: &Item<'_>) -> Refused<()> {
    match item {
        Item::Tag(303 | 40303, inner) => {
            let hdkey = inner.as_ref();
            let Item::Map(entries) = hdkey else {
                return Err(WalletRefusal::Malformed);
            };
            if entries.iter().any(|(key, _)| !(1..=10).contains(key)) {
                return Err(WalletRefusal::Malformed);
            }
            for field in [9, 10] {
                if hdkey
                    .get(field)
                    .is_some_and(|value| !matches!(value, Item::Text(_)))
                {
                    return Err(WalletRefusal::Malformed);
                }
            }
            for field in [1, 2] {
                match hdkey.get(field) {
                    None | Some(Item::Bool(false)) => {}
                    Some(Item::Bool(true)) => return Err(WalletRefusal::SecretKey),
                    Some(_) => return Err(WalletRefusal::Malformed),
                }
            }
            if hdkey.get(5).is_some() && read_network(hdkey)? {
                return Err(WalletRefusal::TestNetwork);
            }
            if hdkey.get(6).is_some() && read_origin(hdkey)?.path.get(1) == Some(&(1, true)) {
                return Err(WalletRefusal::TestNetwork);
            }
            public_key_data(hdkey)?;
            if !matches!(hdkey.get(4), Some(Item::Bytes(bytes)) if bytes.len() == 32) {
                return Err(WalletRefusal::Incomplete);
            }
            for (_, value) in entries {
                scan_nested(value)?;
            }
            Ok(())
        }
        Item::Map(entries) => {
            for (_, value) in entries {
                scan_nested(value)?;
            }
            Ok(())
        }
        Item::Array(items) => {
            for value in items {
                scan_nested(value)?;
            }
            Ok(())
        }
        Item::Tag(_, inner) => scan_account_output(inner),
        _ => Ok(()),
    }
}

fn scan_nested(item: &Item<'_>) -> Refused<()> {
    match item {
        Item::Tag(303 | 40303, _) => scan_account_output(item),
        Item::Tag(_, inner) => scan_nested(inner),
        Item::Map(entries) => {
            for (_, value) in entries {
                scan_nested(value)?;
            }
            Ok(())
        }
        Item::Array(items) => {
            for value in items {
                scan_nested(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn from_output_descriptor(item: &Item<'_>) -> Refused<String> {
    let (tags, inner) = tags_and_inner(item);
    if !matches!(tags.as_slice(), [] | [40308]) {
        return Err(WalletRefusal::Malformed);
    }
    let Item::Map(entries) = inner else {
        return Err(WalletRefusal::Malformed);
    };
    if entries.iter().any(|(key, _)| !(1..=4).contains(key)) {
        return Err(WalletRefusal::Malformed);
    }
    for key in [3, 4] {
        if inner
            .get(key)
            .is_some_and(|value| !matches!(value, Item::Text(_)))
        {
            return Err(WalletRefusal::Malformed);
        }
    }
    let Some(Item::Text(source)) = inner.get(1) else {
        return Err(WalletRefusal::Incomplete);
    };
    let Some(keys_item) = inner.get(2) else {
        if source.contains('@') {
            return Err(WalletRefusal::Unsupported);
        }
        return Ok((*source).to_owned());
    };
    let Item::Array(keys) = keys_item else {
        return Err(WalletRefusal::Malformed);
    };
    let supported_source = matches!(*source, "wpkh(@0)" | "wsh(sortedmulti(2,@0,@1,@2))");
    if !supported_source {
        return Err(WalletRefusal::Unsupported);
    }
    let mut parsed = Vec::with_capacity(keys.len());
    let path_kind = if source.starts_with("wsh(") {
        PathKind::Multi
    } else {
        PathKind::Single
    };
    for key in keys {
        parsed.push(parse_hdkey(tagged_hdkey(key)?, &path_kind, None)?);
    }
    match *source {
        "wpkh(@0)" if parsed.len() == 1 => Ok(format!(
            "wpkh({})",
            parsed.first().ok_or(WalletRefusal::Incomplete)?.expression
        )),
        "wsh(sortedmulti(2,@0,@1,@2))" if parsed.len() == 3 => Ok(format!(
            "wsh(sortedmulti(2,{}))",
            parsed
                .iter()
                .map(|key| key.expression.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )),
        _ => Err(WalletRefusal::Unsupported),
    }
}
