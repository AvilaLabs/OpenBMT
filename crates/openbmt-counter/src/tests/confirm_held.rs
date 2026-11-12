//! Section 24: the record that carries a counter's confirmation of a change
//! to the shop's settings (CR-241, CR-243).

use openbmt_core::approval::CounterConfirmation;
use openbmt_core::records::{Record, RecordKind};
use openbmt_core::settings::{Change, SettingNote};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::{
    batch_digest, BatchHeld, ConfirmHeld, Event, RangeHeld, StaffEvent, StoreFault,
    CONFIRM_HELD_LEN,
};

fn held(n: u8) -> ConfirmHeld {
    ConfirmHeld {
        settings_hash: [n; 32],
        sig: CounterConfirmation::sign(&id(3), &[n; 32]).sig,
    }
}

#[test]
fn cr_241_the_counter_signs_the_hash_and_writes_one_setting_record() {
    let mut w = World::new();
    let hash = [7u8; 32];
    let record = w.c.record_confirmation(&hash, &mut w.m).unwrap();
    assert_eq!(record.kind, RecordKind::Setting);
    assert_eq!(record.body.len(), 97);
    assert_eq!(record.body[0], 36);
    let read = ConfirmHeld::decode(&record.body).unwrap();
    assert_eq!(read.settings_hash, hash);
    // The signature is the counter's, for AP-04, and verifies for the
    // counter's id and no other.
    let conf = read.confirmation(id(3).device_id());
    assert_eq!(conf, CounterConfirmation::sign(&id(3), &hash));
    let mut force = std::collections::BTreeMap::new();
    force.insert(id(3).device_id(), 10_000u16);
    assert_eq!(conf.verify(&hash, &force), Ok(()));
    assert!(read
        .confirmation(id(4).device_id())
        .verify(&hash, &{
            let mut other = std::collections::BTreeMap::new();
            other.insert(id(4).device_id(), 10_000u16);
            other
        })
        .is_err());
    // It was stored first and is the chain's next record (CR-40).
    assert_eq!(w.m.log, vec!["record".to_string()]);
    assert_eq!(w.c.records_len(), 1);
    // It puts no settings in force and names no version.
    assert_eq!(w.c.settings_recorded(), None);
    assert_eq!(w.c.settings().version, settings().version);
}

#[test]
fn cr_241_a_record_that_is_not_stored_changes_nothing() {
    let mut w = World::new();
    w.m.fail_all = true;
    let err = w.c.record_confirmation(&[1u8; 32], &mut w.m).unwrap_err();
    assert!(matches!(err, StoreFault::Write(_)));
    assert_eq!(w.c.records_len(), 0);
}

#[test]
fn cr_243_the_body_round_trips_and_is_read_strictly() {
    let good = held(5).encode();
    assert_eq!(good.len(), CONFIRM_HELD_LEN);
    assert_eq!(good.len(), 97);
    assert_eq!(good[0], 36);
    assert_eq!(good[1..33], [5u8; 32]);
    let read = ConfirmHeld::decode(&good).unwrap();
    assert_eq!(read, held(5));
    assert_eq!(read.encode(), good);
    for n in 0..good.len() {
        assert!(ConfirmHeld::decode(&good[..n]).is_err(), "cut at {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(
        ConfirmHeld::decode(&longer),
        Err(DecodeError::TrailingBytes)
    );
    for tag in (0..=255u8).filter(|t| *t != 36) {
        let mut bad = good.clone();
        bad[0] = tag;
        assert!(ConfirmHeld::decode(&bad).is_err(), "tag {tag}");
    }
}

#[test]
fn cr_243_the_bodies_of_37_41_41_and_97_bytes_are_each_read_as_one_kind() {
    let setting = SettingNote {
        version: 9,
        change: Change::Loosening,
        hash: [3; 32],
    }
    .to_body();
    let batch_body = BatchHeld::of(&batch(100, 50)).unwrap().encode();
    let range_body = RangeHeld {
        start: 0,
        end: 1_000,
        digest: [4; 32],
    }
    .encode();
    let confirm = held(6).encode();
    assert_eq!(
        (
            setting.len(),
            batch_body.len(),
            range_body.len(),
            confirm.len()
        ),
        (37, 41, 41, 97)
    );
    let reads = |body: &[u8]| {
        [
            SettingNote::from_body(body).is_ok(),
            BatchHeld::decode(body).is_ok(),
            RangeHeld::decode(body).is_ok(),
            ConfirmHeld::decode(body).is_ok(),
        ]
    };
    assert_eq!(reads(&setting), [true, false, false, false]);
    assert_eq!(reads(&batch_body), [false, true, false, false]);
    assert_eq!(reads(&range_body), [false, false, true, false]);
    assert_eq!(reads(&confirm), [false, false, false, true]);
    // Whatever the bytes are, at most one reader takes them: every tag on
    // each length, the first byte of a 37-byte body included.
    for len in [37usize, 41, 97] {
        for tag in 0..=255u8 {
            let mut body = vec![0x5au8; len];
            body[0] = tag;
            if len == 41 {
                // A range or a batch needs an end above its start.
                body[5..9].copy_from_slice(&9u32.to_le_bytes());
                body[1..5].copy_from_slice(&1u32.to_le_bytes());
            }
            if len == 37 {
                body[4] = 1;
            }
            assert!(
                reads(&body).iter().filter(|r| **r).count() <= 1,
                "len {len} tag {tag}"
            );
        }
    }
    // No sale or cashier reader takes any of them.
    for body in [&setting, &batch_body, &range_body, &confirm] {
        assert!(Event::decode(body).is_err());
        assert!(StaffEvent::decode(body).is_err());
    }
    let _ = batch_digest;
}

#[test]
fn cr_243_a_chain_with_the_record_loads_and_counts_nothing_from_it() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.record_confirmation(&[8u8; 32], &mut w.m).unwrap();
    w.c.record_setting(
        SettingNote {
            version: 2,
            change: Change::Loosening,
            hash: [2; 32],
        },
        &mut w.m,
    )
    .unwrap();
    let again = w.m.reload();
    assert_eq!(again.records_len(), 3);
    assert_eq!(again.head(), w.c.head());
    assert!(again.sale(o.index).is_some());
    // The record names no settings version; the 37-byte one does.
    assert_eq!(again.settings_recorded(), Some(2));
    let kinds: Vec<RecordKind> =
        w.m.records
            .iter()
            .map(|b| Record::from_bytes(b).unwrap().kind)
            .collect();
    assert_eq!(
        kinds,
        vec![RecordKind::Sale, RecordKind::Setting, RecordKind::Setting]
    );
}
