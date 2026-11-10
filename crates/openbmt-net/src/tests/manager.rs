//! NR-20 to NR-55 at the level of the manager: many peers, the header
//! chain, filters, blocks and unconfirmed transactions. The peers are
//! played by `manager_kit`; every byte the manager writes is decoded and its
//! command checked against the list of NR-55.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use bitcoin::p2p::address::{AddrV2, Address};
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, CompactTarget, Network, ScriptBuf, Txid};
use openbmt_watch::{network_group, HeaderStore, PeerAddr};

use super::kit::{command_field, raw_frame, raw_header, verack_frame, version_frame};
use super::manager_kit::*;
use crate::manager::MAX_UNANSWERED;
use crate::{
    ConnectRefused, Manager, ManagerConfig, Output, PeerAddress, PeerId, MAX_OUTGOING_BYTES,
};

// ----- NR-20, NR-21: which peers -----------------------------------------

#[test]
fn nr_20_a_second_peer_in_the_same_group_is_refused_and_nothing_is_stored() {
    let mut n = Net::new();
    n.up(1);
    // addr(1) is 21.1.0.1: the same /16.
    let same = PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(21, 1, 200, 9)));
    let refused = n.m.connected(PeerId(50), same, 7, n.now);
    assert_eq!(refused, Err(ConnectRefused::SameGroup));
    assert!(n.m.outgoing(PeerId(50)).is_empty());
    assert_eq!(n.m.sizes().peers, 1);
    // The ID is free for another address.
    assert_eq!(n.m.connected(PeerId(50), addr(9), 7, n.now), Ok(()));
}

#[test]
fn nr_20_every_peer_reached_by_a_name_is_in_one_group() {
    let mut n = Net::new();
    assert_eq!(n.m.connected(PeerId(1), PeerAddr::Named, 1, n.now), Ok(()));
    assert_eq!(
        n.m.connected(PeerId(2), PeerAddr::Named, 2, n.now),
        Err(ConnectRefused::SameGroup)
    );
}

#[test]
fn nr_20_the_local_group_is_refused() {
    let mut n = Net::new();
    let local = [
        PeerAddr::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))),
        PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 5))),
        PeerAddr::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ];
    for (i, a) in local.into_iter().enumerate() {
        let id = PeerId(100 + i as u64);
        assert_eq!(
            n.m.connected(id, a, 1, n.now),
            Err(ConnectRefused::LocalGroup),
            "{a:?}"
        );
    }
    assert_eq!(n.m.sizes().peers, 0);
}

#[test]
fn nr_20_a_local_peer_is_taken_only_when_the_caller_allows_it() {
    let config = ManagerConfig {
        network: Network::Regtest,
        target_peers: 8,
        watch_from: 0,
        allow_local: true,
    };
    let mut m = Manager::new(config, HeaderStore::new(Network::Regtest).unwrap());
    let local = PeerAddr::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(m.connected(PeerId(1), local, 1, T0), Ok(()));
}

#[test]
fn nr_20_an_id_in_use_is_refused() {
    let mut n = Net::new();
    n.up(1);
    assert_eq!(
        n.m.connected(PeerId(1), addr(5), 1, n.now),
        Err(ConnectRefused::IdInUse)
    );
    assert_eq!(n.m.sizes().peers, 1);
}

#[test]
fn nr_20_never_more_than_the_target() {
    let mut n = Net::new();
    for i in 1..=8 {
        assert!(n.m.wanted().is_some());
        n.up(i);
    }
    assert_eq!(n.m.wanted(), None);
    assert_eq!(
        n.m.connected(PeerId(9), addr(9), 1, n.now),
        Err(ConnectRefused::Full)
    );
    assert_eq!(n.m.sizes().peers, 8);
    // A peer that goes makes room.
    n.m.closed(PeerId(3), n.now);
    assert!(n.m.wanted().is_some());
    assert_eq!(n.m.connected(PeerId(9), addr(9), 1, n.now), Ok(()));
    assert_eq!(n.m.wanted(), None);
}

#[test]
fn nr_20_a_small_target_is_kept() {
    let mut n = Net::with(0, 2);
    n.up(1);
    n.up(2);
    assert_eq!(n.m.wanted(), None);
    assert_eq!(
        n.m.connected(PeerId(3), addr(3), 1, n.now),
        Err(ConnectRefused::Full)
    );
}

#[test]
fn nr_20_wanted_names_the_groups_that_are_connected() {
    let mut n = Net::new();
    let w = n.m.wanted().unwrap();
    assert!(w.avoid.is_empty());
    n.up(1);
    n.m.connected(PeerId(2), addr(2), 1, n.now).unwrap(); // handshake not done
    let w = n.m.wanted().unwrap();
    let want: std::collections::BTreeSet<_> =
        [network_group(&addr(1)), network_group(&addr(2))].into();
    assert_eq!(w.avoid, want);
    n.m.closed(PeerId(1), n.now);
    let w = n.m.wanted().unwrap();
    assert_eq!(w.avoid.len(), 1);
    assert!(w.avoid.contains(&network_group(&addr(2))));
}

#[test]
fn nr_21_filters_are_wanted_until_two_groups_have_a_ready_filter_peer() {
    let mut n = Net::new();
    assert!(n.m.wanted().unwrap().want_filters);
    // Connected, handshake not done: does not count.
    n.m.connected(PeerId(1), addr(1), 1, n.now).unwrap();
    assert!(n.m.wanted().unwrap().want_filters);
    n.m.closed(PeerId(1), n.now);
    n.up_filters(1);
    assert!(n.m.wanted().unwrap().want_filters);
    n.up(2); // no filters
    assert!(n.m.wanted().unwrap().want_filters);
    n.up_filters(3);
    assert!(!n.m.wanted().unwrap().want_filters);
    n.m.closed(PeerId(1), n.now);
    assert!(n.m.wanted().unwrap().want_filters);
}

#[test]
fn nr_21_health_peers_say_who_serves_filters_and_how_high() {
    let mut n = Net::new();
    n.join(1, filters(), 5);
    n.join(2, plain(), 9);
    n.m.connected(PeerId(3), addr(3), 1, n.now).unwrap(); // not ready: left out
    let peers = n.m.health_peers();
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0].addr, addr(1));
    assert!(peers[0].serves_filters);
    assert_eq!(peers[0].best_height, 5);
    assert!(!peers[1].serves_filters);
    assert_eq!(peers[1].best_height, 9);
}

// ----- the handshake, addresses ------------------------------------------

#[test]
fn nr_22_getaddr_is_sent_once_right_after_ready_and_addresses_pass_through() {
    let mut n = Net::new();
    let j = n.join(1, plain(), 0);
    assert!(matches!(
        j.out[..],
        [Output::Ready { id: PeerId(1), services }] if services == plain()
    ));
    let names: Vec<&str> = j.sent.iter().map(|m| m.cmd()).collect();
    assert_eq!(
        names,
        [
            "sendaddrv2",
            "verack",
            "sendheaders",
            "getheaders",
            "getaddr"
        ]
    );
    let ip = Ipv4Addr::new(8, 8, 4, 4);
    let entry = (
        1_700_000_000u32,
        Address::new(&SocketAddr::from((ip, 8333)), ServiceFlags::NETWORK),
    );
    let out = n.feed(PeerId(1), NetworkMessage::Addr(vec![entry]));
    assert_eq!(
        out,
        vec![Output::Addresses {
            from: network_group(&addr(1)),
            list: vec![PeerAddress {
                addr: AddrV2::Ipv4(ip),
                port: 8333,
                services: ServiceFlags::NETWORK,
                time: 1_700_000_000,
            }],
        }]
    );
    // Nothing more is asked of this peer.
    n.tick(1);
    assert!(n.sent(PeerId(1)).iter().all(|m| m.cmd() != "getaddr"));
}

// ----- NR-25: who is dropped and flagged ----------------------------------

fn only_disconnect(out: &[Output], id: PeerId, misbehaved: bool, rule: &str) {
    assert_eq!(disconnects(out), vec![(id, misbehaved)], "{out:?}");
    let why = reason(out);
    assert!(why.starts_with(&format!("{rule}: ")), "{why}");
}

#[test]
fn nr_25_a_bad_frame_is_a_misbehaviour_and_the_peer_is_forgotten() {
    let mut n = Net::new();
    let a = n.up(1);
    let junk = raw_header([0xde, 0xad, 0xbe, 0xef], &command_field("ping"), 0, [0; 4]);
    let out = n.feed_bytes(a, &junk);
    only_disconnect(&out, a, true, "NR-01");
    // Forgotten: later calls do nothing, and the ID is free.
    assert_eq!(n.m.sizes().peers, 0);
    assert!(n.feed_bytes(a, &junk).is_empty());
    assert!(n.m.outgoing(a).is_empty());
    assert_eq!(n.m.connected(a, addr(1), 1, n.now), Ok(()));
}

#[test]
fn nr_25_a_message_that_does_not_parse_is_a_misbehaviour() {
    let mut n = Net::new();
    let a = n.up(1);
    let out = n.feed_bytes(a, &raw_frame("tx", &[1, 2, 3]));
    only_disconnect(&out, a, true, "NR-04");
}

#[test]
fn nr_25_a_break_of_the_handshake_is_a_misbehaviour() {
    let mut n = Net::new();
    n.m.connected(PeerId(1), addr(1), 1, n.now).unwrap();
    n.sent(PeerId(1));
    let out = n.feed_bytes(PeerId(1), &verack_frame());
    only_disconnect(&out, PeerId(1), true, "NR-12");
    // A second version after the first.
    n.m.connected(PeerId(2), addr(2), 1, n.now).unwrap();
    let mut stream = version_frame();
    stream.extend(version_frame());
    let out = n.feed_bytes(PeerId(2), &stream);
    only_disconnect(&out, PeerId(2), true, "NR-13");
}

#[test]
fn nr_25_a_handshake_that_takes_too_long_is_no_misbehaviour() {
    let mut n = Net::new();
    n.m.connected(PeerId(1), addr(1), 1, n.now).unwrap();
    assert!(n.tick(29).is_empty());
    let out = n.tick(1);
    only_disconnect(&out, PeerId(1), false, "NR-14");
    assert_eq!(n.m.sizes().peers, 0);
}

#[test]
fn nr_25_a_peer_that_does_not_answer_a_ping_is_no_misbehaviour() {
    let mut n = Net::new();
    let a = n.up(1);
    assert!(n.tick(120).is_empty());
    let sent = n.sent(a);
    assert!(sent.iter().any(|m| m.cmd() == "ping"));
    let out = n.tick(120);
    only_disconnect(&out, a, false, "NR-14");
}

#[test]
fn nr_25_a_header_that_breaks_the_work_rules_is_a_misbehaviour() {
    let g = genesis();
    let mut wrong_bits = child(&g, 1);
    wrong_bits.bits = CompactTarget::from_consensus(0x1d00_ffff);
    let bad_work = unmined(&g, 2);
    let mut early = child(&g, 3);
    early.time = g.time; // not after the median time of the headers before
    let early = mine(g.block_hash(), early.time, 0x207f_ffff, early.merkle_root);
    for (i, bad) in [wrong_bits, bad_work, early].into_iter().enumerate() {
        let mut n = Net::new();
        let a = n.up(1);
        let good = child(&g, 10);
        let out = n.headers(a, &[good, bad]);
        // The header before the bad one was taken.
        assert_eq!(
            chain_out(&out),
            vec![(vec![(1, good.block_hash())], vec![])]
        );
        let why = reason(&out);
        assert!(
            why.starts_with("NR-25: header refused: WR-05"),
            "{i}: {why}"
        );
        assert!(disconnects(&out) == vec![(a, true)], "{i}");
        assert_eq!(n.m.chain().tip().0, 1);
    }
}

#[test]
fn nr_25_data_that_was_not_asked_of_the_peer_is_a_misbehaviour() {
    let blocks = blocks_on(&genesis(), vec![vec![]], 1);
    let tx = pay(&script(1), 1_000, 1);
    type Play<'a> = Box<dyn Fn(&mut Net, PeerId) -> Vec<Output> + 'a>;
    let kinds: [(&str, Play); 3] = [
        ("block", Box::new(|n, a| n.block(a, &blocks[0]))),
        ("tx", Box::new(|n, a| n.tx(a, &tx))),
        (
            "filter",
            Box::new(|n, a| n.filter(a, blocks[0].block_hash(), vec![0])),
        ),
    ];
    for (what, send) in kinds {
        let mut n = Net::new();
        let a = n.up(1);
        let out = send(&mut n, a);
        assert_eq!(disconnects(&out), vec![(a, true)], "{what}");
        assert!(reason(&out).contains("was not asked for"), "{what}");
        assert_eq!(n.m.sizes().peers, 0);
    }
}

#[test]
fn nr_25_headers_inv_and_addr_need_no_request() {
    let mut n = Net::new();
    let a = n.up(1);
    let hs = extend(&genesis(), 2, 1);
    assert!(disconnects(&n.headers(a, &hs)).is_empty());
    assert!(disconnects(&n.inv_txs(a, &[fake_txid(1)])).is_empty());
    assert!(disconnects(&n.inv_block(a, hs[1].block_hash())).is_empty());
    assert_eq!(n.m.sizes().peers, 1);
}

#[test]
fn nr_25_three_requests_in_a_row_that_run_out_end_the_peer_without_a_mark() {
    // Three blocks asked of one peer that never answers.
    let mut n = Net::new();
    n.m.set_all_blocks(true);
    let a = n.up(1);
    let blocks = blocks_on(&genesis(), vec![vec![]; 3], 1);
    n.headers(a, &headers_of(&blocks));
    assert_eq!(block_requests(&n.sent(a)).len(), 3);
    assert_eq!(MAX_UNANSWERED, 3);
    assert!(n.tick(59).is_empty());
    let out = n.tick(1);
    only_disconnect(&out, a, false, "NR-42");
    // The same for transactions.
    let mut n = Net::new();
    let a = n.up(1);
    n.inv_txs(a, &[fake_txid(1), fake_txid(2), fake_txid(3)]);
    assert_eq!(tx_requests(&n.sent(a)).len(), 3);
    assert!(n.tick(29).is_empty());
    let out = n.tick(1);
    only_disconnect(&out, a, false, "NR-42");
}

#[test]
fn nr_25_a_request_that_runs_out_is_no_misbehaviour_by_itself() {
    let mut n = Net::new();
    n.m.set_all_blocks(true);
    let a = n.up(1);
    n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]], 1);
    n.headers(a, &headers_of(&blocks));
    let out = n.tick(60);
    assert!(disconnects(&out).is_empty());
    assert_eq!(n.m.sizes().peers, 2);
}

// ----- NR-30 to NR-33: headers -------------------------------------------

#[test]
fn nr_30_every_peer_is_asked_for_headers_when_ready() {
    let mut n = Net::new();
    let g = genesis().block_hash();
    for i in 1..=3 {
        let j = n.join(i, plain(), 0);
        assert_eq!(header_requests(&j.sent), vec![vec![g]], "peer {i}");
    }
    // A later peer gets the locator of the chain as it is then.
    let hs = extend(&genesis(), 3, 1);
    n.headers(PeerId(1), &hs);
    let j = n.join(4, plain(), 0);
    let locator = &header_requests(&j.sent)[0];
    assert_eq!(locator[0], hs[2].block_hash());
    assert_eq!(*locator.last().unwrap(), g);
}

#[test]
fn nr_30_a_full_message_of_2000_headers_is_answered_with_another_request() {
    let mut n = Net::new();
    let a = n.up(1);
    let hs = extend(&genesis(), 2_001, 1);
    let out = n.headers(a, &hs[..2_000]);
    assert_eq!(chain_out(&out).len(), 1);
    assert_eq!(chain_out(&out)[0].0.len(), 2_000);
    let asked = header_requests(&n.sent(a));
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0][0], hs[1_999].block_hash());
    // The last one is a short message: no further request.
    n.headers(a, &hs[2_000..]);
    assert!(header_requests(&n.sent(a)).is_empty());
    assert_eq!(n.m.chain().tip().0, 2_001);
}

#[test]
fn nr_30_a_message_of_fewer_than_2000_headers_is_not_answered() {
    let mut n = Net::new();
    let a = n.up(1);
    let hs = extend(&genesis(), 1_999, 1);
    n.headers(a, &hs);
    assert!(header_requests(&n.sent(a)).is_empty());
}

#[test]
fn nr_30_a_full_message_that_was_not_all_taken_is_not_answered() {
    // 1,999 good headers and one dated too far ahead.
    let mut n = Net::new();
    let a = n.up(1);
    let mut hs = extend(&genesis(), 1_999, 1);
    let last = hs.last().unwrap();
    let ahead = mine(
        last.block_hash(),
        T0 as u32 + 20_000,
        0x207f_ffff,
        last.merkle_root,
    );
    hs.push(ahead);
    let out = n.headers(a, &hs);
    assert!(disconnects(&out).is_empty());
    assert_eq!(n.m.chain().tip().0, 1_999);
    assert!(header_requests(&n.sent(a)).is_empty());
}

#[test]
fn nr_31_headers_go_to_the_store_and_a_second_peer_sees_them_known() {
    let mut n = Net::new();
    let (a, b) = (n.up(1), n.up(2));
    let hs = extend(&genesis(), 3, 1);
    let out = n.headers(a, &hs);
    let joined: Vec<_> = hs
        .iter()
        .enumerate()
        .map(|(i, h)| (i as u32 + 1, h.block_hash()))
        .collect();
    // One output for the run of headers that extended the chain.
    assert_eq!(chain_out(&out), vec![(joined, vec![])]);
    assert_eq!(n.m.chain().tip(), (3, hs[2].block_hash()));
    // The same headers again are known: nothing changes, nobody is dropped.
    let out = n.headers(b, &hs);
    assert!(out.is_empty());
}

#[test]
fn nr_31_a_better_chain_from_a_second_peer_gives_joined_and_left() {
    let mut n = Net::new();
    let (a, b) = (n.up(1), n.up(2));
    let a_chain = extend(&genesis(), 3, 1);
    n.headers(a, &a_chain);
    // The same work is not enough.
    let b_chain = extend(&genesis(), 4, 100);
    let out = n.headers(b, &b_chain[..3]);
    assert!(chain_out(&out).is_empty());
    assert_eq!(n.m.chain().tip().1, a_chain[2].block_hash());
    // One more block is.
    let out = n.headers(b, &b_chain[3..]);
    let joined: Vec<_> = b_chain
        .iter()
        .enumerate()
        .map(|(i, h)| (i as u32 + 1, h.block_hash()))
        .collect();
    let left: Vec<BlockHash> = a_chain.iter().rev().map(|h| h.block_hash()).collect();
    assert_eq!(chain_out(&out), vec![(joined, left)]);
    assert_eq!(n.m.chain().tip().1, b_chain[3].block_hash());
}

#[test]
fn nr_31_headers_before_a_reorganisation_in_one_message_are_a_separate_output() {
    let mut n = Net::new();
    let (a, b) = (n.up(1), n.up(2));
    let a_chain = extend(&genesis(), 2, 1);
    n.headers(a, &a_chain);
    // One message: two more on the old chain, then the other chain.
    let more = extend(&a_chain[1], 1, 50);
    let b_chain = extend(&genesis(), 4, 100);
    let mut message = more.clone();
    message.extend(b_chain.clone());
    let out = n.headers(b, &message);
    let changes = chain_out(&out);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0], (vec![(3, more[0].block_hash())], vec![]));
    assert_eq!(changes[1].0.len(), 4);
    assert_eq!(
        changes[1].1,
        vec![
            more[0].block_hash(),
            a_chain[1].block_hash(),
            a_chain[0].block_hash()
        ]
    );
}

#[test]
fn nr_31_a_header_dated_too_far_ahead_is_skipped_without_penalty_and_taken_later() {
    let mut n = Net::new();
    let a = n.up(1);
    let g = genesis();
    let future = mine(
        g.block_hash(),
        T0 as u32 + 10_000,
        0x207f_ffff,
        child(&g, 1).merkle_root,
    );
    let after = child(&future, 2);
    let out = n.headers(a, &[future, after]);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(n.m.chain().tip().0, 0);
    assert!(header_requests(&n.sent(a)).is_empty());
    // Offered again when the clock has caught up (WR-06).
    n.now += 4_000;
    let out = n.headers(a, &[future, after]);
    assert_eq!(chain_out(&out).len(), 1);
    assert_eq!(n.m.chain().tip().0, 2);
}

#[test]
fn nr_31_the_side_header_limits_are_skipped_without_penalty() {
    let mut n = Net::new();
    let a = n.up(1);
    n.headers(a, &extend(&genesis(), 10, 1));
    // More side branches than the store keeps (4,032): the peer stays.
    for i in 0..1_400u32 {
        let branch = extend(&genesis(), 3, 1_000 + 3 * i);
        let out = n.headers(a, &branch);
        assert!(disconnects(&out).is_empty(), "message {i}");
    }
    assert_eq!(n.m.sizes().side_headers, 4_032);
    assert_eq!(n.m.sizes().peers, 1);
    assert_eq!(n.m.chain().tip().0, 10);
}

#[test]
fn nr_32_an_unknown_parent_is_asked_again_once_a_minute() {
    let mut n = Net::new();
    let a = n.up(1);
    let g = genesis();
    let stranger = child(&child(&g, 5), 6); // its parent is not held
    let good = child(&g, 7);
    // The rest of the message after the stranger is dropped.
    let out = n.headers(a, &[stranger, good]);
    assert!(out.is_empty());
    assert_eq!(n.m.chain().tip().0, 0);
    assert_eq!(header_requests(&n.sent(a)), vec![vec![g.block_hash()]]);
    // Not twice within 60 seconds.
    n.tick(59);
    n.headers(a, &[stranger]);
    assert!(header_requests(&n.sent(a)).is_empty());
    n.tick(1);
    n.headers(a, &[stranger]);
    assert_eq!(header_requests(&n.sent(a)).len(), 1);
    // The limit is per peer.
    let b = n.up(2);
    n.headers(b, &[stranger]);
    assert_eq!(header_requests(&n.sent(b)).len(), 1);
}

#[test]
fn nr_32_an_announced_block_the_store_does_not_hold_makes_the_counter_ask() {
    let mut n = Net::new();
    let a = n.up(1);
    let hs = extend(&genesis(), 2, 1);
    n.headers(a, &hs);
    n.sent(a);
    // A block the store holds: nothing.
    n.inv_block(a, hs[1].block_hash());
    assert!(header_requests(&n.sent(a)).is_empty());
    // One it does not hold: getheaders from the locator.
    let unknown = child(&hs[1], 9).block_hash();
    n.inv_block(a, unknown);
    let asked = header_requests(&n.sent(a));
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0][0], hs[1].block_hash());
    // The same limit as for an unknown parent.
    n.tick(30);
    n.inv_block(a, unknown);
    assert!(header_requests(&n.sent(a)).is_empty());
    n.tick(30);
    n.inv_block(a, unknown);
    assert_eq!(header_requests(&n.sent(a)).len(), 1);
}

#[test]
fn nr_33_the_height_a_peer_shows_is_its_claim_or_what_it_delivered() {
    let mut n = Net::new();
    n.join(1, plain(), 3);
    n.join(2, plain(), 0);
    let hs = extend(&genesis(), 7, 1);
    n.headers(PeerId(2), &hs);
    let heights: Vec<u32> = n.m.health_peers().iter().map(|p| p.best_height).collect();
    // Peer 1 claimed 3 and delivered nothing; peer 2 claimed 0 and showed 7.
    assert_eq!(heights, vec![3, 7]);
    // Delivering fewer than the claim does not lower it.
    n.join(3, plain(), 20);
    n.headers(PeerId(3), &hs[..2]);
    let heights: Vec<u32> = n.m.health_peers().iter().map(|p| p.best_height).collect();
    assert_eq!(heights, vec![3, 7, 20]);
}

#[test]
fn nr_33_ahead_since_is_set_when_a_peer_claims_more_than_two_above_the_tip() {
    let mut n = Net::new();
    assert_eq!(n.m.ahead_since(), None);
    // Within 2 of the tip: not ahead.
    n.join(1, plain(), 2);
    assert_eq!(n.m.ahead_since(), None);
    // 3 above: ahead from now on.
    let t = n.now;
    n.join(2, plain(), 3);
    assert_eq!(n.m.ahead_since(), Some(t));
    // A later peer that is ahead does not move it.
    n.tick(100);
    n.join(3, plain(), 50);
    assert_eq!(n.m.ahead_since(), Some(t));
    // The claim stands while the headers are not delivered.
    n.tick(400);
    assert_eq!(n.m.ahead_since(), Some(t));
}

#[test]
fn nr_33_a_claim_with_no_headers_behind_it_ends_the_peer_after_ten_minutes() {
    let mut n = Net::new();
    n.join(1, plain(), 0);
    n.join(2, plain(), 50);
    let out = n.tick(599);
    assert!(disconnects(&out).is_empty(), "{out:?}");
    let out = n.tick(1);
    assert_eq!(disconnects(&out), vec![(PeerId(2), true)]);
    assert!(reason(&out).starts_with("NR-33"), "{}", reason(&out));
    // The peer no longer holds the check back.
    assert_eq!(n.m.ahead_since(), None);
}

#[test]
fn nr_33_a_peer_whose_headers_the_store_cannot_take_yet_is_kept_and_counts_as_ahead() {
    let mut n = Net::new();
    let t = n.now;
    let a = n.join(1, plain(), 5).id;
    let g = genesis();
    // WR-06: dated too far ahead. Not the peer's doing.
    let future = mine(
        g.block_hash(),
        T0 as u32 + 100_000,
        0x207f_ffff,
        child(&g, 1).merkle_root,
    );
    let out = n.headers(a, &[future]);
    assert!(out.is_empty(), "{out:?}");
    let out = n.tick(5_000);
    assert!(disconnects(&out).is_empty(), "{out:?}");
    assert_eq!(n.m.ahead_since(), Some(t));
}

#[test]
fn nr_33_ahead_since_is_cleared_when_the_headers_arrive() {
    let mut n = Net::new();
    let t = n.now;
    n.join(1, plain(), 10);
    n.join(2, plain(), 0);
    assert_eq!(n.m.ahead_since(), Some(t));
    n.tick(100);
    // At tip 7, 10 is still above 7 + 2; at tip 8 it is not.
    let hs = extend(&genesis(), 8, 1);
    n.headers(PeerId(2), &hs[..7]);
    assert_eq!(n.m.ahead_since(), Some(t));
    n.headers(PeerId(2), &hs[7..]);
    assert_eq!(n.m.ahead_since(), None);
    // A taller claim comes back with a new time.
    n.tick(10);
    let t2 = n.now;
    n.join(3, plain(), 30);
    assert_eq!(n.m.ahead_since(), Some(t2));
    // A peer that closes no longer counts.
    n.m.closed(PeerId(3), n.now);
    assert_eq!(n.m.ahead_since(), None);
}

// ----- NR-40 to NR-46: filters and blocks ---------------------------------

#[test]
fn nr_40_with_no_script_watched_nothing_is_asked() {
    let mut n = Net::new();
    let (a, b) = (n.up_filters(1), n.up_filters(2));
    let blocks = blocks_on(&genesis(), vec![vec![]; 3], 1);
    n.headers(a, &headers_of(&blocks));
    for id in [a, b] {
        let sent = n.sent(id);
        assert!(filter_requests(&sent).is_empty());
        assert!(block_requests(&sent).is_empty());
    }
    assert_eq!(n.m.checked_height(), 3);
    assert_eq!(n.m.sizes().work, 0);
}

#[test]
fn nr_40_filter_peers_take_turns_by_block_height() {
    let mut n = Net::new();
    let peers: Vec<PeerId> = (1..=3).map(|i| n.up_filters(i)).collect();
    assert!(n.m.watch_script(script(1)));
    let hs = extend(&genesis(), 3, 1);
    n.headers(peers[0], &hs);
    let asked: Vec<Vec<u32>> = peers
        .iter()
        .map(|id| {
            filter_requests(&n.sent(*id))
                .into_iter()
                .map(|(height, _)| height)
                .collect()
        })
        .collect();
    // Height 1 starts with the second peer, height 2 with the third, and
    // height 3 with the first: each peer is left out of one block.
    assert_eq!(asked, vec![vec![2, 3], vec![1, 3], vec![1, 2]]);
}

#[test]
fn nr_40_two_peers_in_different_groups_are_asked_for_each_filter() {
    let mut r = Rig::new(vec![vec![]; 3]);
    let want: Vec<_> = (1..=3).map(|h| (h as u32, r.hash(h))).collect();
    assert_eq!(filter_requests(&r.n.sent(r.a)), want);
    assert_eq!(filter_requests(&r.n.sent(r.b)), want);
    // The peer without filters is asked for no filter and no block.
    let sent = r.n.sent(r.c);
    assert!(filter_requests(&sent).is_empty());
    assert!(block_requests(&sent).is_empty());
    assert_eq!(r.n.m.checked_height(), 0);
}

#[test]
fn nr_40_blocks_at_or_below_the_starting_height_are_never_asked() {
    let mut r = Rig::with(2, vec![vec![]; 4]);
    let want = vec![(3, r.hash(3)), (4, r.hash(4))];
    assert_eq!(filter_requests(&r.n.sent(r.a)), want);
    assert_eq!(r.n.m.checked_height(), 2);
}

#[test]
fn nr_40_at_most_eight_blocks_are_in_work_at_once() {
    let mut r = Rig::new(vec![vec![]; 20]);
    let first: Vec<_> = (1..=8).map(|h| (h as u32, r.hash(h))).collect();
    assert_eq!(filter_requests(&r.n.sent(r.a)), first);
    assert_eq!(r.n.m.sizes().work, 8);
    // A block done makes room for the next.
    r.both_honest(1);
    assert_eq!(filter_requests(&r.n.sent(r.a)), vec![(9, r.hash(9))]);
    assert_eq!(r.n.m.sizes().work, 8);
    assert_eq!(r.n.m.checked_height(), 1);
    // Out of order: block 3 done leaves 2 open.
    r.both_honest(3);
    assert_eq!(r.n.m.checked_height(), 1);
    r.both_honest(2);
    assert_eq!(r.n.m.checked_height(), 3);
}

#[test]
fn nr_41_the_watched_scripts_are_limited() {
    let mut n = Net::new();
    for i in 0..100_000u32 {
        let mut bytes = vec![0x00, 0x14];
        bytes.extend(i.to_le_bytes());
        assert!(n.m.watch_script(ScriptBuf::from_bytes(bytes)));
    }
    assert_eq!(n.m.sizes().scripts, 100_000);
    assert!(!n.m.watch_script(script(1)));
    // A script already watched is still fine to name again.
    assert!(n.m.watch_script(ScriptBuf::from_bytes({
        let mut b = vec![0x00, 0x14];
        b.extend(5u32.to_le_bytes());
        b
    })));
    assert_eq!(n.m.sizes().scripts, 100_000);
}

#[test]
fn nr_41_a_filter_is_matched_against_every_watched_script() {
    // The second script is the one the block pays.
    let mut r = Rig::new(vec![vec![pay(&script(9), 1_000, 1)]]);
    assert!(r.n.m.watch_script(script(9)));
    r.both_honest(1);
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![r.hash(1)]);
}

#[test]
fn nr_41_a_filter_that_cannot_be_read_counts_as_a_match() {
    for junk in [
        vec![],
        vec![0xff; 9],
        vec![0xfd, 0xff, 0xff, 1, 2, 3],
        vec![5, 1],
    ] {
        let mut r = Rig::new(vec![vec![]]);
        let hash = r.hash(1);
        r.n.filter(r.a, hash, filter_of(r.at(1)));
        let out = r.n.filter(r.b, hash, junk.clone());
        assert!(disconnects(&out).is_empty(), "{junk:?}");
        // Matched, so the block is fetched; the junk is not "proven false".
        assert_eq!(block_requests(&r.n.sent(r.a)), vec![hash], "{junk:?}");
    }
}

#[test]
fn nr_42_equal_filters_that_match_nothing_need_no_block() {
    let mut r = Rig::new(vec![vec![]; 3]);
    for h in 1..=3 {
        r.both_honest(h);
    }
    for id in [r.a, r.b, r.c] {
        assert!(block_requests(&r.n.sent(id)).is_empty());
    }
    assert_eq!(r.n.m.checked_height(), 3);
    assert_eq!(r.n.m.sizes().work, 0);
}

#[test]
fn nr_42_a_match_makes_the_counter_fetch_the_block() {
    let mut r = Rig::new(vec![vec![], vec![pay(&script(7), 5_000, 1)], vec![]]);
    r.both_honest(1);
    let out = r.both_honest(2);
    assert!(out.is_empty());
    r.both_honest(3);
    // The block is asked of the lowest ready peer.
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![r.hash(2)]);
    assert!(block_requests(&r.n.sent(r.b)).is_empty());
    assert_eq!(r.n.m.checked_height(), 1);
    let block = r.at(2).clone();
    let out = r.n.block(r.a, &block);
    assert_eq!(
        out,
        vec![Output::Block {
            hash: block.block_hash(),
            bytes: bytes_of(&block)
        }]
    );
    assert_eq!(r.n.m.checked_height(), 3);
}

#[test]
fn nr_42_filters_that_differ_make_the_counter_fetch_the_block() {
    let mut r = Rig::new(vec![vec![]; 3]);
    let hash = r.hash(1);
    r.n.filter(r.a, hash, filter_of(r.at(1)));
    // A different filter that matches nothing either: it holds another
    // script.
    let other = filter_with(r.at(1), vec![pay(&script(3), 1_000, 1)]);
    assert_ne!(other, filter_of(r.at(1)));
    r.n.filter(r.b, hash, other);
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![hash]);
    // The block pays nothing watched: nobody is accused.
    let block = r.at(1).clone();
    let out = r.n.block(r.a, &block);
    assert_eq!(blocks_out(&out), vec![hash]);
    assert!(disconnects(&out).is_empty());
}

#[test]
fn nr_42_with_fewer_than_two_filter_peers_the_block_is_fetched() {
    let mut n = Net::new();
    n.m.watch_script(script(7));
    let a = n.up_filters(1);
    let b = n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]], 1);
    n.headers(b, &headers_of(&blocks));
    let sent = n.sent(a);
    assert!(filter_requests(&sent).is_empty());
    assert_eq!(block_requests(&sent), vec![blocks[0].block_hash()]);
}

#[test]
fn nr_42_sixty_seconds_without_two_answers_the_block_is_fetched() {
    let mut r = Rig::new(vec![vec![]]);
    let hash = r.hash(1);
    // One answers, one stays silent.
    r.n.filter(r.a, hash, filter_of(r.at(1)));
    assert!(r.n.tick(59).is_empty());
    assert!(block_requests(&r.n.sent(r.a)).is_empty());
    let out = r.n.tick(1);
    assert!(disconnects(&out).is_empty());
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![hash]);
    // The silent peer is not dropped for it, and its late filter is no
    // unasked data.
    let out = r.n.filter(r.b, hash, filter_of(r.at(1)));
    assert!(disconnects(&out).is_empty());
    assert_eq!(r.n.m.sizes().peers, 3);
    // But a second late one is.
    let out = r.n.filter(r.b, hash, filter_of(r.at(1)));
    assert_eq!(disconnects(&out), vec![(r.b, true)]);
}

#[test]
fn nr_42_a_replacement_filter_peer_is_asked_when_one_goes() {
    let mut r = Rig::new(vec![vec![]]);
    let d = r.n.up_filters(4);
    let hash = r.hash(1);
    // a and b were asked; d was not.
    assert!(filter_requests(&r.n.sent(d)).is_empty());
    r.n.m.closed(r.b, r.n.now);
    assert_eq!(filter_requests(&r.n.sent(d)), vec![(1, hash)]);
}

#[test]
fn nr_43_a_filter_that_hides_a_payment_is_proven_false_and_the_peer_dropped() {
    let mut r = Rig::new(vec![vec![], vec![pay(&script(7), 5_000, 1)]]);
    r.both_honest(1);
    let hash = r.hash(2);
    // a is honest; b hides the payment: its filter is the block's without
    // the payment.
    r.n.filter(r.a, hash, filter_of(r.at(2)));
    r.n.filter(r.b, hash, filter_with(r.at(2), vec![]));
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![hash]);
    let block = r.at(2).clone();
    let out = r.n.block(r.a, &block);
    // The block is still delivered, and b is dropped as having misbehaved.
    assert_eq!(blocks_out(&out), vec![hash]);
    assert_eq!(disconnects(&out), vec![(r.b, true)]);
    assert!(reason(&out).starts_with("NR-25: "));
    assert!(reason(&out).contains("NR-43"));
    assert_eq!(r.n.m.sizes().peers, 2);
}

#[test]
fn nr_43_both_filters_hiding_a_payment_are_both_dropped() {
    let mut r = Rig::new(vec![vec![], vec![pay(&script(7), 5_000, 1)]]);
    r.both_honest(1);
    let hash = r.hash(2);
    // Different bytes (so the block is fetched), both without the match.
    r.n.filter(r.a, hash, filter_with(r.at(2), vec![]));
    r.n.filter(r.b, hash, vec![0]);
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![hash]);
    let block = r.at(2).clone();
    let out = r.n.block(r.a, &block);
    let mut gone = disconnects(&out);
    gone.sort();
    assert_eq!(gone, vec![(r.a, true), (r.b, true)]);
    assert_eq!(blocks_out(&out), vec![hash]);
}

#[test]
fn nr_43_a_matching_filter_for_a_block_that_pays_nothing_accuses_nobody() {
    let mut r = Rig::new(vec![vec![], vec![]]);
    let hash = r.hash(2);
    // a's filter holds the watched script, which the block does not pay
    // (a false positive, or a lie that cannot be told from one).
    r.n.filter(
        r.a,
        hash,
        filter_with(r.at(2), vec![pay(&script(7), 5_000, 1)]),
    );
    r.n.filter(r.b, hash, filter_of(r.at(2)));
    assert_eq!(block_requests(&r.n.sent(r.a)), vec![hash]);
    let block = r.at(2).clone();
    let out = r.n.block(r.a, &block);
    assert!(disconnects(&out).is_empty());
    assert_eq!(blocks_out(&out), vec![hash]);
}

#[test]
fn nr_44_with_a_payment_waiting_every_block_is_fetched_without_a_filter() {
    let mut n = Net::new();
    let a = n.up_filters(1);
    let b = n.up_filters(2);
    n.m.watch_script(script(7));
    n.m.set_all_blocks(true);
    let blocks = blocks_on(&genesis(), vec![vec![]; 2], 1);
    n.headers(b, &headers_of(&blocks));
    let want: Vec<BlockHash> = blocks.iter().map(|b| b.block_hash()).collect();
    let sent = n.sent(a);
    assert!(filter_requests(&sent).is_empty());
    assert_eq!(block_requests(&sent), want);
    assert!(filter_requests(&n.sent(b)).is_empty());
    // Without a script too.
    let mut n = Net::new();
    n.m.set_all_blocks(true);
    let a = n.up(1);
    n.headers(a, &headers_of(&blocks));
    assert_eq!(block_requests(&n.sent(a)), want);
}

#[test]
fn nr_44_turning_it_on_takes_the_blocks_that_wait_for_filters() {
    let mut r = Rig::new(vec![vec![]; 2]);
    assert_eq!(filter_requests(&r.n.sent(r.a)).len(), 2);
    r.n.m.set_all_blocks(true);
    // The requests go out with the next call that returns outputs; the
    // bytes are queued already.
    let want = vec![r.hash(1), r.hash(2)];
    assert_eq!(block_requests(&r.n.sent(r.a)), want);
    // Late filters for the blocks are not unasked data.
    let hash = r.hash(1);
    let out = r.n.filter(r.a, hash, filter_of(r.at(1)));
    assert!(disconnects(&out).is_empty());
    // Turned off again, new blocks go back to filters.
    r.n.m.set_all_blocks(false);
    let more = blocks_on(&r.at(2).header, vec![vec![]], 50);
    r.n.headers(r.c, &headers_of(&more));
    assert_eq!(
        filter_requests(&r.n.sent(r.a)),
        vec![(3, more[0].block_hash())]
    );
}

fn all_blocks_net(k: usize) -> (Net, PeerId, PeerId, Vec<bitcoin::Block>) {
    let mut n = Net::new();
    n.m.set_all_blocks(true);
    let a = n.up(1);
    let b = n.up(2);
    let blocks = blocks_on(&genesis(), vec![vec![]; k], 1);
    n.headers(a, &headers_of(&blocks));
    (n, a, b, blocks)
}

#[test]
fn nr_45_a_block_not_delivered_in_sixty_seconds_is_asked_of_another_peer() {
    let (mut n, a, b, blocks) = all_blocks_net(1);
    let hash = blocks[0].block_hash();
    assert_eq!(block_requests(&n.sent(a)), vec![hash]);
    assert!(n.tick(59).is_empty());
    assert!(block_requests(&n.sent(b)).is_empty());
    let out = n.tick(1);
    assert!(disconnects(&out).is_empty());
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    assert!(block_requests(&n.sent(a)).is_empty());
    // b delivers it.
    let out = n.block(b, &blocks[0]);
    assert_eq!(blocks_out(&out), vec![hash]);
    // The slow peer's late copy is no unasked data.
    let out = n.block(a, &blocks[0]);
    assert!(out.is_empty());
    assert_eq!(n.m.sizes().peers, 2);
}

#[test]
fn nr_45_not_found_moves_on_at_once() {
    let (mut n, a, b, blocks) = all_blocks_net(1);
    let hash = blocks[0].block_hash();
    n.sent(a);
    let out = n.not_found_block(a, hash);
    assert!(out.is_empty());
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    // Both failed: no spinning. The pause is a minute, then it starts over.
    n.not_found_block(b, hash);
    assert!(block_requests(&n.sent(a)).is_empty());
    assert!(block_requests(&n.sent(b)).is_empty());
    n.tick(59);
    assert!(block_requests(&n.sent(a)).is_empty());
    n.tick(1);
    assert_eq!(block_requests(&n.sent(a)), vec![hash]);
    let out = n.block(a, &blocks[0]);
    assert_eq!(blocks_out(&out), vec![hash]);
}

#[test]
fn nr_45_a_peer_cannot_substitute_another_block() {
    let (mut n, a, b, blocks) = all_blocks_net(1);
    let hash = blocks[0].block_hash();
    n.sent(a);
    // A block that is on no chain of the counter's.
    let other = block_on(&genesis(), vec![], 99);
    let out = n.block(a, &other);
    assert_eq!(disconnects(&out), vec![(a, true)]);
    assert!(reason(&out).contains("other than the one asked for"));
    // The right block is asked of the other peer.
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    let out = n.block(b, &blocks[0]);
    assert_eq!(blocks_out(&out), vec![hash]);
}

#[test]
fn nr_45_a_block_that_fails_the_watchers_checks_is_a_misbehaviour() {
    let (mut n, a, b, blocks) = all_blocks_net(1);
    let hash = blocks[0].block_hash();
    n.sent(a);
    // Same header, other transactions: the merkle root is wrong (WR-21).
    let mut bad = blocks[0].clone();
    bad.txdata.push(pay(&script(1), 1_000, 5));
    let out = n.block(a, &bad);
    assert_eq!(disconnects(&out), vec![(a, true)]);
    assert!(
        reason(&out).starts_with("NR-25: block refused: WR-21"),
        "{}",
        reason(&out)
    );
    assert!(blocks_out(&out).is_empty());
    // Another peer is asked.
    assert_eq!(block_requests(&n.sent(b)), vec![hash]);
    let out = n.block(b, &blocks[0]);
    assert_eq!(blocks_out(&out), vec![hash]);
    assert_eq!(n.m.sizes().work, 0);
}

#[test]
fn nr_45_a_peer_that_closes_hands_its_block_to_another() {
    let (mut n, a, b, blocks) = all_blocks_net(2);
    assert_eq!(block_requests(&n.sent(a)).len(), 2);
    let out = n.m.closed(a, n.now);
    assert!(out.is_empty());
    let want: Vec<BlockHash> = blocks.iter().map(|b| b.block_hash()).collect();
    assert_eq!(block_requests(&n.sent(b)), want);
}

#[test]
fn nr_45_a_block_is_handed_out_only_when_the_chain_holds_it() {
    // WR-01: a block that is no longer on the chain is not delivered.
    let (mut n, a, b, blocks) = all_blocks_net(2);
    n.sent(a);
    // A longer chain from the other peer replaces both blocks.
    let better = extend(&genesis(), 3, 100);
    let out = n.headers(b, &better);
    assert_eq!(chain_out(&out).len(), 1);
    // The old blocks are no longer wanted; a's late answers are tolerated
    // and give nothing.
    for block in &blocks {
        let out = n.block(a, block);
        assert!(out.is_empty(), "{out:?}");
    }
    // The new ones are asked for.
    let want: Vec<BlockHash> = better.iter().map(|h| h.block_hash()).collect();
    assert_eq!(block_requests(&n.sent(a)), want);
}

#[test]
fn nr_45_a_clock_that_goes_backwards_does_not_panic() {
    let (mut n, a, _b, _blocks) = all_blocks_net(1);
    n.sent(a);
    // Earlier than the time the request was made: nothing runs out.
    assert!(n.m.tick(5).is_empty());
    assert!(n.m.received(a, &[], 0).is_empty());
    assert!(n.m.tick(0).is_empty());
    assert_eq!(n.m.sizes().peers, 2);
    assert!(n.sent(a).is_empty());
}

#[test]
fn nr_46_a_reorganisation_starts_the_checks_again_from_the_fork() {
    let mut r = Rig::new(vec![vec![]; 4]);
    for h in 1..=4 {
        r.both_honest(h);
    }
    r.n.sent(r.a);
    r.n.sent(r.b);
    assert_eq!(r.n.m.checked_height(), 4);
    // A chain that forks after block 2 and gets longer.
    let other = blocks_on(&r.at(2).header, vec![vec![]; 3], 100);
    let out = r.n.headers(r.c, &headers_of(&other));
    let changes = chain_out(&out);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].1, vec![r.hash(4), r.hash(3)]);
    assert_eq!(changes[0].0.len(), 3);
    assert_eq!(changes[0].0[0], (3, other[0].block_hash()));
    assert_eq!(r.n.m.checked_height(), 2);
    // Blocks 3 to 5 of the new chain are asked for, and nothing below.
    let want: Vec<_> = other
        .iter()
        .enumerate()
        .map(|(i, b)| (i as u32 + 3, b.block_hash()))
        .collect();
    assert_eq!(filter_requests(&r.n.sent(r.a)), want);
    assert_eq!(filter_requests(&r.n.sent(r.b)), want);
}

#[test]
fn nr_46_work_in_progress_for_blocks_that_left_is_dropped() {
    let mut r = Rig::new(vec![vec![]; 4]);
    r.both_honest(1);
    r.both_honest(2);
    r.n.sent(r.a);
    r.n.sent(r.b);
    // Blocks 3 and 4 are waiting for filters when the chain changes.
    let (old3, old4) = (r.hash(3), r.hash(4));
    let other = blocks_on(&r.at(2).header, vec![vec![]; 3], 100);
    r.n.headers(r.c, &headers_of(&other));
    assert_eq!(r.n.m.sizes().work, 3);
    // Their late filters are no unasked data, and finish nothing.
    let f3 = filter_of(r.at(3));
    let f4 = filter_of(r.at(4));
    for (id, hash, f) in [(r.a, old3, &f3), (r.b, old3, &f3), (r.a, old4, &f4)] {
        let out = r.n.filter(id, hash, f.clone());
        assert!(disconnects(&out).is_empty());
    }
    assert_eq!(r.n.m.sizes().peers, 3);
    assert_eq!(r.n.m.checked_height(), 2);
    // The new blocks finish the usual way.
    for block in &other {
        let (hash, f) = (block.block_hash(), filter_of(block));
        r.n.filter(r.a, hash, f.clone());
        r.n.filter(r.b, hash, f);
    }
    assert_eq!(r.n.m.checked_height(), 5);
}

#[test]
fn nr_46_the_checks_never_go_back_below_the_starting_height() {
    let mut r = Rig::with(3, vec![vec![]; 5]);
    r.n.sent(r.a);
    // The fork is after block 1: below the starting height.
    let other = blocks_on(&r.at(1).header, vec![vec![]; 5], 100);
    r.n.headers(r.c, &headers_of(&other));
    let want: Vec<_> = other
        .iter()
        .enumerate()
        .skip(2)
        .map(|(i, b)| (i as u32 + 2, b.block_hash()))
        .collect();
    assert_eq!(want[0].0, 4);
    assert_eq!(filter_requests(&r.n.sent(r.a)), want);
    assert_eq!(r.n.m.checked_height(), 3);
}

// ----- NR-50 to NR-55: unconfirmed transactions ---------------------------

fn three_peers() -> (Net, PeerId, PeerId, PeerId) {
    let mut n = Net::new();
    let (a, b, c) = (n.up(1), n.up(2), n.up(3));
    (n, a, b, c)
}

#[test]
fn nr_50_every_announcement_is_recorded_with_the_peer_that_made_it() {
    let (mut n, a, b, _c) = three_peers();
    n.m.set_fetch_transactions(false);
    let (t1, t2) = (pay(&script(1), 1_000, 1), pay(&script(1), 1_000, 2));
    n.inv_txs(a, &[t1.compute_txid(), t2.compute_txid()]);
    n.inv_txs(b, &[t2.compute_txid()]);
    // Announcements by the same peer twice are one.
    n.inv_txs(b, &[t2.compute_txid()]);
    n.m.set_fetch_transactions(true);
    let asked = tx_requests(&n.sent(a));
    assert_eq!(asked, vec![t1.compute_txid(), t2.compute_txid()]);
    let from = |out: &[Output]| match &out[0] {
        Output::Sighting { from, .. } => from.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(from(&n.tx(a, &t1)), vec![addr(1)]);
    assert_eq!(from(&n.tx(a, &t2)), vec![addr(1), addr(2)]);
}

#[test]
fn nr_51_an_announced_transaction_is_asked_for_from_the_first_announcer() {
    let (mut n, a, b, _c) = three_peers();
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(a, &[txid]);
    assert_eq!(tx_requests(&n.sent(a)), vec![txid]);
    // A second announcer is recorded and asked nothing.
    n.inv_txs(b, &[txid]);
    assert!(tx_requests(&n.sent(b)).is_empty());
    // Nothing is asked twice.
    n.inv_txs(a, &[txid]);
    assert!(tx_requests(&n.sent(a)).is_empty());
}

#[test]
fn nr_53_a_fetched_transaction_is_a_sighting_from_every_announcer() {
    let (mut n, a, b, c) = three_peers();
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(a, &[txid]);
    n.inv_txs(b, &[txid]);
    n.sent(a);
    let out = n.tx(a, &tx);
    assert_eq!(
        out,
        vec![Output::Sighting {
            tx: bitcoin::consensus::encode::serialize(&tx),
            from: vec![addr(1), addr(2)],
        }]
    );
    // A later announcement by another peer is a further sighting.
    let out = n.inv_txs(c, &[txid]);
    assert_eq!(
        out,
        vec![Output::AlsoAnnounced {
            txid,
            from: addr(3)
        }]
    );
    // The same peer again adds nothing, and nothing is fetched twice.
    assert!(n.inv_txs(c, &[txid]).is_empty());
    for id in [a, b, c] {
        assert!(tx_requests(&n.sent(id)).is_empty());
    }
}

#[test]
fn nr_51_thirty_seconds_without_an_answer_the_next_announcer_is_asked() {
    let (mut n, a, b, c) = three_peers();
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(a, &[txid]);
    n.inv_txs(b, &[txid]);
    n.inv_txs(c, &[txid]);
    n.sent(a);
    assert!(n.tick(29).is_empty());
    assert!(tx_requests(&n.sent(b)).is_empty());
    let out = n.tick(1);
    assert!(disconnects(&out).is_empty());
    assert_eq!(tx_requests(&n.sent(b)), vec![txid]);
    // And on to the third.
    n.tick(30);
    assert_eq!(tx_requests(&n.sent(c)), vec![txid]);
    // c answers: all three announced it.
    let out = n.tx(c, &tx);
    let Output::Sighting { from, .. } = &out[0] else {
        panic!("{out:?}");
    };
    assert_eq!(from, &vec![addr(1), addr(2), addr(3)]);
    // The first announcer's late answer is not unasked data.
    assert!(n.tx(a, &tx).is_empty());
    assert_eq!(n.m.sizes().peers, 3);
}

#[test]
fn nr_51_not_found_moves_on_at_once() {
    let (mut n, a, b, _c) = three_peers();
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(a, &[txid]);
    n.inv_txs(b, &[txid]);
    n.sent(a);
    n.not_found_tx(a, txid);
    assert_eq!(tx_requests(&n.sent(b)), vec![txid]);
    // The last announcer has none either: nothing is asked again.
    n.not_found_tx(b, txid);
    assert!(tx_requests(&n.sent(a)).is_empty());
    assert!(tx_requests(&n.sent(b)).is_empty());
    n.tick(100);
    assert!(tx_requests(&n.sent(a)).is_empty());
    // A new announcer is asked.
    let c = PeerId(3);
    n.inv_txs(c, &[txid]);
    assert_eq!(tx_requests(&n.sent(c)), vec![txid]);
}

#[test]
fn nr_51_a_peer_that_closes_hands_its_request_to_the_next_announcer() {
    let (mut n, a, b, _c) = three_peers();
    let txid = pay(&script(1), 1_000, 1).compute_txid();
    n.inv_txs(a, &[txid]);
    n.inv_txs(b, &[txid]);
    n.sent(a);
    n.m.closed(a, n.now);
    assert_eq!(tx_requests(&n.sent(b)), vec![txid]);
}

#[test]
fn nr_51_a_transaction_that_was_not_asked_of_that_peer_is_a_misbehaviour() {
    let (mut n, a, b, _c) = three_peers();
    let (t1, t2) = (pay(&script(1), 1_000, 1), pay(&script(1), 1_000, 2));
    n.inv_txs(a, &[t1.compute_txid()]);
    n.inv_txs(b, &[t1.compute_txid()]);
    // b was not asked for t1 (a was).
    let out = n.tx(b, &t1);
    assert_eq!(disconnects(&out), vec![(b, true)]);
    // a was asked for t1 and sends t2.
    let out = n.tx(a, &t2);
    assert_eq!(disconnects(&out), vec![(a, true)]);
    assert!(reason(&out).starts_with("NR-25: "));
}

#[test]
fn nr_52_while_fetching_is_off_announcements_are_remembered_and_nothing_is_asked() {
    let (mut n, a, b, _c) = three_peers();
    n.m.set_fetch_transactions(false);
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(a, &[txid]);
    n.inv_txs(b, &[txid]);
    n.tick(100);
    for id in [a, b] {
        assert!(tx_requests(&n.sent(id)).is_empty());
    }
    // Turned on: the remembered announcement is asked for, from the first
    // announcer, with no need to be announced again.
    n.m.set_fetch_transactions(true);
    assert_eq!(tx_requests(&n.sent(a)), vec![txid]);
    let out = n.tx(a, &tx);
    let Output::Sighting { from, .. } = &out[0] else {
        panic!("{out:?}");
    };
    assert_eq!(from, &vec![addr(1), addr(2)]);
    // Off again: a new one is remembered and not asked.
    n.m.set_fetch_transactions(false);
    let t2 = pay(&script(1), 1_000, 2).compute_txid();
    n.inv_txs(a, &[t2]);
    assert!(tx_requests(&n.sent(a)).is_empty());
}

#[test]
fn nr_54_only_the_last_100000_announced_ids_are_remembered() {
    let mut n = Net::new();
    let a = n.up(1);
    n.m.set_fetch_transactions(false);
    for k in 0..2u64 {
        let ids: Vec<Txid> = (k * 50_000..(k + 1) * 50_000).map(fake_txid).collect();
        n.inv_txs(a, &ids);
    }
    n.inv_txs(a, &[fake_txid(100_000)]);
    let sizes = n.m.sizes();
    assert_eq!(sizes.txs, 100_000);
    assert_eq!(sizes.tx_order, 100_000);
    // Turned on, the first 1,000 that are remembered are asked of the one
    // peer: the oldest, number 0, is forgotten.
    n.m.set_fetch_transactions(true);
    let asked = tx_requests(&n.sent(a));
    assert_eq!(asked.len(), 1_000);
    assert_eq!(asked[0], fake_txid(1));
    assert_eq!(asked[999], fake_txid(1_000));
    assert!(!asked.contains(&fake_txid(0)));
}

#[test]
fn nr_51_at_most_5000_requests_are_pending_and_room_frees_as_they_end() {
    let mut n = Net::new();
    let peers: Vec<PeerId> = (1..=6).map(|i| n.up(i)).collect();
    // Five peers fill the 5,000 places, 1,000 each; the sixth has to wait.
    let mut first = None;
    for (k, id) in peers.iter().take(5).enumerate() {
        let txs: Vec<_> = (0..1_000u32)
            .map(|i| pay(&script(1), 1_000, k as u32 * 1_000 + i))
            .collect();
        let ids: Vec<Txid> = txs.iter().map(|t| t.compute_txid()).collect();
        n.inv_txs(*id, &ids);
        assert_eq!(tx_requests(&n.sent(*id)), ids);
        first.get_or_insert((*id, txs[0].clone()));
    }
    assert_eq!(n.m.sizes().pending_tx, 5_000);
    let late = pay(&script(1), 1_000, 9_999).compute_txid();
    n.inv_txs(peers[5], &[late]);
    assert!(tx_requests(&n.sent(peers[5])).is_empty());
    // One answer makes room for the one that waited.
    let (id, tx) = first.unwrap();
    n.tx(id, &tx);
    assert_eq!(tx_requests(&n.sent(peers[5])), vec![late]);
    assert_eq!(n.m.sizes().pending_tx, 5_000);
}

#[test]
fn nr_51_one_peer_holds_at_most_a_thousand_requests_and_cannot_starve_the_others() {
    let mut n = Net::new();
    let a = n.up(1);
    let b = n.up(2);
    // Peer a announces far more than its share and answers nothing.
    let junk: Vec<Txid> = (0..6_000u64).map(fake_txid).collect();
    n.inv_txs(a, &junk);
    assert_eq!(tx_requests(&n.sent(a)).len(), 1_000);
    assert_eq!(n.m.sizes().pending_tx, 1_000);
    // A transaction from another peer is asked for at once.
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(b, &[txid]);
    assert_eq!(tx_requests(&n.sent(b)), vec![txid]);
    let out = n.tx(b, &tx);
    assert!(matches!(out[..], [Output::Sighting { .. }]), "{out:?}");
    // What waits behind a full share is not lost: it is asked for as the
    // share frees.
    assert_eq!(n.m.sizes().waiting, 5_000);
    n.not_found_tx(a, junk[0]);
    assert_eq!(tx_requests(&n.sent(a)), vec![junk[1_000]]);
}

#[test]
fn nr_54_a_transaction_fetched_is_not_fetched_again() {
    let (mut n, a, b, _c) = three_peers();
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.inv_txs(a, &[txid]);
    n.sent(a);
    n.tx(a, &tx);
    // Even from a peer that announces it much later.
    n.tick(1_000);
    n.inv_txs(b, &[txid]);
    assert!(tx_requests(&n.sent(b)).is_empty());
}

#[test]
fn nr_53_the_announcers_of_one_transaction_are_limited_to_eight() {
    let mut n = Net::new();
    let ids: Vec<PeerId> = (1..=8).map(|i| n.up(i)).collect();
    let tx = pay(&script(1), 1_000, 1);
    let txid = tx.compute_txid();
    n.m.set_fetch_transactions(false);
    for id in &ids {
        n.inv_txs(*id, &[txid]);
    }
    // Only 8 peers can be connected, so a ninth announcer needs one that
    // left and a new one that came.
    n.m.closed(ids[7], n.now);
    let ninth = n.up(9);
    assert!(n.inv_txs(ninth, &[txid]).is_empty());
    n.m.set_fetch_transactions(true);
    let out = n.tx(ids[0], &tx);
    // The ninth announcement was not recorded: the eight first are
    // reported, the one that left included.
    let Output::Sighting { from, .. } = &out[0] else {
        panic!("{out:?}");
    };
    let want: Vec<PeerAddr> = (1..=8).map(addr).collect();
    assert_eq!(from, &want);
    // And a further announcer of a fetched transaction is not reported
    // once the eight are full.
    assert!(n.inv_txs(ninth, &[txid]).is_empty());
}

// ----- NR-55: the manager only asks ---------------------------------------

#[test]
fn nr_55_requests_from_peers_are_never_answered() {
    let mut n = Net::new();
    let a = n.up(1);
    let hs = extend(&genesis(), 2, 1);
    n.headers(a, &hs);
    n.sent(a);
    let g = genesis().block_hash();
    // getdata, getheaders, getaddr, mempool: unknown to the session or
    // skipped; none gets an answer.
    let requests = [
        NetworkMessage::GetData(vec![
            bitcoin::p2p::message_blockdata::Inventory::Transaction(fake_txid(1)),
        ]),
        NetworkMessage::GetHeaders(bitcoin::p2p::message_blockdata::GetHeadersMessage::new(
            vec![g],
            g,
        )),
        NetworkMessage::GetAddr,
        NetworkMessage::MemPool,
    ];
    for request in requests {
        let out = n.feed(a, request);
        assert!(out.is_empty());
    }
    n.tick(1);
    assert!(n.sent(a).is_empty());
    // Only a ping is answered, and with a pong.
    n.feed(a, NetworkMessage::Ping(5));
    let sent = n.sent(a);
    assert!(matches!(sent[..], [NetworkMessage::Pong(5)]));
}

#[test]
fn nr_55_everything_the_manager_writes_is_a_request_or_a_handshake_message() {
    // `Net::sent` checks every command against the list of NR-55 in each
    // test of this file; here the list itself is pinned.
    assert_eq!(
        MAY_SEND,
        [
            "version",
            "verack",
            "sendaddrv2",
            "sendheaders",
            "ping",
            "pong",
            "getheaders",
            "getdata",
            "getcfilters",
            "getaddr"
        ]
    );
    // A full exchange: headers, filters, a block, a transaction.
    let mut r = Rig::new(vec![vec![pay(&script(7), 1_000, 1)]]);
    r.both_honest(1);
    let block = r.at(1).clone();
    r.n.block(r.a, &block);
    let t = pay(&script(2), 1_000, 9);
    r.n.inv_txs(r.c, &[t.compute_txid()]);
    r.n.tx(r.c, &t);
    r.n.tick(200);
    for id in [r.a, r.b, r.c] {
        r.n.sent(id);
    }
}

// ----- bounds ---------------------------------------------------------------

#[test]
fn nr_54_a_peer_that_announces_a_million_transactions_leaves_every_collection_bounded() {
    let mut n = Net::new();
    let a = n.up(1);
    for k in 0..20u64 {
        let ids: Vec<Txid> = (k * 50_000..(k + 1) * 50_000).map(fake_txid).collect();
        n.inv_txs(a, &ids);
        let sizes = n.m.sizes();
        assert!(sizes.tx_order <= 100_000, "{sizes:?}");
        assert!(sizes.waiting <= 100_000, "{sizes:?}");
        assert!(sizes.pending_tx <= 1_000, "{sizes:?}");
        assert!(sizes.txs <= 105_000, "{sizes:?}");
    }
    let sent = n.m.outgoing(a);
    assert!(sent.len() <= MAX_OUTGOING_BYTES);
    // Only one peer's share was written: 1,000 requests.
    assert_eq!(tx_requests(&decode(&sent)).len(), 1_000);
    // The line empties as the requests run out, and the peer is dropped for
    // not answering, without a mark.
    let out = n.tick(30);
    assert_eq!(disconnects(&out), vec![(a, false)]);
    assert_eq!(n.m.sizes().pending_tx, 0);
}

fn decode(bytes: &[u8]) -> Vec<NetworkMessage> {
    super::kit::decode_all(bytes)
}

#[test]
fn nr_31_two_thousand_side_branch_messages_leave_the_header_store_bounded() {
    let mut n = Net::new();
    let a = n.up(1);
    n.headers(a, &extend(&genesis(), 10, 1));
    for i in 0..2_000u32 {
        let branch = extend(&genesis(), 3, 10_000 + 3 * i);
        n.headers(a, &branch);
    }
    let sizes = n.m.sizes();
    assert!(sizes.side_headers <= 4_032, "{sizes:?}");
    assert_eq!(sizes.peers, 1);
    assert_eq!(n.m.chain().tip().0, 10);
    // Nothing was asked of the peer on account of the side branches.
    assert!(header_requests(&n.sent(a)).is_empty());
}

#[test]
fn nr_45_a_block_that_cannot_be_had_stays_one_item_and_the_rest_go_on() {
    // Nobody has block 1; block 2 is delivered. The stuck one does not
    // hold the others back, and takes one place in work.
    let (mut n, a, b, blocks) = all_blocks_net(2);
    let (h1, h2) = (blocks[0].block_hash(), blocks[1].block_hash());
    n.sent(a);
    n.not_found_block(a, h1);
    n.not_found_block(b, h1);
    let out = n.block(a, &blocks[1]);
    assert_eq!(blocks_out(&out), vec![h2]);
    assert_eq!(n.m.sizes().work, 1);
    assert_eq!(n.m.checked_height(), 0);
}
