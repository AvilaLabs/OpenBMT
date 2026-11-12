//! NR-106 to NR-111: the shop's own node as the only peer, in the manager.

use bitcoin::consensus::encode::serialize;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{Block, Transaction, Txid, Witness};
use openbmt_watch::HeaderChain;
use std::net::{Ipv4Addr, SocketAddr};

use super::kit::peer_version;
use super::manager_kit::*;
use super::v2kit::{in_v2, mode, Mode, Wire};
use crate::manager::FILTER_SETTLE_SECS;
use crate::onion::onion_name;
use crate::{
    ConnectRefused, Output, OwnNode, PeerId, RelayState, TransportChoice, V2Entropy,
    NODE_ASK_AFTER_SECS, NODE_ASK_EVERY_SECS, NOT_SEEN_SECS,
};

const PORT: u16 = 8333;

fn node() -> OwnNode {
    OwnNode::onion(&onion_name(&[7; 32]), PORT).expect("a v3 name")
}

fn limited() -> ServiceFlags {
    ServiceFlags::NETWORK_LIMITED | ServiceFlags::WITNESS | ServiceFlags::COMPACT_FILTERS
}

/// A manager in the mode, with the script of the sale watched.
fn net() -> Net {
    let mut n = Net::new();
    n.m.set_node_only(true);
    assert!(n.m.watch_script(script(7)));
    n
}

/// Connect the own node as peer `id` and finish the handshake.
fn join_node(
    n: &mut Net,
    id: u8,
    services: ServiceFlags,
    start_height: i32,
) -> (PeerId, Vec<Output>, Vec<NetworkMessage>) {
    let id = PeerId(u64::from(id));
    let nonce = 0x3000 + id.0;
    let mut wire = Wire::for_mode(id.0 as u8);
    match mode() {
        Mode::V1 => {
            n.m.connected_node_only(id, &node(), nonce, n.now, TransportChoice::V1)
                .expect("connected")
        }
        Mode::V2 => {
            let entropy = V2Entropy::new([id.0 as u8 | 1; 32], [id.0 as u8 ^ 0x77; 32]);
            n.m.connected_node_only(id, &node(), nonce, n.now, TransportChoice::V2(entropy))
                .expect("connected");
            let reply = wire.answer(&n.m.outgoing(id));
            assert!(n.m.received(id, &reply, n.now).is_empty());
        }
    }
    n.wires.insert(id, wire);
    let first = n.sent(id);
    assert!(
        matches!(first[..], [NetworkMessage::Version(_)]),
        "{first:?}"
    );
    let mut version = peer_version();
    version.services = services;
    version.start_height = start_height;
    let mut stream = n.frame(id, NetworkMessage::Version(version));
    stream.extend(n.frame(id, NetworkMessage::Verack));
    let out = n.m.received(id, &stream, n.now);
    let sent = n.sent(id);
    (id, out, sent)
}

fn has(out: &[Output]) -> Vec<Txid> {
    out.iter()
        .filter_map(|o| match o {
            Output::OwnNodeHas { txid } => Some(*txid),
            _ => None,
        })
        .collect()
}

fn sightings(out: &[Output]) -> usize {
    out.iter()
        .filter(|o| matches!(o, Output::Sighting { .. }))
        .count()
}

/// Two blocks, the first paying the watched script; the node at height 2
/// delivers their headers and is asked for the filters.
fn synced(n: &mut Net) -> (PeerId, Vec<Block>) {
    let blocks = blocks_on(&genesis(), vec![vec![pay(&script(7), 5_000, 1)], vec![]], 1);
    let (id, _, _) = join_node(n, 1, filters(), 2);
    n.headers(id, &headers_of(&blocks));
    n.tick(FILTER_SETTLE_SECS);
    (id, blocks)
}

// NR-106.

#[test]
fn nr_106_the_own_node_is_the_one_peer_and_serves_headers() {
    let mut n = net();
    let (id, out, sent) = join_node(&mut n, 1, filters(), 2);
    assert!(matches!(
        out[..],
        [Output::OwnNodeReady { id: i, height: 2, .. }] if i == id
    ));
    let names: Vec<&str> = sent.iter().map(|m| m.cmd()).collect();
    assert_eq!(names, ["sendaddrv2", "verack", "sendheaders", "getheaders"]);
    // It is not one of the light client's peers, and there is one only.
    assert!(n.m.health_peers().is_empty());
    let again =
        n.m.connected_node_only(PeerId(9), &node(), 1, n.now, TransportChoice::V1);
    assert_eq!(again, Err(ConnectRefused::OwnNodeConnected));
    // Its headers build the chain.
    let blocks = blocks_on(&genesis(), vec![vec![], vec![]], 1);
    let out = n.headers(id, &headers_of(&blocks));
    assert_eq!(chain_out(&out).len(), 1);
    assert_eq!(n.m.chain().tip_height(), 2);
    let view = n.m.node_only_view().expect("the mode is on");
    assert!(view.ready && view.best_height == Some(2) && view.last_heard == Some(n.now));
    assert!(view.authenticated && view.unfit_since.is_none() && view.lacks.is_none());
}

#[test]
fn nr_106_one_filter_from_the_node_is_the_whole_check_and_a_match_fetches_the_block() {
    let mut n = net();
    let (id, blocks) = synced(&mut n);
    let sent = n.sent(id);
    let asked = filter_requests(&sent);
    assert!(asked.contains(&(1, blocks[0].block_hash())), "{asked:?}");
    assert!(asked.contains(&(2, blocks[1].block_hash())), "{asked:?}");
    // The block that does not match is finished by its one filter; the
    // other asks for the block.
    n.filter(id, blocks[1].block_hash(), filter_of(&blocks[1]));
    assert!(block_requests(&n.sent(id)).is_empty());
    n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
    assert_eq!(block_requests(&n.sent(id)), vec![blocks[0].block_hash()]);
    let out = n.block(id, &blocks[0]);
    assert_eq!(blocks_out(&out), vec![blocks[0].block_hash()]);
    assert_eq!(n.m.checked_height(), 2);
}

#[test]
fn nr_106_the_checks_of_the_peer_protocol_still_apply_to_what_it_sends() {
    // A header without its proof of work.
    let mut n = net();
    let (id, _, _) = join_node(&mut n, 1, filters(), 1);
    let out = n.headers(id, &[unmined(&genesis(), 1)]);
    assert_eq!(disconnects(&out), vec![(id, true)]);
    assert!(reason(&out).starts_with("NR-25: "), "{}", reason(&out));
    // A block that does not match its header.
    let mut n = net();
    let (id, blocks) = synced(&mut n);
    n.sent(id);
    n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
    n.sent(id);
    let mut bad = blocks[0].clone();
    bad.txdata.push(coinbase(99));
    let out = n.block(id, &bad);
    assert_eq!(disconnects(&out), vec![(id, true)]);
    // A filter nobody asked for.
    let mut n = net();
    let (id, _, _) = join_node(&mut n, 1, filters(), 0);
    let out = n.filter(id, genesis().block_hash(), vec![0]);
    assert_eq!(disconnects(&out), vec![(id, true)]);
}

#[test]
fn nr_106_a_node_that_claims_a_height_and_sends_no_headers_is_dropped_as_any_peer() {
    let mut n = net();
    let (id, _, _) = join_node(&mut n, 1, filters(), 50);
    let out = n.tick(crate::manager::CLAIM_SECS - 1);
    assert!(disconnects(&out).is_empty(), "{out:?}");
    let out = n.tick(1);
    assert_eq!(disconnects(&out), vec![(id, true)]);
    assert!(reason(&out).starts_with("NR-33: "));
}

#[test]
fn nr_106_a_transaction_the_node_announces_is_held_fetched_and_never_a_light_source() {
    let mut n = net();
    let (id, _, _) = join_node(&mut n, 1, filters(), 0);
    let tx = pay(&script(7), 5_000, 5);
    let txid = tx.compute_txid();
    let out = n.inv_txs(id, &[txid]);
    assert_eq!(has(&out), vec![txid]);
    assert_eq!(tx_requests(&n.sent(id)), vec![txid]);
    let out = n.tx(id, &tx);
    // A sighting for the sale to know the transaction by, from no address
    // of the light client, and the node's report again after it.
    assert_eq!(sightings(&out), 1);
    assert!(out
        .iter()
        .any(|o| matches!(o, Output::Sighting { from, .. } if from.is_empty())));
    assert_eq!(has(&out), vec![txid]);
    // Asked about later, it is held: the answer is at once.
    assert_eq!(has(&n.m.ask_own_node(txid)), vec![txid]);
}

#[test]
fn nr_106_a_transaction_the_app_asks_about_is_asked_of_the_node_and_is_no_sighting() {
    let mut n = net();
    let (id, _, _) = join_node(&mut n, 1, filters(), 0);
    let tx = pay(&script(7), 5_000, 6);
    let txid = tx.compute_txid();
    assert!(has(&n.m.ask_own_node(txid)).is_empty());
    assert_eq!(tx_requests(&n.sent(id)), vec![txid]);
    let out = n.tx(id, &tx);
    assert_eq!(has(&out), vec![txid]);
    assert_eq!(sightings(&out), 0);
    // Not found is not a "no", and is not a violation.
    let other = pay(&script(7), 5_000, 7);
    assert!(has(&n.m.ask_own_node(other.compute_txid())).is_empty());
    assert_eq!(tx_requests(&n.sent(id)), vec![other.compute_txid()]);
    let out = n.not_found_tx(id, other.compute_txid());
    assert!(out.is_empty());
    // A transaction nobody asked for is a violation.
    let out = n.tx(id, &pay(&script(7), 5_000, 8));
    assert_eq!(disconnects(&out), vec![(id, true)]);
}

#[test]
fn nr_106_what_the_app_asked_is_asked_again_when_the_node_connects_again() {
    let mut n = net();
    let tx = pay(&script(7), 5_000, 9);
    assert!(has(&n.m.ask_own_node(tx.compute_txid())).is_empty());
    let (id, _, sent) = join_node(&mut n, 1, filters(), 0);
    assert_eq!(tx_requests(&sent), vec![tx.compute_txid()]);
    let out = n.tx(id, &tx);
    assert_eq!(has(&out), vec![tx.compute_txid()]);
}

#[test]
fn nr_106_the_own_node_gives_no_address_to_the_book_and_is_not_asked_for_any() {
    let mut n = net();
    let (id, _, sent) = join_node(&mut n, 1, filters(), 0);
    assert!(sent.iter().all(|m| m.cmd() != "getaddr"));
    let entry = (
        1_700_000_000u32,
        Address::new(
            &SocketAddr::from((Ipv4Addr::new(8, 8, 4, 4), 8333)),
            ServiceFlags::NETWORK,
        ),
    );
    let out = n.feed(id, NetworkMessage::Addr(vec![entry]));
    assert!(out.is_empty(), "{out:?}");
}

// NR-107.

#[test]
fn nr_107_a_node_without_compact_filters_cannot_serve_and_is_reported() {
    for services in [
        plain(),
        ServiceFlags::NETWORK_LIMITED | ServiceFlags::WITNESS,
    ] {
        let mut n = net();
        let (id, out, sent) = join_node(&mut n, 1, services, 5);
        assert_eq!(disconnects(&out), vec![(id, false)], "{services}");
        assert!(reason(&out).starts_with("NR-107: "));
        assert!(!out
            .iter()
            .any(|o| matches!(o, Output::Ready { .. } | Output::OwnNodeReady { .. })));
        assert!(sent.iter().all(|m| m.cmd() != "getheaders"));
        let view = n.m.node_only_view().unwrap();
        assert!(!view.ready);
        assert_eq!(view.unfit_since, Some(n.now));
    }
}

#[test]
fn nr_107_a_node_with_filters_and_either_block_service_is_taken_and_clears_the_state() {
    let mut n = net();
    join_node(&mut n, 1, plain(), 0);
    assert!(n.m.node_only_view().unwrap().unfit_since.is_some());
    for (id, services) in [(2, filters()), (3, limited())] {
        let (pid, out, sent) = join_node(&mut n, id, services, 0);
        assert!(matches!(out[..], [Output::OwnNodeReady { .. }]), "{out:?}");
        assert!(sent.iter().any(|m| m.cmd() == "getheaders"));
        assert!(n.m.node_only_view().unwrap().unfit_since.is_none());
        n.m.closed(pid, n.now);
    }
}

// NR-108.

/// Blocks from height 1: the first pays the watched script.
fn long_chain(count: usize) -> Vec<Block> {
    let mut specs = vec![vec![pay(&script(7), 5_000, 1)]];
    specs.extend((1..count).map(|_| vec![]));
    blocks_on(&genesis(), specs, 1)
}

#[test]
fn nr_108_a_block_older_than_the_node_serves_is_not_asked_for_and_is_reported() {
    let mut n = net();
    let blocks = long_chain(300);
    let (id, _, _) = join_node(&mut n, 1, limited(), 300);
    n.headers(id, &headers_of(&blocks));
    n.tick(FILTER_SETTLE_SECS);
    n.sent(id);
    n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
    assert!(block_requests(&n.sent(id)).is_empty());
    let view = n.m.node_only_view().unwrap();
    assert_eq!(view.lacks, Some((1, n.now)));
    // A block inside the last 288 is asked for.
    n.filter(id, blocks[2].block_hash(), filter_of(&blocks[2]));
    assert!(block_requests(&n.sent(id)).is_empty());
    assert_eq!(crate::NODE_LIMITED_BLOCKS, 288);
}

#[test]
fn nr_108_a_node_that_keeps_every_block_is_asked_whatever_the_depth() {
    let mut n = net();
    let blocks = long_chain(300);
    let (id, _, _) = join_node(&mut n, 1, filters(), 300);
    n.headers(id, &headers_of(&blocks));
    n.tick(FILTER_SETTLE_SECS);
    n.sent(id);
    n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
    assert_eq!(block_requests(&n.sent(id)), vec![blocks[0].block_hash()]);
    assert_eq!(n.m.node_only_view().unwrap().lacks, None);
}

#[test]
fn nr_108_a_node_that_says_it_has_not_the_block_is_reported_and_the_block_waits() {
    let mut n = net();
    let (id, blocks) = synced(&mut n);
    n.sent(id);
    n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
    assert_eq!(block_requests(&n.sent(id)), vec![blocks[0].block_hash()]);
    let out = n.not_found_block(id, blocks[0].block_hash());
    assert!(disconnects(&out).is_empty());
    assert_eq!(n.m.node_only_view().unwrap().lacks.map(|l| l.0), Some(1));
    // Nobody else is asked, and the block stays in work.
    assert!(n.m.checked_height() < 1);
}

#[test]
fn nr_108_with_the_random_peers_running_the_block_comes_from_one_and_the_state_clears() {
    let mut n = net();
    let blocks = long_chain(300);
    let (id, _, _) = join_node(&mut n, 1, limited(), 300);
    n.headers(id, &headers_of(&blocks));
    n.tick(FILTER_SETTLE_SECS);
    n.sent(id);
    n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
    assert!(n.m.node_only_view().unwrap().lacks.is_some());
    // NR-110: the random peers start; one is asked for the block.
    n.m.set_node_only_fallback(true);
    let joined = n.join(2, filters(), 300);
    let other = joined.id;
    let mut sent = joined.sent;
    n.tick(1);
    sent.extend(n.sent(other));
    assert_eq!(block_requests(&sent), vec![blocks[0].block_hash()]);
    let out = n.block(other, &blocks[0]);
    assert_eq!(blocks_out(&out), vec![blocks[0].block_hash()]);
    assert_eq!(n.m.node_only_view().unwrap().lacks, None);
}

// NR-109.

#[test]
fn nr_109_a_transaction_goes_out_to_the_own_node_alone() {
    let mut n = net();
    n.allow_broadcast = true;
    n.m.set_node_only_fallback(true);
    let (node_id, _, _) = join_node(&mut n, 1, filters(), 0);
    let other = n.up(2);
    n.sent(other);
    let tx = pay(&script(1), 10_000, 3);
    let txid = n.m.broadcast(&serialize(&tx)).expect("held");
    let to_node = n.sent(node_id);
    assert!(to_node
        .iter()
        .any(|m| matches!(m, NetworkMessage::Inv(l) if l == &vec![Inventory::Transaction(txid)])));
    assert!(n.sent(other).iter().all(|m| m.cmd() != "inv"));
    // A peer that joins later is not told either; one that asks for it is
    // sent nothing, because it was never offered it.
    let late = n.up(3);
    assert!(n.sent(late).iter().all(|m| m.cmd() != "inv"));
    // The node asks for it and is served.
    let out = n.feed(
        node_id,
        NetworkMessage::GetData(vec![Inventory::Transaction(txid)]),
    );
    assert!(out.iter().any(|o| matches!(o, Output::TxRelayed { .. })));
    assert!(n.sent(node_id).iter().any(|m| m.cmd() == "tx"));
}

#[test]
fn nr_109_without_a_node_connected_a_held_transaction_waits_for_it() {
    let mut n = net();
    n.allow_broadcast = true;
    let other = n.up(2);
    n.sent(other);
    let tx = pay(&script(1), 10_000, 4);
    let txid = n.m.broadcast(&serialize(&tx)).expect("held");
    assert!(n.sent(other).iter().all(|m| m.cmd() != "inv"));
    // The node connects and is told at once.
    let (node_id, _, sent) = join_node(&mut n, 1, filters(), 0);
    let mut all = sent;
    all.extend(n.sent(node_id));
    assert!(all
        .iter()
        .any(|m| matches!(m, NetworkMessage::Inv(l) if l == &vec![Inventory::Transaction(txid)])));
}

// NR-110.

#[test]
fn nr_110_with_the_random_peers_running_two_filters_check_a_block_again() {
    let mut n = net();
    n.m.set_node_only_fallback(true);
    let blocks = blocks_on(&genesis(), vec![vec![]], 1);
    let (id, _, _) = join_node(&mut n, 1, filters(), 1);
    let other = n.up_filters(2);
    n.headers(id, &headers_of(&blocks));
    n.headers(other, &headers_of(&blocks));
    n.tick(FILTER_SETTLE_SECS);
    let hash = blocks[0].block_hash();
    n.filter(id, hash, filter_of(&blocks[0]));
    assert!(n.m.checked_height() < 1, "one answer is not enough now");
    n.filter(other, hash, filter_of(&blocks[0]));
    assert_eq!(n.m.checked_height(), 1);
    // And back to one with the mode's own check.
    n.m.set_node_only_fallback(false);
    let more = blocks_on(&blocks[0].header, vec![vec![]], 2);
    n.headers(id, &headers_of(&more));
    n.tick(FILTER_SETTLE_SECS);
    n.filter(id, more[0].block_hash(), filter_of(&more[0]));
    assert_eq!(n.m.checked_height(), 2);
}

// NR-111.

#[test]
fn nr_111_the_own_node_is_told_only_what_a_peer_would_be() {
    let mut n = net();
    let (id, _, _) = join_node(&mut n, 1, filters(), 0);
    // What goes to it is requests (NR-55): the kit checks every command.
    n.tick(1);
    let sent = n.sent(id);
    assert!(
        sent.iter().all(|m| matches!(m.cmd(), "ping" | "pong")),
        "{sent:?}"
    );
    // The mode is off: an ordinary peer is one of the light client's.
    let mut off = Net::new();
    assert!(off.m.node_only_view().is_none());
    off.up_filters(1);
    assert_eq!(off.m.health_peers().len(), 1);
}

#[test]
fn nr_106_the_same_over_the_encrypted_transport() {
    in_v2(|| {
        let mut n = net();
        let (id, blocks) = synced(&mut n);
        n.sent(id);
        n.filter(id, blocks[0].block_hash(), filter_of(&blocks[0]));
        assert_eq!(block_requests(&n.sent(id)), vec![blocks[0].block_hash()]);
        let out = n.block(id, &blocks[0]);
        assert_eq!(blocks_out(&out), vec![blocks[0].block_hash()]);
    });
}

// NR-112.

/// A transaction with witness data, so that a copy without it differs.
fn witnessed(tag: u32) -> Transaction {
    let mut tx = pay(&script(1), 10_000, tag);
    tx.input[0].witness = Witness::from_slice(&[vec![1, 2, 3], vec![4; 33]]);
    tx
}

/// The mode on, the node ready, and a witnessed transaction held and
/// announced to the node.
fn held(tag: u32) -> (Net, PeerId, Transaction, Txid) {
    let mut n = net();
    n.allow_broadcast = true;
    let (node_id, _, _) = join_node(&mut n, 1, filters(), 0);
    let tx = witnessed(tag);
    let txid = n.m.broadcast(&serialize(&tx)).expect("held");
    let inv = n.sent(node_id);
    assert!(inv
        .iter()
        .any(|m| matches!(m, NetworkMessage::Inv(l) if l == &vec![Inventory::Transaction(txid)])));
    (n, node_id, tx, txid)
}

/// Let `secs` pass in steps of at most 10, answering every ping so that no
/// peer is dropped for NR-14. Returns what the manager said and what each
/// peer was sent that is not a ping.
fn pass(n: &mut Net, ids: &[PeerId], secs: u64) -> (Vec<Output>, Vec<(PeerId, NetworkMessage)>) {
    let mut outs = Vec::new();
    let mut got = Vec::new();
    let mut left = secs;
    while left > 0 {
        let step = left.min(10);
        left -= step;
        outs.extend(n.tick(step));
        for id in ids {
            for message in n.sent(*id) {
                match message {
                    NetworkMessage::Ping(nonce) => {
                        n.feed(*id, NetworkMessage::Pong(nonce));
                    }
                    other => got.push((*id, other)),
                }
            }
        }
    }
    (outs, got)
}

/// The `getdata` for a transaction ID in these messages, with witness.
fn asks_for(got: &[(PeerId, NetworkMessage)], txid: Txid) -> Vec<PeerId> {
    got.iter()
        .filter(|(_, m)| {
            matches!(m, NetworkMessage::GetData(l) if l == &vec![Inventory::WitnessTransaction(txid)])
        })
        .map(|(id, _)| *id)
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

#[test]
fn nr_112_the_node_is_asked_only_after_130_seconds() {
    assert_eq!(NODE_ASK_AFTER_SECS, 130);
    let (mut n, node_id, _, txid) = held(1);
    let (_, got) = pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS - 1);
    assert!(asks_for(&got, txid).is_empty(), "{got:?}");
    let (_, got) = pass(&mut n, &[node_id], 1);
    assert_eq!(asks_for(&got, txid), vec![node_id]);
}

#[test]
fn nr_112_the_asking_is_repeated_every_60_seconds_until_answered() {
    assert_eq!(NODE_ASK_EVERY_SECS, 60);
    let (mut n, node_id, _, txid) = held(2);
    let (_, got) = pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS);
    assert_eq!(asks_for(&got, txid).len(), 1);
    for _ in 0..3 {
        let (_, got) = pass(&mut n, &[node_id], NODE_ASK_EVERY_SECS - 1);
        assert!(asks_for(&got, txid).is_empty(), "{got:?}");
        let (_, got) = pass(&mut n, &[node_id], 1);
        assert_eq!(asks_for(&got, txid).len(), 1);
    }
}

#[test]
fn nr_112_the_same_transaction_with_its_witness_is_held_by_the_node() {
    let (mut n, node_id, tx, txid) = held(3);
    pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS);
    let out = n.tx(node_id, &tx);
    assert_eq!(states(&out), vec![RelayState::HeldByNode]);
    assert!(disconnects(&out).is_empty());
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::HeldByNode));
    // Answered: no more asking, and no NotSeen after ten minutes.
    let (outs, got) = pass(&mut n, &[node_id], NOT_SEEN_SECS + 120);
    assert!(asks_for(&got, txid).is_empty());
    assert!(states(&outs).is_empty(), "{outs:?}");
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::HeldByNode));
}

#[test]
fn nr_112_a_transaction_that_is_not_the_one_held_is_a_fault_of_the_node() {
    // The same ID with other witness data.
    let (mut n, node_id, tx, txid) = held(4);
    pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS);
    let mut other = tx.clone();
    other.input[0].witness = Witness::from_slice(&[vec![9]]);
    assert_eq!(other.compute_txid(), txid);
    let out = n.tx(node_id, &other);
    assert_eq!(disconnects(&out), vec![(node_id, true)]);
    assert!(reason(&out).starts_with("NR-112"), "{}", reason(&out));
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
    // The same with the witness stripped.
    let (mut n, node_id, tx, txid) = held(5);
    pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS);
    let mut bare = tx.clone();
    bare.input[0].witness = Witness::new();
    let out = n.tx(node_id, &bare);
    assert_eq!(disconnects(&out), vec![(node_id, true)]);
    assert!(reason(&out).starts_with("NR-112"), "{}", reason(&out));
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
}

#[test]
fn nr_112_a_transaction_that_was_not_asked_for_is_still_a_fault() {
    // Before the 130 seconds the node was not asked.
    let (mut n, node_id, tx, txid) = held(6);
    let out = n.tx(node_id, &tx);
    assert_eq!(disconnects(&out), vec![(node_id, true)]);
    assert!(reason(&out).starts_with("NR-25"), "{}", reason(&out));
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
}

#[test]
fn nr_112_notfound_changes_nothing_and_the_asking_goes_on() {
    let (mut n, node_id, _, txid) = held(7);
    let (_, got) = pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS);
    assert_eq!(asks_for(&got, txid).len(), 1);
    let out = n.not_found_tx(node_id, txid);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::Sent));
    let (_, got) = pass(&mut n, &[node_id], NODE_ASK_EVERY_SECS);
    assert_eq!(asks_for(&got, txid).len(), 1);
}

#[test]
fn nr_112_with_the_fallback_either_state_may_come_first_and_both_are_kept() {
    // The node first, then two groups announce it back.
    let (mut n, node_id, tx, txid) = held(8);
    n.m.set_node_only_fallback(true);
    let a = n.up(2);
    let b = n.up(3);
    n.sent(a);
    n.sent(b);
    pass(&mut n, &[node_id, a, b], NODE_ASK_AFTER_SECS);
    let out = n.tx(node_id, &tx);
    assert_eq!(states(&out), vec![RelayState::HeldByNode]);
    assert!(states(&n.inv_txs(a, &[txid])).is_empty());
    let out = n.inv_txs(b, &[txid]);
    assert_eq!(
        states(&out),
        vec![RelayState::SeenBackAndHeldByNode { groups: 2 }]
    );
    // Two groups first, then the node.
    let (mut n, node_id, tx, txid) = held(9);
    n.m.set_node_only_fallback(true);
    let a = n.up(2);
    let b = n.up(3);
    n.sent(a);
    n.sent(b);
    n.inv_txs(a, &[txid]);
    let out = n.inv_txs(b, &[txid]);
    assert_eq!(states(&out), vec![RelayState::SeenBack { groups: 2 }]);
    // Still asked: the evidence of the node is the stronger one.
    let (_, got) = pass(&mut n, &[node_id, a, b], NODE_ASK_AFTER_SECS);
    assert_eq!(asks_for(&got, txid), vec![node_id]);
    let out = n.tx(node_id, &tx);
    assert_eq!(
        states(&out),
        vec![RelayState::SeenBackAndHeldByNode { groups: 2 }]
    );
    let (_, got) = pass(&mut n, &[node_id, a, b], NODE_ASK_EVERY_SECS * 2);
    assert!(asks_for(&got, txid).is_empty());
}

#[test]
fn nr_112_nr_88_still_fires_when_the_node_never_answers() {
    let (mut n, node_id, _, txid) = held(10);
    let (outs, got) = pass(&mut n, &[node_id], NOT_SEEN_SECS);
    // The first report (`Sent`) was kept for this tick; `NotSeen` follows.
    assert_eq!(states(&outs).last(), Some(&RelayState::NotSeen));
    assert!(!asks_for(&got, txid).is_empty());
    // The announcement went again, to the node alone.
    let announced = got.iter().any(|(id, m)| {
        *id == node_id
            && matches!(m, NetworkMessage::Inv(l) if l == &vec![Inventory::Transaction(txid)])
    });
    assert!(announced, "{got:?}");
    // The time to ask starts again from that announcement.
    let (_, got) = pass(&mut n, &[node_id], NODE_ASK_AFTER_SECS - 1);
    assert!(asks_for(&got, txid).is_empty(), "{got:?}");
    let (_, got) = pass(&mut n, &[node_id], 1);
    assert_eq!(asks_for(&got, txid), vec![node_id]);
    // And an answer then is still taken.
    let tx = witnessed(10);
    let out = n.tx(node_id, &tx);
    assert_eq!(states(&out), vec![RelayState::HeldByNode]);
}

#[test]
fn nr_112_in_the_mixed_mode_no_getdata_for_a_held_transaction_is_ever_sent() {
    let mut n = Net::new();
    n.allow_broadcast = true;
    let ids: Vec<PeerId> = (1..=3).map(|i| n.up(i)).collect();
    for id in &ids {
        n.sent(*id);
    }
    let txid = n.m.broadcast(&serialize(&witnessed(11))).expect("held");
    let (outs, got) = pass(&mut n, &ids, 3 * NOT_SEEN_SECS);
    assert!(got.iter().all(|(_, m)| m.cmd() != "getdata"), "{got:?}");
    // NR-88 ran as before.
    assert!(states(&outs).contains(&RelayState::NotSeen));
    assert_eq!(n.m.relay_state(&txid), Some(RelayState::NotSeen));
}
