//! LK-79, SS-32, SS-36, SS-38: the starter line the owner node makes. The
//! node of a shop on the starter wallet is started on fake drivers, its
//! coins come from blocks the test mines, and the line goes through a link
//! on a relay in memory to a courier that stands in for a counter.
//!
//! TEST ONLY, NEVER USE: the keys come from fixed bytes.

use bitcoin::consensus::serialize;
use openbmt_core::records::DeviceList;
use openbmt_core::starter_line::StarterLine;
use openbmt_courier::{Courier, FirstStart, MemoryPost, MemoryRelay};
use openbmt_link::{Kind, Role};

use super::former::{former_setup, pay};
use super::own::{counter, own_receive};
use super::*;
use crate::tests::T0;
use crate::{OwnerLink, OwnerLinkConfig, STARTER_LINE_FILE};

fn owner() -> DeviceId {
    id(0x11).device_id()
}

struct Shop {
    link: OwnerLink<MemoryPost>,
    counter_side: Courier<MemoryPost>,
    _link_dir: Dir,
    _counter_dir: Dir,
}

fn shop_link() -> Shop {
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let (link_dir, counter_dir) = (Dir::new(), Dir::new());
    let courier = |who: openbmt_core::identity::Identity, role, peer, dir: &Dir| {
        Courier::start_with(
            relay.post(),
            who,
            role,
            [peer].into_iter().collect(),
            dir.path(),
            FirstStart::Yes,
        )
        .unwrap()
    };
    let owner_side = courier(id(0x11), Role::Owner, (counter(), Role::Counter), &link_dir);
    let mut counter_side = courier(
        id(0x31),
        Role::Counter,
        (owner(), Role::Owner),
        &counter_dir,
    );
    counter_side.poll().unwrap();
    let link = OwnerLink::new(
        owner_side,
        link_dir.path(),
        DeviceList::new([counter(), owner()]),
        vec![counter()],
        OwnerLinkConfig::default(),
    )
    .unwrap();
    Shop {
        link,
        counter_side,
        _link_dir: link_dir,
        _counter_dir: counter_dir,
    }
}

impl Shop {
    /// The starter lines the counter has been sent since it last looked.
    fn lines(&mut self) -> Vec<StarterLine> {
        self.counter_side
            .poll()
            .unwrap()
            .iter()
            .filter(|r| r.kind == Kind::Starter)
            .map(|r| StarterLine::from_bytes(&r.content).unwrap())
            .collect()
    }
}

/// A node with the owner's own wallet and 9,000 sats in it, and a link.
fn funded(dir: &Dir) -> (Rig, Shop, Transaction) {
    let mut rig = start(dir, &former_setup()).unwrap();
    let coin = pay(&mut rig, 8, &own_receive(0), 9_000);
    (rig, shop_link(), coin)
}

#[test]
fn lk_79_no_line_is_made_before_the_app_gives_the_limit_and_the_stop_setting() {
    let dir = Dir::new();
    let (mut rig, mut shop, _) = funded(&dir);
    let step = rig.node.starter_line_step(&mut shop.link, T0);
    assert_eq!(step.number, None);
    assert!(shop.lines().is_empty());
    assert!(!dir.path().join(STARTER_LINE_FILE).exists());
}

#[test]
fn lk_79_the_line_says_what_the_wallet_holds_the_limit_and_the_stop_signed_by_the_owner_root() {
    let dir = Dir::new();
    let (mut rig, mut shop, _) = funded(&dir);
    rig.node.set_starter_line(20_000, true);
    let step = rig.node.starter_line_step(&mut shop.link, T0);
    assert_eq!((step.sent, step.failed), (1, 0));
    let lines = shop.lines();
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.verify(&owner()));
    assert_eq!(step.number, Some(line.number));
    assert_eq!(line.time, T0);
    assert_eq!(line.held_sats, 9_000);
    assert_eq!(line.limit_sats, 20_000);
    assert!(line.stop);
    assert!(line.payments.is_empty());
    // The file holds the frame and nothing else.
    assert_eq!(
        std::fs::read(dir.path().join(STARTER_LINE_FILE)).unwrap(),
        line.to_bytes()
    );
}

#[test]
fn lk_79_a_new_line_when_anything_changes_and_at_least_once_each_heartbeat_interval() {
    let dir = Dir::new();
    let (mut rig, mut shop, _) = funded(&dir);
    rig.node.set_starter_line(20_000, false);
    rig.node.starter_line_step(&mut shop.link, T0);
    let first = shop.lines().remove(0);
    // Nothing changed and the interval is not up: nothing is sent.
    let every = shop.link.heartbeat_every_secs();
    assert_eq!(every, 20);
    assert_eq!(
        rig.node
            .starter_line_step(&mut shop.link, T0 + every - 1)
            .number,
        None
    );
    assert!(shop.lines().is_empty());
    // The interval is up: a line with the same news, a higher number and the
    // time of the step.
    rig.node.starter_line_step(&mut shop.link, T0 + every);
    let again = shop.lines().remove(0);
    assert!(again.number > first.number);
    assert_eq!(again.time, T0 + every);
    assert_eq!(
        (again.held_sats, again.limit_sats, again.stop),
        (first.held_sats, first.limit_sats, first.stop)
    );
    // The app changes the limit and the stop: the next step sends at once.
    rig.node.set_starter_line(5_000, true);
    rig.node.starter_line_step(&mut shop.link, T0 + every + 1);
    let changed = shop.lines().remove(0);
    assert!(changed.number > again.number);
    assert_eq!((changed.limit_sats, changed.stop), (5_000, true));
    // More arrives in the wallet: the next step sends at once.
    pay(&mut rig, 9, &own_receive(1), 4_000);
    rig.node.starter_line_step(&mut shop.link, T0 + every + 2);
    let more = shop.lines().remove(0);
    assert_eq!(more.held_sats, 13_000);
    assert!(more.payments.is_empty());
}

#[test]
fn ss_32_a_spend_a_block_holds_is_a_payment_that_the_owner_phone_did_not_make() {
    let dir = Dir::new();
    let (mut rig, mut shop, coin) = funded(&dir);
    rig.node.set_starter_line(20_000, false);
    rig.node.starter_line_step(&mut shop.link, T0);
    shop.lines();
    // Someone else spends the coin: 6,000 go out and 2,500 come back.
    let theft = tx_from(
        OutPoint::new(coin.compute_txid(), 0),
        &[(&external(7), 6_000), (&own_receive(3), 2_500)],
    );
    rig.mine(std::slice::from_ref(&theft));
    // The wallet's own count follows the block (SS-36)...
    assert_eq!(rig.node.starter_held(), Some(2_500));
    // ...and the next step says who it was.
    rig.node.starter_line_step(&mut shop.link, T0 + 5);
    let line = shop.lines().remove(0);
    assert_eq!(line.held_sats, 2_500);
    assert_eq!(line.payments.len(), 1);
    let p = line.payments[0];
    assert_eq!(p.txid, theft.compute_txid().to_byte_array());
    // What left the wallet: what the transaction took less what came back.
    assert_eq!(p.sats, 6_500);
    assert_eq!(p.time, T0 + 5);
    assert!(!p.by_owner);
}

#[test]
fn ss_32_a_payment_the_owner_phone_made_is_seen_at_once_and_says_so() {
    let dir = Dir::new();
    let (mut rig, mut shop, coin) = funded(&dir);
    rig.node.set_starter_line(20_000, false);
    rig.node.starter_line_step(&mut shop.link, T0);
    shop.lines();
    // The owner phone takes a payment of its own: 7,000 out and 1,500 back.
    let mine = tx_from(
        OutPoint::new(coin.compute_txid(), 0),
        &[(&external(8), 7_000), (&own_receive(4), 1_500)],
    );
    let txid = rig.node.take_starter(&serialize(&mine), &[]).unwrap();
    // Held as the wallet's own change before a block holds it (SS-36).
    rig.node.starter_line_step(&mut shop.link, T0 + 3);
    let line = shop.lines().remove(0);
    assert_eq!(line.payments.len(), 1);
    assert_eq!(line.payments[0].txid, txid.to_byte_array());
    assert_eq!(line.payments[0].sats, 7_500);
    assert!(line.payments[0].by_owner);
    // A block holds it: still one payment, still the owner's, same time.
    rig.mine(std::slice::from_ref(&mine));
    rig.node.starter_line_step(&mut shop.link, T0 + 40);
    let after = shop.lines().remove(0);
    assert_eq!(after.payments.len(), 1);
    assert!(after.payments[0].by_owner);
    assert_eq!(after.payments[0].time, T0 + 3);
}

#[test]
fn ss_32_the_line_holds_the_last_16_payments_and_they_survive_a_restart() {
    let dir = Dir::new();
    let mut rig = start(&dir, &former_setup()).unwrap();
    let mut shop = shop_link();
    let mut coins: Vec<Transaction> = (0..18u8)
        .map(|n| pay(&mut rig, 20 + n, &own_receive(u32::from(n)), 10_000))
        .collect();
    rig.node.set_starter_line(1, false);
    rig.node.starter_line_step(&mut shop.link, T0);
    shop.lines();
    // Each coin is spent by someone else, one at a time.
    let mut txids = Vec::new();
    for (n, coin) in coins.drain(..).enumerate() {
        let spend = tx_from(
            OutPoint::new(coin.compute_txid(), 0),
            &[(&external(9), 9_000)],
        );
        rig.mine(std::slice::from_ref(&spend));
        rig.node
            .starter_line_step(&mut shop.link, T0 + 1 + n as u64);
        txids.push(spend.compute_txid().to_byte_array());
    }
    let line = shop.lines().pop().unwrap();
    assert_eq!(line.payments.len(), 16);
    assert_eq!(
        line.payments.iter().map(|p| p.txid).collect::<Vec<_>>(),
        txids[2..].to_vec()
    );
    // A restart goes on from the file: the payments, and a number above it.
    drop(rig);
    let mut rig = start_at(&dir, &former_setup(), Vec::new()).unwrap();
    rig.node.set_starter_line(1, false);
    rig.node.starter_line_step(&mut shop.link, T0 + 100);
    let back = shop.lines().pop().unwrap();
    assert_eq!(back.payments, line.payments);
    assert!(back.number > line.number);
}

#[test]
fn lk_79_the_number_rises_even_when_the_file_is_lost() {
    let dir = Dir::new();
    let (mut rig, mut shop, _) = funded(&dir);
    rig.node.set_starter_line(1, false);
    rig.node.starter_line_step(&mut shop.link, T0);
    let first = shop.lines().remove(0);
    drop(rig);
    std::fs::remove_file(dir.path().join(STARTER_LINE_FILE)).unwrap();
    let mut rig = start_at(&dir, &former_setup(), Vec::new()).unwrap();
    rig.node.set_starter_line(1, false);
    rig.node.starter_line_step(&mut shop.link, T0 + 60);
    assert!(shop.lines().remove(0).number > first.number);
    // A file that is not the owner's line is not used.
    drop(rig);
    std::fs::write(dir.path().join(STARTER_LINE_FILE), b"junk").unwrap();
    let mut rig = start_at(&dir, &former_setup(), Vec::new()).unwrap();
    rig.node.set_starter_line(1, false);
    rig.node.starter_line_step(&mut shop.link, T0 + 120);
    assert_eq!(shop.lines().len(), 1);
    assert!(rig
        .node
        .watcher_mut()
        .take_notes()
        .iter()
        .any(|n| n.contains("LK-79")));
}

#[test]
fn ss_32_blocks_from_before_the_setting_and_from_a_rescan_add_no_payment() {
    let dir = Dir::new();
    let mut rig = start(&dir, &former_setup()).unwrap();
    let mut shop = shop_link();
    let coin = pay(&mut rig, 8, &own_receive(0), 9_000);
    let old = tx_from(
        OutPoint::new(coin.compute_txid(), 0),
        &[(&external(7), 8_000), (&own_receive(5), 900)],
    );
    // A spend in a block the node took before the app gave the setting.
    rig.mine(std::slice::from_ref(&old));
    rig.node.set_starter_line(1, false);
    // The node reads its old blocks again: no payment of the old spend.
    rig.rescan();
    rig.node.starter_line_step(&mut shop.link, T0);
    let line = shop.lines().remove(0);
    assert!(line.payments.is_empty());
    assert_eq!(line.held_sats, 900);
}
