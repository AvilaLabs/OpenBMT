//! NR-98 to NR-102: the merchant's own node, in the parser and the manager.

use bitcoin::hashes::Hash;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::Txid;
use openbmt_watch::HeaderChain;

use super::kit::peer_version;
use super::manager_kit::{extend, genesis, pay, plain, script, Net};
use super::v2kit::{in_v2, mode, Mode, Wire};
use crate::onion::onion_name;
use crate::{
    ConnectRefused, Output, OwnNode, OwnNodeHost, OwnNodeRefused, PeerId, TransportChoice,
    V2Entropy, OWN_ANSWER_SECS,
};

const PORT: u16 = 8333;

fn name() -> String {
    onion_name(&[7; 32])
}

fn node() -> OwnNode {
    OwnNode::onion(&name(), PORT).expect("a v3 name")
}

fn txid(n: u8) -> Txid {
    Txid::from_byte_array([n; 32])
}

/// Connect the own node as `id` and finish the handshake: what the manager
/// said, and what it wrote to the node.
fn join_own(n: &mut Net, id: u8, start_height: i32) -> (PeerId, Vec<Output>, Vec<NetworkMessage>) {
    let id = PeerId(u64::from(id));
    let nonce = 0x2000 + id.0;
    let mut wire = Wire::for_mode(id.0 as u8);
    match mode() {
        Mode::V1 => {
            n.m.connected_own_node(id, &node(), nonce, n.now, TransportChoice::V1)
                .expect("connected")
        }
        Mode::V2 => {
            let entropy = V2Entropy::new([id.0 as u8 | 1; 32], [id.0 as u8 ^ 0x55; 32]);
            n.m.connected_own_node(id, &node(), nonce, n.now, TransportChoice::V2(entropy))
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
    version.services = plain();
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

fn getdata_txids(messages: &[NetworkMessage]) -> Vec<Txid> {
    let mut out = Vec::new();
    for m in messages {
        if let NetworkMessage::GetData(list) = m {
            for item in list {
                if let Inventory::WitnessTransaction(t) | Inventory::Transaction(t) = item {
                    out.push(*t);
                }
            }
        }
    }
    out
}

fn disconnects(out: &[Output]) -> Vec<(PeerId, bool)> {
    out.iter()
        .filter_map(|o| match o {
            Output::Disconnect { id, misbehaved, .. } => Some((*id, *misbehaved)),
            _ => None,
        })
        .collect()
}

// NR-98.

#[test]
fn nr_98_a_tor_v3_name_is_the_own_node() {
    let own = OwnNode::onion(&name(), PORT).unwrap();
    assert_eq!(own.host(), &OwnNodeHost::Onion(name()));
    assert_eq!(own.port(), PORT);
    assert!(own.is_authenticated());
    // The text form: with a port, and without one.
    let with = OwnNode::parse(&format!("{}:9999", name()), PORT).unwrap();
    assert_eq!(with.port(), 9999);
    let without = OwnNode::parse(&format!("  {}  ", name()), PORT).unwrap();
    assert_eq!(without, own);
}

#[test]
fn nr_98_anything_that_is_not_a_v3_name_is_refused_and_says_so() {
    let good = name();
    let upper = good.to_uppercase().replace(".ONION", ".onion");
    let mut bad_sum = good.clone().into_bytes();
    bad_sum[0] = if bad_sum[0] == b'a' { b'b' } else { b'a' };
    let bad_sum = String::from_utf8(bad_sum).unwrap();
    let v2_name = format!("{}.onion", &good[..16]);
    for text in [
        upper.as_str(),
        bad_sum.as_str(),
        v2_name.as_str(),
        "",
        ".onion",
        "example.com",
        "example.com:8333",
        "10.0.0.5",
        "192.168.1.5:8333",
        "127.0.0.1:8333",
        "[::1]:8333",
        "fe80::1",
    ] {
        let err = OwnNode::parse(text, PORT).expect_err(text);
        assert_eq!(err, OwnNodeRefused::NotOnion, "{text}");
        assert!(err.to_string().starts_with("NR-98: "));
    }
}

#[test]
fn nr_98_a_bad_port_is_refused() {
    assert_eq!(OwnNode::onion(&name(), 0), Err(OwnNodeRefused::BadPort));
    for port in ["0", "65536", "-1", "", "http"] {
        let err = OwnNode::parse(&format!("{}:{port}", name()), PORT).unwrap_err();
        assert_eq!(err, OwnNodeRefused::BadPort, "{port}");
        assert!(err.to_string().starts_with("NR-98: "));
    }
}

#[test]
fn nr_98_one_own_node_at_a_time() {
    let mut n = Net::new();
    let (id, ..) = join_own(&mut n, 5, 0);
    let again =
        n.m.connected_own_node(PeerId(6), &node(), 1, n.now, TransportChoice::V1);
    assert_eq!(again, Err(ConnectRefused::OwnNodeConnected));
    assert!(again.unwrap_err().to_string().starts_with("NR-98: "));
    // The ID of the connection that is there is in use.
    let same =
        n.m.connected_own_node(id, &node(), 1, n.now, TransportChoice::V1);
    assert_eq!(same, Err(ConnectRefused::IdInUse));
    // And so is the ID of a peer.
    let peer = n.up(9);
    let same =
        n.m.connected_own_node(peer, &node(), 1, n.now, TransportChoice::V1);
    assert_eq!(same, Err(ConnectRefused::IdInUse));
}

// NR-99.

#[test]
fn nr_99_nothing_on_a_lan_or_clear_net_makes_an_own_node() {
    // The only public ways to make one take a v3 name.
    for text in [
        "192.168.0.10:8333",
        "10.1.2.3",
        "localhost:8333",
        "node.local:8333",
        "1.2.3.4:8333",
    ] {
        assert_eq!(
            OwnNode::parse(text, PORT),
            Err(OwnNodeRefused::NotOnion),
            "{text}"
        );
        assert!(OwnNode::onion(text, PORT).is_err(), "{text}");
    }
    // Such an address stays a peer: with local peers refused, the manager
    // refuses it as a peer too (NR-20).
    let mut n = Net::new();
    let lan = openbmt_watch::PeerAddr::Ip("192.168.0.10".parse().unwrap());
    assert_eq!(
        n.m.connected(PeerId(1), lan, 1, n.now),
        Err(ConnectRefused::LocalGroup)
    );
}

// NR-100.

#[test]
fn nr_100_the_own_node_takes_no_place_of_the_peers() {
    let mut n = Net::with(0, 1);
    let (own, out, _) = join_own(&mut n, 5, 0);
    assert!(out
        .iter()
        .any(|o| matches!(o, Output::OwnNodeReady { id, .. } if *id == own)));
    // The one place of the target number is still free, and the group of the
    // node does not stand in the way of a peer.
    assert!(n.m.wanted().is_some());
    let peer = n.up(1);
    assert!(n.m.wanted().is_none());
    // Only the peer is a peer of the health check.
    let health = n.m.health_peers();
    assert_eq!(health.len(), 1);
    assert!(n.m.is_own_node(own) && !n.m.is_own_node(peer));
    // And a peer that the target number refuses is refused whatever the own
    // node does.
    assert_eq!(
        n.m.connected(PeerId(2), super::manager_kit::addr(2), 2, n.now),
        Err(ConnectRefused::Full)
    );
}

#[test]
fn nr_100_a_request_that_runs_out_does_not_drop_the_own_node() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    for round in 0..6u8 {
        n.m.ask_own_node(txid(round + 1));
        // 31 seconds: past NR-101's limit, under the ping timer of NR-14.
        let out = n.tick(OWN_ANSWER_SECS + 1);
        assert!(disconnects(&out).is_empty(), "round {round}: {out:?}");
        // Answer the ping that NR-14 sends at 120 seconds, so that only the
        // requests are under test.
        let sent = n.sent(own);
        for m in sent {
            if let NetworkMessage::Ping(nonce) = m {
                let pong = n.feed(own, NetworkMessage::Pong(nonce));
                assert!(disconnects(&pong).is_empty());
            }
        }
    }
    assert!(n.m.own_node().is_some_and(|v| v.ready));
}

#[test]
fn nr_100_a_protocol_violation_drops_it_and_says_so() {
    for what in ["block", "filter", "tx"] {
        let mut n = Net::new();
        let (own, ..) = join_own(&mut n, 5, 0);
        let genesis = genesis();
        let out = match what {
            "block" => n.block(own, &super::manager_kit::block_on(&genesis, vec![], 1)),
            "filter" => n.filter(own, genesis.block_hash(), vec![0]),
            _ => n.tx(own, &pay(&script(1), 5_000, 1)),
        };
        assert_eq!(disconnects(&out), vec![(own, true)], "{what}");
        let reason = out.iter().find_map(|o| match o {
            Output::Disconnect { reason, .. } => Some(reason.clone()),
            _ => None,
        });
        assert!(reason.is_some_and(|r| r.starts_with("NR-25")), "{what}");
        // The manager has forgotten it.
        assert!(n.m.own_node().is_none());
        assert!(!n.m.is_own_node(own));
        // A new one may be connected.
        let (again, ..) = join_own(&mut n, 6, 0);
        assert!(n.m.is_own_node(again));
    }
}

#[test]
fn nr_100_the_timers_of_nr_14_still_drop_it_without_a_mark() {
    let mut n = Net::new();
    let id = PeerId(5);
    n.m.connected_own_node(id, &node(), 7, n.now, TransportChoice::V1)
        .unwrap();
    let out = n.tick(31);
    assert_eq!(disconnects(&out), vec![(id, false)]);
    assert!(n.m.own_node().is_none());
}

#[test]
fn nr_100_a_closed_connection_is_forgotten() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    let out = n.m.closed(own, n.now);
    assert!(disconnects(&out).is_empty());
    assert!(n.m.own_node().is_none());
    // A close of an ID that is not there is passed over.
    assert!(n.m.closed(own, n.now).is_empty());
}

// NR-101.

#[test]
fn nr_101_an_announced_transaction_is_reported_when_asked_about() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    let t = txid(1);
    let out = n.inv_txs(own, &[t]);
    // Not a sighting, not fetched: the own node's announcements are its
    // own (NR-51, NR-53).
    assert!(out.is_empty(), "{out:?}");
    assert!(getdata_txids(&n.sent(own)).is_empty());
    let out = n.m.ask_own_node(t);
    assert_eq!(has(&out), vec![t]);
    assert!(getdata_txids(&n.sent(own)).is_empty());
}

#[test]
fn nr_101_an_announcement_after_the_question_answers_it() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    let t = txid(2);
    n.m.ask_own_node(t);
    assert_eq!(getdata_txids(&n.sent(own)), vec![t]);
    let out = n.inv_txs(own, &[t, txid(3)]);
    assert_eq!(has(&out), vec![t]);
    // Only what was asked about is reported: not the other announcement.
    assert_eq!(out.len(), 1);
}

#[test]
fn nr_101_a_transaction_the_node_sends_when_asked_is_a_report() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    let tx = pay(&script(1), 10_000, 4);
    let t = tx.compute_txid();
    n.m.ask_own_node(t);
    assert_eq!(getdata_txids(&n.sent(own)), vec![t]);
    let out = n.tx(own, &tx);
    assert_eq!(has(&out), vec![t]);
    assert!(!out.iter().any(|o| matches!(o, Output::Sighting { .. })));
    // The same transaction again, unasked, is a violation.
    let out = n.tx(own, &tx);
    assert_eq!(disconnects(&out), vec![(own, true)]);
}

#[test]
fn nr_101_a_transaction_nobody_asked_for_is_a_violation() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    n.m.ask_own_node(txid(9));
    let out = n.tx(own, &pay(&script(1), 10_000, 5));
    assert_eq!(disconnects(&out), vec![(own, true)]);
}

#[test]
fn nr_101_no_answer_is_not_a_no() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    let t = txid(6);
    n.m.ask_own_node(t);
    assert_eq!(getdata_txids(&n.sent(own)), vec![t]);
    // "Not found" ends the request and reports nothing, and drops nothing.
    let out = n.not_found_tx(own, t);
    assert!(out.is_empty(), "{out:?}");
    // A request that is let go after 30 seconds is asked again at the next
    // question.
    n.m.ask_own_node(txid(7));
    assert_eq!(getdata_txids(&n.sent(own)), vec![txid(7)]);
    n.tick(OWN_ANSWER_SECS + 1);
    n.m.ask_own_node(txid(7));
    assert_eq!(getdata_txids(&n.sent(own)), vec![txid(7)]);
}

#[test]
fn nr_101_the_questions_are_asked_again_at_the_next_connection() {
    let mut n = Net::new();
    let t = txid(8);
    // Asked before there is a connection: nothing is sent, nothing is lost.
    n.m.ask_own_node(t);
    let (own, _, sent) = join_own(&mut n, 5, 0);
    assert_eq!(getdata_txids(&sent), vec![t]);
    // The node goes and comes back: asked again.
    n.m.closed(own, n.now);
    let (_, _, sent) = join_own(&mut n, 6, 0);
    assert_eq!(getdata_txids(&sent), vec![t]);
    // Answered, it is not asked again.
    let out = n.inv_txs(PeerId(6), &[t]);
    assert_eq!(has(&out), vec![t]);
    n.m.closed(PeerId(6), n.now);
    let (_, _, sent) = join_own(&mut n, 7, 0);
    assert!(getdata_txids(&sent).is_empty());
}

#[test]
fn nr_101_the_announced_ids_are_bounded() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    // Past the bound the oldest is forgotten; a question about it is sent
    // to the node instead of answered from memory.
    let id = |i: u32| {
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&i.to_le_bytes());
        Txid::from_byte_array(bytes)
    };
    let all: Vec<Txid> = (0..crate::MAX_OWN_ANNOUNCED as u32).map(id).collect();
    let out = n.inv_txs(own, &all);
    assert!(out.is_empty());
    n.inv_txs(own, &[id(crate::MAX_OWN_ANNOUNCED as u32)]);
    assert!(has(&n.m.ask_own_node(id(0))).is_empty());
    assert_eq!(getdata_txids(&n.sent(own)), vec![id(0)]);
    assert_eq!(has(&n.m.ask_own_node(id(1))), vec![id(1)]);
}

#[test]
fn nr_101_what_is_asked_about_is_bounded() {
    let mut n = Net::new();
    for i in 0..(crate::MAX_OWN_WANTED + 50) {
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&(i as u32).to_le_bytes());
        n.m.ask_own_node(Txid::from_byte_array(bytes));
    }
    // After the connection only the most recent are asked.
    let (_, _, sent) = join_own(&mut n, 5, 0);
    assert_eq!(getdata_txids(&sent).len(), crate::MAX_OWN_WANTED);
}

// NR-102.

#[test]
fn nr_102_the_tip_is_its_statement_and_the_blocks_it_shows() {
    let mut n = Net::new();
    let headers = extend(&genesis(), 3, 1);
    let peer = n.up(1);
    n.headers(peer, &headers);
    assert_eq!(n.m.chain().tip_height(), 3);
    assert!(n.m.own_node().is_none());
    let (own, ..) = join_own(&mut n, 5, 2);
    let view = n.m.own_node().unwrap();
    assert_eq!(view.best_height, Some(2));
    assert!(view.ready && view.authenticated);
    assert_eq!(view.last_heard, Some(n.now));
    // A block it announces that the chain holds raises its height.
    n.now += 5;
    n.inv_block(own, headers[2].block_hash());
    let view = n.m.own_node().unwrap();
    assert_eq!(view.best_height, Some(3));
    assert_eq!(view.last_heard, Some(n.now));
    // A lower block does not lower it.
    n.inv_block(own, headers[0].block_hash());
    assert_eq!(n.m.own_node().unwrap().best_height, Some(3));
}

#[test]
fn nr_102_a_block_the_chain_does_not_hold_yet_is_placed_later() {
    let mut n = Net::new();
    let headers = extend(&genesis(), 4, 1);
    let peer = n.up(1);
    n.headers(peer, &headers[..2]);
    let (own, ..) = join_own(&mut n, 5, 1);
    n.inv_block(own, headers[3].block_hash());
    assert_eq!(n.m.own_node().unwrap().best_height, Some(1));
    // The chain catches up: the block is placed.
    n.headers(peer, &headers[2..]);
    assert_eq!(n.m.own_node().unwrap().best_height, Some(4));
}

#[test]
fn nr_102_at_most_eight_unplaced_blocks_are_remembered() {
    let mut n = Net::new();
    let later = extend(&genesis(), 10, 1);
    let (own, ..) = join_own(&mut n, 5, 0);
    // The highest is announced first, so it is the oldest and is the first
    // to be forgotten, with the lowest after it.
    let mut order = vec![later[9].block_hash()];
    order.extend(later[..9].iter().map(|h| h.block_hash()));
    assert_eq!(order.len(), crate::MAX_OWN_BLOCKS + 2);
    for hash in order {
        n.inv_block(own, hash);
    }
    let peer = n.up(1);
    n.headers(peer, &later);
    // The blocks of heights 2 to 9 were kept; height 10 and 1 were not.
    assert_eq!(n.m.own_node().unwrap().best_height, Some(9));
}

#[test]
fn nr_102_headers_from_the_own_node_are_not_taken_into_the_chain() {
    let mut n = Net::new();
    let headers = extend(&genesis(), 3, 1);
    let (own, ..) = join_own(&mut n, 5, 0);
    let out = n.headers(own, &headers);
    assert!(!out.iter().any(|o| matches!(o, Output::Chain { .. })));
    assert_eq!(n.m.chain().tip_height(), 0);
    // And the manager does not ask it for any.
    assert!(super::manager_kit::header_requests(&n.sent(own)).is_empty());
}

#[test]
fn nr_102_its_addresses_do_not_reach_the_book() {
    let mut n = Net::new();
    let (own, ..) = join_own(&mut n, 5, 0);
    let out = n.feed(own, NetworkMessage::Addr(vec![]));
    assert!(out.is_empty());
    assert!(!out.iter().any(|o| matches!(o, Output::Addresses { .. })));
}

#[test]
fn nr_101_the_own_node_reports_on_the_encrypted_transport_too() {
    in_v2(|| {
        let mut n = Net::new();
        let (own, ..) = join_own(&mut n, 5, 7);
        assert_eq!(n.m.own_node().unwrap().best_height, Some(7));
        let tx = pay(&script(1), 10_000, 8);
        let t = tx.compute_txid();
        n.m.ask_own_node(t);
        assert_eq!(getdata_txids(&n.sent(own)), vec![t]);
        assert_eq!(has(&n.tx(own, &tx)), vec![t]);
        // A block nobody asked for is a violation on either transport.
        let out = n.block(own, &super::manager_kit::block_on(&genesis(), vec![], 1));
        assert_eq!(disconnects(&out), vec![(own, true)]);
    });
}
