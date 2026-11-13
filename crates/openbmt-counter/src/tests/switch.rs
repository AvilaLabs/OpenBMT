//! Section 26: the ledger and the record of a switch from the starter wallet
//! (CR-261, CR-262). The frame, the checks and the files are `openbmt-core`'s
//! and `openbmt-counter-node`'s.

use openbmt_core::identity::TAG_SWITCHED_OWN;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::records::{Record, RecordKind};
use openbmt_core::settings::{Change, SettingNote};

use super::kit::*;
use super::own::{own_pairing, range, MULTI, SINGLE};
use crate::{
    BatchHeld, ConfirmHeld, Counter, Event, LoadError, RangeHeld, RangeTakeRefused, Slot,
    StaffEvent, SwitchedHeld, SWITCHED_HELD_LEN,
};

fn world() -> (World, crate::OwnPairing) {
    let p = own_pairing(SINGLE, range(0, 1_000));
    let c = Counter::new_own(id(3), &p, settings()).unwrap();
    (World::with_counter(c, Mem::default()), p)
}

/// The new wallet's first range: above every sale number the old wallet's
/// ranges used, since a sale is kept by its address index.
fn new_range() -> openbmt_core::pinned_wallet::SignedRange {
    range(1_000, 2_000)
}

fn multi() -> PinnedWallet {
    PinnedWallet::from_text(MULTI).unwrap()
}

#[test]
fn cr_261_after_the_switch_the_next_sale_is_the_new_wallets_first_index() {
    let (mut w, p) = world();
    let old_sale = w.open_five();
    assert_eq!(old_sale.script, p.wallet.receive_script(0).unwrap());
    w.tick(60);
    w.c.switch_wallet(&multi(), &new_range(), &mut w.m).unwrap();
    let next = w.open_five();
    assert_eq!(next.index, 1_000);
    assert_eq!(next.script, multi().receive_script(1_000).unwrap());
    assert_ne!(next.script, old_sale.script);
    assert_eq!(w.c.own_wallet(), Some(&multi()));
}

#[test]
fn cr_261_a_sale_opened_before_the_switch_still_ends_as_it_would() {
    let (mut w, p) = world();
    let before = w.open_five();
    w.c.switch_wallet(&multi(), &new_range(), &mut w.m).unwrap();
    // The sale keeps the old wallet's script, and the counter still knows it
    // as its own.
    assert_eq!(w.c.sale(before.index).unwrap().script(), &before.script);
    assert_eq!(before.script, p.wallet.receive_script(0).unwrap());
    assert!(w.c.watched_scripts().contains(&before.script));
    assert!(w.c.cancel(before.index, w.now, &mut w.m).is_ok());
}

#[test]
fn cr_261_the_old_scripts_stay_watched_from_the_closed_ranges_after_a_restart() {
    let (mut w, p) = world();
    for _ in 0..3 {
        w.open_five();
        w.tick(60);
    }
    w.c.switch_wallet(&multi(), &new_range(), &mut w.m).unwrap();
    w.open_five();
    // A restart with the record as the switch left it: the new wallet.
    let mut after = p.clone();
    after.wallet = multi();
    let c = Counter::load_own(id(3), &after, settings(), w.m.stored(), &w.m).unwrap();
    let watched = c.watched_scripts();
    for i in 0..3 {
        assert!(
            watched.contains(&p.wallet.receive_script(i).unwrap()),
            "{i}"
        );
    }
    assert!(watched.contains(&multi().receive_script(1_000).unwrap()));
    assert!(!watched.contains(&multi().receive_script(1_001).unwrap()));
    let ledger = c.range_ledger().unwrap();
    assert_eq!(ledger.closed().len(), 1);
    assert_eq!(ledger.closed()[0].next, 3);
}

#[test]
fn cr_261_a_ledger_for_another_wallet_than_the_pairing_is_a_fault() {
    let (mut w, p) = world();
    w.c.switch_wallet(&multi(), &new_range(), &mut w.m).unwrap();
    // The record still names the old wallet: the node's recovery changes
    // that before the counter starts, so the counter itself refuses.
    let err = Counter::load_own(id(3), &p, settings(), w.m.stored(), &w.m).err();
    assert!(matches!(err, Some(LoadError::Ranges(_))));
}

#[test]
fn cr_261_the_ledger_is_written_whole_by_the_first_step_and_a_second_is_nothing() {
    let (mut w, _) = world();
    w.open_five();
    w.m.reset();
    w.c.switch_wallet(&multi(), &new_range(), &mut w.m).unwrap();
    assert_eq!(w.m.log, ["Ranges"]);
    assert_eq!(
        w.m.slots[&Slot::Ranges],
        w.c.range_ledger().unwrap().to_bytes()
    );
    assert_eq!(&w.m.slots[&Slot::Ranges][..5], b"OBM;\x02");
    // The same switch again changes nothing and writes nothing.
    w.m.reset();
    w.c.switch_wallet(&multi(), &new_range(), &mut w.m).unwrap();
    assert!(w.m.log.is_empty());
}

#[test]
fn cr_261_a_new_range_that_holds_a_number_a_sale_already_has_is_refused() {
    let (mut w, _) = world();
    for _ in 0..3 {
        w.open_five();
        w.tick(60);
    }
    let before = w.c.range_ledger().unwrap().to_bytes();
    // Sales 0, 1 and 2 exist: a range that holds any of them is refused.
    for (start, end) in [(0, 1_000), (2, 1_002), (0, 3_000)] {
        let r = w.c.switch_wallet(&multi(), &range(start, end), &mut w.m);
        assert!(matches!(r, Err(RangeTakeRefused::Range(_))), "{start}");
    }
    assert_eq!(w.c.range_ledger().unwrap().to_bytes(), before);
    // One that starts after them is taken.
    w.c.switch_wallet(&multi(), &range(3, 1_003), &mut w.m)
        .unwrap();
    assert_eq!(w.open_five().index, 3);
}

#[test]
fn cr_261_a_range_that_fails_cr_232_or_a_failed_store_changes_nothing() {
    let (mut w, _) = world();
    let before = w.c.range_ledger().unwrap().to_bytes();
    // Below the smallest range, for another counter, signed by another root.
    let small = range(0, 10);
    assert!(matches!(
        w.c.switch_wallet(&multi(), &small, &mut w.m),
        Err(RangeTakeRefused::Range(_))
    ));
    let other_counter = openbmt_core::pinned_wallet::SignedRange::sign(
        &id(1),
        openbmt_core::pinned_wallet::IndexRange::new(id(4).device_id(), 0, 1_000).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        w.c.switch_wallet(&multi(), &other_counter, &mut w.m),
        Err(RangeTakeRefused::Range(_))
    ));
    let forged = openbmt_core::pinned_wallet::SignedRange::sign(
        &id(2),
        openbmt_core::pinned_wallet::IndexRange::new(id(3).device_id(), 0, 1_000).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        w.c.switch_wallet(&multi(), &forged, &mut w.m),
        Err(RangeTakeRefused::Range(_))
    ));
    w.m.fail_all = true;
    assert!(matches!(
        w.c.switch_wallet(&multi(), &new_range(), &mut w.m),
        Err(RangeTakeRefused::Store(_))
    ));
    assert_eq!(w.c.range_ledger().unwrap().to_bytes(), before);
    assert_eq!(
        w.c.own_wallet(),
        Some(&PinnedWallet::from_text(SINGLE).unwrap())
    );
}

#[test]
fn cr_262_the_record_is_98_bytes_tag_37_and_read_strictly() {
    let held = SwitchedHeld::sign(&id(3), true, [7; 32]);
    let good = held.encode();
    assert_eq!(good.len(), SWITCHED_HELD_LEN);
    assert_eq!(SWITCHED_HELD_LEN, 98);
    assert_eq!(good[0], 37);
    assert_eq!(good[1], 1);
    assert_eq!(&good[2..34], &[7; 32]);
    let read = SwitchedHeld::decode(&good).unwrap();
    assert_eq!(read, held);
    assert_eq!(read.encode(), good);
    assert!(read.verify(&id(3).device_id()));
    // Cut at every length, and a byte left over.
    for n in 0..good.len() {
        assert!(SwitchedHeld::decode(&good[..n]).is_err(), "cut at {n}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert!(SwitchedHeld::decode(&longer).is_err());
    // Another tag, and a byte that is neither 0 nor 1.
    for tag in 0..=255u8 {
        let mut bad = good.clone();
        bad[0] = tag;
        assert_eq!(SwitchedHeld::decode(&bad).is_ok(), tag == 37, "tag {tag}");
    }
    for byte in 2..=255u8 {
        let mut bad = good.clone();
        bad[1] = byte;
        assert!(SwitchedHeld::decode(&bad).is_err(), "byte {byte}");
    }
    // The refusal is the same record with the byte 0.
    let refused = SwitchedHeld::sign(&id(3), false, [7; 32]);
    assert_eq!(refused.encode()[1], 0);
    assert_ne!(refused.sig, held.sig);
}

#[test]
fn cr_262_the_signature_covers_the_tag_the_byte_and_the_hash() {
    let held = SwitchedHeld::sign(&id(3), true, [7; 32]);
    assert!(held.verify(&id(3).device_id()));
    assert!(!held.verify(&id(4).device_id()));
    // The byte and the hash are covered.
    let mut flipped = held;
    flipped.switched = false;
    assert!(!flipped.verify(&id(3).device_id()));
    let mut moved = held;
    moved.wallet = [8; 32];
    assert!(!moved.verify(&id(3).device_id()));
    // Another tag does not verify.
    let mut msg = [0u8; 34];
    msg[0] = 37;
    msg[1] = 1;
    msg[2..].copy_from_slice(&[7; 32]);
    let other = id(3).sign_tagged("OpenBMT/confirm/v1", &msg);
    assert!(!SwitchedHeld { sig: other, ..held }.verify(&id(3).device_id()));
    assert_eq!(TAG_SWITCHED_OWN, "OpenBMT/switched-own/v1");
}

#[test]
fn cr_262_a_body_of_98_bytes_is_read_as_this_kind_only() {
    let held = SwitchedHeld::sign(&id(3), true, [7; 32]).encode();
    let setting = SettingNote {
        version: 3,
        change: Change::Loosening,
        hash: [3; 32],
    }
    .to_body();
    let confirm = ConfirmHeld {
        settings_hash: [6; 32],
        sig: openbmt_core::identity::Sig([9; 64]),
    }
    .encode();
    // CR-243's sizes now: 37, 41 (batch), 41 (range), 97 and 98.
    assert_eq!((setting.len(), confirm.len(), held.len()), (37, 97, 98));
    let reads = |body: &[u8]| {
        [
            SettingNote::from_body(body).is_ok(),
            BatchHeld::decode(body).is_ok(),
            RangeHeld::decode(body).is_ok(),
            ConfirmHeld::decode(body).is_ok(),
            SwitchedHeld::decode(body).is_ok(),
        ]
    };
    assert_eq!(reads(&held), [false, false, false, false, true]);
    assert_eq!(reads(&confirm), [false, false, false, true, false]);
    assert_eq!(reads(&setting), [true, false, false, false, false]);
    for len in [37usize, 41, 97, 98] {
        for tag in 0..=255u8 {
            let mut body = vec![0x5au8; len];
            body[0] = tag;
            if len == 41 {
                body[5..9].copy_from_slice(&9u32.to_le_bytes());
                body[1..5].copy_from_slice(&1u32.to_le_bytes());
            }
            if len == 37 {
                body[4] = 1;
            }
            if len == 98 {
                body[1] = 1;
            }
            assert!(
                reads(&body).iter().filter(|r| **r).count() <= 1,
                "len {len} tag {tag}"
            );
        }
    }
    // No sale or cashier reader takes it.
    assert!(Event::decode(&held).is_err());
    assert!(StaffEvent::decode(&held).is_err());
}

#[test]
fn cr_262_the_record_is_written_once_per_answer_and_read_back_at_start() {
    let (mut w, p) = world();
    let hash = multi().checksum32();
    assert!(w.c.switched_record(&hash).is_none());
    assert!(w.c.record_switched(hash, true, &mut w.m).unwrap());
    // The same answer is not written twice.
    assert!(!w.c.record_switched(hash, true, &mut w.m).unwrap());
    assert_eq!(w.m.records.len(), 1);
    let rec = Record::from_bytes(&w.m.records[0]).unwrap();
    assert_eq!(rec.kind, RecordKind::Setting);
    let body = SwitchedHeld::decode(&rec.body).unwrap();
    assert!(body.switched);
    assert!(body.verify(&id(3).device_id()));
    // A chain that holds it reads it at start.
    let again = Counter::load_own(id(3), &p, settings(), w.m.stored(), &w.m).unwrap();
    assert_eq!(again.switched_record(&hash), Some(&body));
    // The record counts nothing in the books.
    assert_eq!(again.records_len(), 1);
}

#[test]
fn cr_262_the_refusal_is_a_record_with_the_byte_zero() {
    let (mut w, _) = world();
    let hash = multi().checksum32();
    assert!(w.c.record_switched(hash, false, &mut w.m).unwrap());
    let held = *w.c.switched_record(&hash).unwrap();
    assert!(!held.switched);
    let rec = Record::from_bytes(&w.m.records[0]).unwrap();
    assert_eq!(rec.body[1], 0);
    // A failed write leaves nothing remembered.
    let (mut w, _) = world();
    w.m.fail_all = true;
    assert!(w.c.record_switched(hash, true, &mut w.m).is_err());
    assert!(w.c.switched_record(&hash).is_none());
}
