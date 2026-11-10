//! NR-05 and NR-11 to NR-14 at the level of the session, and NR-55's
//! silence.

use bitcoin::hashes::Hash;
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, Txid};

use super::kit::*;
use crate::{Drop, Event, Request, Session, SessionConfig};

fn dropped<T: std::fmt::Debug>(result: Result<T, Drop>, rule: &str) -> Drop {
    let d = result.unwrap_err();
    assert_eq!(d.rule(), rule, "{d:?}");
    assert!(d.to_string().starts_with(&format!("{rule}: ")), "{d}");
    d
}

fn hear(s: &mut Session, bytes: &[u8]) -> Result<Vec<Event>, Drop> {
    s.receive(bytes, START)
}

#[test]
fn nr_11_the_version_the_counter_sends_says_as_little_as_it_can() {
    let mut s = session();
    let sent = outgoing(&mut s);
    assert_eq!(sent.len(), 1);
    let NetworkMessage::Version(v) = &sent[0] else {
        panic!("not a version: {:?}", sent[0]);
    };
    let zero = Address {
        services: ServiceFlags::NONE,
        address: [0; 8],
        port: 0,
    };
    assert_eq!(v.version, 70_016);
    assert_eq!(v.services, ServiceFlags::NONE);
    assert_eq!(v.timestamp, START as i64);
    assert_eq!(v.receiver, zero);
    assert_eq!(v.sender, zero);
    assert_eq!(v.nonce, OUR_NONCE);
    assert_eq!(v.user_agent, "");
    assert_eq!(v.start_height, 800_000);
    assert!(v.relay);
    assert!(s.take_outgoing().is_empty());
}

#[test]
fn nr_11_the_start_height_saturates_and_the_time_is_the_callers() {
    let config = SessionConfig {
        tip_height: u32::MAX,
        ..config()
    };
    let mut s = Session::new(config, u64::MAX);
    let NetworkMessage::Version(v) = outgoing(&mut s).remove(0) else {
        panic!("not a version");
    };
    assert_eq!(v.start_height, i32::MAX);
    assert_eq!(v.timestamp, i64::MAX);
}

#[test]
fn nr_11_nothing_asks_for_compact_blocks_or_addresses() {
    let mut s = session();
    let mut stream = version_frame();
    stream.extend(verack_frame());
    hear(&mut s, &stream).unwrap();
    let sent = outgoing(&mut s);
    assert!(sent.iter().all(|m| !matches!(
        m,
        NetworkMessage::SendCmpct(_)
            | NetworkMessage::WtxidRelay
            | NetworkMessage::GetAddr
            | NetworkMessage::Addr(_)
            | NetworkMessage::AddrV2(_)
    )));
}

#[test]
fn nr_12_a_good_version_is_answered_in_the_order_of_nr_13() {
    let mut s = session();
    s.take_outgoing();
    let events = hear(&mut s, &version_frame()).unwrap();
    assert!(events.is_empty());
    assert_eq!(
        outgoing(&mut s),
        vec![NetworkMessage::SendAddrV2, NetworkMessage::Verack]
    );
    // sendheaders waits for the peer's verack: a peer does not read it
    // before the handshake is over.
    hear(&mut s, &verack_frame()).unwrap();
    assert_eq!(outgoing(&mut s), vec![NetworkMessage::SendHeaders]);
}

#[test]
fn nr_95_sendaddrv2_goes_out_before_the_verack() {
    let mut s = session();
    s.take_outgoing();
    hear(&mut s, &version_frame()).unwrap();
    let sent: Vec<&'static str> = outgoing(&mut s).iter().map(|m| m.cmd()).collect();
    let at = |name: &str| sent.iter().position(|c| *c == name);
    assert!(at("sendaddrv2") < at("verack"), "{sent:?}");
    assert!(at("sendaddrv2").is_some());
}

#[test]
fn nr_12_the_peer_services_and_start_height_are_reported_as_claims() {
    let mut s = session();
    let mut v = peer_version();
    v.services =
        ServiceFlags::NETWORK_LIMITED | ServiceFlags::WITNESS | ServiceFlags::COMPACT_FILTERS;
    v.start_height = 123;
    hear(&mut s, &frame(NetworkMessage::Version(v))).unwrap();
    let events = hear(&mut s, &verack_frame()).unwrap();
    assert_eq!(
        events,
        vec![Event::Ready {
            services: ServiceFlags::NETWORK_LIMITED
                | ServiceFlags::WITNESS
                | ServiceFlags::COMPACT_FILTERS,
            start_height: 123
        }]
    );
}

#[test]
fn nr_12_a_negative_start_height_is_zero() {
    let mut s = session();
    let mut v = peer_version();
    v.start_height = -5;
    hear(&mut s, &frame(NetworkMessage::Version(v))).unwrap();
    let events = hear(&mut s, &verack_frame()).unwrap();
    assert!(matches!(
        events[..],
        [Event::Ready {
            start_height: 0,
            ..
        }]
    ));
}

#[test]
fn nr_12_a_first_message_other_than_version_is_dropped() {
    for message in [
        NetworkMessage::Verack,
        NetworkMessage::SendHeaders,
        NetworkMessage::SendAddrV2,
        NetworkMessage::WtxidRelay,
    ] {
        let mut s = session();
        let d = dropped(hear(&mut s, &frame(message.clone())), "NR-12");
        assert_eq!(d, Drop::FirstNotVersion, "{message:?}");
    }
    // sendcmpct and feefilter are read and ignored, but not first.
    for name in ["sendcmpct", "feefilter"] {
        let mut s = session();
        let d = dropped(hear(&mut s, &raw_frame(name, &[0; 9])), "NR-12");
        assert_eq!(d, Drop::FirstNotVersion);
    }
}

#[test]
fn nr_12_a_first_message_for_after_the_handshake_is_dropped() {
    let mut s = session();
    let d = dropped(hear(&mut s, &frame(NetworkMessage::Ping(1))), "NR-13");
    assert_eq!(d, Drop::BeforeHandshake { command: "ping" });
}

#[test]
fn nr_12_an_unknown_command_before_the_version_is_skipped() {
    let mut s = session();
    s.take_outgoing();
    let mut stream = raw_frame("getheaders", &[0; 64]);
    stream.extend(version_frame());
    assert!(hear(&mut s, &stream).unwrap().is_empty());
    assert_eq!(outgoing(&mut s).len(), 2);
}

#[test]
fn nr_12_a_version_below_70016_is_dropped() {
    for (version, ok) in [
        (70_015u32, false),
        (70_016, true),
        (70_017, true),
        (0, false),
    ] {
        let mut s = session();
        let mut v = peer_version();
        v.version = version;
        let result = hear(&mut s, &frame(NetworkMessage::Version(v)));
        if ok {
            result.unwrap();
        } else {
            assert_eq!(dropped(result, "NR-12"), Drop::VersionTooOld { version });
        }
    }
}

#[test]
fn nr_12_a_peer_without_witness_or_network_services_is_dropped() {
    let cases = [
        (ServiceFlags::NETWORK, false),
        (ServiceFlags::WITNESS, false),
        (ServiceFlags::NONE, false),
        (ServiceFlags::NETWORK_LIMITED, false),
        (ServiceFlags::COMPACT_FILTERS | ServiceFlags::BLOOM, false),
        (ServiceFlags::NETWORK | ServiceFlags::WITNESS, true),
        (ServiceFlags::NETWORK_LIMITED | ServiceFlags::WITNESS, true),
        (
            ServiceFlags::NETWORK
                | ServiceFlags::NETWORK_LIMITED
                | ServiceFlags::WITNESS
                | ServiceFlags::COMPACT_FILTERS,
            true,
        ),
    ];
    for (services, ok) in cases {
        let mut s = session();
        let mut v = peer_version();
        v.services = services;
        let result = hear(&mut s, &frame(NetworkMessage::Version(v)));
        if ok {
            result.unwrap();
        } else {
            let d = dropped(result, "NR-12");
            assert_eq!(
                d,
                Drop::MissingServices {
                    services: services.to_u64()
                }
            );
        }
    }
}

#[test]
fn nr_12_the_counters_own_nonce_is_dropped() {
    let mut s = session();
    let mut v = peer_version();
    v.nonce = OUR_NONCE;
    assert_eq!(
        dropped(hear(&mut s, &frame(NetworkMessage::Version(v))), "NR-12"),
        Drop::OwnNonce
    );
}

#[test]
fn nr_12_a_user_agent_over_256_bytes_is_dropped() {
    for (len, ok) in [(256usize, true), (257, false)] {
        let mut s = session();
        let mut v = peer_version();
        v.user_agent = "a".repeat(len);
        let result = hear(&mut s, &frame(NetworkMessage::Version(v)));
        if ok {
            result.unwrap();
        } else {
            assert_eq!(dropped(result, "NR-12"), Drop::UserAgentTooLong { len });
        }
    }
}

#[test]
fn nr_13_only_the_handshake_is_sent_before_both_veracks() {
    let mut s = session();
    // Before the peer's version: ours only.
    assert!(matches!(outgoing(&mut s)[..], [NetworkMessage::Version(_)]));
    // A request now is refused.
    let mut early = session();
    early.take_outgoing();
    let d = dropped(early.send(Request::GetAddr), "NR-13");
    assert_eq!(d, Drop::NotReady);
    assert!(early.take_outgoing().is_empty());

    // After the peer's version, before its verack: the two answers only.
    let mut s = session();
    s.take_outgoing();
    hear(&mut s, &version_frame()).unwrap();
    s.tick(START + 1).unwrap();
    let sent = outgoing(&mut s);
    assert_eq!(
        sent,
        vec![NetworkMessage::SendAddrV2, NetworkMessage::Verack]
    );
    let d = dropped(s.send(Request::GetAddr), "NR-13");
    assert_eq!(d, Drop::NotReady);
    assert!(s.take_outgoing().is_empty());
}

#[test]
fn nr_13_a_request_is_sent_once_both_veracks_are_in() {
    let mut s = ready_session();
    s.send(Request::GetAddr).unwrap();
    assert_eq!(outgoing(&mut s), vec![NetworkMessage::GetAddr]);
}

#[test]
fn nr_13_every_message_for_after_the_handshake_is_dropped_before_it() {
    let one_inv = vec![Inventory::Transaction(Txid::from_byte_array([1; 32]))];
    let messages = [
        ("headers", NetworkMessage::Headers(vec![header(1)])),
        ("inv", NetworkMessage::Inv(one_inv.clone())),
        ("notfound", NetworkMessage::NotFound(one_inv)),
        ("tx", NetworkMessage::Tx(simple_tx())),
        ("addr", NetworkMessage::Addr(vec![])),
        ("addrv2", NetworkMessage::AddrV2(vec![])),
        ("ping", NetworkMessage::Ping(1)),
        ("pong", NetworkMessage::Pong(1)),
    ];
    let mut streams: Vec<(&str, Vec<u8>)> = messages
        .into_iter()
        .map(|(name, m)| (name, frame(m)))
        .collect();
    streams.push(("block", raw_frame("block", &[0; 100])));
    streams.push((
        "cfilter",
        frame(NetworkMessage::CFilter(
            bitcoin::p2p::message_filter::CFilter {
                filter_type: 0,
                block_hash: BlockHash::from_byte_array([2; 32]),
                filter: vec![1],
            },
        )),
    ));
    for (name, stream) in streams {
        // After the peer's version, before its verack.
        let mut s = session();
        hear(&mut s, &version_frame()).unwrap();
        let d = dropped(hear(&mut s, &stream), "NR-13");
        assert_eq!(d, Drop::BeforeHandshake { command: name });
    }
}

#[test]
fn nr_13_the_handshake_messages_are_accepted_before_the_peers_verack() {
    let mut s = session();
    let mut stream = version_frame();
    stream.extend(frame(NetworkMessage::WtxidRelay));
    stream.extend(frame(NetworkMessage::SendAddrV2));
    stream.extend(frame(NetworkMessage::SendHeaders));
    stream.extend(raw_frame("sendcmpct", &[0; 9]));
    stream.extend(raw_frame("feefilter", &[0; 8]));
    assert!(hear(&mut s, &stream).unwrap().is_empty());
    let events = hear(&mut s, &verack_frame()).unwrap();
    assert!(matches!(events[..], [Event::Ready { .. }]));
}

#[test]
fn nr_13_a_second_version_is_dropped() {
    let mut stream = version_frame();
    stream.extend(version_frame());
    let mut s = session();
    assert_eq!(
        dropped(hear(&mut s, &stream), "NR-13"),
        Drop::DuplicateVersion
    );
    let mut s = ready_session();
    assert_eq!(
        dropped(hear(&mut s, &version_frame()), "NR-13"),
        Drop::DuplicateVersion
    );
}

#[test]
fn nr_13_a_second_verack_is_dropped() {
    let mut s = ready_session();
    assert_eq!(
        dropped(hear(&mut s, &verack_frame()), "NR-13"),
        Drop::DuplicateVerack
    );
}

#[test]
fn nr_13_a_handshake_fed_one_byte_at_a_time_completes() {
    let mut s = session();
    let mut stream = version_frame();
    stream.extend(verack_frame());
    let mut events = Vec::new();
    for byte in &stream {
        events.extend(hear(&mut s, std::slice::from_ref(byte)).unwrap());
    }
    assert!(matches!(events[..], [Event::Ready { .. }]));
    let sent = outgoing(&mut s);
    assert!(matches!(sent[0], NetworkMessage::Version(_)));
    assert_eq!(
        sent[1..],
        [
            NetworkMessage::SendAddrV2,
            NetworkMessage::Verack,
            NetworkMessage::SendHeaders
        ]
    );
}

#[test]
fn nr_13_a_handshake_cut_at_every_byte_boundary_completes() {
    let mut stream = version_frame();
    stream.extend(verack_frame());
    for cut in 0..=stream.len() {
        let mut s = session();
        let mut events = hear(&mut s, &stream[..cut]).unwrap();
        events.extend(hear(&mut s, &stream[cut..]).unwrap());
        assert!(matches!(events[..], [Event::Ready { .. }]), "cut {cut}");
    }
}

#[test]
fn nr_14_a_handshake_not_finished_in_30_seconds_is_dropped() {
    for (late, ok) in [(29u64, true), (30, false), (31, false)] {
        let mut s = session();
        let result = s.tick(START + late);
        if ok {
            result.unwrap();
        } else {
            assert_eq!(dropped(result, "NR-14"), Drop::HandshakeTimeout);
        }
    }
}

#[test]
fn nr_14_a_handshake_that_ends_late_is_dropped() {
    let mut stream = version_frame();
    stream.extend(verack_frame());
    let mut s = session();
    assert!(s.receive(&stream, START + 29).is_ok());
    let mut s = session();
    assert_eq!(
        dropped(s.receive(&stream, START + 30), "NR-14"),
        Drop::HandshakeTimeout
    );
}

#[test]
fn nr_14_a_finished_handshake_has_no_deadline() {
    let mut s = ready_session();
    s.tick(START + 29).unwrap();
    s.tick(START + 31).unwrap();
}

#[test]
fn nr_14_a_ping_is_sent_every_two_minutes_and_must_be_answered() {
    let mut s = ready_session();
    s.tick(START + 119).unwrap();
    assert!(s.take_outgoing().is_empty());
    s.tick(START + 120).unwrap();
    assert_eq!(outgoing(&mut s), vec![NetworkMessage::Ping(OUR_NONCE + 1)]);
    // Nothing more until the next interval.
    s.tick(START + 121).unwrap();
    assert!(s.take_outgoing().is_empty());
    // The answer, and the next ping with the next nonce.
    hear(&mut s, &frame(NetworkMessage::Pong(OUR_NONCE + 1))).unwrap();
    s.tick(START + 239).unwrap();
    assert!(s.take_outgoing().is_empty());
    s.tick(START + 240).unwrap();
    assert_eq!(outgoing(&mut s), vec![NetworkMessage::Ping(OUR_NONCE + 2)]);
}

#[test]
fn nr_14_the_ping_clock_starts_when_the_handshake_ends() {
    let mut s = session();
    let mut stream = version_frame();
    stream.extend(verack_frame());
    s.receive(&stream, START + 10).unwrap();
    s.take_outgoing();
    s.tick(START + 129).unwrap();
    assert!(s.take_outgoing().is_empty());
    s.tick(START + 130).unwrap();
    assert_eq!(outgoing(&mut s).len(), 1);
}

#[test]
fn nr_14_an_unanswered_ping_when_the_next_is_due_is_dropped() {
    let mut s = ready_session();
    s.tick(START + 120).unwrap();
    s.take_outgoing();
    s.tick(START + 239).unwrap();
    assert_eq!(dropped(s.tick(START + 240), "NR-14"), Drop::PingUnanswered);
}

#[test]
fn nr_14_a_pong_with_the_wrong_nonce_is_ignored() {
    let mut s = ready_session();
    s.tick(START + 120).unwrap();
    s.take_outgoing();
    let events = hear(&mut s, &frame(NetworkMessage::Pong(OUR_NONCE + 2))).unwrap();
    assert!(events.is_empty());
    hear(&mut s, &frame(NetworkMessage::Pong(0))).unwrap();
    assert_eq!(dropped(s.tick(START + 240), "NR-14"), Drop::PingUnanswered);
}

#[test]
fn nr_14_a_pong_when_none_is_awaited_is_ignored() {
    let mut s = ready_session();
    let events = hear(&mut s, &frame(NetworkMessage::Pong(OUR_NONCE + 1))).unwrap();
    assert!(events.is_empty());
}

#[test]
fn nr_14_the_peers_ping_is_answered_with_the_same_nonce() {
    let mut s = ready_session();
    let events = hear(&mut s, &frame(NetworkMessage::Ping(0xdead_beef))).unwrap();
    assert!(events.is_empty());
    assert_eq!(outgoing(&mut s), vec![NetworkMessage::Pong(0xdead_beef)]);
}

#[test]
fn nr_14_a_receive_sends_the_ping_that_is_due() {
    let mut s = ready_session();
    s.receive(&frame(NetworkMessage::Ping(1)), START + 125)
        .unwrap();
    let sent = outgoing(&mut s);
    assert_eq!(
        sent,
        vec![NetworkMessage::Pong(1), NetworkMessage::Ping(OUR_NONCE + 1)]
    );
}

#[test]
fn nr_05_more_than_2000000_queued_bytes_is_a_drop() {
    let mut s = ready_session();
    // A pong is 32 bytes: 62,500 of them are exactly 2,000,000.
    let ping = frame(NetworkMessage::Ping(1));
    let mut stream = Vec::new();
    for _ in 0..62_500 {
        stream.extend_from_slice(&ping);
    }
    hear(&mut s, &stream).unwrap();
    let mut s2 = ready_session();
    hear(&mut s2, &stream).unwrap();
    let d = dropped(hear(&mut s2, &ping), "NR-05");
    assert_eq!(d, Drop::SendBacklog);
    assert!(s2.take_outgoing().is_empty());
    // Emptying the queue makes room.
    assert_eq!(s.take_outgoing().len(), 2_000_000);
    hear(&mut s, &ping).unwrap();
}

#[test]
fn nr_05_a_request_that_would_pass_the_cap_is_a_drop() {
    let mut s = ready_session();
    let txids = (0..60_000u32)
        .map(|n| {
            let mut h = [0u8; 32];
            h[..4].copy_from_slice(&n.to_le_bytes());
            Txid::from_byte_array(h)
        })
        .collect();
    assert_eq!(
        dropped(s.send(Request::GetTx(txids)), "NR-05"),
        Drop::SendBacklog
    );
}

#[test]
fn nr_05_a_large_receive_is_read_in_reads_the_reader_accepts() {
    let mut s = ready_session();
    let mut stream = Vec::new();
    for n in 0..5_000u64 {
        stream.extend(frame(NetworkMessage::Ping(n)));
    }
    assert!(stream.len() > 65_536 * 2);
    hear(&mut s, &stream).unwrap();
    assert_eq!(outgoing(&mut s).len(), 5_000);
}

#[test]
fn nr_05_a_message_in_progress_is_held_while_it_arrives() {
    // A block of 200 KB arrives in pieces; the session holds it.
    let mut s = ready_session();
    let stream = raw_frame("block", &vec![7u8; 200_000]);
    let mut events = Vec::new();
    for piece in stream.chunks(10_000) {
        events.extend(hear(&mut s, piece).unwrap());
    }
    assert_eq!(events, vec![Event::Block(vec![7u8; 200_000])]);
}

#[test]
fn nr_55_a_request_from_the_peer_is_not_answered() {
    let requests = [
        frame(NetworkMessage::GetHeaders(
            bitcoin::p2p::message_blockdata::GetHeadersMessage {
                version: 70_016,
                locator_hashes: vec![BlockHash::from_byte_array([1; 32])],
                stop_hash: BlockHash::all_zeros(),
            },
        )),
        frame(NetworkMessage::GetAddr),
        frame(NetworkMessage::MemPool),
    ];
    // After the handshake.
    let mut s = ready_session();
    for request in &requests {
        assert!(hear(&mut s, request).unwrap().is_empty());
        assert!(s.take_outgoing().is_empty());
    }
    // And before it: they are skipped, and only the handshake is sent.
    let mut s = session();
    s.take_outgoing();
    for request in &requests {
        assert!(hear(&mut s, request).unwrap().is_empty());
        assert!(s.take_outgoing().is_empty());
    }
}

#[test]
fn nr_55_no_request_sends_a_transaction() {
    let mut s = ready_session();
    let hash = BlockHash::from_byte_array([9; 32]);
    let txid = Txid::from_byte_array([8; 32]);
    for request in [
        Request::GetHeaders {
            locator: vec![hash],
        },
        Request::GetTx(vec![txid]),
        Request::GetBlock(hash),
        Request::GetFilter {
            height: 5,
            block: hash,
        },
        Request::GetAddr,
    ] {
        s.send(request).unwrap();
    }
    for message in outgoing(&mut s) {
        assert!(!matches!(
            message,
            NetworkMessage::Tx(_) | NetworkMessage::Inv(_) | NetworkMessage::Block(_)
        ));
    }
}

#[test]
fn nr_30_each_request_is_written_as_the_rules_say() {
    let mut s = ready_session();
    let a = BlockHash::from_byte_array([1; 32]);
    let b = BlockHash::from_byte_array([2; 32]);
    let t1 = Txid::from_byte_array([3; 32]);
    let t2 = Txid::from_byte_array([4; 32]);
    s.send(Request::GetHeaders {
        locator: vec![a, b],
    })
    .unwrap();
    s.send(Request::GetTx(vec![t1, t2])).unwrap();
    s.send(Request::GetTx(vec![])).unwrap();
    s.send(Request::GetBlock(a)).unwrap();
    s.send(Request::GetFilter {
        height: 77,
        block: b,
    })
    .unwrap();
    s.send(Request::GetAddr).unwrap();
    let sent = outgoing(&mut s);
    assert_eq!(sent.len(), 5);
    let NetworkMessage::GetHeaders(g) = &sent[0] else {
        panic!("{:?}", sent[0])
    };
    assert_eq!(g.version, 70_016);
    assert_eq!(g.locator_hashes, vec![a, b]);
    assert_eq!(g.stop_hash, BlockHash::all_zeros());
    assert_eq!(
        sent[1],
        NetworkMessage::GetData(vec![
            Inventory::WitnessTransaction(t1),
            Inventory::WitnessTransaction(t2)
        ])
    );
    assert_eq!(
        sent[2],
        NetworkMessage::GetData(vec![Inventory::WitnessBlock(a)])
    );
    let NetworkMessage::GetCFilters(f) = &sent[3] else {
        panic!("{:?}", sent[3])
    };
    assert_eq!((f.filter_type, f.start_height, f.stop_hash), (0, 77, b));
    assert_eq!(sent[4], NetworkMessage::GetAddr);
}

#[test]
fn nr_05_a_dropped_session_stays_dropped() {
    let mut s = ready_session();
    let first = dropped(hear(&mut s, &verack_frame()), "NR-13");
    assert_eq!(
        s.receive(&frame(NetworkMessage::Ping(1)), START)
            .unwrap_err(),
        first
    );
    assert_eq!(s.receive(&[], START).unwrap_err(), first);
    assert_eq!(s.tick(START + 1).unwrap_err(), first);
    assert_eq!(s.send(Request::GetAddr).unwrap_err(), first);
    assert!(s.take_outgoing().is_empty());

    // A drop from the reader is just as lasting.
    let mut s = session();
    let first = dropped(hear(&mut s, &header_only("verack", 101)), "NR-02");
    assert_eq!(hear(&mut s, &version_frame()).unwrap_err(), first);
}

#[test]
fn nr_01_a_session_drops_a_message_for_another_network() {
    let mut s = session();
    let other = bitcoin::p2p::Magic::from(bitcoin::Network::Bitcoin);
    let d = dropped(
        hear(
            &mut s,
            &crate::encode(other, &NetworkMessage::Version(peer_version())),
        ),
        "NR-01",
    );
    assert_eq!(d, Drop::WrongMagic);
}

// ----- the same, on the encrypted transport ----------------------------------

use super::v2kit::in_v2;

macro_rules! also_over_v2 {
    ($($name:ident => $test:ident,)*) => {
        $(
            #[test]
            fn $name() {
                in_v2($test);
            }
        )*
    };
}

also_over_v2! {
    nr_11_the_version_the_counter_sends_says_as_little_as_it_can_over_v2 =>
        nr_11_the_version_the_counter_sends_says_as_little_as_it_can,
    nr_11_nothing_asks_for_compact_blocks_or_addresses_over_v2 =>
        nr_11_nothing_asks_for_compact_blocks_or_addresses,
    nr_12_the_peer_services_and_start_height_are_reported_as_claims_over_v2 =>
        nr_12_the_peer_services_and_start_height_are_reported_as_claims,
    nr_13_a_request_is_sent_once_both_veracks_are_in_over_v2 =>
        nr_13_a_request_is_sent_once_both_veracks_are_in,
    nr_14_a_ping_is_sent_every_two_minutes_and_must_be_answered_over_v2 =>
        nr_14_a_ping_is_sent_every_two_minutes_and_must_be_answered,
    nr_14_an_unanswered_ping_when_the_next_is_due_is_dropped_over_v2 =>
        nr_14_an_unanswered_ping_when_the_next_is_due_is_dropped,
    nr_14_a_pong_with_the_wrong_nonce_is_ignored_over_v2 =>
        nr_14_a_pong_with_the_wrong_nonce_is_ignored,
    nr_14_the_peers_ping_is_answered_with_the_same_nonce_over_v2 =>
        nr_14_the_peers_ping_is_answered_with_the_same_nonce,
    nr_14_a_receive_sends_the_ping_that_is_due_over_v2 =>
        nr_14_a_receive_sends_the_ping_that_is_due,
    nr_30_each_request_is_written_as_the_rules_say_over_v2 =>
        nr_30_each_request_is_written_as_the_rules_say,
}
