//! Switching a counter from the starter wallet through `dispatch`
//! (`docs/COUNTER-RULES.md` CR-256 to CR-263): a counter paired with an offer
//! marked as a starter wallet is started and linked to a relay; the owner
//! phone, which the test plays with the functions of `openbmt_core` and a
//! courier, sends the switch; the manager checks the address and says "Same
//! address" (the PIN is the app's: the calls are made after it); the app keeps
//! the new pairing record; the owner phone reads the counter's record.

use std::net::SocketAddr;

use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::SwitchOwn;
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::records::RecordKind;
use openbmt_core::relay::{RelayHint, RelayRoute};
use openbmt_counter::SwitchedHeld;
use openbmt_counter_node::OwnPairingRecord;
use openbmt_link::{Kind, MemoryStore, Relay, RelayConfig};

use super::confirm::{
    config, owner, owner_records, paired_with, relay_for, relay_key, step, world, World, FLOOR,
    OP_PUMP, SINGLE,
};
use super::kit::{peers_event, Rig, FIVE_DOLLARS, RATE, T0};
use super::wire::{Rd, W};
use crate::StartConfig;

const OP_OPEN: u16 = 11;
const OP_STOP: u16 = 2;
const OP_PENDING_SWITCH: u16 = 200;
const OP_SWITCH_SAME: u16 = 201;
const OP_SWITCH_RECORD_KEPT: u16 = 202;
const OP_SWITCH_DIFFERENT: u16 = 203;
/// Seconds between two sales (CR-04).
const GAP: u64 = 31;

const NEW: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";

/// What `pending_switch` returns, as the app reads it.
struct State {
    pending: Option<(String, u8, u64, u64)>,
    notice: Option<String>,
    record_update: Option<Vec<u8>>,
}

fn state(rig: &Rig, handle: u64, now: u64) -> State {
    let response = rig.call(OP_PENDING_SWITCH, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let pending = r.opt(|r| (r.string(), r.u8(), r.u64(), r.u64()));
    let notice = r.opt(|r| r.words().heading);
    let record_update = r.opt(Rd::bytes);
    r.done();
    State {
        pending,
        notice,
        record_update,
    }
}

fn open_sale(rig: &Rig, handle: u64, now: u64) -> super::wire::Opened {
    let response = rig.call(
        OP_OPEN,
        handle,
        &W::new()
            .open(now, FIVE_DOLLARS, "USD", RATE, "test-fixed")
            .0,
    );
    Rd::ok(&response).opened()
}

fn old() -> PinnedWallet {
    PinnedWallet::from_text(SINGLE).unwrap()
}

fn new() -> PinnedWallet {
    PinnedWallet::from_text(NEW).unwrap()
}

/// A counter paired with a starter wallet and started on a relay, and the
/// owner's courier.
struct Shop {
    rig: Rig,
    w: World,
    handle: u64,
    record: Vec<u8>,
    counter: openbmt_core::identity::DeviceId,
}

fn shop() -> Shop {
    let rig = Rig::new();
    let placeholder: SocketAddr = "127.0.0.1:9".parse().unwrap();
    let members = Relay::new(relay_key(), RelayConfig::default(), MemoryStore::new()).unwrap();
    let (record, counter) = paired_with(&rig, placeholder, &members, true);
    let (server, courier, dir) = relay_for(&counter);
    let mut record = OwnPairingRecord::from_bytes(&record).unwrap();
    // The offer was marked: the record carries the flag (CR-256).
    assert!(record.starter);
    record.relay = Some(
        RelayHint::new(
            RelayRoute::Direct(server.local_addr()),
            relay_key().device_id().to_bytes(),
        )
        .unwrap(),
    );
    let record = record.to_bytes();
    let w = world(Some((server, courier, dir)));
    let handle = rig.start_with(&config(&rig, &record));
    rig.send(vec![peers_event()]);
    Rd::ok(&rig.call(OP_PUMP, handle, &W::new().u64(T0 + 100).u32(50).0));
    assert_eq!(step(&rig, handle, T0 + 100).0, FLOOR + 1);
    Shop {
        rig,
        w,
        handle,
        record,
        counter,
    }
}

impl Shop {
    fn switch(&self, first: (u32, u32)) -> SwitchOwn {
        let range = SignedRange::sign(
            &owner(),
            IndexRange::new(self.counter, first.0, first.1).unwrap(),
        )
        .unwrap();
        SwitchOwn::sign(&owner(), self.counter, old().checksum32(), new(), range)
    }

    fn send(&mut self, sw: &SwitchOwn, now: u64) {
        self.w
            .owner
            .send(&self.counter, Kind::Switch, &sw.to_bytes())
            .unwrap();
        step(&self.rig, self.handle, now);
    }

    fn restart(&mut self, record: &[u8], now: u64) {
        let _ = self.rig.call(OP_STOP, self.handle, &[]);
        self.rig.arm();
        let cfg: StartConfig = config(&self.rig, record);
        self.handle = self.rig.start_with(&cfg);
        self.rig.send(vec![peers_event()]);
        Rd::ok(
            &self
                .rig
                .call(OP_PUMP, self.handle, &W::new().u64(now).u32(50).0),
        );
    }
}

#[test]
fn cr_256_to_cr_262_the_manager_confirms_the_new_wallet_and_the_owner_reads_the_record() {
    let mut s = shop();
    // Until the switch is confirmed the counter pays into the starter
    // wallet.
    let before = open_sale(&s.rig, s.handle, T0 + 200);
    assert_eq!(
        before.payment.address,
        old().receive_address(before.index).unwrap().to_string()
    );
    assert!(state(&s.rig, s.handle, T0 + 201).pending.is_none());

    // The owner phone sends the switch (LK-78); the counter shows the new
    // wallet's first address and the kind of wallet (CR-260).
    let sw = s.switch((1_000, 2_000));
    s.send(&sw, T0 + 210);
    assert!(s.rig.dir().join("switch.bin").exists());
    let st = state(&s.rig, s.handle, T0 + 211);
    let (address, kind, arrived, expires_at) = st.pending.expect("a switch waits");
    assert_eq!(address, new().receive_address(0).unwrap().to_string());
    assert_eq!(kind, 2, "two of three");
    assert_eq!((arrived, expires_at), (T0 + 210, T0 + 210 + 7 * 86_400));
    assert!(st.notice.is_none() && st.record_update.is_none());
    // A sale opened meanwhile still goes to the starter wallet.
    let during = open_sale(&s.rig, s.handle, T0 + 200 + GAP);
    assert_eq!(
        during.payment.address,
        old().receive_address(during.index).unwrap().to_string()
    );

    // "Same address" after the PIN: the book is written, the app gets the new
    // record to keep.
    let response = s
        .rig
        .call(OP_SWITCH_SAME, s.handle, &W::new().u64(T0 + 220).0);
    let mut r = Rd::ok(&response);
    let words = r.words();
    let record = r.bytes();
    r.done();
    assert_eq!(words.heading, "Switched to the new wallet");
    let kept = OwnPairingRecord::from_bytes(&record).unwrap();
    assert!(kept.wallet == new() && !kept.starter);
    let st = state(&s.rig, s.handle, T0 + 221);
    assert!(st.pending.is_none());
    assert_eq!(st.record_update.as_deref(), Some(&record[..]));
    // From here the sales go to the new wallet; the sale opened before keeps
    // its address.
    let after = open_sale(&s.rig, s.handle, T0 + 300);
    assert_eq!(after.index, 1_000);
    assert_eq!(
        after.payment.address,
        new().receive_address(1_000).unwrap().to_string()
    );
    // The app has sealed and kept the record.
    let response = s
        .rig
        .call(OP_SWITCH_RECORD_KEPT, s.handle, &W::new().u64(T0 + 310).0);
    Rd::ok(&response);
    assert!(state(&s.rig, s.handle, T0 + 311).record_update.is_none());
    assert!(!s.rig.dir().join("switch.bin").exists());
    // Nothing is left to keep or answer.
    let again = s
        .rig
        .call(OP_SWITCH_SAME, s.handle, &W::new().u64(T0 + 320).0);
    assert_eq!(Rd::error(&again).unwrap().rule, "CR-261");

    // The owner phone reads the record from the counter's records (CR-262).
    step(&s.rig, s.handle, T0 + 330);
    let records = owner_records(&mut s.w);
    let held = records
        .iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .find_map(|r| SwitchedHeld::decode(&r.body).ok())
        .expect("the record among the records the owner holds");
    assert!(held.switched);
    assert_eq!(held.wallet, new().checksum32());
    assert!(held.verify(&s.counter));

    // A start with the record the app kept: the folder's book proves the
    // switch, the counter pays into the new wallet, and takes no second
    // switch (CR-263).
    s.restart(&record, T0 + 390);
    assert!(state(&s.rig, s.handle, T0 + 400).record_update.is_none());
    let next = open_sale(&s.rig, s.handle, T0 + 500);
    assert_eq!(
        next.payment.address,
        new().receive_address(next.index).unwrap().to_string()
    );
    s.send(&s.switch((5_000, 6_000)), T0 + 510);
    assert!(state(&s.rig, s.handle, T0 + 511).pending.is_none());
    assert!(!s.rig.dir().join("switch.bin").exists());
    let _ = s.record.len();
}

#[test]
fn cr_260_different_drops_the_switch_and_the_owner_reads_the_refusal() {
    let mut s = shop();
    let sw = s.switch((1_000, 2_000));
    s.send(&sw, T0 + 210);
    assert!(state(&s.rig, s.handle, T0 + 211).pending.is_some());
    let response = s
        .rig
        .call(OP_SWITCH_DIFFERENT, s.handle, &W::new().u64(T0 + 220).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.words().heading, "New wallet not taken");
    r.done();
    assert!(state(&s.rig, s.handle, T0 + 221).pending.is_none());
    assert!(!s.rig.dir().join("switch.bin").exists());
    // The old wallet stays pinned.
    let sale = open_sale(&s.rig, s.handle, T0 + 300);
    assert_eq!(
        sale.payment.address,
        old().receive_address(sale.index).unwrap().to_string()
    );
    step(&s.rig, s.handle, T0 + 310);
    let held = owner_records(&mut s.w)
        .iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .find_map(|r| SwitchedHeld::decode(&r.body).ok())
        .expect("the refusal");
    assert!(!held.switched && held.verify(&s.counter));
    assert_eq!(held.wallet, new().checksum32());
    // Nothing waits to be answered again.
    let again = s
        .rig
        .call(OP_SWITCH_DIFFERENT, s.handle, &W::new().u64(T0 + 320).0);
    assert_eq!(Rd::error(&again).unwrap().rule, "CR-261");
}

#[test]
fn cr_261_a_stop_before_the_app_kept_the_record_is_finished_at_the_next_start() {
    let mut s = shop();
    let sw = s.switch((1_000, 2_000));
    s.send(&sw, T0 + 210);
    let response = s
        .rig
        .call(OP_SWITCH_SAME, s.handle, &W::new().u64(T0 + 220).0);
    let mut r = Rd::ok(&response);
    r.words();
    let record = r.bytes();
    r.done();
    // The app is gone before it kept the record: it starts with the old one.
    let old_record = s.record.clone();
    s.restart(&old_record, T0 + 390);
    let st = state(&s.rig, s.handle, T0 + 400);
    assert_eq!(st.record_update.as_deref(), Some(&record[..]));
    let sale = open_sale(&s.rig, s.handle, T0 + 410);
    assert_eq!(
        sale.payment.address,
        new().receive_address(sale.index).unwrap().to_string()
    );
    // It keeps the record this time.
    Rd::ok(
        &s.rig
            .call(OP_SWITCH_RECORD_KEPT, s.handle, &W::new().u64(T0 + 420).0),
    );
    assert!(state(&s.rig, s.handle, T0 + 421).record_update.is_none());
    s.restart(&record, T0 + 490);
    assert!(state(&s.rig, s.handle, T0 + 500).record_update.is_none());
    let _: Identity = owner();
}
