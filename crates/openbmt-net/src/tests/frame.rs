//! NR-01 to NR-05 at the level of the frame reader.

use bitcoin::consensus::encode::{serialize, VarInt};
use bitcoin::hashes::Hash;
use bitcoin::p2p::address::{AddrV2, AddrV2Message, Address};
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_filter::CFilter;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, Network, Txid};

use super::kit::*;
use crate::frame::HEADER_LEN;
use crate::{Command, Drop, Frame, FrameReader, Message, PUSH_SLACK};

/// Push in reads of 16 KiB and drain after each, as a driver would.
fn feed(reader: &mut FrameReader, bytes: &[u8]) -> Result<Vec<Frame>, Drop> {
    let mut out = Vec::new();
    for chunk in bytes.chunks(16 * 1024) {
        reader.push(chunk)?;
        while let Some(frame) = reader.next()? {
            out.push(frame);
        }
    }
    Ok(out)
}

fn expect_drop(result: Result<Vec<Frame>, Drop>, rule: &str) -> Drop {
    let d = result.unwrap_err();
    assert_eq!(d.rule(), rule, "{d:?}");
    assert!(d.to_string().starts_with(&format!("{rule}: ")), "{d}");
    d
}

#[test]
fn nr_01_a_good_message_comes_out_whole() {
    let mut r = open_reader();
    let got = feed(&mut r, &frame(NetworkMessage::Ping(9))).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].command, Command::Ping);
    assert_eq!(got[0].payload, 9u64.to_le_bytes());
    assert_eq!(r.buffered(), 0);
}

#[test]
fn nr_01_wrong_magic_is_dropped_from_the_header() {
    let mut r = open_reader();
    let other = bitcoin::p2p::Magic::from(Network::Bitcoin).to_bytes();
    let header = raw_header(other, &command_field("verack"), 0, checksum(&[]));
    let d = expect_drop(feed(&mut r, &header), "NR-01");
    assert_eq!(d, Drop::WrongMagic);
}

#[test]
fn nr_01_bad_checksum_is_dropped() {
    let mut r = open_reader();
    let mut bytes = frame(NetworkMessage::Ping(9));
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let d = expect_drop(feed(&mut r, &bytes), "NR-01");
    assert_eq!(d, Drop::BadChecksum);
    // A wrong checksum field with a right payload is the same.
    let mut r = open_reader();
    let mut bytes = frame(NetworkMessage::Ping(9));
    bytes[20] ^= 1;
    assert_eq!(
        expect_drop(feed(&mut r, &bytes), "NR-01"),
        Drop::BadChecksum
    );
}

#[test]
fn nr_01_command_with_bytes_after_the_nul_is_dropped() {
    let mut field = command_field("verack");
    field[11] = b'x';
    let mut r = open_reader();
    let header = raw_header(magic().to_bytes(), &field, 0, checksum(&[]));
    assert_eq!(
        expect_drop(feed(&mut r, &header), "NR-01"),
        Drop::BadCommand
    );
}

#[test]
fn nr_01_command_with_non_printable_bytes_is_dropped() {
    for bad in [0x01u8, 0x1f, 0x7f, 0x80, 0xff] {
        let mut field = command_field("verack");
        field[2] = bad;
        let mut r = open_reader();
        let header = raw_header(magic().to_bytes(), &field, 0, checksum(&[]));
        assert_eq!(
            expect_drop(feed(&mut r, &header), "NR-01"),
            Drop::BadCommand
        );
    }
}

#[test]
fn nr_01_an_empty_command_is_dropped() {
    let mut r = open_reader();
    let header = raw_header(magic().to_bytes(), &[0; 12], 0, checksum(&[]));
    assert_eq!(
        expect_drop(feed(&mut r, &header), "NR-01"),
        Drop::BadCommand
    );
}

#[test]
fn nr_01_a_full_twelve_byte_command_is_read() {
    // Twelve printable bytes and no NUL is a valid, unknown command.
    let mut r = open_reader();
    let mut bytes = raw_frame("abcdefghijkl", &[1, 2, 3]);
    bytes.extend(frame(NetworkMessage::Verack));
    let got = feed(&mut r, &bytes).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].command, Command::Verack);
}

/// Every command with a limit, and the limit.
const LIMITS: [(&str, Command, u32); 17] = [
    ("block", Command::Block, 4_000_000),
    ("tx", Command::Tx, 400_000),
    ("cfilter", Command::CFilter, 1_000_000),
    ("headers", Command::Headers, 162_003),
    ("inv", Command::Inv, 1_800_003),
    ("notfound", Command::NotFound, 1_800_003),
    ("addr", Command::Addr, 50_003),
    ("addrv2", Command::AddrV2, 50_003),
    ("version", Command::Version, 400),
    ("verack", Command::Verack, 100),
    ("ping", Command::Ping, 100),
    ("pong", Command::Pong, 100),
    ("sendaddrv2", Command::SendAddrV2, 100),
    ("sendheaders", Command::SendHeaders, 100),
    ("wtxidrelay", Command::WtxidRelay, 100),
    ("feefilter", Command::FeeFilter, 100),
    ("sendcmpct", Command::SendCmpct, 100),
];

#[test]
fn nr_02_each_command_is_accepted_at_exactly_its_limit() {
    for (name, command, limit) in LIMITS {
        assert_eq!(command.limit(), limit, "{name}");
        assert_eq!(command.name(), name);
        let payload = vec![0u8; limit as usize];
        let mut r = open_reader();
        let got = feed(&mut r, &raw_frame(name, &payload)).unwrap();
        assert_eq!(got.len(), 1, "{name}");
        assert_eq!(got[0].command, command);
        assert_eq!(got[0].payload.len(), limit as usize);
    }
}

#[test]
fn nr_02_each_command_is_dropped_one_over_its_limit_from_the_header_alone() {
    for (name, command, limit) in LIMITS {
        let mut r = open_reader();
        // Only the header is fed: not one payload byte has arrived.
        let d = expect_drop(feed(&mut r, &header_only(name, limit + 1)), "NR-02");
        assert_eq!(
            d,
            Drop::TooLong {
                command: command.name(),
                len: limit + 1,
                limit
            }
        );
        assert!(r.buffered() == 0);
    }
}

#[test]
fn nr_02_a_huge_length_is_dropped_before_anything_is_kept() {
    let mut r = open_reader();
    let d = expect_drop(feed(&mut r, &header_only("block", u32::MAX)), "NR-02");
    assert!(matches!(d, Drop::TooLong { len: u32::MAX, .. }));
}

#[test]
fn nr_03_an_unknown_command_is_skipped_at_100000_and_dropped_at_100001() {
    let mut r = open_reader();
    let mut bytes = raw_frame("getdata", &vec![0u8; 100_000]);
    bytes.extend(frame(NetworkMessage::Ping(5)));
    let got = feed(&mut r, &bytes).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].command, Command::Ping);

    let mut r = open_reader();
    let d = expect_drop(feed(&mut r, &header_only("getdata", 100_001)), "NR-03");
    assert_eq!(d, Drop::UnknownTooLong { len: 100_001 });
}

#[test]
fn nr_03_an_unknown_command_still_needs_its_checksum() {
    let mut r = open_reader();
    let mut bytes = raw_frame("mempool", &[1, 2, 3]);
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    assert_eq!(
        expect_drop(feed(&mut r, &bytes), "NR-01"),
        Drop::BadChecksum
    );
}

#[test]
fn nr_03_unknown_commands_are_skipped_before_the_handshake() {
    let mut r = FrameReader::new(magic());
    let mut bytes = raw_frame("getheaders", &[0; 50]);
    bytes.extend(raw_frame("alert", &[0; 50]));
    assert!(feed(&mut r, &bytes).unwrap().is_empty());
}

#[test]
fn nr_04_trailing_bytes_in_a_payload_are_dropped() {
    let tx = simple_tx();
    let one_inv = vec![Inventory::Transaction(Txid::from_byte_array([1; 32]))];
    let messages = [
        NetworkMessage::Version(peer_version()),
        NetworkMessage::Ping(1),
        NetworkMessage::Pong(1),
        NetworkMessage::Headers(vec![header(1)]),
        NetworkMessage::Inv(one_inv.clone()),
        NetworkMessage::NotFound(one_inv),
        NetworkMessage::Tx(tx),
        NetworkMessage::CFilter(CFilter {
            filter_type: 0,
            block_hash: BlockHash::from_byte_array([2; 32]),
            filter: vec![1, 2, 3],
        }),
        NetworkMessage::Addr(vec![]),
        NetworkMessage::AddrV2(vec![]),
    ];
    for message in messages {
        let good = Frame {
            command: command_of(&message),
            payload: payload_of(&message),
        };
        good.clone()
            .parse()
            .unwrap_or_else(|d| panic!("{message:?}: {d}"));
        let mut over = good;
        over.payload.push(0);
        let d = over.parse().unwrap_err();
        assert_eq!(d.rule(), "NR-04", "{message:?}");
        assert!(matches!(d, Drop::Malformed { .. }));
    }
    // A message with no payload has none.
    for command in [Command::Verack, Command::SendAddrV2, Command::SendHeaders] {
        let d = Frame {
            command,
            payload: vec![0],
        }
        .parse()
        .unwrap_err();
        assert_eq!(
            d,
            Drop::Malformed {
                command: command.name()
            }
        );
    }
}

fn command_of(message: &NetworkMessage) -> Command {
    match message {
        NetworkMessage::Version(_) => Command::Version,
        NetworkMessage::Ping(_) => Command::Ping,
        NetworkMessage::Pong(_) => Command::Pong,
        NetworkMessage::Headers(_) => Command::Headers,
        NetworkMessage::Inv(_) => Command::Inv,
        NetworkMessage::NotFound(_) => Command::NotFound,
        NetworkMessage::Tx(_) => Command::Tx,
        NetworkMessage::CFilter(_) => Command::CFilter,
        NetworkMessage::Addr(_) => Command::Addr,
        NetworkMessage::AddrV2(_) => Command::AddrV2,
        other => panic!("not in the table: {other:?}"),
    }
}

#[test]
fn nr_04_a_payload_that_does_not_parse_is_dropped() {
    let cases = [
        (Command::Version, vec![1u8; 20]),
        (Command::Ping, vec![1u8; 7]),
        (Command::Ping, vec![]),
        (Command::Pong, vec![1u8; 9]),
        (Command::Headers, vec![]),
        (Command::Headers, vec![1, 0]),
        (Command::Inv, vec![1, 1, 0, 0, 0]),
        (Command::Tx, vec![1, 2, 3]),
        (Command::CFilter, vec![0, 1, 2]),
        (Command::Addr, vec![1]),
        (Command::AddrV2, vec![1, 0]),
    ];
    for (command, payload) in cases {
        let d = Frame { command, payload }.parse().unwrap_err();
        assert_eq!(
            d,
            Drop::Malformed {
                command: command.name()
            }
        );
        assert_eq!(d.rule(), "NR-04");
    }
}

#[test]
fn nr_04_headers_over_2000_are_dropped_and_2000_are_read() {
    let headers: Vec<_> = (0..2_000).map(header).collect();
    let payload = payload_of(&NetworkMessage::Headers(headers.clone()));
    assert_eq!(payload.len(), 162_003);
    let msg = Frame {
        command: Command::Headers,
        payload,
    }
    .parse()
    .unwrap();
    assert_eq!(msg, Message::Headers(headers));
    // The count states 2,001; the length check on the count comes first.
    let mut payload = serialize(&VarInt(2_001));
    payload.extend([0u8; 10]);
    let d = Frame {
        command: Command::Headers,
        payload,
    }
    .parse()
    .unwrap_err();
    assert_eq!(
        d,
        Drop::TooManyItems {
            command: "headers",
            count: 2_001,
            limit: 2_000
        }
    );
    assert_eq!(d.rule(), "NR-04");
}

#[test]
fn nr_04_a_header_followed_by_a_transaction_count_is_dropped() {
    let mut payload = payload_of(&NetworkMessage::Headers(vec![header(1)]));
    let last = payload.len() - 1;
    payload[last] = 1;
    let d = Frame {
        command: Command::Headers,
        payload,
    }
    .parse()
    .unwrap_err();
    assert_eq!(d, Drop::Malformed { command: "headers" });
}

#[test]
fn nr_04_inventory_of_50000_is_read() {
    let list: Vec<_> = (0..50_000u32)
        .map(|n| {
            let mut h = [0u8; 32];
            h[..4].copy_from_slice(&n.to_le_bytes());
            Inventory::Transaction(Txid::from_byte_array(h))
        })
        .collect();
    let payload = serialize(&list);
    assert_eq!(payload.len(), 1_800_003);
    let msg = Frame {
        command: Command::Inv,
        payload,
    }
    .parse()
    .unwrap();
    assert!(matches!(msg, Message::Inv { ref txids, .. } if txids.len() == 50_000));
    // More than 50,000 entries cannot fit the payload limit; a count that
    // states them is refused all the same.
    let mut payload = serialize(&VarInt(50_001));
    payload.extend([0u8; 36]);
    let d = Frame {
        command: Command::Inv,
        payload,
    }
    .parse()
    .unwrap_err();
    assert_eq!(d.rule(), "NR-04");
}

fn v2(n: u8) -> AddrV2Message {
    AddrV2Message {
        time: 1_700_000_000,
        services: ServiceFlags::NETWORK,
        addr: AddrV2::Ipv4(std::net::Ipv4Addr::new(1, 2, 3, n)),
        port: 8333,
    }
}

#[test]
fn nr_04_more_than_1000_addresses_are_dropped_and_1000_are_read() {
    for count in [1_000usize, 1_001] {
        let v2s = NetworkMessage::AddrV2((0..count).map(|n| v2(n as u8)).collect());
        let old = NetworkMessage::Addr(
            (0..count)
                .map(|n| {
                    let sock = std::net::SocketAddr::from(([1, 2, 3, n as u8], 8333));
                    (7u32, Address::new(&sock, ServiceFlags::NETWORK))
                })
                .collect(),
        );
        for (command, message) in [(Command::AddrV2, v2s), (Command::Addr, old)] {
            let result = Frame {
                command,
                payload: payload_of(&message),
            }
            .parse();
            if count == 1_000 {
                assert!(matches!(result, Ok(Message::Addresses(ref a)) if a.len() == 1_000));
            } else {
                let d = result.unwrap_err();
                assert_eq!(
                    d,
                    Drop::TooManyItems {
                        command: command.name(),
                        count: 1_001,
                        limit: 1_000
                    }
                );
                assert_eq!(d.rule(), "NR-04");
            }
        }
    }
}

#[test]
fn nr_04_a_filter_of_another_type_is_dropped() {
    let payload = payload_of(&NetworkMessage::CFilter(CFilter {
        filter_type: 1,
        block_hash: BlockHash::from_byte_array([2; 32]),
        filter: vec![],
    }));
    let d = Frame {
        command: Command::CFilter,
        payload,
    }
    .parse()
    .unwrap_err();
    assert_eq!(d, Drop::WrongFilterType { filter_type: 1 });
    assert_eq!(d.rule(), "NR-04");
}

#[test]
fn nr_01_a_message_split_at_every_byte_boundary_is_read() {
    let mut stream = frame(NetworkMessage::Ping(1));
    stream.extend(frame(NetworkMessage::Headers(vec![header(1), header(2)])));
    stream.extend(frame(NetworkMessage::Pong(2)));
    let mut whole = open_reader();
    let expected = feed(&mut whole, &stream).unwrap();
    assert_eq!(expected.len(), 3);
    for cut in 0..=stream.len() {
        let mut r = open_reader();
        let mut got = feed(&mut r, &stream[..cut]).unwrap();
        got.extend(feed(&mut r, &stream[cut..]).unwrap());
        assert_eq!(got, expected, "cut at {cut}");
    }
}

#[test]
fn nr_01_a_stream_fed_one_byte_at_a_time_is_read() {
    let mut stream = frame(NetworkMessage::Ping(1));
    stream.extend(raw_frame("getdata", &[9; 40]));
    stream.extend(frame(NetworkMessage::Pong(2)));
    let mut r = open_reader();
    let mut got = Vec::new();
    for byte in &stream {
        got.extend(feed(&mut r, std::slice::from_ref(byte)).unwrap());
    }
    assert_eq!(got.len(), 2);
    assert_eq!(
        (got[0].command, got[1].command),
        (Command::Ping, Command::Pong)
    );
}

#[test]
fn nr_01_two_messages_in_one_push_come_out_one_at_a_time() {
    let mut stream = frame(NetworkMessage::Ping(1));
    stream.extend(frame(NetworkMessage::Pong(2)));
    let mut r = open_reader();
    r.push(&stream).unwrap();
    assert_eq!(r.next().unwrap().unwrap().command, Command::Ping);
    assert_eq!(r.next().unwrap().unwrap().command, Command::Pong);
    assert_eq!(r.next().unwrap(), None);
    assert_eq!(r.buffered(), 0);
}

#[test]
fn nr_05_a_push_beyond_the_message_in_progress_and_one_read_is_dropped() {
    // No header has been read: a header and one read may be held.
    let mut r = open_reader();
    assert_eq!(r.buffer_cap(), HEADER_LEN + PUSH_SLACK);
    r.push(&vec![0u8; PUSH_SLACK]).unwrap();
    let mut r = open_reader();
    let d = r.push(&vec![0u8; PUSH_SLACK + HEADER_LEN + 1]).unwrap_err();
    assert_eq!(d, Drop::ReceiveBacklog);
    assert_eq!(d.rule(), "NR-05");
    assert_eq!(r.buffered(), 0);
}

#[test]
fn nr_05_the_cap_follows_the_limit_of_the_message_in_progress() {
    // A ping header: the cap is 24 + 100 + one read.
    let mut r = open_reader();
    r.push(&header_only("ping", 100)).unwrap();
    assert_eq!(r.next().unwrap(), None);
    let cap = HEADER_LEN + 100 + PUSH_SLACK;
    assert_eq!(r.buffer_cap(), cap);
    let room = cap - r.buffered();
    let mut over = r;
    assert_eq!(
        over.push(&vec![0u8; room + 1]).unwrap_err(),
        Drop::ReceiveBacklog
    );

    let mut r = open_reader();
    r.push(&header_only("ping", 100)).unwrap();
    assert_eq!(r.next().unwrap(), None);
    r.push(&vec![0u8; room]).unwrap();
    assert_eq!(r.buffered(), cap);
}

#[test]
fn nr_05_a_block_of_the_largest_size_is_held_in_reads_of_a_driver() {
    let payload = vec![0u8; 4_000_000];
    let mut r = open_reader();
    let bytes = raw_frame("block", &payload);
    let mut most = 0;
    let mut got = 0;
    for chunk in bytes.chunks(PUSH_SLACK) {
        r.push(chunk).unwrap();
        most = most.max(r.buffered());
        while let Some(f) = r.next().unwrap() {
            assert_eq!(f.payload.len(), 4_000_000);
            got += 1;
        }
    }
    assert_eq!(got, 1);
    assert!(most <= HEADER_LEN + 4_000_000);
    assert_eq!(r.buffered(), 0);
}

#[test]
fn nr_05_a_dropped_reader_stays_dropped() {
    let mut r = open_reader();
    let first = r
        .push(&header_only("verack", 101))
        .and_then(|()| r.next())
        .unwrap_err();
    assert_eq!(r.next().unwrap_err(), first);
    assert_eq!(r.push(&[0]).unwrap_err(), first);
}

#[test]
fn nr_13_a_command_for_after_the_handshake_is_dropped_at_its_header() {
    for name in [
        "headers", "inv", "notfound", "tx", "block", "cfilter", "addr", "addrv2", "ping", "pong",
    ] {
        let mut r = FrameReader::new(magic());
        let d = expect_drop(feed(&mut r, &header_only(name, 5)), "NR-13");
        assert!(matches!(d, Drop::BeforeHandshake { command } if command == name));
    }
    for message in [
        NetworkMessage::Verack,
        NetworkMessage::SendAddrV2,
        NetworkMessage::SendHeaders,
        NetworkMessage::WtxidRelay,
        NetworkMessage::Version(peer_version()),
    ] {
        let mut r = FrameReader::new(magic());
        assert_eq!(feed(&mut r, &frame(message)).unwrap().len(), 1);
    }
}
