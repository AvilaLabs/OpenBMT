//! Section 23: a counter paired with the shop's own wallet (CR-234,
//! CR-235). The pairing itself is tested in `openbmt-core` (`pairing_own`)
//! and `openbmt-counter-node` (`paired`).

use openbmt_core::batch::BatchPolicy;
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, RangeError, SignedRange};
use openbmt_core::records::{Record, RecordKind};

use super::kit::*;
use crate::{
    range_digest, BatchTakeRefused, Counter, LoadError, OpenRefused, OwnPairing, RangeHeld,
    RangeTakeRefused, Slot, State,
};

/// The Key Maker's test seed, account 0 (WL-06): P2WPKH addresses.
pub const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
/// A 2-of-3 description: P2WSH addresses.
pub const MULTI: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";

pub fn range(start: u32, end: u32) -> SignedRange {
    SignedRange::sign(
        &id(1),
        IndexRange::new(id(3).device_id(), start, end).unwrap(),
    )
    .unwrap()
}

pub fn own_pairing(text: &str, first: SignedRange) -> OwnPairing {
    OwnPairing {
        owner: id(1).device_id(),
        wallet: PinnedWallet::from_text(text).unwrap(),
        first_range: first,
        batch_policy: BatchPolicy { min_count: 1_000 },
    }
}

fn own_world(text: &str, first: SignedRange) -> (World, OwnPairing) {
    let p = own_pairing(text, first);
    let c = Counter::new_own(id(3), &p, settings()).unwrap();
    (World::with_counter(c, Mem::default()), p)
}

fn restart(w: &mut World, p: &OwnPairing) {
    w.c = Counter::load_own(id(3), p, settings(), w.m.stored(), &w.m).unwrap();
}

fn try_open(w: &mut World) -> Result<crate::Opened, OpenRefused<&'static str>> {
    let facts = w.open_facts();
    w.c.open(
        five_dollars(),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        &mut w.m,
    )
}

// CR-234.

#[test]
fn cr_234_a_sales_address_is_the_wallets_receive_address_at_the_next_index() {
    for (text, len) in [(SINGLE, 22), (MULTI, 34)] {
        let (mut w, p) = own_world(text, range(40, 2_040));
        let first = w.open_five();
        let second = w.open_five();
        assert_eq!((first.index, second.index), (40, 41));
        assert_eq!(first.script, p.wallet.receive_script(40).unwrap());
        assert_eq!(second.script, p.wallet.receive_script(41).unwrap());
        assert_eq!(first.script.len(), len);
        assert_eq!(w.c.sale(40).unwrap().script(), &first.script);
    }
}

#[test]
fn cr_234_the_ledger_is_stored_whole_before_the_address_is_shown() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    w.open_five();
    assert_eq!(w.m.log, ["Desk", "Ranges", "Sale(0)", "record"]);
    assert!(!w.m.slots.contains_key(&Slot::Batches));
    assert_eq!(
        w.m.slots[&Slot::Ranges],
        w.c.range_ledger().unwrap().to_bytes()
    );
}

#[test]
fn cr_234_if_the_store_fails_no_address_is_shown_and_the_index_is_not_used() {
    let (mut w, p) = own_world(SINGLE, range(0, 1_000));
    w.m.fail_on = Some(2); // the ranges
    let err = try_open(&mut w).unwrap_err();
    assert!(matches!(err, OpenRefused::AddressNotStored(_)));
    w.m.reset();
    assert_eq!(w.open_five().index, 0);
    // A crash after the ranges were stored wastes the index, never reuses it.
    let (mut w, p2) = own_world(SINGLE, range(0, 1_000));
    w.m.fail_on = Some(3); // the sale
    assert!(matches!(
        try_open(&mut w).unwrap_err(),
        OpenRefused::Store(_)
    ));
    w.m.reset();
    restart(&mut w, &p2);
    assert_eq!(w.open_five().index, 1);
    let _ = p;
}

#[test]
fn cr_234_a_restart_reads_the_ranges_slot_and_continues_at_the_right_index() {
    let (mut w, p) = own_world(SINGLE, range(10, 1_010));
    for want in 10..13 {
        assert_eq!(w.open_five().index, want);
        w.tick(60);
    }
    restart(&mut w, &p);
    let next = w.open_five();
    assert_eq!(next.index, 13);
    assert_eq!(next.script, p.wallet.receive_script(13).unwrap());
    // The scripts handed out are still watched.
    let watched = w.c.watched_scripts();
    for i in 10..14 {
        assert!(watched.contains(&p.wallet.receive_script(i).unwrap()));
    }
}

#[test]
fn cr_234_an_empty_slot_is_made_from_the_pairings_first_range() {
    let p = own_pairing(SINGLE, range(500, 1_500));
    let c = Counter::load_own(
        id(3),
        &p,
        settings(),
        Mem::default().stored(),
        &Mem::default(),
    )
    .unwrap();
    assert_eq!(c.range_ledger().unwrap().remaining(), 1_000);
    assert_eq!(c.range_ledger().unwrap().signed(), [p.first_range]);
}

#[test]
fn cr_234_a_slot_that_does_not_read_or_check_is_a_fault() {
    let (mut w, p) = own_world(SINGLE, range(0, 1_000));
    w.open_five();
    let good = w.m.slots[&Slot::Ranges].clone();
    let load = |bytes: &[u8]| {
        let mut m = Mem::default();
        m.slots.insert(Slot::Ranges, bytes.to_vec());
        Counter::load_own(id(3), &p, settings(), m.stored(), &m).err()
    };
    assert!(load(&good).is_none());
    assert!(matches!(
        load(&good[..good.len() - 1]),
        Some(LoadError::Ranges(_))
    ));
    let mut flipped = good.clone();
    flipped[20] ^= 1;
    assert!(matches!(load(&flipped), Some(LoadError::Ranges(_))));
    // A range signed by another root.
    let forged = SignedRange::sign(
        &id(2),
        IndexRange::new(id(3).device_id(), 0, 1_000).unwrap(),
    )
    .unwrap();
    let mut ledger =
        openbmt_core::pairing_own::RangeLedger::new(id(2).device_id(), id(3).device_id());
    ledger.accept(&forged).unwrap();
    assert!(matches!(
        load(&ledger.to_bytes()),
        Some(LoadError::Ranges(_))
    ));
    assert_eq!(
        LoadError::Ranges(openbmt_core::wire::DecodeError::Truncated).rule(),
        "CR-42"
    );
}

#[test]
fn cr_234_a_used_up_book_refuses_the_sale_as_an_empty_batch_does() {
    let p = own_pairing(SINGLE, range(0, 1_000));
    let mut ledger = openbmt_core::pairing_own::RangeLedger::new(p.owner, id(3).device_id());
    ledger.accept(&p.first_range).unwrap();
    for _ in 0..1_000 {
        ledger.next().unwrap();
    }
    let mut m = Mem::default();
    m.slots.insert(Slot::Ranges, ledger.to_bytes());
    let c = Counter::load_own(id(3), &p, settings(), m.stored(), &m).unwrap();
    let mut w = World::with_counter(c, m);
    let writes = w.m.writes;
    let err = try_open(&mut w).unwrap_err();
    assert_eq!(err, OpenRefused::NoAddress);
    assert_eq!(err.rule(), "CR-02");
    assert_eq!(w.m.writes, writes, "a refusal writes nothing");
}

#[test]
fn cr_234_a_payment_to_the_address_is_matched_whatever_its_length() {
    for text in [SINGLE, MULTI] {
        let (mut w, p) = own_world(text, range(0, 1_000));
        let o = w.open_five();
        assert_eq!(o.script, p.wallet.receive_script(0).unwrap());
        let tx = w.pay(&o, 5_000);
        w.see(&tx, 1);
        let screen = w.judge(o.index);
        assert_eq!(screen.state, State::Waiting, "{text}");
        let sale = w.c.sale(o.index).unwrap();
        assert_eq!(sale.script(), &o.script);
        // The sale reads back from its slot with the shorter script too.
        restart(&mut w, &p);
        assert_eq!(w.c.sale(o.index).unwrap().script(), &o.script);
    }
}

#[test]
fn cr_234_the_watched_scripts_are_those_handed_out() {
    let (mut w, p) = own_world(SINGLE, range(0, 1_000));
    assert!(w.c.watched_scripts().is_empty());
    let o = w.open_five();
    assert_eq!(w.c.watched_scripts(), vec![o.script.clone()]);
    // A refund to one of the shop's own addresses is refused (CR-63).
    assert!(w
        .c
        .watched_scripts()
        .contains(&p.wallet.receive_script(0).unwrap()));
}

// CR-235.

#[test]
fn cr_235_a_range_taken_stores_the_ranges_and_then_writes_one_record() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let before = w.c.records_len();
    let r = range(5_000, 6_000);
    w.c.take_range(r, &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), before + 1);
    assert_eq!(
        w.m.log,
        vec!["Ranges".to_string(), "record".to_string()],
        "the ranges first, the record last"
    );
    let stored = Record::from_bytes(w.m.records.last().unwrap()).unwrap();
    assert_eq!(stored.kind, RecordKind::Setting);
    assert_eq!(stored.body[0], 35);
    let held = RangeHeld::decode(&stored.body).unwrap();
    assert_eq!((held.start, held.end), (5_000, 6_000));
    assert_eq!(held.digest, range_digest(&r));
    assert_eq!(held, RangeHeld::of(&r));
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
}

#[test]
fn cr_235_the_digest_covers_the_range_and_its_signature() {
    let r = range(5_000, 6_000);
    assert_eq!(range_digest(&r), range_digest(&r));
    assert_ne!(range_digest(&r), range_digest(&range(5_000, 6_001)));
    let mut other = r;
    other.sig.0[0] ^= 1;
    assert_ne!(range_digest(&r), range_digest(&other));
    assert_ne!(
        range_digest(&r),
        crate::batch_digest(&batch(0, 1)),
        "not the digest of a batch"
    );
}

#[test]
fn cr_235_the_second_range_is_used_after_the_first_and_survives_a_restart() {
    let (mut w, p) = own_world(SINGLE, range(0, 1_000));
    w.c.take_range(range(5_000, 6_000), &mut w.m).unwrap();
    restart(&mut w, &p);
    assert_eq!(w.c.range_ledger().unwrap().remaining(), 2_000);
    // The held record is read back at start, so a second send is nothing.
    let records = w.c.records_len();
    w.c.take_range(range(5_000, 6_000), &mut w.m).unwrap();
    assert_eq!(w.c.records_len(), records);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
}

#[test]
fn cr_235_a_range_overlapping_what_is_handed_out_or_in_use_is_refused() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    w.open_five();
    let before = w.c.records_len();
    w.m.reset();
    // In use (part of the first range).
    let e = w.c.take_range(range(500, 1_500), &mut w.m).unwrap_err();
    assert_eq!(e, RangeTakeRefused::Range(RangeError::OverlapsTaken.into()));
    assert_eq!(e.rule(), "CR-235");
    // Over the index handed out.
    let e = w.c.take_range(range(0, 1_500), &mut w.m).unwrap_err();
    assert_eq!(e, RangeTakeRefused::Range(RangeError::OverlapsUsed.into()));
    assert_eq!(w.c.records_len(), before);
    assert!(w.m.log.is_empty(), "nothing was written: {:?}", w.m.log);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 1);
}

#[test]
fn cr_235_a_range_for_another_counter_or_not_signed_by_the_owner_or_too_small_is_refused() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let other = SignedRange::sign(
        &id(1),
        IndexRange::new(id(4).device_id(), 5_000, 6_000).unwrap(),
    )
    .unwrap();
    let forged = SignedRange::sign(
        &id(2),
        IndexRange::new(id(3).device_id(), 5_000, 6_000).unwrap(),
    )
    .unwrap();
    let small = range(5_000, 5_999);
    for r in [other, forged, small] {
        let e = w.c.take_range(r, &mut w.m).unwrap_err();
        assert!(matches!(e, RangeTakeRefused::Range(_)), "{e}");
        assert!(e.to_string().starts_with("CR-235"));
    }
    assert!(w.m.log.is_empty());
    assert!(w.m.records.is_empty());
}

#[test]
fn cr_235_a_store_that_fails_records_nothing_and_leaves_the_ranges_as_they_were() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let r = range(5_000, 6_000);
    let ledger = w.c.range_ledger().unwrap().to_bytes();
    w.m.fail_on = Some(1); // the ranges
    let e = w.c.take_range(r, &mut w.m).unwrap_err();
    assert!(matches!(e, RangeTakeRefused::Store(_)));
    assert_eq!(e.rule(), "CR-235");
    assert!(w.m.records.is_empty(), "no record before the ranges");
    assert_eq!(w.c.records_len(), 0);
    assert_eq!(w.c.range_ledger().unwrap().to_bytes(), ledger);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 1);
    // A second try takes it whole.
    w.m.reset();
    w.c.take_range(r, &mut w.m).unwrap();
    assert_eq!(w.m.records.len(), 1);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
}

#[test]
fn cr_235_the_ledger_is_written_before_the_record() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let r = range(5_000, 6_000);
    w.m.fail_on = Some(2); // the record, after the ranges
    let e = w.c.take_range(r, &mut w.m).unwrap_err();
    assert!(matches!(e, RangeTakeRefused::Record(_)));
    assert_eq!(w.m.log, ["Ranges", "FAILED record"]);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
    assert_eq!(
        w.m.slots[&Slot::Ranges],
        w.c.range_ledger().unwrap().to_bytes()
    );
}

#[test]
fn cr_235_a_record_that_fails_after_the_store_gets_its_record_once_when_the_range_comes_again() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let r = range(5_000, 6_000);
    w.m.fail_on = Some(2);
    assert!(w.c.take_range(r, &mut w.m).is_err());
    assert!(w.m.records.is_empty());
    let ledger = w.m.slots[&Slot::Ranges].clone();
    w.m.reset();
    w.c.take_range(r, &mut w.m).unwrap();
    assert_eq!(w.m.log, ["record"], "the record and nothing else");
    assert_eq!(w.m.records.len(), 1);
    assert_eq!(w.m.slots[&Slot::Ranges], ledger);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
    // And a third time is nothing.
    w.m.reset();
    w.c.take_range(r, &mut w.m).unwrap();
    assert!(w.m.log.is_empty());
    assert_eq!(w.m.records.len(), 1);
}

#[test]
fn cr_235_a_range_recorded_but_not_stored_is_stored_with_no_second_record() {
    let (mut w, p) = own_world(SINGLE, range(0, 1_000));
    let old = w.c.range_ledger().unwrap().to_bytes();
    let r = range(5_000, 6_000);
    w.c.take_range(r, &mut w.m).unwrap();
    // What a counter of the older order left: the record, not the ranges.
    w.m.slots.insert(Slot::Ranges, old.clone());
    restart(&mut w, &p);
    assert_eq!(w.c.range_ledger().unwrap().to_bytes(), old);
    let records = w.m.records.len();
    w.m.reset();
    w.c.take_range(r, &mut w.m).unwrap();
    assert_eq!(w.m.records.len(), records);
    assert_eq!(w.m.log, ["Ranges"]);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
    assert_eq!(
        w.m.slots[&Slot::Ranges],
        w.c.range_ledger().unwrap().to_bytes()
    );
}

#[test]
fn cr_235_a_range_stored_and_recorded_is_taken_as_nothing() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let r = range(5_000, 6_000);
    w.c.take_range(r, &mut w.m).unwrap();
    let (records, ledger) = (w.c.records_len(), w.m.slots[&Slot::Ranges].clone());
    w.m.reset();
    w.c.take_range(r, &mut w.m).unwrap();
    assert!(w.m.log.is_empty(), "nothing written: {:?}", w.m.log);
    assert_eq!(w.c.records_len(), records);
    assert_eq!(w.m.slots[&Slot::Ranges], ledger);
}

#[test]
fn cr_235_a_stop_between_the_store_and_the_record_then_a_restart_then_the_range_again() {
    let (mut w, p) = own_world(SINGLE, range(0, 1_000));
    let r = range(5_000, 6_000);
    w.m.fail_on = Some(2);
    assert!(w.c.take_range(r, &mut w.m).is_err());
    w.m.reset();
    restart(&mut w, &p);
    // The ledger kept the range; the chain has no record of it.
    assert_eq!(w.c.range_ledger().unwrap().remaining(), 2_000);
    assert_eq!(w.c.records_len(), 0);
    w.c.take_range(r, &mut w.m).unwrap();
    assert_eq!(w.m.records.len(), 1);
    assert_eq!(w.c.range_ledger().unwrap().signed().len(), 2);
    // The range is there for sales: the first range's indexes go first, and
    // the book still holds both.
    assert_eq!(w.open_five().index, 0);
    assert_eq!(w.c.range_ledger().unwrap().remaining(), 1_999);
    // A restart reads the record and the ranges, and a third arrival is nothing.
    restart(&mut w, &p);
    w.m.reset();
    w.c.take_range(r, &mut w.m).unwrap();
    assert!(w.m.log.is_empty());
}

#[test]
fn cr_235_an_own_counter_drops_a_batch_and_a_key_app_counter_drops_a_range() {
    let (mut w, _) = own_world(SINGLE, range(0, 1_000));
    let e = w.c.take_batch(batch(0, 100), &mut w.m).unwrap_err();
    assert_eq!(e, BatchTakeRefused::OwnWallet);
    assert_eq!(
        e.to_string(),
        "CR-235: This counter makes its own addresses."
    );
    assert!(w.m.log.is_empty());
    let mut key = World::new();
    let e = key.c.take_range(range(0, 1_000), &mut key.m).unwrap_err();
    assert_eq!(e, RangeTakeRefused::KeyApp);
    assert_eq!(e.rule(), "CR-235");
    assert!(key.m.log.is_empty());
}
