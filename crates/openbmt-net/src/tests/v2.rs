//! NR-80 to NR-85: the encrypted transport. The peer is the responder of
//! `v2kit`, written with the `bip324` crate; the session and the manager are
//! the counter's own. The tests that already pass on the old transport and
//! are run again on the encrypted one are at the end.

use std::net::{IpAddr, Ipv4Addr};

use bitcoin::p2p::address::AddrV2;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::ServiceFlags;
use openbmt_watch::{PeerAddr, PeerTransport};

use super::kit::*;
use super::manager_kit::*;
use super::v2kit::{in_v2, message_of, Peer2};
use crate::book::{Source, V2_RETRY_SECS};
use crate::v2::{
    contents_of, short_id, short_name, wipe, MAX_CONTENTS, MAX_DECOY_BYTES, MAX_GARBAGE,
    MAX_HANDSHAKE_DECOYS,
};
use crate::{
    AddressBook, ConnectRefused, Drop, Event, Output, PeerId, Session, TransportChoice, V2Entropy,
    MAX_BLOCK_PAYLOAD,
};

/// The counter's entropy in these tests.
fn entropy() -> V2Entropy {
    V2Entropy::new([7; 32], [9; 32])
}

fn dropped<T: std::fmt::Debug>(result: Result<T, Drop>, rule: &str) -> Drop {
    let d = result.unwrap_err();
    assert_eq!(d.rule(), rule, "{d:?}");
    assert!(d.to_string().starts_with(&format!("{rule}: ")), "{d}");
    d
}

/// A session whose first 64 bytes have been given to the responder and
/// whose reply has been taken. The responder has read the counter's
/// terminator and version packet and its `version`.
fn up(mut peer: Peer2) -> (Session, Peer2) {
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let first = s.take_outgoing();
    assert_eq!(first.len(), 64);
    let reply = peer.answer(&first);
    assert!(s.receive(&reply, START).unwrap().is_empty());
    let heard = peer.hear(&s.take_outgoing());
    assert_eq!(heard.len(), 1);
    assert!(matches!(
        message_of(&heard[0].contents),
        NetworkMessage::Version(_)
    ));
    (s, peer)
}

/// A session that has finished the handshake of the peer protocol too.
fn ready(peer: Peer2) -> (Session, Peer2) {
    let (mut s, mut peer) = up(peer);
    let mut stream = peer.say(
        &contents_of(&NetworkMessage::Version(peer_version())),
        false,
    );
    stream.extend(peer.say(&contents_of(&NetworkMessage::Verack), false));
    let events = s.receive(&stream, START).unwrap();
    assert!(matches!(events[..], [Event::Ready { .. }]));
    let heard = peer.hear(&s.take_outgoing());
    let names: Vec<_> = heard
        .iter()
        .map(|p| message_of(&p.contents).cmd())
        .collect();
    assert_eq!(names, ["sendaddrv2", "verack", "sendheaders"]);
    (s, peer)
}

fn ping(peer: &mut Peer2, nonce: u64) -> Vec<u8> {
    peer.say(&contents_of(&NetworkMessage::Ping(nonce)), false)
}

// ----- NR-80: the first bytes, and the choice -------------------------------

#[test]
fn nr_80_the_first_bytes_are_a_key_that_is_not_a_message_and_the_version_waits() {
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let first = s.take_outgoing();
    assert_eq!(first.len(), 64);
    // Nothing a listener can match: not the network's magic.
    assert_ne!(first[..4], magic().to_bytes());
    assert!(s.take_outgoing().is_empty());
    assert!(s.is_v2());
    // The same entropy gives the same key; other entropy another.
    let mut again = Session::new_v2(config(), START, entropy()).unwrap();
    assert_eq!(again.take_outgoing(), first);
    let other = V2Entropy::new([8; 32], [9; 32]);
    let mut s2 = Session::new_v2(config(), START, other).unwrap();
    assert_ne!(s2.take_outgoing(), first);
    // The aux randomness changes the encoding but not the key behind it.
    let other = V2Entropy::new([7; 32], [10; 32]);
    let mut s3 = Session::new_v2(config(), START, other).unwrap();
    assert_ne!(s3.take_outgoing(), first);
}

#[test]
fn nr_80_the_counters_version_follows_the_peers_version_packet() {
    let mut peer = Peer2::new(1);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let reply = peer.answer(&s.take_outgoing());
    s.receive(&reply, START).unwrap();
    let out = s.take_outgoing();
    let heard = peer.hear(&out);
    assert_eq!(heard.len(), 1);
    let NetworkMessage::Version(v) = message_of(&heard[0].contents) else {
        panic!("not a version");
    };
    assert_eq!(v.nonce, OUR_NONCE);
    assert_eq!(v.user_agent, "");
    // The garbage terminator and the version packet came before it: 16 and
    // 20 bytes, then the packet of the message.
    assert!(out.len() > 36);
}

#[test]
fn nr_80_entropy_that_gives_no_key_is_refused() {
    for secret in [[0u8; 32], [0xff; 32]] {
        let result = Session::new_v2(config(), START, V2Entropy::new(secret, [1; 32]));
        assert_eq!(dropped(result, "NR-83"), Drop::V2Crypto);
    }
}

#[test]
fn nr_80_entropy_is_not_printed_and_is_overwritten() {
    let e = V2Entropy::new([0x41; 32], [0x42; 32]);
    let text = format!("{e:?}");
    assert!(!text.contains("41") && !text.contains("65"), "{text}");
    let s = Session::new_v2(config(), START, entropy()).unwrap();
    let text = format!("{s:?}");
    assert!(!text.contains("[7, 7"), "{text}");
    let mut bytes = [0xaau8; 32];
    wipe(&mut bytes);
    assert_eq!(bytes, [0u8; 32]);
}

fn peer_ip(n: u8) -> AddrV2 {
    AddrV2::Ipv4(Ipv4Addr::new(30 + n, 1, 0, 1))
}

#[test]
fn nr_80_v2_is_tried_first_unless_the_peer_itself_said_it_does_not_speak_it() {
    let mut book = AddressBook::new();
    let known = peer_ip(1);
    let seed = ServiceFlags::NETWORK | ServiceFlags::WITNESS;
    book.add(&known, 8333, seed, Source::Seed, 0);
    // Unknown services (a seed's claim) and an address not in the book.
    assert!(book.v2_first(&known, 8333, 10));
    assert!(book.v2_first(&peer_ip(2), 8333, 10));
    // What the peer itself stated, with the bit: v2. Without it: v1.
    book.connected_ok(&known, 8333, seed | ServiceFlags::P2P_V2, 20);
    assert!(book.v2_first(&known, 8333, 30));
    book.connected_ok(&known, 8333, seed, 40);
    assert!(!book.v2_first(&known, 8333, 50));
    // Gossip about a peer, without the bit, does not stop v2.
    let told = peer_ip(3);
    book.add(&told, 8333, seed, Source::Peer(group_of_peer()), 0);
    assert!(book.v2_first(&told, 8333, 50));
}

fn group_of_peer() -> openbmt_watch::NetGroup {
    openbmt_watch::network_group(&PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(99, 1, 0, 1))))
}

#[test]
fn nr_80_a_manager_connection_speaks_the_transport_it_was_given() {
    let mut n = Net::new();
    let a = PeerId(1);
    n.m.connected_with(a, addr(1), 1, n.now, TransportChoice::V2(entropy()))
        .unwrap();
    assert_eq!(n.m.outgoing(a).len(), 64);
    let b = PeerId(2);
    n.m.connected_with(b, addr(2), 2, n.now, TransportChoice::V1)
        .unwrap();
    let first = n.m.outgoing(b);
    assert_eq!(first[..4], magic().to_bytes());
    let refused = n.m.connected_with(
        PeerId(3),
        addr(3),
        3,
        n.now,
        TransportChoice::V2(V2Entropy::new([0; 32], [0; 32])),
    );
    assert_eq!(refused, Err(ConnectRefused::BadEntropy));
    assert_eq!(n.m.sizes().peers, 2);
}

// ----- NR-81: not spoken here -----------------------------------------------

#[test]
fn nr_81_an_old_transport_version_in_answer_is_not_spoken_here() {
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    s.take_outgoing();
    let d = dropped(s.receive(&version_frame(), START), "NR-81");
    assert_eq!(d, Drop::V2NotSpoken);
    assert!(s.v2_unproven());
    assert!(s.take_outgoing().is_empty());
}

#[test]
fn nr_81_the_old_start_is_seen_when_it_arrives_in_pieces() {
    let bytes = version_frame();
    for cut in [1, 4, 15, 16, 17, 40] {
        let mut s = Session::new_v2(config(), START, entropy()).unwrap();
        s.take_outgoing();
        let before = s.receive(&bytes[..cut], START);
        if cut < 16 {
            assert!(before.unwrap().is_empty(), "cut {cut}");
            let d = dropped(s.receive(&bytes[cut..], START), "NR-81");
            assert_eq!(d, Drop::V2NotSpoken);
        } else {
            assert_eq!(dropped(before, "NR-81"), Drop::V2NotSpoken);
        }
    }
}

#[test]
fn nr_81_only_a_version_start_counts_not_any_old_message() {
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    s.take_outgoing();
    // The magic and another command: 24 bytes that are nobody's key.
    assert!(s.receive(&verack_frame(), START).unwrap().is_empty());
    assert!(s.v2_unproven());
}

#[test]
fn nr_81_after_an_authenticated_packet_there_is_no_way_back() {
    let (mut s, _peer) = up(Peer2::new(1));
    assert!(!s.v2_unproven());
    // The same old bytes now are not a packet that authenticates: they are
    // waited for, or they end the peer, but not as a peer that does not
    // speak v2.
    match s.receive(&version_frame(), START) {
        Ok(events) => assert!(events.is_empty()),
        Err(d) => {
            assert_ne!(d, Drop::V2NotSpoken, "{d:?}");
            assert_eq!(d.rule(), "NR-83");
        }
    }
    assert!(!s.v2_unproven());
}

#[test]
fn nr_81_the_manager_says_so_when_the_peer_answers_with_an_old_version() {
    let mut n = Net::new();
    let id = PeerId(1);
    n.m.connected_with(id, addr(1), 1, n.now, TransportChoice::V2(entropy()))
        .unwrap();
    n.m.outgoing(id);
    let out = n.feed_bytes(id, &version_frame());
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0], Output::V2NotSpoken { id });
    // Not a misbehaviour: a path can make it happen.
    assert_eq!(disconnects(&out), vec![(id, false)]);
    assert!(reason(&out).starts_with("NR-81: "));
}

#[test]
fn nr_81_the_manager_says_so_when_the_peer_closes_before_any_packet() {
    let mut n = Net::new();
    let id = PeerId(1);
    n.m.connected_with(id, addr(1), 1, n.now, TransportChoice::V2(entropy()))
        .unwrap();
    n.m.outgoing(id);
    // The peer answered with some of its key and closed.
    n.feed_bytes(id, &[9u8; 30]);
    let out = n.m.closed(id, n.now);
    assert_eq!(out, vec![Output::V2NotSpoken { id }]);
    // An old-transport connection that closes says nothing of the kind.
    let old = PeerId(2);
    n.m.connected(old, addr(2), 2, n.now).unwrap();
    assert!(n.m.closed(old, n.now).is_empty());
}

#[test]
fn nr_81_a_close_after_the_peers_version_packet_is_no_refusal() {
    in_v2(|| {
        let mut n = Net::new();
        let id = n.up(1);
        assert!(n.m.closed(id, n.now).is_empty());
    });
}

#[test]
fn nr_81_a_failure_after_an_authenticated_packet_is_a_dropped_peer_and_no_refusal() {
    in_v2(|| {
        let mut n = Net::new();
        let id = n.up(1);
        let mut bytes = n.frame(id, NetworkMessage::Ping(5));
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let out = n.feed_bytes(id, &bytes);
        // No `V2NotSpoken`, and no mark against the peer (NR-25 does not
        // apply to this).
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(disconnects(&out), vec![(id, false)]);
        assert!(reason(&out).starts_with("NR-83: "));
    });
}

#[test]
fn nr_81_a_peer_that_did_not_speak_v2_is_dialed_with_v1_for_a_day() {
    let mut book = AddressBook::new();
    let a = peer_ip(1);
    let services = ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::P2P_V2;
    book.add(&a, 8333, services, Source::Own, 0);
    book.connected_ok(&a, 8333, services, 5);
    assert!(book.v2_first(&a, 8333, 100));
    book.v2_failed(&a, 8333, 100);
    assert!(!book.v2_first(&a, 8333, 100));
    assert!(!book.v2_first(&a, 8333, 100 + V2_RETRY_SECS - 1));
    assert!(book.v2_first(&a, 8333, 100 + V2_RETRY_SECS));
    assert_eq!(V2_RETRY_SECS, 86_400);
    // An address the book does not hold is not touched.
    book.v2_failed(&peer_ip(9), 8333, 100);
    assert_eq!(book.len(), 1);
}

// ----- NR-82: the peers the app is told of ---------------------------------

#[test]
fn nr_82_the_peers_say_which_transport_carries_them() {
    let mut n = Net::new();
    n.up(1);
    in_v2(|| {
        let mut v2 = Net::new();
        v2.up(1);
        let peers = v2.m.health_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].transport, PeerTransport::V2);
    });
    let peers = n.m.health_peers();
    assert_eq!(peers[0].transport, PeerTransport::V1);
    // A peer that has not finished is not listed.
    let id = PeerId(5);
    n.m.connected_with(id, addr(5), 5, n.now, TransportChoice::V2(entropy()))
        .unwrap();
    assert_eq!(n.m.health_peers().len(), 1);
}

// ----- NR-83: limits ---------------------------------------------------------

#[test]
fn nr_83_garbage_of_0_1_and_4095_bytes_is_accepted_and_4096_is_refused() {
    for n in [0, 1, 17, 4_094, MAX_GARBAGE] {
        let (_s, _peer) = ready(Peer2::new(1).with_garbage(n));
    }
    // One byte more than the limit before the terminator.
    let mut peer = Peer2::new(1).with_garbage(MAX_GARBAGE);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let mut reply = peer.answer(&s.take_outgoing());
    reply.insert(64 + MAX_GARBAGE, 0xab);
    let d = dropped(s.receive(&reply, START), "NR-83");
    assert_eq!(d, Drop::V2Garbage);
}

#[test]
fn nr_83_garbage_that_arrives_one_byte_at_a_time_is_accepted() {
    let mut peer = Peer2::new(1).with_garbage(MAX_GARBAGE);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let mut reply = peer.answer(&s.take_outgoing());
    // The version packet of the peer comes with it.
    let mut app = Vec::new();
    // The cipher of the responder exists once it has read the counter's
    // packets, which the counter sends after this reply is in. A copy of
    // the counter makes the same bytes.
    let mut twin = Session::new_v2(config(), START, entropy()).unwrap();
    twin.take_outgoing();
    twin.receive(&reply, START).unwrap();
    peer.hear(&twin.take_outgoing());
    app.extend(peer.say(
        &contents_of(&NetworkMessage::Version(peer_version())),
        false,
    ));
    app.extend(peer.say(&contents_of(&NetworkMessage::Verack), false));
    reply.extend(app);
    let mut events = Vec::new();
    for byte in reply.chunks(1) {
        events.extend(s.receive(byte, START).unwrap());
    }
    assert!(matches!(events[..], [Event::Ready { .. }]));
}

#[test]
fn nr_83_decoy_packets_in_the_handshake_are_ignored_and_the_ninth_is_too_many() {
    let decoys = |n: usize| (0..n).map(|i| vec![i as u8; i * 7]).collect::<Vec<_>>();
    let (_s, _peer) = ready(Peer2::new(1).with_decoys(decoys(MAX_HANDSHAKE_DECOYS)));
    let mut peer = Peer2::new(1).with_decoys(decoys(MAX_HANDSHAKE_DECOYS + 1));
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let reply = peer.answer(&s.take_outgoing());
    let d = dropped(s.receive(&reply, START), "NR-83");
    assert_eq!(d, Drop::V2Decoys);
    // With garbage as well: the garbage is authenticated by the first
    // packet, a decoy here.
    let (_s, _peer) = ready(Peer2::new(2).with_garbage(33).with_decoys(decoys(3)));
}

#[test]
fn nr_83_a_handshake_decoy_over_the_size_limit_is_refused() {
    let mut peer = Peer2::new(1).with_decoys(vec![vec![0; 4_097]]);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let reply = peer.answer(&s.take_outgoing());
    let d = dropped(s.receive(&reply, START), "NR-83");
    assert!(
        matches!(
            d,
            Drop::V2TooLong {
                len: 4_097,
                limit: 4_096
            }
        ),
        "{d:?}"
    );
    let mut peer = Peer2::new(1).with_decoys(vec![vec![0; 4_096]]);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let reply = peer.answer(&s.take_outgoing());
    assert!(s.receive(&reply, START).is_ok());
}

#[test]
fn nr_83_a_handshake_cut_at_every_byte_boundary_completes() {
    // The whole flight of the peer, made once.
    let mut peer = Peer2::new(1)
        .with_garbage(5)
        .with_decoys(vec![vec![1, 2, 3], vec![4; 40]]);
    let mut master = Session::new_v2(config(), START, entropy()).unwrap();
    let mut stream = peer.answer(&master.take_outgoing());
    master.receive(&stream, START).unwrap();
    peer.hear(&master.take_outgoing());
    stream.extend(peer.say(
        &contents_of(&NetworkMessage::Version(peer_version())),
        false,
    ));
    stream.extend(peer.say(&contents_of(&NetworkMessage::Verack), true));
    stream.extend(peer.say(&contents_of(&NetworkMessage::Verack), false));
    for cut in 0..=stream.len() {
        let mut s = Session::new_v2(config(), START, entropy()).unwrap();
        s.take_outgoing();
        let mut events = s.receive(&stream[..cut], START).unwrap();
        events.extend(s.receive(&stream[cut..], START).unwrap());
        assert!(matches!(events[..], [Event::Ready { .. }]), "cut {cut}");
    }
}

#[test]
fn nr_83_every_single_bit_flip_in_a_packet_is_refused() {
    let (_s, mut probe) = ready(Peer2::new(1));
    let good = ping(&mut probe, 77);
    assert_eq!(good.len(), 29);
    for bit in 0..good.len() * 8 {
        let (mut s, mut peer) = ready(Peer2::new(1));
        let mut bytes = ping(&mut peer, 77);
        assert_eq!(bytes, good);
        bytes[bit / 8] ^= 1 << (bit % 8);
        let result = s.receive(&bytes, START);
        if bit < 24 {
            // The length: the packet is cut or lengthened, and then it
            // does not authenticate, or has not all arrived. Either way
            // nothing is delivered and nothing is answered.
            if let Ok(events) = &result {
                assert!(events.is_empty(), "bit {bit}");
            }
            assert!(s.take_outgoing().is_empty(), "bit {bit}");
        } else {
            assert_eq!(dropped(result, "NR-83"), Drop::V2Auth, "bit {bit}");
        }
    }
}

#[test]
fn nr_83_a_packet_cut_at_every_byte_boundary_resumes() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    let mut stream = ping(&mut peer, 1);
    stream.extend(peer.say(&[0xee; 300], true));
    stream.extend(ping(&mut peer, 2));
    for cut in 0..=stream.len() {
        let (mut s2, mut peer2) = ready(Peer2::new(1));
        let mut again = ping(&mut peer2, 1);
        again.extend(peer2.say(&[0xee; 300], true));
        again.extend(ping(&mut peer2, 2));
        assert_eq!(again, stream);
        assert!(s2.receive(&stream[..cut], START).unwrap().is_empty());
        assert!(s2.receive(&stream[cut..], START).unwrap().is_empty());
        let heard: Vec<_> = peer2
            .hear(&s2.take_outgoing())
            .iter()
            .map(|p| message_of(&p.contents))
            .collect();
        assert_eq!(
            heard,
            vec![NetworkMessage::Pong(1), NetworkMessage::Pong(2)],
            "cut {cut}"
        );
    }
    // The first session takes the stream in one piece.
    s.receive(&stream, START).unwrap();
    assert_eq!(peer.hear(&s.take_outgoing()).len(), 2);
}

#[test]
fn nr_83_both_ciphers_rekey_every_224_packets() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    // 700 pings from the peer: 3 re-keys of its cipher; 700 pongs from the
    // counter: 3 re-keys of its own.
    for i in 0..700u64 {
        let events = s.receive(&ping(&mut peer, i), START).unwrap();
        assert!(events.is_empty());
        let heard = peer.hear(&s.take_outgoing());
        assert_eq!(heard.len(), 1, "packet {i}");
        assert_eq!(message_of(&heard[0].contents), NetworkMessage::Pong(i));
    }
}

#[test]
fn nr_83_decoys_after_the_handshake_are_ignored_and_counted() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    let mut stream = peer.say(&[1, 2, 3], true);
    stream.extend(ping(&mut peer, 5));
    assert!(s.receive(&stream, START).unwrap().is_empty());
    assert_eq!(
        peer.hear(&s.take_outgoing())
            .iter()
            .map(|p| message_of(&p.contents))
            .collect::<Vec<_>>(),
        vec![NetworkMessage::Pong(5)]
    );
    // More decoy bytes than the budget end the peer.
    let mut sent = 0;
    let d = loop {
        let packet = peer.say(&[0; 50_000], true);
        sent += 50_000;
        match s.receive(&packet, START) {
            Ok(_) => assert!(sent <= MAX_DECOY_BYTES + 50_000),
            Err(d) => break d,
        }
    };
    assert_eq!(d, Drop::V2Decoys);
    assert_eq!(d.rule(), "NR-83");
}

#[test]
fn nr_83_a_packet_longer_than_a_message_may_be_is_refused_at_its_length() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    let packet = peer.say(&vec![14u8; MAX_CONTENTS + 1], false);
    // Only the first bytes are needed to see it.
    let d = dropped(s.receive(&packet[..3], START), "NR-83");
    assert_eq!(
        d,
        Drop::V2TooLong {
            len: (MAX_CONTENTS + 1) as u32,
            limit: MAX_CONTENTS as u32
        }
    );
}

#[test]
fn nr_83_the_largest_block_is_read_across_many_reads() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    let mut contents = vec![2u8];
    contents.extend(vec![0x5a; MAX_BLOCK_PAYLOAD as usize]);
    assert_eq!(contents.len(), MAX_CONTENTS - 12);
    let packet = peer.say(&contents, false);
    let events = s.receive(&packet, START).unwrap();
    let [Event::Block(bytes)] = &events[..] else {
        panic!("{}", events.len());
    };
    assert_eq!(bytes.len(), MAX_BLOCK_PAYLOAD as usize);
    // One byte more is over the limit of NR-02.
    let mut contents = vec![2u8];
    contents.extend(vec![0x5a; MAX_BLOCK_PAYLOAD as usize + 1]);
    let packet = peer.say(&contents, false);
    let d = dropped(s.receive(&packet, START), "NR-02");
    assert!(matches!(
        d,
        Drop::TooLong {
            command: "block",
            ..
        }
    ));
}

#[test]
fn nr_83_the_limits_of_nr_02_and_nr_03_hold_for_the_contents_of_a_packet() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    // A ping with 101 bytes.
    let mut contents = vec![18u8];
    contents.extend([0u8; 101]);
    let packet = peer.say(&contents, false);
    let d = dropped(s.receive(&packet, START), "NR-02");
    assert_eq!(
        d,
        Drop::TooLong {
            command: "ping",
            len: 101,
            limit: 100
        }
    );
    // An unknown command is skipped up to 100,000 bytes, then refused.
    let unknown = |len: usize| {
        let mut c = vec![0u8];
        c.extend(b"mystery\0\0\0\0\0");
        c.extend(vec![0u8; len]);
        c
    };
    let (mut s, mut peer) = ready(Peer2::new(1));
    assert!(s
        .receive(&peer.say(&unknown(100_000), false), START)
        .unwrap()
        .is_empty());
    // An ID that stands for no command is skipped the same way.
    let mut reserved = vec![29u8];
    reserved.extend([1u8; 9]);
    assert!(s
        .receive(&peer.say(&reserved, false), START)
        .unwrap()
        .is_empty());
    // And a command that has an ID, read in the long form.
    let mut long_ping = vec![0u8];
    long_ping.extend(b"ping\0\0\0\0\0\0\0\0");
    long_ping.extend(9u64.to_le_bytes());
    assert!(s
        .receive(&peer.say(&long_ping, false), START)
        .unwrap()
        .is_empty());
    assert_eq!(
        peer.hear(&s.take_outgoing())
            .iter()
            .map(|p| message_of(&p.contents))
            .collect::<Vec<_>>(),
        vec![NetworkMessage::Pong(9)]
    );
    let d = dropped(
        s.receive(&peer.say(&unknown(100_001), false), START),
        "NR-03",
    );
    assert_eq!(d, Drop::UnknownTooLong { len: 100_001 });
}

#[test]
fn nr_83_a_packet_with_no_message_in_it_or_a_bad_command_name_is_refused() {
    let (mut s, mut peer) = ready(Peer2::new(1));
    let d = dropped(s.receive(&peer.say(&[], false), START), "NR-04");
    assert!(matches!(d, Drop::Malformed { .. }));
    let (mut s, mut peer) = ready(Peer2::new(1));
    let mut bad = vec![0u8];
    bad.extend(b"pi\0ng\0\0\0\0\0\0\0");
    let d = dropped(s.receive(&peer.say(&bad, false), START), "NR-01");
    assert_eq!(d, Drop::BadCommand);
}

#[test]
fn nr_83_a_message_for_after_the_handshake_is_refused_before_it() {
    let (mut s, mut peer) = up(Peer2::new(1));
    let mut stream = peer.say(
        &contents_of(&NetworkMessage::Version(peer_version())),
        false,
    );
    stream.extend(peer.say(
        &contents_of(&NetworkMessage::Headers(vec![header(1)])),
        false,
    ));
    let d = s.receive(&stream, START).unwrap_err();
    assert_eq!(d, Drop::BeforeHandshake { command: "headers" });
    assert_eq!(d.rule(), "NR-13");
}

#[test]
fn nr_83_a_version_packet_that_does_not_authenticate_is_refused() {
    let mut peer = Peer2::new(1);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    let mut reply = peer.answer(&s.take_outgoing());
    let last = reply.len() - 1;
    reply[last] ^= 0x10;
    assert_eq!(dropped(s.receive(&reply, START), "NR-83"), Drop::V2Auth);
    assert!(s.take_outgoing().is_empty());
    // And the same drop every time after.
    assert_eq!(s.tick(START).unwrap_err(), Drop::V2Auth);
}

#[test]
fn nr_83_the_ids_are_those_of_the_table_of_the_bip() {
    let ids = [
        (NetworkMessage::Ping(0), 18u8),
        (NetworkMessage::Pong(0), 19),
        (NetworkMessage::GetAddr, 0),
        (NetworkMessage::Verack, 0),
        (NetworkMessage::SendHeaders, 0),
        (NetworkMessage::SendAddrV2, 0),
        (NetworkMessage::GetData(vec![]), 11),
        (
            NetworkMessage::GetHeaders(bitcoin::p2p::message_blockdata::GetHeadersMessage {
                version: 1,
                locator_hashes: vec![],
                stop_hash: bitcoin::BlockHash::from_raw_hash(bitcoin::hashes::Hash::all_zeros()),
            }),
            12,
        ),
        (NetworkMessage::Headers(vec![]), 13),
        (NetworkMessage::Inv(vec![]), 14),
        (NetworkMessage::NotFound(vec![]), 17),
        (NetworkMessage::Addr(vec![]), 1),
        (NetworkMessage::AddrV2(vec![]), 28),
        (NetworkMessage::Tx(simple_tx()), 21),
        (NetworkMessage::FeeFilter(1), 5),
    ];
    for (message, id) in ids {
        let contents = contents_of(&message);
        assert_eq!(contents[0], id, "{}", message.cmd());
        if id == 0 {
            // The long form: 12 bytes of command, padded with NULs.
            let field = &contents[1..13];
            let name = message.cmd().as_bytes();
            assert_eq!(&field[..name.len()], name);
            assert!(field[name.len()..].iter().all(|b| *b == 0));
        }
    }
}

// ----- NR-84: the session ID ------------------------------------------------

#[test]
fn nr_84_both_ends_derive_the_same_session_id_and_it_shows_when_the_peer_is_heard() {
    let mut peer = Peer2::new(1);
    let mut s = Session::new_v2(config(), START, entropy()).unwrap();
    assert_eq!(s.v2_session_id(), None);
    let reply = peer.answer(&s.take_outgoing());
    s.receive(&reply, START).unwrap();
    peer.hear(&s.take_outgoing());
    let ours = s.v2_session_id().expect("the version packet is in");
    assert_eq!(Some(ours), peer.session_id());
    assert_ne!(ours, [0u8; 32]);
    // Another connection, another ID.
    let (other, _) = up(Peer2::new(2));
    assert_ne!(other.v2_session_id(), Some(ours));
    // The old transport has none.
    assert_eq!(session().v2_session_id(), None);
}

// ----- NR-85: Tor ------------------------------------------------------------

#[test]
fn nr_85_a_peer_reached_by_name_takes_the_same_transport_rules() {
    // The manager does not know how the bytes travel: a peer reached by a
    // name (through a proxy) is one group and speaks either transport.
    let mut n = Net::new();
    let id = PeerId(1);
    n.m.connected_with(
        id,
        PeerAddr::Named,
        1,
        n.now,
        TransportChoice::V2(entropy()),
    )
    .unwrap();
    let mut peer = Peer2::new(1);
    let reply = peer.answer(&n.m.outgoing(id));
    assert!(n.m.received(id, &reply, n.now).is_empty());
    let heard = peer.hear(&n.m.outgoing(id));
    assert_eq!(heard.len(), 1);
    assert_eq!(
        n.m.connected_with(PeerId(2), PeerAddr::Named, 2, n.now, TransportChoice::V1),
        Err(ConnectRefused::SameGroup)
    );
}

/// BIP324's list of one-byte message IDs, written out here by number and
/// name, apart from the table in `v2.rs`.
const BIP_IDS: [(u8, &str); 28] = [
    (1, "addr"),
    (2, "block"),
    (3, "blocktxn"),
    (4, "cmpctblock"),
    (5, "feefilter"),
    (6, "filteradd"),
    (7, "filterclear"),
    (8, "filterload"),
    (9, "getblocks"),
    (10, "getblocktxn"),
    (11, "getdata"),
    (12, "getheaders"),
    (13, "headers"),
    (14, "inv"),
    (15, "mempool"),
    (16, "merkleblock"),
    (17, "notfound"),
    (18, "ping"),
    (19, "pong"),
    (20, "sendcmpct"),
    (21, "tx"),
    (22, "getcfilters"),
    (23, "cfilter"),
    (24, "getcfheaders"),
    (25, "cfheaders"),
    (26, "getcfcheckpt"),
    (27, "cfcheckpt"),
    (28, "addrv2"),
];

#[test]
fn nr_80_the_table_of_one_byte_ids_is_the_one_of_bip324_by_name_and_number() {
    for (id, name) in BIP_IDS {
        assert_eq!(short_name(id), Some(name), "ID {id}");
        assert_eq!(short_id(name), Some(id), "{name}");
    }
    // Nothing else has a short ID: 0 means a long command follows, and the
    // numbers above 28 are not assigned.
    assert_eq!(short_name(0), None);
    for id in 29..=255u8 {
        assert_eq!(short_name(id), None, "ID {id}");
    }
    assert_eq!(short_id("version"), None);
    assert_eq!(short_id("verack"), None);
    assert_eq!(short_id(""), None);
}
