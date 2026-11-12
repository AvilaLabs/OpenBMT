//! Dependency-free parsing and canonicalization of the wallet text carried by
//! a scanned UR. Successful output is in the exact form accepted by the core
//! wallet importers.

use crate::base58::decode_check;
use crate::point::is_compressed_point;
use crate::wallet::WalletRefusal;

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
const MAX_DESCRIPTOR_BYTES: usize = 4096;

fn polymod(symbols: &[u8]) -> u64 {
    let mut check = 1_u64;
    for value in symbols {
        let top = check >> 35;
        check = ((check & 0x7_ffff_ffff) << 5) ^ u64::from(*value);
        for (i, generator) in GENERATOR.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                check ^= generator;
            }
        }
    }
    check
}

fn checksum_symbols(text: &str) -> Option<Vec<u8>> {
    let mut symbols = Vec::with_capacity(text.len() + text.len() / 3 + 1);
    let mut groups = Vec::with_capacity(3);
    for character in text.chars() {
        let value = u8::try_from(INPUT_CHARSET.find(character)?).ok()?;
        symbols.push(value & 31);
        groups.push(value >> 5);
        if groups.len() == 3 {
            let a = *groups.first()?;
            let b = *groups.get(1)?;
            let c = *groups.get(2)?;
            symbols.push(a * 9 + b * 3 + c);
            groups.clear();
        }
    }
    match groups.len() {
        1 => symbols.push(*groups.first()?),
        2 => symbols.push(*groups.first()? * 3 + *groups.get(1)?),
        _ => {}
    }
    Some(symbols)
}

fn descriptor_checksum(body: &str) -> Option<String> {
    let mut symbols = checksum_symbols(body)?;
    symbols.extend_from_slice(&[0; 8]);
    let check = polymod(&symbols) ^ 1;
    let mut result = String::with_capacity(8);
    for i in 0..8 {
        let shift = 5 * (7 - i);
        let slot = usize::try_from((check >> shift) & 31).ok()?;
        result.push(char::from(*CHECKSUM_CHARSET.get(slot)?));
    }
    Some(result)
}

fn validate_checksum(text: &str) -> Result<&str, WalletRefusal> {
    let Some((body, supplied)) = text.split_once('#') else {
        return Ok(text);
    };
    if supplied.contains('#') {
        return Err(WalletRefusal::Malformed);
    }
    let expected = descriptor_checksum(body).ok_or(WalletRefusal::Malformed)?;
    if supplied != expected {
        return Err(WalletRefusal::Malformed);
    }
    Ok(body)
}

fn hardened_index(text: &str) -> Option<u32> {
    let digits = text.strip_suffix(['h', 'H', '\''])?;
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits
        .parse::<u32>()
        .ok()
        .filter(|value| *value < (1 << 31))
}

fn origin(text: &str, expected_path: &[u32]) -> Result<String, WalletRefusal> {
    let mut parts = text.split('/');
    let fingerprint = parts.next().ok_or(WalletRefusal::Malformed)?;
    if fingerprint.len() != 8 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(WalletRefusal::Malformed);
    }
    for (position, expected) in expected_path.iter().enumerate() {
        let part = parts.next().ok_or(WalletRefusal::Malformed)?;
        match hardened_index(part) {
            Some(value) if value == *expected => {}
            Some(1) if position == 1 => return Err(WalletRefusal::TestNetwork),
            _ => return Err(WalletRefusal::Unsupported),
        }
    }
    if parts.next().is_some() {
        return Err(WalletRefusal::Malformed);
    }
    Ok(fingerprint.to_ascii_lowercase())
}

fn validate_xpub(text: &str, expected_depth: u8, expected_child: u32) -> Result<(), WalletRefusal> {
    let payload = decode_check(text).ok_or_else(|| {
        if text.starts_with("tpub") || text.starts_with("upub") || text.starts_with("vpub") {
            WalletRefusal::TestNetwork
        } else if is_private_extended(text) {
            WalletRefusal::SecretKey
        } else {
            WalletRefusal::Malformed
        }
    })?;
    if payload.len() != 78 {
        return Err(WalletRefusal::Malformed);
    }
    let version = payload.get(0..4).ok_or(WalletRefusal::Malformed)?;
    if version != [0x04, 0x88, 0xb2, 0x1e] {
        if version == [0x04, 0x35, 0x87, 0xcf]
            || version == [0x04, 0x4a, 0x52, 0x62]
            || version == [0x04, 0x5f, 0x1c, 0xf6]
            || version == [0x02, 0x42, 0x89, 0xef]
            || version == [0x02, 0x57, 0x54, 0x83]
        {
            return Err(WalletRefusal::TestNetwork);
        }
        return Err(WalletRefusal::Unsupported);
    }
    if payload.get(4).copied() != Some(expected_depth) {
        return Err(WalletRefusal::Unsupported);
    }
    let child = payload.get(9..13).ok_or(WalletRefusal::Malformed)?;
    let child_number = u32::from_be_bytes(child.try_into().map_err(|_| WalletRefusal::Malformed)?);
    if child_number != (0x8000_0000 | expected_child) {
        return Err(WalletRefusal::Unsupported);
    }
    let public_key = payload.get(45..78).ok_or(WalletRefusal::Malformed)?;
    if !is_compressed_point(public_key) {
        return Err(WalletRefusal::Malformed);
    }
    Ok(())
}

fn is_private_extended(text: &str) -> bool {
    [
        "xprv", "tprv", "yprv", "zprv", "uprv", "vprv", "Yprv", "Zprv", "Uprv", "Vprv",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

fn looks_like_wif(token: &str) -> bool {
    let Some(first) = token.as_bytes().first().copied() else {
        return false;
    };
    let length_and_prefix = match token.len() {
        51 => matches!(first, b'5' | b'9'),
        52 => matches!(first, b'K' | b'L' | b'c'),
        _ => false,
    };
    length_and_prefix
        && token.bytes().all(|byte| {
            b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".contains(&byte)
        })
}

fn reject_secrets_or_testnet(text: &str) -> Result<(), WalletRefusal> {
    let tokens = text.split(|character: char| !character.is_ascii_alphanumeric());
    for token in tokens.filter(|token| !token.is_empty()) {
        if is_private_extended(token) || looks_like_wif(token) {
            return Err(WalletRefusal::SecretKey);
        }
        if ["tpub", "upub", "vpub"]
            .iter()
            .any(|prefix| token.starts_with(prefix))
        {
            return Err(WalletRefusal::TestNetwork);
        }
    }
    if text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| {
            token
                .get(..3)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("tb1"))
                || token
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bcrt1"))
        })
    {
        return Err(WalletRefusal::TestNetwork);
    }
    Ok(())
}

struct KeyText {
    fingerprint: String,
    xpub: String,
}

fn parse_key(
    expression: &str,
    path: &[u32],
    depth: u8,
    child: u32,
) -> Result<KeyText, WalletRefusal> {
    let rest = expression
        .strip_prefix('[')
        .ok_or(WalletRefusal::Malformed)?;
    let (origin_text, after_origin) = rest.split_once(']').ok_or(WalletRefusal::Malformed)?;
    let (xpub, suffix) = after_origin
        .split_once('/')
        .ok_or(WalletRefusal::Malformed)?;
    if suffix != "<0;1>/*" {
        if suffix == "0/*" || suffix == "1/*" {
            return Err(WalletRefusal::Incomplete);
        }
        return Err(WalletRefusal::Unsupported);
    }
    let fingerprint = origin(origin_text, path)?;
    validate_xpub(xpub, depth, child)?;
    Ok(KeyText {
        fingerprint,
        xpub: xpub.to_owned(),
    })
}

/// Validate supported mainnet wallet descriptor text and return its canonical
/// spelling with a BIP380 checksum. A supplied checksum is checked against
/// the input spelling before any path or fingerprint normalization.
pub(crate) fn canonicalize_descriptor(text: &str) -> Result<String, WalletRefusal> {
    if text.len() > MAX_DESCRIPTOR_BYTES {
        return Err(WalletRefusal::TooLarge);
    }
    if text.chars().any(char::is_control) {
        return Err(WalletRefusal::Malformed);
    }
    reject_secrets_or_testnet(text)?;
    let body = validate_checksum(text)?;

    if let Some(inner) = body.strip_prefix("wpkh(").and_then(|s| s.strip_suffix(')')) {
        let key = parse_key(inner, &[84, 0, 0], 3, 0)?;
        let canonical_body = format!("wpkh([{}/84h/0h/0h]{}/<0;1>/*)", key.fingerprint, key.xpub);
        let checksum = descriptor_checksum(&canonical_body).ok_or(WalletRefusal::Malformed)?;
        return Ok(format!("{canonical_body}#{checksum}"));
    }

    if body.starts_with("wpkh") {
        return Err(WalletRefusal::Malformed);
    }

    let Some(inner) = body
        .strip_prefix("wsh(sortedmulti(2,")
        .and_then(|s| s.strip_suffix("))"))
    else {
        if body.contains("/0/*") || body.contains("/1/*") {
            return Err(WalletRefusal::Incomplete);
        }
        if body.starts_with("wsh(sortedmulti") {
            return Err(WalletRefusal::Malformed);
        }
        return Err(WalletRefusal::Unsupported);
    };
    let expressions: Vec<&str> = inner.split(',').collect();
    let [first, second, third] = expressions.as_slice() else {
        return Err(WalletRefusal::Malformed);
    };
    let mut keys = Vec::with_capacity(3);
    for expression in [*first, *second, *third] {
        keys.push(parse_key(expression, &[48, 0, 0, 2], 4, 2)?);
    }
    let a = keys.first().ok_or(WalletRefusal::Malformed)?;
    let b = keys.get(1).ok_or(WalletRefusal::Malformed)?;
    let c = keys.get(2).ok_or(WalletRefusal::Malformed)?;
    if a.fingerprint == b.fingerprint
        || a.fingerprint == c.fingerprint
        || b.fingerprint == c.fingerprint
    {
        return Err(WalletRefusal::Unsupported);
    }
    if a.xpub == b.xpub || a.xpub == c.xpub || b.xpub == c.xpub {
        return Err(WalletRefusal::Unsupported);
    }
    let canonical_body = format!(
        "wsh(sortedmulti(2,[{}/48h/0h/0h/2h]{}/<0;1>/*,[{}/48h/0h/0h/2h]{}/<0;1>/*,[{}/48h/0h/0h/2h]{}/<0;1>/*))",
        a.fingerprint, a.xpub, b.fingerprint, b.xpub, c.fingerprint, c.xpub
    );
    let checksum = descriptor_checksum(&canonical_body).ok_or(WalletRefusal::Malformed)?;
    Ok(format!("{canonical_body}#{checksum}"))
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_descriptor, descriptor_checksum};
    use crate::base58::encode_check;

    #[test]
    fn bip380_checksum_known_vector() {
        assert_eq!(
            descriptor_checksum("raw(deadbeef)"),
            Some("89f8spxm".to_owned())
        );
    }

    fn account_xpub(id: u8) -> String {
        let mut payload = vec![0x04, 0x88, 0xb2, 0x1e, 4, id, 0, 0, 0];
        payload.extend_from_slice(&[0x80, 0, 0, 2]);
        payload.extend_from_slice(&[id; 32]);
        payload.extend_from_slice(&[
            0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce,
            0x87, 0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81,
            0x5b, 0x16, 0xf8, 0x17, 0x98,
        ]);
        encode_check(&payload).expect("78-byte extended key")
    }

    #[test]
    fn accepts_and_canonicalizes_a_valid_two_of_three_descriptor() {
        let xpubs = [account_xpub(1), account_xpub(2), account_xpub(3)];
        let body = format!(
            "wsh(sortedmulti(2,[aaaaaaaa/48H/0H/0H/2H]{}/<0;1>/*,[bbbbbbbb/48H/0H/0H/2H]{}/<0;1>/*,[cccccccc/48H/0H/0H/2H]{}/<0;1>/*))",
            xpubs[0], xpubs[1], xpubs[2]
        );
        let canonical = canonicalize_descriptor(&body).expect("supported 2-of-3 descriptor");
        assert!(canonical.starts_with("wsh(sortedmulti(2,[aaaaaaaa/48h/0h/0h/2h]"));
        assert!(canonical.contains("[bbbbbbbb/48h/0h/0h/2h]"));
        assert!(canonical.contains("[cccccccc/48h/0h/0h/2h]"));
        let (canonical_body, checksum) = canonical.split_once('#').expect("checksum");
        assert!(canonical_body.ends_with("))"));
        assert_eq!(
            descriptor_checksum(canonical_body).as_deref(),
            Some(checksum)
        );
    }
}
