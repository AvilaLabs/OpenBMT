//! The three stored forms: the desk (`OBMG`), the sale (`OBML`) and the
//! record bodies. Each one round-trips, refuses every cut-short input, a
//! byte too many, another magic and another version, and never decodes a
//! flipped bit into something that would not encode back to the same bytes.

use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::{Amount, Desk, Event, OverrideEntry, Role, Sale};

/// Same checks as the core's helper, kept here because that one is private
/// to core's tests.
fn assert_strict<T>(
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

fn a_desk() -> Desk {
    let mut d = Desk::new();
    d.push_address_time(T0);
    d.push_address_time(T0 + 5);
    d.push_override(OverrideEntry {
        role: Role::Cashier,
        name: "ann".to_string(),
        at: T0 + 6,
        sats: 5_000,
    });
    d.push_override(OverrideEntry {
        role: Role::Manager,
        name: "max".to_string(),
        at: T0 + 9,
        sats: 70_000,
    });
    d.set_alarm(true);
    d.set_last_command_seq(7);
    d
}

fn a_sale() -> Sale {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 6_000);
    w.see(&t, 2);
    w.judge(o.index);
    w.override_sale(o.index, Role::Cashier, "ann").unwrap();
    w.c.sale(o.index).unwrap().clone()
}

fn every_event() -> Vec<Event> {
    vec![
        Event::Opened {
            index: 4,
            at: T0,
            amount: Amount(500),
            currency: usd(),
            needs_sats: 5_000,
            minor_per_btc: 10_000_000,
        },
        Event::Expired {
            index: 4,
            at: T0 + 900,
        },
        Event::Cancelled {
            index: 4,
            at: T0 + 10,
        },
        Event::PaidAtOnce {
            index: 4,
            at: T0 + 20,
            at_risk_sats: 5_000,
            label: InstantLabel::LightCheck,
            mode: CheckMode::Light,
        },
        Event::Confirmed {
            index: 4,
            at: T0 + 30,
            received_sats: 5_000,
            depth: 1,
        },
        Event::Final {
            index: 4,
            at: T0 + 40,
            received_sats: 5_000,
            depth: 6,
        },
        Event::LatePayment {
            index: 4,
            at: T0 + 50,
            txid: [7; 32],
            sats: 2_000,
        },
        Event::SurplusOwed {
            index: 4,
            at: T0 + 60,
            sats: 1_000,
        },
        Event::Dust {
            index: 4,
            at: T0 + 70,
            sats: 600,
        },
        Event::Override {
            index: 4,
            at: T0 + 80,
            role: Role::Manager,
            name: "max".to_string(),
            sats: 5_000,
        },
        Event::Replaced {
            index: 4,
            at: T0 + 90,
        },
        Event::Reopened {
            index: 4,
            at: T0 + 100,
        },
    ]
}

#[test]
fn the_desk_is_strict() {
    assert_strict(&a_desk().to_bytes(), Desk::from_bytes, Desk::to_bytes);
    assert_strict(&Desk::new().to_bytes(), Desk::from_bytes, Desk::to_bytes);
}

#[test]
fn the_desk_round_trips_and_refuses_another_magic_or_version() {
    let d = a_desk();
    let good = d.to_bytes();
    assert_eq!(&good[..4], b"OBMG");
    assert_eq!(Desk::from_bytes(&good).unwrap(), d);
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(Desk::from_bytes(&bad).err(), Some(DecodeError::BadMagic));
    let mut bad = good.clone();
    bad[4] = 9;
    assert_eq!(
        Desk::from_bytes(&bad).err(),
        Some(DecodeError::UnsupportedVersion(9))
    );
    assert_eq!(Desk::from_bytes(&[]).err(), Some(DecodeError::Truncated));
}

#[test]
fn a_desk_without_a_heartbeat_is_still_written_as_version_one() {
    let bytes = a_desk().to_bytes();
    assert_eq!(bytes[4], 1);
    assert_eq!(Desk::from_bytes(&bytes).unwrap().last_heartbeat_seq(), 0);
}

#[test]
fn a_desk_with_a_heartbeat_number_is_version_two_and_strict() {
    let mut d = a_desk();
    d.set_last_heartbeat_seq(41);
    let bytes = d.to_bytes();
    assert_eq!(&bytes[..5], b"OBMG\x02");
    // the number follows the alarm byte, and the rest is the version 1 form
    assert_eq!(&bytes[14..22], &41u64.to_le_bytes());
    let old = a_desk().to_bytes();
    assert_eq!(&bytes[22..], &old[14..]);
    assert_eq!(Desk::from_bytes(&bytes).unwrap(), d);
    assert_strict(&bytes, Desk::from_bytes, Desk::to_bytes);
    // version 2 with a zero number is not what the counter writes
    let mut bad = bytes.clone();
    bad[14..22].fill(0);
    assert!(matches!(
        Desk::from_bytes(&bad),
        Err(DecodeError::NonCanonical(_))
    ));
    // version 1 with the extra eight bytes does not read back
    let mut bad = bytes;
    bad[4] = 1;
    assert!(Desk::from_bytes(&bad).is_err());
}

#[test]
fn the_desk_refuses_what_the_counter_never_writes() {
    let good = a_desk().to_bytes();
    // Header: magic 4, version 1, seq 8, alarm 1, count 4, then two times.
    let times_at = 4 + 1 + 8 + 1 + 4;
    // Times out of order.
    let mut bad = good.clone();
    bad[times_at..times_at + 8].copy_from_slice(&(T0 + 50).to_le_bytes());
    assert!(matches!(
        Desk::from_bytes(&bad),
        Err(DecodeError::NonCanonical(_))
    ));
    // An alarm byte that is not a flag.
    let mut bad = good.clone();
    bad[13] = 2;
    assert!(Desk::from_bytes(&bad).is_err());
    // An unknown role.
    let role_at = times_at + 16 + 4;
    let mut bad = good.clone();
    bad[role_at] = 9;
    assert!(matches!(
        Desk::from_bytes(&bad),
        Err(DecodeError::NonCanonical(_))
    ));
    // An empty name.
    let mut bad = good.clone();
    bad[role_at + 1] = 0;
    assert!(Desk::from_bytes(&bad).is_err());
    // Too many address times.
    let mut bad = good.clone();
    bad[14..18].copy_from_slice(&1_000u32.to_le_bytes());
    assert!(Desk::from_bytes(&bad).is_err());
}

#[test]
fn the_sale_is_strict() {
    assert_strict(&a_sale().to_bytes(), Sale::from_bytes, |s| s.to_bytes());
}

#[test]
fn a_plain_open_sale_is_strict_too() {
    let mut w = World::new();
    let o = w.open_five();
    let sale = w.c.sale(o.index).unwrap().clone();
    assert_strict(&sale.to_bytes(), Sale::from_bytes, |s| s.to_bytes());
}

#[test]
fn the_sale_round_trips_and_refuses_another_magic_or_version() {
    let s = a_sale();
    let good = s.to_bytes();
    assert_eq!(&good[..4], b"OBML");
    assert_eq!(Sale::from_bytes(&good).unwrap().to_bytes(), good);
    let mut bad = good.clone();
    bad[3] = b'X';
    assert_eq!(Sale::from_bytes(&bad).err(), Some(DecodeError::BadMagic));
    let mut bad = good.clone();
    bad[4] = 2;
    assert_eq!(
        Sale::from_bytes(&bad).err(),
        Some(DecodeError::UnsupportedVersion(2))
    );
    // A desk is not a sale.
    assert_eq!(
        Sale::from_bytes(&a_desk().to_bytes()).err(),
        Some(DecodeError::BadMagic)
    );
}

#[test]
fn every_event_is_strict() {
    for e in every_event() {
        let bytes = e.encode();
        assert_eq!(Event::decode(&bytes).unwrap(), e);
        assert_strict(&bytes, Event::decode, Event::encode);
    }
    assert_eq!(every_event().len(), 12);
}

#[test]
fn an_event_with_an_unknown_tag_or_field_is_refused() {
    let mut bad = Event::Expired { index: 1, at: 2 }.encode();
    bad[0] = 0;
    assert!(matches!(
        Event::decode(&bad),
        Err(DecodeError::NonCanonical(_))
    ));
    bad[0] = 99;
    assert!(Event::decode(&bad).is_err());
    assert_eq!(Event::decode(&[]).err(), Some(DecodeError::Truncated));

    let paid = Event::PaidAtOnce {
        index: 1,
        at: 2,
        at_risk_sats: 3,
        label: InstantLabel::YourNode,
        mode: CheckMode::Light,
    }
    .encode();
    let n = paid.len();
    for at in [n - 2, n - 1] {
        let mut bad = paid.clone();
        bad[at] = 0;
        assert!(Event::decode(&bad).is_err(), "byte {at}");
        bad[at] = 200;
        assert!(Event::decode(&bad).is_err(), "byte {at}");
    }

    let over = Event::Override {
        index: 1,
        at: 2,
        role: Role::Cashier,
        name: "ann".to_string(),
        sats: 3,
    }
    .encode();
    let mut bad = over.clone();
    bad[13] = 7; // role
    assert!(Event::decode(&bad).is_err());
    let mut bad = over.clone();
    bad[14] = 0; // name length
    assert!(Event::decode(&bad).is_err());
    let mut bad = over.clone();
    bad[15] = 0xff; // not text
    assert!(Event::decode(&bad).is_err());

    let mut opened = every_event()[0].encode();
    opened[21..24].copy_from_slice(b"usd");
    assert!(Event::decode(&opened).is_err(), "lower case currency");
}

#[test]
fn an_event_keeps_its_index_and_record_kind() {
    use openbmt_core::records::RecordKind;
    let kinds: Vec<RecordKind> = every_event().iter().map(Event::kind).collect();
    assert_eq!(kinds[0], RecordKind::Sale);
    assert_eq!(kinds[3], RecordKind::Payment);
    assert_eq!(kinds[9], RecordKind::Override);
    assert_eq!(kinds[10], RecordKind::Conflict);
    assert!(every_event().iter().all(|e| e.index() == 4));
}
