//! WO-50: the fixed vectors the plugin's tests read. A test here writes
//! them (with `OPENBMT_BLESS_WEB_ORDER=1`) or checks the committed file is
//! the one the code makes. The plugin checks every signature and HMAC in it
//! and that each tampered form is refused.

use std::path::PathBuf;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::{Address, KnownHrp, WitnessProgram, WitnessVersion};

use super::{
    hex, pairing_id_of, Notice, NoticeState, PairingCode, Range, SignedRequest, WebOrderKey,
    NONCE_BYTES, ROUTE_ORDERS,
};

/// The folder the vectors are in.
pub(super) fn folder() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it when it runs tests"))
        .join("../../integrations/woocommerce/openbmt-pay/tests_standalone/vectors")
}

pub(super) fn seed(first: u8) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = first.wrapping_add(u8::try_from(i).unwrap_or(0));
    }
    out
}

/// A fixed list of addresses: version 0 with 20 and 32 bytes, version 1
/// with 32 bytes, so both bech32 and bech32m are in it.
pub(super) fn addresses(count: u32) -> Vec<String> {
    (0..count)
        .map(|i| {
            let digest = sha256::Hash::hash(&i.to_be_bytes()).to_byte_array();
            let (version, program): (WitnessVersion, &[u8]) = match i % 3 {
                0 => (WitnessVersion::V0, digest.get(..20).unwrap_or(&[])),
                1 => (WitnessVersion::V0, &digest),
                _ => (WitnessVersion::V1, &digest),
            };
            let program = WitnessProgram::new(version, program).expect("a program");
            Address::from_witness_program(program, KnownHrp::Mainnet).to_string()
        })
        .collect()
}

pub(super) fn txid(n: u8) -> [u8; 32] {
    sha256::Hash::hash(&[b't', b'x', n]).to_byte_array()
}

/// A JSON string of printable ASCII and `\u` escapes.
fn q(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn flip(bytes: &[u8], at: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if let Some(b) = out.get_mut(at) {
        *b ^= 1;
    }
    out
}

fn named(list: &[(&str, String)]) -> String {
    let items: Vec<String> = list
        .iter()
        .map(|(name, value)| format!("{{\"name\":{},\"frame\":{}}}", q(name), q(value)))
        .collect();
    format!("[{}]", items.join(",\n    "))
}

/// The tampered forms of a signed frame: one flipped byte in each part,
/// a byte added, a byte taken away.
fn tampered_frames(frame: &[u8], parts: &[(&str, usize)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = parts
        .iter()
        .map(|(name, at)| (format!("flip_{name}"), hex(&flip(frame, *at))))
        .collect();
    out.push(("flip_signature".into(), hex(&flip(frame, frame.len() - 1))));
    let mut longer = frame.to_vec();
    longer.push(0);
    out.push(("trailing_byte".into(), hex(&longer)));
    out.push((
        "truncated".into(),
        hex(frame.get(..frame.len() - 1).unwrap_or(&[])),
    ));
    out.push(("empty".into(), String::new()));
    out
}

pub(super) fn key() -> WebOrderKey {
    WebOrderKey::from_seed(&seed(1))
}

pub(super) fn secret() -> [u8; 32] {
    seed(0x40)
}

pub(super) const SHOP_NAME: &str = "Caf\u{e9} Luna & Sons";
pub(super) const RANGE_START: u32 = 7;
pub(super) const NOTICE_ORDER: u64 = 1042;

pub(super) fn range() -> Range {
    Range::new(RANGE_START, addresses(5)).expect("a range")
}

pub(super) fn notice() -> Notice {
    Notice {
        order: NOTICE_ORDER,
        address: addresses(5).get(2).cloned().unwrap_or_default(),
        state: NoticeState::Overpaid,
        counted_sats: 123_456,
        depth: 3,
        txids: vec![txid(1), txid(2)],
    }
}

pub(super) fn request_inputs() -> (&'static str, &'static str, u64, [u8; NONCE_BYTES], Vec<u8>) {
    (
        "POST",
        ROUTE_ORDERS,
        1_800_000_000,
        [0xa5; NONCE_BYTES],
        super::orders_request_body(41).into_bytes(),
    )
}

/// The text of the vector file.
pub(super) fn vector_file() -> String {
    let key = key();
    let public = key.public();
    let other = WebOrderKey::from_seed(&seed(0x80));
    let code = PairingCode::new(public, secret(), SHOP_NAME).expect("a code");

    let range = range();
    let id = pairing_id_of(&secret());
    let range_frame = range.sign(&key, &id);
    let first_address = 4 + 1 + 8 + 4 + 1 + 1;
    let mut range_bad = tampered_frames(
        &range_frame,
        &[
            ("magic", 0),
            ("version", 4),
            ("pairing_id", 5),
            ("start", 16),
            ("count", 17),
            ("address", first_address + 3),
        ],
    );
    range_bad.push(("other_key".into(), hex(&range.sign(&other, &id))));
    let other_id = pairing_id_of(&seed(0x90));
    range_bad.push(("other_pairing".into(), hex(&range.sign(&key, &other_id))));
    let range_bad: Vec<(&str, String)> = range_bad
        .iter()
        .map(|(n, v)| (n.as_str(), v.clone()))
        .collect();

    let notice = notice();
    let notice_frame = notice.sign(&key).expect("a notice");
    let address_at = 4 + 1 + 8 + 1;
    let state_at = address_at + notice.address.len();
    let mut notice_bad = tampered_frames(
        &notice_frame,
        &[
            ("magic", 0),
            ("version", 4),
            ("order", 12),
            ("address", address_at + 4),
            ("state", state_at),
            ("amount", state_at + 9),
            ("depth", state_at + 12),
            ("txid", state_at + 15),
        ],
    );
    notice_bad.push((
        "other_key".into(),
        hex(&notice.sign(&other).expect("a notice")),
    ));
    let notice_bad: Vec<(&str, String)> = notice_bad
        .iter()
        .map(|(n, v)| (n.as_str(), v.clone()))
        .collect();

    let (method, route, time, nonce, body) = request_inputs();
    let secret = secret();
    let made = SignedRequest::make(&secret, method, route, time, nonce, &body).expect("a request");
    let one = |m: &str, r: &str, t: u64, n: &[u8], b: &[u8], mac: &str, s: &[u8]| {
        format!(
            "{{\"secret\":{},\"method\":{},\"route\":{},\"time\":{t},\"nonce\":{},\"body\":{},\"mac\":{}}}",
            q(&hex(s)),
            q(m),
            q(r),
            q(&hex(n)),
            q(&hex(b)),
            q(mac)
        )
    };
    let mac = made.mac_text();
    let mut flipped_mac = made.mac;
    if let Some(b) = flipped_mac.first_mut() {
        *b ^= 1;
    }
    let mut other_secret = secret;
    if let Some(b) = other_secret.first_mut() {
        *b ^= 1;
    }
    let mut other_body = body.clone();
    other_body.push(b' ');
    let mut other_nonce = nonce;
    other_nonce[0] ^= 1;
    let variants = [
        (
            "method",
            one("GET", route, time, &nonce, &body, &mac, &secret),
        ),
        (
            "route",
            one(
                "POST",
                "/openbmt/v1/notice",
                time,
                &nonce,
                &body,
                &mac,
                &secret,
            ),
        ),
        (
            "time",
            one(method, route, time + 1, &nonce, &body, &mac, &secret),
        ),
        (
            "nonce",
            one(method, route, time, &other_nonce, &body, &mac, &secret),
        ),
        (
            "body",
            one(method, route, time, &nonce, &other_body, &mac, &secret),
        ),
        (
            "mac",
            one(
                method,
                route,
                time,
                &nonce,
                &body,
                &hex(&flipped_mac),
                &secret,
            ),
        ),
        (
            "secret",
            one(method, route, time, &nonce, &body, &mac, &other_secret),
        ),
    ];
    let variants: Vec<String> = variants
        .iter()
        .map(|(name, v)| format!("{{\"name\":{},\"request\":{v}}}", q(name)))
        .collect();

    let addresses: Vec<String> = range.addresses.iter().map(|a| q(a)).collect();
    let txids: Vec<String> = notice.txids.iter().map(|t| q(&hex(t))).collect();
    format!(
        "{{\n\"version\":1,\n\
         \"key\":{{\"seed\":{},\"public\":{},\"fingerprint\":{}}},\n\
         \"pairing\":{{\"secret\":{},\"id\":{},\"shop_name\":{},\"bytes\":{},\"text\":{}}},\n\
         \"range\":{{\"start\":{},\"addresses\":[{}],\n \"frame\":{},\n \"tampered\":{}}},\n\
         \"notice\":{{\"order\":{},\"address\":{},\"state\":{},\"counted_sats\":{},\"depth\":{},\"txids\":[{}],\n \"frame\":{},\n \"tampered\":{}}},\n\
         \"request\":{},\n\
         \"request_tampered\":[{}]\n}}\n",
        q(&hex(&key.seed())),
        q(&hex(&public)),
        q(&key.fingerprint()),
        q(&hex(&secret)),
        q(&hex(&id)),
        q(SHOP_NAME),
        q(&hex(&code.to_bytes())),
        q(&code.to_text()),
        range.start,
        addresses.join(",\n  "),
        q(&hex(&range_frame)),
        named(&range_bad),
        notice.order,
        q(&notice.address),
        q("overpaid"),
        notice.counted_sats,
        notice.depth,
        txids.join(","),
        q(&hex(&notice_frame)),
        named(&notice_bad),
        one(method, route, time, &nonce, &body, &mac, &secret),
        variants.join(",\n  "),
    )
}
