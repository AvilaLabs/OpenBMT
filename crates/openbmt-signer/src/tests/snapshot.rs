//! The `OBMK` snapshot: round trips, strictness and the invariants that are
//! checked again on reading.

use bitcoin::hashes::Hash;
use bitcoin::{Network, OutPoint, ScriptBuf, Txid};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::state::AllowlistEntry;
use crate::{
    Ceilings, FeeCeilings, LogEntry, SignerState, MAX_ALLOWLIST, MAX_MONEY_SATS, WEEK_SECS,
};

/// Strictness checks for one wire type, as `openbmt-core` runs them: the
/// message re-encodes to itself, every proper prefix is refused, one extra
/// byte is refused, and flipping any single bit never panics and either
/// fails or decodes to a value that encodes back to the mutated bytes.
fn assert_strict_decoding<T>(
    valid: &[u8],
    decode: impl Fn(&[u8]) -> Result<T, DecodeError>,
    encode: impl Fn(&T) -> Vec<u8>,
) {
    let value = decode(valid).expect("valid message decodes");
    assert_eq!(encode(&value), valid, "canonical re-encoding");
    for n in 0..valid.len() {
        assert!(decode(&valid[..n]).is_err(), "prefix of {n} bytes decoded");
    }
    let mut longer = valid.to_vec();
    longer.push(0);
    assert_eq!(decode(&longer).err(), Some(DecodeError::TrailingBytes));
    let mut m = valid.to_vec();
    for i in 0..m.len() {
        for bit in 0..8 {
            m[i] ^= 1 << bit;
            if let Ok(v) = decode(&m) {
                assert_eq!(encode(&v), m, "byte {i} bit {bit} decoded non-canonically");
            }
            m[i] ^= 1 << bit;
        }
    }
}

fn decode(bytes: &[u8]) -> Result<SignerState, DecodeError> {
    SignerState::from_bytes(bytes, NET, &kit().xpubs[OWN])
}

/// A state with history: three destinations, ceilings of the owner's own,
/// a fee bump, a two-input payment, an allowlisted payment, marks.
fn busy() -> SignerState {
    let mut s = state();
    for (n, label) in [(30u8, "Exchange"), (10, "Bank"), (20, "Caf\u{e9}")] {
        s.enroll_destination(external(n), label, NOW, ok).unwrap();
    }
    s.set_ceilings(
        Ceilings {
            per_tx: 150_000,
            per_day: 400_000,
            per_week: 2_000_000,
        },
        Ceilings {
            per_tx: 4_000_000,
            per_day: 8_000_000,
            per_week: 20_000_000,
        },
        ok,
    )
    .unwrap();
    s.set_fee_ceilings(
        FeeCeilings {
            sat_per_vb: 120,
            absolute_sats: 150_000,
        },
        ok,
    )
    .unwrap();
    let c = coin(100_000);
    let slow = build(std::slice::from_ref(&c), &[pay(60_000), change(0, 39_000)]);
    let fast = build(&[c], &[pay(60_000), change(1, 30_000)]);
    sign_ok(&mut s, &slow, NOW + 10);
    sign_ok(&mut s, &fast, NOW + 70);
    sign_ok(
        &mut s,
        &build(&[coin(40_000), coin(40_000)], &[pay(70_000)]),
        NOW + 80,
    );
    let later = NOW + 86_400 + 100;
    sign_ok(&mut s, &simple_to(external(30), 1_000_000, 1_000), later);
    s.raise_change_mark(5, ok).unwrap();
    s.receive_mark = 7;
    s
}

#[test]
fn an_empty_state_round_trips() {
    let s = state();
    let bytes = s.to_bytes();
    assert_eq!(&bytes[..4], b"OBMK");
    assert_eq!(bytes[4], 1);
    let back = decode(&bytes).unwrap();
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(back.own_slot(), 2);
    assert_eq!(back.key3_slot(), Some(3));
    assert_eq!(back.wallet(), s.wallet());
}

#[test]
fn a_state_without_key_three_round_trips() {
    let k = kit();
    let s = SignerState::register(&k.description, NET, &k.xpubs[0], None).unwrap();
    let bytes = s.to_bytes();
    let back = SignerState::from_bytes(&bytes, NET, &k.xpubs[0]).unwrap();
    assert_eq!(back.key3_slot(), None);
    assert_eq!(back.own_slot(), 1);
    assert_eq!(back.to_bytes(), bytes);
}

#[test]
fn a_busy_state_round_trips_with_everything_in_it() {
    let s = busy();
    assert_eq!(s.allowlist().count(), 3);
    assert_eq!(s.log().count(), 3);
    assert!(s.log().any(|e| e.inputs().len() == 2));
    let bytes = s.to_bytes();
    let back = decode(&bytes).unwrap();
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(back.receive_mark(), 7);
    assert_eq!(back.change_mark(), 5);
    assert_eq!(back.latest_time_seen(), s.latest_time_seen());
    assert_eq!(back.ceilings_other(), s.ceilings_other());
    assert_eq!(back.ceilings_allowlisted(), s.ceilings_allowlisted());
    assert_eq!(back.fee_ceilings(), s.fee_ceilings());
    assert_eq!(
        back.allowlist().cloned().collect::<Vec<_>>(),
        s.allowlist().cloned().collect::<Vec<_>>()
    );
    assert_eq!(
        back.log().cloned().collect::<Vec<_>>(),
        s.log().cloned().collect::<Vec<_>>()
    );
    // A restored state behaves like the original.
    let p = simple(10_000, 500);
    let (mut a, mut b) = (s, back);
    assert_eq!(
        sign_ok(&mut a, &p, NOW + 200_000).psbt,
        sign_ok(&mut b, &p, NOW + 200_000).psbt
    );
    assert_eq!(a.to_bytes(), b.to_bytes());
}

#[test]
fn decoding_an_empty_state_is_strict() {
    assert_strict_decoding(&state().to_bytes(), decode, |s| s.to_bytes());
}

#[test]
fn decoding_a_busy_state_is_strict() {
    assert_strict_decoding(&busy().to_bytes(), decode, |s| s.to_bytes());
}

#[test]
fn a_snapshot_for_another_network_is_refused() {
    let bytes = state().to_bytes();
    for net in [
        Network::Bitcoin,
        Network::Testnet,
        Network::Signet,
        Network::Testnet4,
    ] {
        assert!(
            matches!(
                SignerState::from_bytes(&bytes, net, &kit().xpubs[OWN]),
                Err(DecodeError::NonCanonical(_))
            ),
            "{net}"
        );
    }
    // The network byte of another network in the bytes themselves.
    for byte in [0u8, 1, 2, 4, 255] {
        let mut m = bytes.clone();
        m[5] = byte;
        assert!(decode(&m).is_err(), "{byte}");
    }
}

#[test]
fn a_snapshot_for_another_key_is_refused() {
    let k = kit();
    let bytes = state().to_bytes();
    for other in [&k.xpubs[0], &k.xpubs[2]] {
        assert!(matches!(
            SignerState::from_bytes(&bytes, NET, other),
            Err(DecodeError::NonCanonical(_))
        ));
    }
    // The own slot cannot be edited to name another key's slot.
    let mut m = bytes.clone();
    m[6] = 1;
    assert!(decode(&m).is_err());
}

#[test]
fn the_magic_and_the_version_are_checked() {
    let bytes = state().to_bytes();
    let mut m = bytes.clone();
    m[3] = b'X';
    assert_eq!(decode(&m).err(), Some(DecodeError::BadMagic));
    for v in [0u8, 2, 255] {
        let mut m = bytes.clone();
        m[4] = v;
        assert_eq!(decode(&m).err(), Some(DecodeError::UnsupportedVersion(v)));
    }
    // The message of another type is not read as a signer state.
    let mut m = bytes.clone();
    m[..4].copy_from_slice(b"OBMH");
    assert_eq!(decode(&m).err(), Some(DecodeError::BadMagic));
}

/// Where each field of a snapshot starts.
struct Layout {
    own: usize,
    flag: usize,
    text_len: usize,
    text: usize,
    receive: usize,
    change: usize,
    latest: usize,
    other: usize,
    allowlisted: usize,
    fee: usize,
}

fn layout(b: &[u8]) -> Layout {
    let own = 6;
    let flag = 7;
    let mut o = flag + if b[flag] == 1 { 2 } else { 1 };
    let text_len = o;
    let n = usize::from(u16::from_le_bytes([b[o], b[o + 1]]));
    o += 2;
    let text = o;
    o += n;
    let receive = o;
    Layout {
        own,
        flag,
        text_len,
        text,
        receive,
        change: receive + 4,
        latest: receive + 8,
        other: receive + 16,
        allowlisted: receive + 40,
        fee: receive + 64,
    }
}

fn put(b: &mut [u8], at: usize, bytes: &[u8]) {
    b[at..at + bytes.len()].copy_from_slice(bytes);
}

fn non_canonical(bytes: &[u8]) -> bool {
    matches!(decode(bytes), Err(DecodeError::NonCanonical(_)))
}

#[test]
fn the_header_fields_are_checked_again() {
    let bytes = busy().to_bytes();
    let l = layout(&bytes);
    // Own slot: 0, 4 and a slot that is not this key.
    for slot in [0u8, 4, 1, 3] {
        let mut m = bytes.clone();
        m[l.own] = slot;
        assert!(non_canonical(&m), "own slot {slot}");
    }
    // Key 3 slot: not 1..=3, or the same as the own slot.
    for slot in [0u8, 4, 2] {
        let mut m = bytes.clone();
        m[l.flag + 1] = slot;
        assert!(non_canonical(&m), "key 3 slot {slot}");
    }
    let mut m = bytes.clone();
    m[l.flag] = 2;
    assert!(non_canonical(&m), "flag");
    // Marks.
    let high = ((1u32 << 31) + 1).to_le_bytes();
    let edge = (1u32 << 31).to_le_bytes();
    for at in [l.receive, l.change] {
        let mut m = bytes.clone();
        put(&mut m, at, &high);
        assert!(non_canonical(&m), "mark at {at}");
        put(&mut m, at, &edge);
        assert!(decode(&m).is_ok(), "mark of exactly 2^31");
    }
}

#[test]
fn the_ceilings_and_fee_ceilings_are_checked_again() {
    let bytes = busy().to_bytes();
    let l = layout(&bytes);
    let over = (MAX_MONEY_SATS + 1).to_le_bytes();
    for at in [
        l.other,
        l.other + 8,
        l.other + 16,
        l.allowlisted,
        l.allowlisted + 8,
        l.allowlisted + 16,
    ] {
        let mut m = bytes.clone();
        put(&mut m, at, &over);
        assert!(non_canonical(&m), "ceiling at {at}");
        put(&mut m, at, &MAX_MONEY_SATS.to_le_bytes());
        assert!(decode(&m).is_ok());
    }
    for (at, v) in [
        (l.fee, 0u64),
        (l.fee, 1_001),
        (l.fee + 8, 0),
        (l.fee + 8, 2_000_001),
    ] {
        let mut m = bytes.clone();
        put(&mut m, at, &v.to_le_bytes());
        assert!(non_canonical(&m), "fee ceiling {v} at {at}");
    }
    for (at, v) in [
        (l.fee, 1_000u64),
        (l.fee, 1),
        (l.fee + 8, 2_000_000),
        (l.fee + 8, 1),
    ] {
        let mut m = bytes.clone();
        put(&mut m, at, &v.to_le_bytes());
        assert!(decode(&m).is_ok(), "fee ceiling {v} at {at}");
    }
}

#[test]
fn the_wallet_text_must_be_the_pinned_form_of_a_valid_wallet() {
    let bytes = state().to_bytes();
    let l = layout(&bytes);
    // A changed character fails the checksum.
    let mut m = bytes.clone();
    m[l.text + 40] ^= 1;
    assert!(non_canonical(&m));
    // The apostrophe spelling, with a correct checksum, describes the same
    // wallet but is not the pinned text.
    let text = std::str::from_utf8(
        &bytes[l.text
            ..l.text
                + usize::from(u16::from_le_bytes([
                    bytes[l.text_len],
                    bytes[l.text_len + 1],
                ]))],
    )
    .unwrap();
    let body = text.rsplit_once('#').unwrap().0;
    let mut apostrophes = body.to_string();
    for marker in ["/48h", "/1h", "/0h", "/2h"] {
        apostrophes = apostrophes.replace(marker, &marker.replace('h', "'"));
    }
    let respelled = format!(
        "{apostrophes}#{}",
        openbmt_core::wallet::descriptor_checksum(&apostrophes).unwrap()
    );
    assert!(openbmt_core::wallet::WalletDescription::parse(&respelled, NET).is_ok());
    let mut m = bytes[..l.text_len].to_vec();
    m.extend_from_slice(&(respelled.len() as u16).to_le_bytes());
    m.extend_from_slice(respelled.as_bytes());
    m.extend_from_slice(&bytes[l.receive..]);
    assert!(non_canonical(&m));
    // Not UTF-8.
    let mut m = bytes.clone();
    m[l.text] = 0xff;
    assert!(non_canonical(&m));
}

fn entry(script: ScriptBuf, label: &str, enrolled_at: u64) -> AllowlistEntry {
    AllowlistEntry {
        script,
        label: label.to_string(),
        enrolled_at,
    }
}

fn log_entry(n: u32) -> ([u8; 32], LogEntry) {
    let mut id = [0u8; 32];
    id[..4].copy_from_slice(&n.to_be_bytes());
    (
        id,
        LogEntry {
            txid: Txid::from_byte_array(id),
            time: 1,
            other_sats: 1,
            allowlisted_sats: 1,
            fee_sats: 1,
            inputs: vec![OutPoint {
                txid: Txid::from_byte_array([n as u8; 32]),
                vout: n,
            }],
        },
    )
}

/// Mutate a busy state in memory, write it out and read it back: the
/// reader must refuse what the live state could never hold.
#[track_caller]
fn refused_after(why: &str, mutate: impl FnOnce(&mut SignerState)) {
    let mut s = busy();
    mutate(&mut s);
    let r = decode(&s.to_bytes());
    assert!(r.is_err(), "{why}: decoded");
}

#[test]
fn the_allowlist_is_checked_again() {
    refused_after("empty label", |s| s.allowlist[0].label = String::new());
    refused_after("label of 65 characters", |s| {
        s.allowlist[0].label = "x".repeat(65)
    });
    refused_after("control character in a label", |s| {
        s.allowlist[0].label = "a\u{7}b".into()
    });
    refused_after("script that is not a payment script", |s| {
        let last = s.allowlist.len() - 1;
        let mut bytes = s.allowlist[last].script.to_bytes();
        bytes[0] = 0x51;
        s.allowlist[last].script = ScriptBuf::from_bytes(bytes);
    });
    refused_after("entries out of order", |s| s.allowlist.swap(0, 1));
    refused_after("a script twice", |s| {
        let again = s.allowlist[0].clone();
        s.allowlist[1] = again;
    });
    refused_after("enrolled after the latest time seen", |s| {
        s.allowlist[0].enrolled_at = s.latest_time_seen + 1
    });
    refused_after("65 entries", |s| {
        s.allowlist = (0..=MAX_ALLOWLIST as u8)
            .map(|n| entry(external(n), "x", 1))
            .collect();
        s.allowlist
            .sort_by(|a, b| a.script.as_bytes().cmp(b.script.as_bytes()));
    });
    // Exactly 64 entries are fine.
    let mut s = busy();
    s.allowlist = (0..MAX_ALLOWLIST as u8)
        .map(|n| entry(external(n), "x", 1))
        .collect();
    s.allowlist
        .sort_by(|a, b| a.script.as_bytes().cmp(b.script.as_bytes()));
    assert!(decode(&s.to_bytes()).is_ok());
    // A label that is not UTF-8.
    let bytes = {
        let mut s = busy();
        s.allowlist[0].label = "ab".into();
        s.to_bytes()
    };
    let l = layout(&bytes);
    let first_label = l.fee + 16 + 4 + 1 + 22 + 1;
    let mut m = bytes.clone();
    put(&mut m, first_label, &[0xff, 0xfe]);
    assert!(non_canonical(&m));
}

#[test]
fn the_spend_log_is_checked_again() {
    refused_after("an entry dated after the latest time seen", |s| {
        let k = *s.log.keys().next().unwrap();
        s.log.get_mut(&k).unwrap().time = s.latest_time_seen + 1;
    });
    for field in 0..3 {
        refused_after("an amount above all bitcoin", |s| {
            let k = *s.log.keys().next().unwrap();
            let e = s.log.get_mut(&k).unwrap();
            match field {
                0 => e.other_sats = MAX_MONEY_SATS + 1,
                1 => e.allowlisted_sats = MAX_MONEY_SATS + 1,
                _ => e.fee_sats = MAX_MONEY_SATS + 1,
            }
        });
    }
    refused_after("an entry with no inputs", |s| {
        let k = *s.log.keys().next().unwrap();
        s.log.get_mut(&k).unwrap().inputs.clear();
    });
    refused_after("inputs out of order", |s| {
        let k = *s.log.iter().find(|(_, e)| e.inputs.len() == 2).unwrap().0;
        s.log.get_mut(&k).unwrap().inputs.reverse();
    });
    refused_after("an input twice", |s| {
        let k = *s.log.iter().find(|(_, e)| e.inputs.len() == 2).unwrap().0;
        let e = s.log.get_mut(&k).unwrap();
        e.inputs[1] = e.inputs[0];
    });
    refused_after("1,001 inputs in one entry", |s| {
        let k = *s.log.keys().next().unwrap();
        s.log.get_mut(&k).unwrap().inputs = (0..1_001)
            .map(|v| OutPoint {
                txid: Txid::from_byte_array([1; 32]),
                vout: v,
            })
            .collect();
    });
    refused_after("more than 100,000 inputs in all", |s| {
        s.log.clear();
        for n in 0..101u32 {
            let (id, mut e) = log_entry(n + 1);
            e.inputs = (0..1_000)
                .map(|v| OutPoint {
                    txid: Txid::from_byte_array([n as u8; 32]),
                    vout: v,
                })
                .collect();
            s.log.insert(id, e);
        }
    });
    refused_after("more than 10,000 entries", |s| {
        s.log.clear();
        for n in 0..10_001u32 {
            let (id, e) = log_entry(n + 1);
            s.log.insert(id, e);
        }
    });
    refused_after("totals that overflow 64 bits", |s| {
        s.log.clear();
        for n in 0..5_000u32 {
            let (id, mut e) = log_entry(n + 1);
            e.other_sats = MAX_MONEY_SATS;
            e.fee_sats = MAX_MONEY_SATS;
            s.log.insert(id, e);
        }
    });
    // Limits met exactly are fine.
    let mut s = busy();
    s.log.clear();
    for n in 0..10_000u32 {
        let (id, e) = log_entry(n + 1);
        s.log.insert(id, e);
    }
    assert!(decode(&s.to_bytes()).is_ok());
    let mut s = busy();
    s.log.clear();
    for n in 0..100u32 {
        let (id, mut e) = log_entry(n + 1);
        e.inputs = (0..1_000)
            .map(|v| OutPoint {
                txid: Txid::from_byte_array([n as u8; 32]),
                vout: v,
            })
            .collect();
        s.log.insert(id, e);
    }
    assert!(decode(&s.to_bytes()).is_ok());
}

#[test]
fn the_log_must_be_in_ascending_transaction_order() {
    let bytes = busy().to_bytes();
    let l = layout(&bytes);
    // Find the log: after the allowlist.
    let mut o = l.fee + 16;
    let n = u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    o += 4;
    for _ in 0..n {
        o += 1 + usize::from(bytes[o]);
        o += 1 + usize::from(bytes[o]);
        o += 8;
    }
    let count = u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    assert_eq!(count, 3);
    let first = o + 4;
    // Make the first transaction ID the largest possible.
    let mut m = bytes.clone();
    put(&mut m, first, &[0xff; 32]);
    assert!(non_canonical(&m));
    // Make it equal to the second one: find the second entry.
    let inputs = u32::from_le_bytes(bytes[first + 64..first + 68].try_into().unwrap()) as usize;
    let second = first + 68 + 36 * inputs;
    let mut m = bytes.clone();
    let id2: [u8; 32] = bytes[second..second + 32].try_into().unwrap();
    put(&mut m, first, &id2);
    assert!(non_canonical(&m));
}

#[test]
fn counts_are_checked_before_anything_is_allocated() {
    let bytes = state().to_bytes();
    let l = layout(&bytes);
    let count_at = l.fee + 16;
    let mut m = bytes.clone();
    put(&mut m, count_at, &(MAX_ALLOWLIST as u32 + 1).to_le_bytes());
    assert_eq!(decode(&m).err(), Some(DecodeError::TooLarge));
    let mut m = bytes.clone();
    put(&mut m, count_at, &u32::MAX.to_le_bytes());
    assert_eq!(decode(&m).err(), Some(DecodeError::TooLarge));
    // A count that the bytes cannot hold is refused as cut short.
    let mut m = bytes.clone();
    put(&mut m, count_at, &10u32.to_le_bytes());
    assert_eq!(decode(&m).err(), Some(DecodeError::Truncated));
    let log_count_at = count_at + 4;
    let mut m = bytes.clone();
    put(&mut m, log_count_at, &10_001u32.to_le_bytes());
    assert_eq!(decode(&m).err(), Some(DecodeError::TooLarge));
    let mut m = bytes.clone();
    put(&mut m, log_count_at, &1u32.to_le_bytes());
    assert_eq!(decode(&m).err(), Some(DecodeError::Truncated));
    let _ = (l.latest, WEEK_SECS);
}
