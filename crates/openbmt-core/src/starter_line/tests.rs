use super::*;
use crate::testutil::{assert_strict_decoding, identity};

fn payment(n: u8) -> LinePayment {
    LinePayment {
        txid: [n; 32],
        sats: 1000 * u64::from(n),
        time: 1_700_000_000 + u64::from(n),
        by_owner: n.is_multiple_of(2),
    }
}

fn line(count: u8) -> StarterLine {
    let payments: Vec<LinePayment> = (1..=count).map(payment).collect();
    StarterLine::sign(
        &identity(1),
        7,
        1_700_000_100,
        250_000,
        300_000,
        true,
        &payments,
    )
}

#[test]
fn lk_79_line_is_strict() {
    for count in [0u8, 1, 16] {
        let bytes = line(count).to_bytes();
        assert_eq!(&bytes[..4], b"OBM_");
        assert_eq!(bytes.len(), 5 + 34 + 49 * usize::from(count) + 64);
        assert_strict_decoding(&bytes, StarterLine::from_bytes, StarterLine::to_bytes);
    }
}

#[test]
fn lk_79_line_is_signed_by_the_owner_root_only() {
    let l = line(3);
    assert!(l.verify(&identity(1).device_id()));
    assert!(!l.verify(&identity(2).device_id()));
    for field in 0..5 {
        let mut m = l.clone();
        match field {
            0 => m.number += 1,
            1 => m.time += 1,
            2 => m.held_sats += 1,
            3 => m.limit_sats += 1,
            _ => m.stop = !m.stop,
        }
        assert!(!m.verify(&identity(1).device_id()), "field {field}");
    }
    let mut m = l.clone();
    m.payments[1].by_owner = !m.payments[1].by_owner;
    assert!(!m.verify(&identity(1).device_id()));
}

#[test]
fn lk_79_line_refuses_more_than_16_payments_and_repeats() {
    let mut bytes = line(16).to_bytes();
    bytes[5 + 33] = 17;
    assert_eq!(
        StarterLine::from_bytes(&bytes).err(),
        Some(DecodeError::TooLarge)
    );
    let mut twice = line(2);
    twice.payments[1].txid = twice.payments[0].txid;
    assert!(matches!(
        StarterLine::from_bytes(&twice.to_bytes()),
        Err(DecodeError::NonCanonical(_))
    ));
}

#[test]
fn lk_79_signing_keeps_the_last_16() {
    let many: Vec<LinePayment> = (1..=20).map(payment).collect();
    let l = StarterLine::sign(&identity(1), 1, 2, 3, 4, false, &many);
    assert_eq!(l.payments.len(), 16);
    assert_eq!(l.payments[0].txid, [5; 32]);
    assert!(l.verify(&identity(1).device_id()));
}

#[test]
fn lk_79_flags_are_zero_or_one() {
    let mut bytes = line(1).to_bytes();
    bytes[5 + 32] = 2;
    assert!(StarterLine::from_bytes(&bytes).is_err());
    let mut bytes = line(1).to_bytes();
    let last_flag = 5 + 34 + 48;
    bytes[last_flag] = 2;
    assert!(StarterLine::from_bytes(&bytes).is_err());
}
