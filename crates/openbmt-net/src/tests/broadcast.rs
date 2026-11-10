//! NR-86 to NR-90: a transaction the app asks the manager to broadcast, the
//! evidence that the network took it, and a rescan.

use bitcoin::consensus::encode::serialize;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::{Transaction, Txid};

use super::kit::raw_frame;
use super::manager_kit::{filter_requests, pay, script, Net, Rig};
use crate::{
    BroadcastRefused, Output, PeerId, RelayState, MAX_BROADCAST_WEIGHT, MAX_HELD, NOT_SEEN_SECS,
};

fn net() -> Net {
    let mut n = Net::new();
    n.allow_broadcast = true;
    n
}

fn the_tx(tag: u32) -> Transaction {
    pay(&script(1), 10_000, tag)
}

/// The IDs announced with `inv` in these messages.
fn announced(messages: &[NetworkMessage]) -> Vec<Txid> {
    let mut out = Vec::new();
    for message in messages {
        if let NetworkMessage::Inv(list) = message {
            for item in list {
                if let Inventory::Transaction(txid) = item {
                    out.push(*txid);
                }
            }
        }
    }
    out
}

/// The transactions sent with `tx` in these messages.
fn sent_txs(messages: &[NetworkMessage]) -> Vec<Transaction> {
    messages
        .iter()
        .filter_map(|m| match m {
            NetworkMessage::Tx(tx) => Some(tx.clone()),
            _ => None,
        })
        .collect()
}

fn relayed(out: &[Output]) -> Vec<(usize, usize)> {
    out.iter()
        .filter_map(|o| match o {
            Output::TxRelayed {
                peers_asked,
                peers_fetched,
                ..
            } => Some((*peers_asked, *peers_fetched)),
            _ => None,
        })
        .collect()
}

fn states(out: &[Output]) -> Vec<RelayState> {
    out.iter()
        .filter_map(|o| match o {
            Output::TxState { state, .. } => Some(*state),
            _ => None,
        })
        .collect()
}

/// The outputs the manager keeps for the next call: a tick takes them.
fn flush(n: &mut Net) -> Vec<Output> {
    n.tick(0)
}

/// Let `secs` pass in steps of 100, answering every ping so that no peer is
/// dropped for NR-14. Returns the outputs and, for each peer, what it was
/// sent that is not a ping.
fn run(n: &mut Net, ids: &[PeerId], secs: u64) -> (Vec<Output>, Vec<Vec<NetworkMessage>>) {
    let mut outs = Vec::new();
    let mut got: Vec<Vec<NetworkMessage>> = vec![Vec::new(); ids.len()];
    let mut left = secs;
    while left > 0 {
        let step = left.min(100);
        left -= step;
        outs.extend(n.tick(step));
        for (at, id) in ids.iter().enumerate() {
            for message in n.sent(*id) {
                match message {
                    NetworkMessage::Ping(nonce) => {
                        n.feed(*id, NetworkMessage::Pong(nonce));
                    }
                    other => {
                        if let Some(list) = got.get_mut(at) {
                            list.push(other);
                        }
                    }
                }
            }
        }
    }
    (outs, got)
}

fn getdata_witness(txid: Txid) -> NetworkMessage {
    NetworkMessage::GetData(vec![Inventory::WitnessTransaction(txid)])
}

#[test]
fn nr_86_a_broadcast_is_announced_by_txid_to_half_of_the_ready_peers() {
    let mut n = net();
    let ids: Vec<PeerId> = (1..=4).map(|i| n.up(i)).collect();
    let tx = the_tx(1);
    let txid = n.m.broadcast(&serialize(&tx)).unwrap();
    assert_eq!(txid, tx.compute_txid());
    let got: Vec<usize> = ids.iter().map(|id| announced(&n.sent(*id)).len()).collect();
    assert_eq!(got, vec![1, 1, 0, 0]);
    let out = flush(&mut n);
    assert_eq!(relayed(&out), vec![(2, 0)]);
    assert_eq!(states(&out), vec![RelayState::Sent]);
}

#[test]
fn nr_86_one_ready_peer_is_asked_and_no_peer_is_left_to_listen() {
    let mut n = net();
    let a = n.up(1);
    n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    assert_eq!(announced(&n.sent(a)).len(), 1);
}

#[test]
fn nr_86_getdata_is_answered_with_the_exact_transaction() {
    let mut n = net();
    let a = n.up(1);
    let tx = the_tx(1);
    let txid = n.m.broadcast(&serialize(&tx)).unwrap();
    n.sent(a);
    flush(&mut n);
    let out = n.feed(a, getdata_witness(txid));
    let sent = sent_txs(&n.sent(a));
    assert_eq!(sent, vec![tx.clone()]);
    assert_eq!(serialize(&sent[0]), serialize(&tx));
    assert_eq!(relayed(&out), vec![(1, 1)]);
    // The plain form of the request is answered the same way.
    let b = n.up(2);
    n.sent(b);
    n.feed(
        b,
        NetworkMessage::GetData(vec![Inventory::Transaction(txid)]),
    );
    assert_eq!(sent_txs(&n.sent(b)), vec![tx]);
}

#[test]
fn nr_86_a_request_for_another_transaction_is_not_answered() {
    let mut n = net();
    let a = n.up(1);
    n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.sent(a);
    flush(&mut n);
    let other = the_tx(2).compute_txid();
    let out = n.feed(a, getdata_witness(other));
    assert!(out.is_empty());
    assert!(n.sent(a).is_empty());
}

#[test]
fn nr_86_a_peer_that_never_fetches_is_counted_as_asked_only() {
    let mut n = net();
    let a = n.up(1);
    let _b = n.up(2);
    n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.sent(a);
    let out = flush(&mut n);
    assert_eq!(relayed(&out), vec![(1, 0)]);
    // Time passes; nothing is invented.
    let out = n.tick(5);
    assert!(relayed(&out).is_empty());
}

#[test]
fn nr_86_a_peer_that_becomes_ready_is_announced_to() {
    let mut n = net();
    let a = n.up(1);
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.sent(a);
    flush(&mut n);
    let joined = n.join(2, super::manager_kit::plain(), 0);
    assert_eq!(announced(&joined.sent), vec![txid]);
    assert_eq!(relayed(&joined.out), vec![(2, 0)]);
}

#[test]
fn nr_86_with_no_ready_peer_the_transaction_waits_for_one() {
    let mut n = net();
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    assert_eq!(n.m.held(), vec![txid]);
    let joined = n.join(1, super::manager_kit::plain(), 0);
    assert_eq!(announced(&joined.sent), vec![txid]);
}

#[test]
fn nr_86_at_most_64_transactions_are_held_and_none_too_heavy() {
    let mut n = net();
    n.up(1);
    for tag in 0..MAX_HELD as u32 {
        n.m.broadcast(&serialize(&the_tx(tag))).unwrap();
    }
    let extra = n.m.broadcast(&serialize(&the_tx(1_000)));
    assert_eq!(extra, Err(BroadcastRefused::Full));
    // The same transaction again is not a new one.
    assert!(n.m.broadcast(&serialize(&the_tx(0))).is_ok());
    assert_eq!(n.m.held().len(), MAX_HELD);

    let mut heavy = the_tx(5);
    heavy.output.clear();
    for _ in 0..4_000 {
        heavy.output.push(heavy_output());
    }
    let weight = heavy.weight().to_wu();
    assert!(weight > MAX_BROADCAST_WEIGHT);
    let mut m = net();
    assert_eq!(
        m.m.broadcast(&serialize(&heavy)),
        Err(BroadcastRefused::TooHeavy { weight })
    );
    assert_eq!(m.m.broadcast(&[1, 2, 3]), Err(BroadcastRefused::Malformed));
}

fn heavy_output() -> bitcoin::TxOut {
    bitcoin::TxOut {
        value: bitcoin::Amount::from_sat(1_000),
        script_pubkey: script(9),
    }
}

#[test]
fn nr_86_a_forgotten_transaction_is_neither_announced_nor_served() {
    let mut n = net();
    let a = n.up(1);
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.sent(a);
    n.m.forget(&txid);
    assert!(n.m.held().is_empty());
    n.feed(a, getdata_witness(txid));
    assert!(n.sent(a).is_empty());
    let joined = n.join(2, super::manager_kit::plain(), 0);
    assert!(announced(&joined.sent).is_empty());
    let (_, got) = run(&mut n, &[a], NOT_SEEN_SECS);
    assert!(announced(&got[0]).is_empty());
}

#[test]
fn nr_87_two_groups_that_were_not_asked_count_as_seen_back() {
    let mut n = net();
    let a = n.up(1);
    let b = n.up(2);
    let c = n.up(3);
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.sent(a);
    flush(&mut n);
    // The peer that was asked announcing it back counts for nothing.
    let out = n.inv_txs(a, &[txid]);
    assert!(states(&out).is_empty());
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
    // One group that was not asked, twice.
    let out = n.inv_txs(b, &[txid]);
    assert!(states(&out).is_empty());
    n.inv_txs(b, &[txid]);
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
    // A second group.
    let out = n.inv_txs(c, &[txid]);
    assert_eq!(states(&out), vec![RelayState::SeenBack { groups: 2 }]);
    // The announcement back is never asked for: no getdata went out.
    assert!(n.sent(b).is_empty());
    assert!(n.sent(c).is_empty());
}

#[test]
fn nr_87_a_state_is_reported_once() {
    let mut n = net();
    n.up(1);
    let b = n.up(2);
    let c = n.up(3);
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.inv_txs(b, &[txid]);
    let first = n.inv_txs(c, &[txid]);
    assert_eq!(states(&first).len(), 1);
    let again = n.inv_txs(c, &[txid]);
    assert!(states(&again).is_empty());
}

#[test]
fn nr_88_a_transaction_not_seen_back_in_ten_minutes_is_reported_and_announced_again() {
    let mut n = net();
    let ids: Vec<PeerId> = (1..=3).map(|i| n.up(i)).collect();
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    for id in &ids {
        n.sent(*id);
    }
    flush(&mut n);
    let (outs, got) = run(&mut n, &ids, NOT_SEEN_SECS - 100);
    assert!(states(&outs).is_empty());
    assert!(got.iter().all(|g| announced(g).is_empty()));
    let (outs, got) = run(&mut n, &ids, 100);
    assert_eq!(states(&outs), vec![RelayState::NotSeen]);
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::NotSeen));
    for g in &got {
        assert_eq!(announced(g), vec![txid]);
    }
    // It stays held, and is announced again after another ten minutes.
    let (outs, got) = run(&mut n, &ids, NOT_SEEN_SECS);
    assert!(states(&outs).is_empty());
    for g in &got {
        assert_eq!(announced(g), vec![txid]);
    }
    assert_eq!(n.m.held(), vec![txid]);
}

#[test]
fn nr_88_a_transaction_held_before_the_first_time_is_given_waits_its_ten_minutes() {
    // A node that starts and sends what it still holds: the manager has had
    // no time yet, and that must not count as ten minutes long gone.
    let mut n = net();
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    let ids: Vec<PeerId> = (1..=3).map(|i| n.up(i)).collect();
    flush(&mut n);
    let (outs, _) = run(&mut n, &ids, NOT_SEEN_SECS - 100);
    assert!(states(&outs).is_empty(), "{:?}", states(&outs));
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
    let (outs, _) = run(&mut n, &ids, 200);
    assert_eq!(states(&outs), vec![RelayState::NotSeen]);
}

#[test]
fn nr_88_a_transaction_seen_back_is_not_reported_as_not_seen() {
    let mut n = net();
    n.up(1);
    let b = n.up(2);
    let c = n.up(3);
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.inv_txs(b, &[txid]);
    n.inv_txs(c, &[txid]);
    let (outs, _) = run(&mut n, &[PeerId(1), b, c], NOT_SEEN_SECS);
    assert!(states(&outs)
        .iter()
        .all(|s| matches!(s, RelayState::SeenBack { .. })));
    assert_eq!(
        n.m.relay_state(&txid),
        Some(RelayState::SeenBack { groups: 2 })
    );
}

#[test]
fn nr_89_a_reject_or_silence_from_a_peer_changes_nothing() {
    let mut n = net();
    let a = n.up(1);
    let b = n.up(2);
    let txid = n.m.broadcast(&serialize(&the_tx(1))).unwrap();
    n.sent(a);
    flush(&mut n);
    // A `reject` is not read: it is skipped, as any unknown command is.
    let bytes = raw_frame("reject", &[0u8; 40]);
    let out = n.feed_bytes(a, &bytes);
    assert!(out.is_empty());
    // A peer that says it does not have the transaction changes nothing.
    let out = n.feed(
        b,
        NetworkMessage::NotFound(vec![Inventory::WitnessTransaction(txid)]),
    );
    assert!(out.is_empty());
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
    assert_eq!(n.m.held(), vec![txid]);
}

#[test]
fn nr_90_a_rescan_checks_the_blocks_above_the_height_again() {
    let mut r = Rig::new(vec![vec![]; 3]);
    for h in 1..=3 {
        r.both_honest(h);
    }
    r.n.sent(r.a);
    r.n.sent(r.b);
    assert_eq!(r.n.m.checked_height(), 3);
    // A script is watched that was not when the blocks were checked.
    assert!(r.n.m.watch_script(script(9)));
    r.n.m.rescan_from(1);
    r.n.tick(0);
    assert_eq!(r.n.m.checked_height(), 1);
    let want: Vec<_> = (2..=3).map(|h| (h as u32, r.hash(h))).collect();
    assert_eq!(filter_requests(&r.n.sent(r.a)), want);
    assert_eq!(filter_requests(&r.n.sent(r.b)), want);
    for h in 2..=3 {
        r.both_honest(h);
    }
    assert_eq!(r.n.m.checked_height(), 3);
}

#[test]
fn nr_90_a_rescan_gives_up_work_in_progress() {
    let mut r = Rig::new(vec![vec![]; 3]);
    r.both_honest(1);
    r.n.sent(r.a);
    r.n.sent(r.b);
    let before = r.n.m.checked_height();
    r.n.m.rescan_from(0);
    r.n.tick(0);
    assert!(r.n.m.checked_height() <= before);
    // The late filters for the old work are not unasked data.
    let f2 = super::manager_kit::filter_of(r.at(2));
    let hash = r.hash(2);
    let out = r.n.filter(r.a, hash, f2);
    assert!(super::manager_kit::disconnects(&out).is_empty());
}
