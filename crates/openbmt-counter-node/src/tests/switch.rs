//! CR-256 to CR-263: switching a counter from the starter wallet. The node
//! of a counter paired with a starter wallet is started on a fake driver;
//! the owner phone's steps are the functions of `openbmt_core`, and the
//! app's part (keeping the new pairing record) is a variable.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use openbmt_core::pairing_own::{OwnPairError, SwitchOwn};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, RangeError, SignedRange, WalletKind};
use openbmt_core::records::{Record, RecordKind};
use openbmt_core::wire::MAGIC_OWN_PAIRING_RECORD;
use openbmt_counter::SwitchedHeld;

use super::kit::*;
use super::paired_own::{record, POLICY};
use crate::dirstore::SWITCH_FILE;
use crate::node::StartError;
use crate::paired_own::{OwnPairRefused, OwnPairingRecord, PairedRecord};
use crate::switch::{SwitchNotice, SwitchRefused, SwitchStepRefused, SWITCH_PENDING_SECS};
use crate::{DirStore, NodeConfig};

const OLD: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
const NEW: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";

fn old() -> PinnedWallet {
    PinnedWallet::from_text(OLD).unwrap()
}

fn new() -> PinnedWallet {
    PinnedWallet::from_text(NEW).unwrap()
}

/// The record of a counter paired with a starter wallet (CR-256).
pub(super) fn starter_record() -> OwnPairingRecord {
    let mut r = record();
    r.starter = true;
    r
}

fn range_for(counter: u8, start: u32, end: u32, signer: u8) -> SignedRange {
    SignedRange::sign(
        &id(signer),
        IndexRange::new(id(counter).device_id(), start, end).unwrap(),
    )
    .unwrap()
}

/// A good switch: counter 3, the owner root 1, from the old wallet to the new
/// one, with a first range above the first range of the pairing, since a
/// sale is kept by its address index.
fn good() -> SwitchOwn {
    SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        old().checksum32(),
        new(),
        range_for(3, 1_000, 2_000, 1),
    )
}

fn config_for(
    dir: &std::path::Path,
    time: &Arc<AtomicU64>,
    record: &OwnPairingRecord,
) -> NodeConfig {
    let mut config = NodeConfig::new_own(
        dir,
        id(3),
        record.own_pairing(POLICY),
        settings(),
        openbmt_net_io::DriverConfig::new(bitcoin::Network::Bitcoin, 0),
    );
    config.clock = Box::new(TestClock(time.clone()));
    config.health = Box::new(|_| healthy());
    config.own_record = Some(record.clone());
    config
}

fn rig_for(dir: &std::path::Path, record: &OwnPairingRecord) -> Rig {
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with(config_for(dir, &time, record), time);
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    rig
}

fn try_rig_for(dir: &std::path::Path, record: &OwnPairingRecord) -> Result<Rig, StartError> {
    let time = Arc::new(AtomicU64::new(T0));
    try_start_with(config_for(dir, &time, record), time)
}

fn fresh_rig(dir: &std::path::Path) -> Rig {
    rig_for(dir, &starter_record())
}

fn open_one(rig: &mut Rig) -> openbmt_counter::Opened {
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap()
}

fn chain(dir: &std::path::Path) -> Vec<Record> {
    use openbmt_counter::Store;
    let store = DirStore::open(dir).unwrap();
    let mut all: Vec<Record> = Vec::new();
    loop {
        let chunk = store.read_records(all.len() as u64, 64).unwrap();
        if chunk.is_empty() {
            break;
        }
        all.extend(chunk.iter().map(|b| Record::from_bytes(b).unwrap()));
    }
    all
}

fn switched_records(dir: &std::path::Path) -> Vec<SwitchedHeld> {
    chain(dir)
        .iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .filter_map(|r| SwitchedHeld::decode(&r.body).ok())
        .collect()
}

fn switch_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join(SWITCH_FILE)
}

// CR-256.

#[test]
fn cr_256_a_record_with_the_flag_is_version_3_and_reads_back_strictly() {
    let r = starter_record();
    let bytes = r.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC_OWN_PAIRING_RECORD);
    assert_eq!(bytes[4], 3);
    let back = OwnPairingRecord::from_bytes(&bytes).unwrap();
    assert!(back.starter);
    assert!(back == r);
    assert_eq!(back.to_bytes(), bytes);
    assert!(back.check(&POLICY).is_ok());
    // Cut at every length, and a byte over.
    for n in 0..bytes.len() {
        assert!(
            OwnPairingRecord::from_bytes(&bytes[..n]).is_err(),
            "cut {n}"
        );
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(OwnPairingRecord::from_bytes(&longer).is_err());
    // The flags are 1 and nothing else.
    let at = bytes.len() - 1;
    for flags in 0..=255u8 {
        let mut m = bytes.clone();
        m[at] = flags;
        assert_eq!(
            OwnPairingRecord::from_bytes(&m).is_ok(),
            flags == 1,
            "flags {flags}"
        );
    }
    // Version 1 has no flag and still reads.
    let plain = record();
    let v1 = plain.to_bytes();
    assert_eq!(v1[4], 1);
    assert!(!OwnPairingRecord::from_bytes(&v1).unwrap().starter);
    // A version 4 record is not read.
    let mut v4 = bytes.clone();
    v4[4] = 4;
    assert!(OwnPairingRecord::from_bytes(&v4).is_err());
}

#[test]
fn cr_256_the_flag_is_kept_with_a_relay_and_a_starter_record_must_pin_its_pairings_wallet() {
    use openbmt_core::relay::{RelayHint, RelayRoute};
    let mut r = starter_record();
    r.relay = Some(
        RelayHint::new(
            RelayRoute::Direct("192.168.1.20:48333".parse().unwrap()),
            id(40).device_id().to_bytes(),
        )
        .unwrap(),
    );
    let bytes = r.to_bytes();
    assert_eq!(bytes[4], 3);
    let back = OwnPairingRecord::from_bytes(&bytes).unwrap();
    assert!(back.starter && back.relay.is_some());
    assert_eq!(back.to_bytes(), bytes);
    // Without the flag, a relay is version 2 as before.
    r.starter = false;
    assert_eq!(r.to_bytes()[4], 2);
    // The pairing names the wallet the counter was paired with: a record
    // that pins another wallet is refused, with the flag or without.
    let mut moved = starter_record();
    moved.wallet = new();
    assert_eq!(
        moved.check(&POLICY).err(),
        Some(OwnPairRefused::Pair(OwnPairError::WrongWallet))
    );
    moved.starter = false;
    assert_eq!(
        moved.check(&POLICY).err(),
        Some(OwnPairRefused::Pair(OwnPairError::WrongWallet))
    );
}

#[test]
fn cr_261_a_record_that_switched_is_accepted_only_when_the_book_proves_it() {
    use openbmt_core::pairing_own::RangeLedger;
    let owner = id(1).device_id();
    let me = id(3).device_id();
    let mut switched = starter_record();
    switched.wallet = new();
    switched.starter = false;
    let sw = good();
    let mut ledger = RangeLedger::new(owner, me);
    ledger.accept(&switched.first_range).unwrap();
    let plain_book = ledger.to_bytes();
    ledger
        .switch(&old(), sw.wallet_hash(), &sw.first_range)
        .unwrap();
    let book = ledger.to_bytes();
    assert!(switched.check_switched(&POLICY, &book).is_ok());
    let kept = PairedRecord::from_bytes(&switched.to_bytes()).unwrap();
    assert!(kept.check(&POLICY).is_err());
    assert!(kept.check_with_ranges(&POLICY, Some(&book)).is_ok());
    // No book, a book that never switched, or one that switched to another
    // wallet does not prove it.
    assert!(kept.check_with_ranges(&POLICY, None).is_err());
    assert!(kept.check_with_ranges(&POLICY, Some(&plain_book)).is_err());
    assert!(kept.check_with_ranges(&POLICY, Some(b"junk")).is_err());
    let mut third = RangeLedger::new(owner, me);
    third.accept(&switched.first_range).unwrap();
    third.switch(&old(), [5; 32], &sw.first_range).unwrap();
    assert!(kept
        .check_with_ranges(&POLICY, Some(&third.to_bytes()))
        .is_err());
    // A record that still has the starter flag has not switched.
    let mut flagged = switched.clone();
    flagged.starter = true;
    assert!(flagged.check_switched(&POLICY, &book).is_err());
    // A record for its own wallet needs no book.
    assert!(PairedRecord::from_bytes(&record().to_bytes())
        .unwrap()
        .check_with_ranges(&POLICY, Some(&book))
        .is_ok());
}

// CR-258.

/// Take `sw` on a fresh node and say what was refused; nothing may have
/// changed.
fn refused_with(record: &OwnPairingRecord, sw: &SwitchOwn) -> SwitchRefused {
    let dir = Scratch::new();
    let mut rig = rig_for(dir.path(), record);
    let before = rig.node.records_len();
    let err = rig.node.take_switch(&sw.to_bytes()).unwrap_err();
    assert!(rig.node.pending_switch().is_none());
    assert!(!switch_path(dir.path()).exists());
    assert_eq!(rig.node.records_len(), before);
    assert!(rig.node.record_update().is_none());
    assert!(rig.node.is_starter() == record.starter);
    // A note names the rule.
    assert!(
        rig.node.take_notes().iter().any(|n| n.starts_with("CR-2")),
        "{err}"
    );
    err
}

#[test]
fn cr_258_a_record_without_the_flag_refuses_every_switch() {
    let plain = record();
    assert!(!plain.starter);
    let err = refused_with(&plain, &good());
    assert_eq!(err, SwitchRefused::NotStarter);
    assert!(err.to_string().starts_with("CR-263"));
    // A version 1 record, as it was paired before the flag existed.
    let v1 = OwnPairingRecord::from_bytes(&record().to_bytes()).unwrap();
    assert_eq!(refused_with(&v1, &good()), SwitchRefused::NotStarter);
}

#[test]
fn cr_258_a_switch_not_signed_by_the_owner_root_is_refused() {
    let sw = SwitchOwn::sign(
        &id(2),
        id(3).device_id(),
        old().checksum32(),
        new(),
        range_for(3, 1_000, 2_000, 1),
    );
    assert_eq!(
        refused_with(&starter_record(), &sw),
        SwitchRefused::NotFromOwner
    );
    // A changed part after signing is the same refusal.
    let mut moved = good();
    moved.first_range = range_for(3, 1_001, 2_001, 1);
    assert_eq!(
        refused_with(&starter_record(), &moved),
        SwitchRefused::NotFromOwner
    );
}

#[test]
fn cr_258_a_switch_for_another_counter_is_refused() {
    let sw = SwitchOwn::sign(
        &id(1),
        id(4).device_id(),
        old().checksum32(),
        new(),
        range_for(4, 1_000, 2_000, 1),
    );
    assert_eq!(
        refused_with(&starter_record(), &sw),
        SwitchRefused::WrongCounter
    );
}

#[test]
fn cr_258_a_switch_from_another_wallet_than_the_one_pinned_is_refused() {
    let sw = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        new().checksum32(),
        new(),
        range_for(3, 1_000, 2_000, 1),
    );
    assert_eq!(
        refused_with(&starter_record(), &sw),
        SwitchRefused::WrongWallet
    );
    let sw = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        [9; 32],
        new(),
        range_for(3, 1_000, 2_000, 1),
    );
    assert_eq!(
        refused_with(&starter_record(), &sw),
        SwitchRefused::WrongWallet
    );
}

#[test]
fn cr_258_the_wallet_pinned_now_is_not_a_new_wallet() {
    let sw = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        old().checksum32(),
        old(),
        range_for(3, 1_000, 2_000, 1),
    );
    assert_eq!(
        refused_with(&starter_record(), &sw),
        SwitchRefused::SameWallet
    );
}

#[test]
fn cr_258_a_wallet_that_is_not_canonical_does_not_decode() {
    let mut bytes = good().to_bytes();
    // The wallet's own checksum is the 32 bytes before the range and the
    // signature.
    let at = bytes.len() - 64 - 109 - 1;
    bytes[at] ^= 1;
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    let err = rig.node.take_switch(&bytes).unwrap_err();
    assert!(matches!(err, SwitchRefused::Frame(_)), "{err}");
    assert!(err.to_string().starts_with("CR-257"));
    assert!(rig.node.pending_switch().is_none());
}

#[test]
fn cr_258_the_range_must_pass_cr_232_for_this_counter_and_hold_no_sale_number() {
    let cases: [(SignedRange, &str); 4] = [
        // Below the smallest range.
        (range_for(3, 1_000, 1_010, 1), "small"),
        // For another counter.
        (range_for(4, 1_000, 2_000, 1), "counter"),
        // Signed by another root.
        (range_for(3, 1_000, 2_000, 2), "signed"),
        // Holds the number of a sale this counter has (the first sale of the
        // pairing's range is 0).
        (range_for(3, 0, 1_000, 1), "sale"),
    ];
    for (range, why) in cases {
        let sw = SwitchOwn::sign(&id(1), id(3).device_id(), old().checksum32(), new(), range);
        let dir = Scratch::new();
        let mut rig = fresh_rig(dir.path());
        open_one(&mut rig);
        let err = rig.node.take_switch(&sw.to_bytes()).unwrap_err();
        assert!(matches!(err, SwitchRefused::Range(_)), "{why}: {err}");
        assert!(err.to_string().starts_with("CR-258"));
        assert!(rig.node.pending_switch().is_none(), "{why}");
        assert!(!switch_path(dir.path()).exists(), "{why}");
    }
    // The error of the sale case is the book's own.
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    open_one(&mut rig);
    let sw = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        old().checksum32(),
        new(),
        range_for(3, 0, 1_000, 1),
    );
    assert_eq!(
        rig.node.take_switch(&sw.to_bytes()).unwrap_err(),
        SwitchRefused::Range(OwnPairError::Range(RangeError::OverlapsUsed))
    );
}

#[test]
fn cr_258_the_checks_run_in_the_order_the_rule_gives() {
    // Fails everything: not signed by the owner, another counter, another
    // wallet, the same wallet, a bad range.
    let all_wrong = SwitchOwn::sign(
        &id(2),
        id(4).device_id(),
        [9; 32],
        old(),
        range_for(4, 0, 10, 2),
    );
    assert_eq!(
        refused_with(&record(), &all_wrong),
        SwitchRefused::NotStarter
    );
    let sr = starter_record();
    assert_eq!(refused_with(&sr, &all_wrong), SwitchRefused::NotFromOwner);
    let signed = |counter: u8, from: [u8; 32], to: PinnedWallet, range: SignedRange| {
        SwitchOwn::sign(&id(1), id(counter).device_id(), from, to, range)
    };
    let sw = signed(4, [9; 32], old(), range_for(4, 0, 10, 2));
    assert_eq!(refused_with(&sr, &sw), SwitchRefused::WrongCounter);
    let sw = signed(3, [9; 32], old(), range_for(3, 0, 10, 2));
    assert_eq!(refused_with(&sr, &sw), SwitchRefused::WrongWallet);
    let sw = signed(3, old().checksum32(), old(), range_for(3, 0, 10, 2));
    assert_eq!(refused_with(&sr, &sw), SwitchRefused::SameWallet);
    let sw = signed(3, old().checksum32(), new(), range_for(3, 0, 10, 2));
    assert!(matches!(refused_with(&sr, &sw), SwitchRefused::Range(_)));
}

#[test]
fn cr_258_a_counter_without_the_record_takes_no_switch() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config_for(dir.path(), &time, &starter_record());
    config.own_record = None;
    let mut rig = start_with(config, time);
    assert_eq!(
        rig.node.take_switch(&good().to_bytes()).unwrap_err(),
        SwitchRefused::NotStarter
    );
}

// CR-259.

#[test]
fn cr_259_a_switch_that_passes_waits_in_its_file_and_changes_nothing_yet() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    let first = open_one(&mut rig);
    let before = rig.node.records_len();
    let sw = good();
    rig.node.take_switch(&sw.to_bytes()).unwrap();
    // The file is the OBM] frame and nothing else.
    let kept = std::fs::read(switch_path(dir.path())).unwrap();
    assert_eq!(kept, sw.to_bytes());
    assert_eq!(&kept[..4], b"OBM]");
    // Nothing else changed: no record, the old wallet, the same ranges.
    assert_eq!(rig.node.records_len(), before);
    assert!(rig.node.is_starter());
    assert_eq!(rig.node.counter().own_wallet(), Some(&old()));
    assert!(rig
        .node
        .counter()
        .range_ledger()
        .unwrap()
        .closed()
        .is_empty());
    assert!(rig.node.record_update().is_none());
    // The sales go on into the wallet pinned now.
    let next = open_one(&mut rig);
    assert_eq!(next.index, first.index + 1);
    assert_eq!(next.script, old().receive_script(next.index).unwrap());
    // The manager is shown the new wallet's first address, the kind of
    // wallet and when the switch is dropped.
    let waiting = rig.node.pending_switch().unwrap();
    assert_eq!(
        waiting.address,
        new().receive_address(0).unwrap().to_string()
    );
    assert_eq!(waiting.kind, WalletKind::TwoOfThree);
    assert_eq!(waiting.wallet, new().checksum32());
    assert_eq!(waiting.arrived, T0);
    assert_eq!(waiting.expires_at, T0 + SWITCH_PENDING_SECS);
    assert!(rig
        .node
        .take_notes()
        .iter()
        .any(|n| n.starts_with("CR-259")));
}

#[test]
fn cr_259_a_newer_switch_replaces_it_and_the_same_one_again_changes_nothing() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    let first = good();
    rig.node.take_switch(&first.to_bytes()).unwrap();
    rig.time
        .store(T0 + 100, std::sync::atomic::Ordering::SeqCst);
    // The same one again keeps its arrival.
    rig.node.take_switch(&first.to_bytes()).unwrap();
    assert_eq!(rig.node.pending_switch().unwrap().arrived, T0);
    // Another range makes it a different switch, and the newer one stands.
    let second = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        old().checksum32(),
        new(),
        range_for(3, 5_000, 6_000, 1),
    );
    rig.node.take_switch(&second.to_bytes()).unwrap();
    assert_eq!(rig.node.pending_switch().unwrap().arrived, T0 + 100);
    assert_eq!(
        std::fs::read(switch_path(dir.path())).unwrap(),
        second.to_bytes()
    );
    // One that fails a check leaves the waiting one as it was.
    let bad = SwitchOwn::sign(
        &id(2),
        id(3).device_id(),
        old().checksum32(),
        new(),
        range_for(3, 7_000, 8_000, 1),
    );
    assert!(rig.node.take_switch(&bad.to_bytes()).is_err());
    assert_eq!(
        std::fs::read(switch_path(dir.path())).unwrap(),
        second.to_bytes()
    );
}

#[test]
fn cr_259_a_switch_is_dropped_seven_days_after_it_arrived() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    rig.node.take_switch(&good().to_bytes()).unwrap();
    rig.time.store(
        T0 + SWITCH_PENDING_SECS - 1,
        std::sync::atomic::Ordering::SeqCst,
    );
    assert!(rig.node.pending_switch().is_some());
    assert!(switch_path(dir.path()).exists());
    rig.time.store(
        T0 + SWITCH_PENDING_SECS,
        std::sync::atomic::Ordering::SeqCst,
    );
    assert!(rig.node.pending_switch().is_none());
    assert!(!switch_path(dir.path()).exists());
    assert_eq!(rig.node.switch_notice(), Some(SwitchNotice::Dropped));
    // The owner phone may send it again.
    rig.node.take_switch(&good().to_bytes()).unwrap();
    assert!(rig.node.pending_switch().is_some());
    assert_eq!(rig.node.switch_notice(), None);
}

#[test]
fn cr_259_the_waiting_switch_is_read_again_at_start_and_a_useless_file_is_removed() {
    let dir = Scratch::new();
    {
        let mut rig = fresh_rig(dir.path());
        rig.node.take_switch(&good().to_bytes()).unwrap();
    }
    let mut rig = fresh_rig(dir.path());
    assert_eq!(
        rig.node.pending_switch().unwrap().wallet,
        new().checksum32()
    );
    drop(rig);
    // A file that is not a switch, or fails the checks, is of no use.
    std::fs::write(switch_path(dir.path()), b"junk").unwrap();
    let mut rig = rig_for(dir.path(), &starter_record());
    assert!(rig.node.pending_switch().is_none());
    assert!(!switch_path(dir.path()).exists());
    drop(rig);
    let other = SwitchOwn::sign(
        &id(2),
        id(3).device_id(),
        old().checksum32(),
        new(),
        range_for(3, 1_000, 2_000, 1),
    );
    std::fs::write(switch_path(dir.path()), other.to_bytes()).unwrap();
    let mut rig = rig_for(dir.path(), &starter_record());
    assert!(rig.node.pending_switch().is_none());
    assert!(!switch_path(dir.path()).exists());
}

// CR-260.

#[test]
fn cr_260_different_drops_the_switch_writes_the_refused_record_and_keeps_the_old_wallet() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    rig.node.take_switch(&good().to_bytes()).unwrap();
    let ledger_before = rig.node.counter().range_ledger().unwrap().to_bytes();
    rig.node.switch_different().unwrap();
    assert!(rig.node.pending_switch().is_none());
    assert!(!switch_path(dir.path()).exists());
    // The record: byte 0, the new wallet's hash, signed by the counter.
    let records = switched_records(dir.path());
    assert_eq!(records.len(), 1);
    assert!(!records[0].switched);
    assert_eq!(records[0].wallet, new().checksum32());
    assert!(records[0].verify(&id(3).device_id()));
    let body = chain(dir.path()).last().unwrap().body.clone();
    assert_eq!((body.len(), body[0], body[1]), (98, 37, 0));
    // The old wallet stays pinned: the ranges are as they were, the sales go
    // to the old wallet, the counter can still take a switch.
    assert_eq!(
        rig.node.counter().range_ledger().unwrap().to_bytes(),
        ledger_before
    );
    assert!(rig.node.record_update().is_none());
    assert!(rig.node.is_starter());
    let sale = open_one(&mut rig);
    assert_eq!(sale.script, old().receive_script(sale.index).unwrap());
    // Nothing waits to be answered twice.
    assert_eq!(
        rig.node.switch_different().unwrap_err(),
        SwitchStepRefused::NothingWaits
    );
    assert_eq!(
        rig.node.switch_same().unwrap_err(),
        SwitchStepRefused::NothingWaits
    );
    // Sent again, it waits again, and a second refusal writes no second
    // record.
    rig.node.take_switch(&good().to_bytes()).unwrap();
    rig.node.switch_different().unwrap();
    assert_eq!(switched_records(dir.path()).len(), 1);
}

#[test]
fn cr_260_a_restart_after_different_does_not_show_the_switch_again() {
    let dir = Scratch::new();
    {
        let mut rig = fresh_rig(dir.path());
        rig.node.take_switch(&good().to_bytes()).unwrap();
        rig.node.switch_different().unwrap();
    }
    // The file was removed after the record; put it back as a stop between
    // the two would leave it.
    DirStore::open(dir.path())
        .unwrap()
        .put_switch(&good().to_bytes())
        .unwrap();
    let mut rig = fresh_rig(dir.path());
    assert!(rig.node.pending_switch().is_none());
    assert!(!switch_path(dir.path()).exists());
}

// CR-261.

/// The switch is waiting on a node with some sales; "Same address" is said.
/// Returns the rig and the record the app is to keep.
fn confirmed(dir: &std::path::Path, sales: usize) -> (Rig, Vec<u8>) {
    let mut rig = fresh_rig(dir);
    for _ in 0..sales {
        open_one(&mut rig);
    }
    rig.node.take_switch(&good().to_bytes()).unwrap();
    let bytes = rig.node.switch_same().unwrap();
    (rig, bytes)
}

#[test]
fn cr_261_same_address_writes_the_book_first_and_hands_the_app_the_new_record() {
    let dir = Scratch::new();
    let (mut rig, bytes) = confirmed(dir.path(), 3);
    // Step one: the book, version 2, the old wallet's ranges closed.
    let ledger = rig.node.counter().range_ledger().unwrap();
    assert_eq!(ledger.closed().len(), 1);
    assert_eq!(ledger.closed()[0].next, 3);
    assert_eq!(ledger.closed()[0].wallet, old().checksum32());
    assert_eq!(ledger.switched_to(), Some(new().checksum32()));
    let file = std::fs::read(dir.path().join("ranges.bin")).unwrap();
    assert_eq!(&file[..5], b"OBM;\x02");
    assert_eq!(file, ledger.to_bytes());
    // Step two is the app's: the record as it was with the new wallet
    // pinned and the flag cleared.
    assert_eq!(rig.node.record_update(), Some(&bytes[..]));
    let kept = OwnPairingRecord::from_bytes(&bytes).unwrap();
    assert!(kept.wallet == new());
    assert!(!kept.starter);
    assert_eq!(bytes[4], 1);
    let was = starter_record();
    assert!(kept.pairing == was.pairing);
    assert!(kept.settings == was.settings);
    assert!(kept.first_range == was.first_range);
    assert!(kept.identity_secret == was.identity_secret);
    assert!(kept.check_switched(&POLICY, &file).is_ok());
    // Steps three and four have not been done.
    assert!(switched_records(dir.path()).is_empty());
    assert!(switch_path(dir.path()).exists());
    assert!(rig.node.pending_switch().is_none());
    assert!(!rig.node.is_starter());
    // The app kept it: step three, then step four.
    rig.node.switch_record_kept().unwrap();
    let records = switched_records(dir.path());
    assert_eq!(records.len(), 1);
    assert!(records[0].switched && records[0].verify(&id(3).device_id()));
    assert_eq!(records[0].wallet, new().checksum32());
    assert!(!switch_path(dir.path()).exists());
    assert!(rig.node.record_update().is_none());
}

#[test]
fn cr_261_the_next_sale_is_the_new_wallets_and_a_sale_opened_before_ends_as_it_would() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    let before = open_one(&mut rig);
    rig.node.take_switch(&good().to_bytes()).unwrap();
    rig.node.switch_same().unwrap();
    // The sale already showing an address of the old wallet keeps it and
    // can be cancelled as before.
    assert_eq!(
        rig.node.counter().sale(before.index).unwrap().script(),
        &before.script
    );
    let after = open_one(&mut rig);
    assert_eq!(after.index, 1_000);
    assert_eq!(after.script, new().receive_script(1_000).unwrap());
    assert_ne!(after.script, before.script);
    // The old script stays watched from the closed range.
    assert!(rig
        .node
        .counter()
        .watched_scripts()
        .contains(&before.script));
    assert!(rig.node.counter().watched_scripts().contains(&after.script));
}

#[test]
fn cr_261_a_wrong_pin_is_the_apps_and_signs_nothing_so_nothing_is_written_before_it() {
    // The library checks no PIN (CR-84): until `switch_same` is called,
    // nothing about the ranges, the record or the file changes.
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    rig.node.take_switch(&good().to_bytes()).unwrap();
    let ledger = std::fs::read(dir.path().join("ranges.bin")).ok();
    assert!(rig.node.pending_switch().is_some());
    assert_eq!(std::fs::read(dir.path().join("ranges.bin")).ok(), ledger);
    assert!(rig.node.record_update().is_none());
    assert!(switched_records(dir.path()).is_empty());
}

#[test]
fn cr_261_same_address_needs_the_record_and_a_waiting_switch() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    assert_eq!(
        rig.node.switch_same().unwrap_err(),
        SwitchStepRefused::NothingWaits
    );
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config_for(dir.path(), &time, &starter_record());
    config.own_record = None;
    // Without the record the counter takes no switch at all, so a switch
    // that was kept before it lost the record waits and is refused here.
    {
        let mut rig = rig_for(dir.path(), &starter_record());
        rig.node.take_switch(&good().to_bytes()).unwrap();
    }
    let mut rig = start_with(config, time);
    assert!(rig.node.pending_switch().is_none());
}

#[test]
fn cr_261_a_stop_after_the_book_recovers_to_the_switched_state_at_start() {
    let dir = Scratch::new();
    let (rig, _lost) = confirmed(dir.path(), 2);
    // The app stopped before it kept the record: the node is gone and the
    // app still holds the old one.
    drop(rig);
    assert!(switch_path(dir.path()).exists());
    assert!(switched_records(dir.path()).is_empty());
    let mut rig = fresh_rig(dir.path());
    // The start finds the book ahead of the record and makes the record
    // again, from the switch.
    let update = rig
        .node
        .record_update()
        .expect("the record is made again")
        .to_vec();
    let kept = OwnPairingRecord::from_bytes(&update).unwrap();
    assert!(kept.wallet == new() && !kept.starter);
    assert!(rig.node.counter().own_wallet() == Some(&new()));
    assert!(!rig.node.is_starter());
    // Sales go to the new wallet at once.
    let sale = open_one(&mut rig);
    assert_eq!(sale.script, new().receive_script(sale.index).unwrap());
    assert_eq!(sale.index, 1_000);
    // The app keeps it; steps three and four follow.
    assert!(switched_records(dir.path()).is_empty());
    rig.node.switch_record_kept().unwrap();
    assert_eq!(switched_records(dir.path()).len(), 1);
    assert!(!switch_path(dir.path()).exists());
    drop(rig);
    // From the next start the counter is the switched one.
    let mut rig = rig_for(dir.path(), &kept);
    assert!(rig.node.record_update().is_none());
    assert!(rig.node.pending_switch().is_none());
    assert_eq!(switched_records(dir.path()).len(), 1);
}

#[test]
fn cr_261_a_stop_after_the_record_recovers_at_start_and_writes_the_owners_record() {
    let dir = Scratch::new();
    let (rig, bytes) = confirmed(dir.path(), 2);
    let kept = OwnPairingRecord::from_bytes(&bytes).unwrap();
    // The app kept the record; the node stopped before it wrote its own.
    drop(rig);
    assert!(switched_records(dir.path()).is_empty());
    let mut rig = rig_for(dir.path(), &kept);
    assert!(rig.node.record_update().is_none());
    assert_eq!(switched_records(dir.path()).len(), 1);
    assert!(!switch_path(dir.path()).exists());
    assert!(rig.node.counter().own_wallet() == Some(&new()));
    let sale = open_one(&mut rig);
    assert_eq!(sale.script, new().receive_script(sale.index).unwrap());
}

#[test]
fn cr_261_a_stop_after_the_owners_record_removes_the_file_and_writes_no_second_record() {
    let dir = Scratch::new();
    let (mut rig, bytes) = confirmed(dir.path(), 1);
    rig.node.switch_record_kept().unwrap();
    let kept = OwnPairingRecord::from_bytes(&bytes).unwrap();
    let records = chain(dir.path()).len();
    drop(rig);
    // The file is still there, as a stop before its removal leaves it.
    DirStore::open(dir.path())
        .unwrap()
        .put_switch(&good().to_bytes())
        .unwrap();
    let rig = rig_for(dir.path(), &kept);
    assert!(!switch_path(dir.path()).exists());
    assert_eq!(chain(dir.path()).len(), records);
    assert_eq!(switched_records(dir.path()).len(), 1);
    assert!(rig.node.counter().own_wallet() == Some(&new()));
}

#[test]
fn cr_261_a_book_for_another_wallet_with_no_switch_to_explain_it_is_a_fault() {
    let dir = Scratch::new();
    let (rig, _) = confirmed(dir.path(), 1);
    drop(rig);
    // The file is gone and the record is the old one: nothing explains the
    // book, so the counter does not start (CR-234).
    std::fs::remove_file(switch_path(dir.path())).unwrap();
    let err = try_rig_for(dir.path(), &starter_record()).err().unwrap();
    assert!(matches!(err, StartError::Switch(_)), "{err}");
    // A file for some other wallet does not explain it either.
    let other = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        old().checksum32(),
        old(),
        range_for(3, 1_000, 2_000, 1),
    );
    std::fs::write(switch_path(dir.path()), other.to_bytes()).unwrap();
    assert!(matches!(
        try_rig_for(dir.path(), &starter_record()).err().unwrap(),
        StartError::Switch(_)
    ));
}

// CR-262.

#[test]
fn cr_262_the_record_the_owner_reads_is_the_countersigned_hash_and_byte() {
    let dir = Scratch::new();
    let (mut rig, _) = confirmed(dir.path(), 1);
    rig.node.switch_record_kept().unwrap();
    let records = chain(dir.path());
    let last = records.last().unwrap();
    assert_eq!(last.kind, RecordKind::Setting);
    assert_eq!(last.body.len(), 98);
    assert_eq!(last.body[0], 37);
    assert_eq!(last.body[1], 1);
    assert_eq!(&last.body[2..34], &new().checksum32());
    let held = SwitchedHeld::decode(&last.body).unwrap();
    assert!(held.verify(&id(3).device_id()));
    assert!(!held.verify(&id(4).device_id()));
    // The chain with the record loads, and the record counts for nothing in
    // the books.
    drop(rig);
    let kept = OwnPairingRecord::from_bytes(&{
        let mut r = starter_record();
        r.wallet = new();
        r.starter = false;
        r.to_bytes()
    })
    .unwrap();
    let rig = rig_for(dir.path(), &kept);
    assert!(rig
        .node
        .counter()
        .switched_record(&new().checksum32())
        .is_some());
}

// CR-263.

#[test]
fn cr_263_a_counter_that_switched_refuses_every_switch_and_so_does_a_key_app_counter() {
    let dir = Scratch::new();
    let (mut rig, bytes) = confirmed(dir.path(), 1);
    rig.node.switch_record_kept().unwrap();
    // The same switch again, and a switch to a third wallet from the new one.
    let err = rig.node.take_switch(&good().to_bytes()).unwrap_err();
    assert_eq!(err, SwitchRefused::NotStarter);
    assert!(err.to_string().starts_with("CR-263"));
    let again = SwitchOwn::sign(
        &id(1),
        id(3).device_id(),
        new().checksum32(),
        old(),
        range_for(3, 9_000, 10_000, 1),
    );
    assert_eq!(
        rig.node.take_switch(&again.to_bytes()).unwrap_err(),
        SwitchRefused::NotStarter
    );
    assert!(!switch_path(dir.path()).exists());
    drop(rig);
    // After a restart with the record that was kept: the same.
    let kept = OwnPairingRecord::from_bytes(&bytes).unwrap();
    let mut rig = rig_for(dir.path(), &kept);
    assert_eq!(
        rig.node.take_switch(&again.to_bytes()).unwrap_err(),
        SwitchRefused::NotStarter
    );
    // A counter paired with the Key app has no own wallet to switch.
    let dir = Scratch::new();
    let mut key = start(dir.path());
    assert_eq!(
        key.node.take_switch(&good().to_bytes()).unwrap_err(),
        SwitchRefused::NotStarter
    );
}
