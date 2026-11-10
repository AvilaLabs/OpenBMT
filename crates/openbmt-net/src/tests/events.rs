//! What a good peer's messages become, after the handshake.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::p2p::address::{AddrV2, AddrV2Message, Address};
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::Inventory;
use bitcoin::p2p::message_filter::CFilter;
use bitcoin::p2p::ServiceFlags;
use bitcoin::{BlockHash, Txid, Wtxid};

use super::kit::*;
use crate::{Drop, Event, PeerAddress};

fn hear(bytes: &[u8]) -> Vec<Event> {
    ready_session().receive(bytes, START).unwrap()
}

fn txid(n: u8) -> Txid {
    Txid::from_byte_array([n; 32])
}

fn block(n: u8) -> BlockHash {
    BlockHash::from_byte_array([n; 32])
}

#[test]
fn nr_04_headers_become_an_event() {
    let headers: Vec<_> = (0..3).map(header).collect();
    let events = hear(&frame(NetworkMessage::Headers(headers.clone())));
    assert_eq!(events, vec![Event::Headers(headers)]);
    // An empty answer is an event too: the peer has nothing more.
    let events = hear(&frame(NetworkMessage::Headers(vec![])));
    assert_eq!(events, vec![Event::Headers(vec![])]);
}

#[test]
fn nr_04_2000_headers_are_read_through_a_session() {
    let headers: Vec<_> = (0..2_000).map(header).collect();
    let events = hear(&frame(NetworkMessage::Headers(headers.clone())));
    assert_eq!(events, vec![Event::Headers(headers)]);
}

#[test]
fn nr_04_inventory_becomes_txids_and_blocks_and_nothing_else() {
    let list = vec![
        Inventory::Transaction(txid(1)),
        Inventory::Block(block(2)),
        Inventory::WitnessTransaction(txid(3)),
        Inventory::WitnessBlock(block(4)),
        Inventory::CompactBlock(block(5)),
        Inventory::WTx(Wtxid::from_byte_array([6; 32])),
        Inventory::Error,
        Inventory::Unknown {
            inv_type: 99,
            hash: [7; 32],
        },
        Inventory::Transaction(txid(8)),
    ];
    let events = hear(&frame(NetworkMessage::Inv(list)));
    assert_eq!(
        events,
        vec![Event::Announced {
            txids: vec![txid(1), txid(8)],
            blocks: vec![block(2)]
        }]
    );
}

#[test]
fn nr_04_notfound_names_what_was_asked_for_with_or_without_witness() {
    let list = vec![
        Inventory::WitnessTransaction(txid(1)),
        Inventory::Transaction(txid(2)),
        Inventory::WitnessBlock(block(3)),
        Inventory::Block(block(4)),
        Inventory::CompactBlock(block(5)),
    ];
    let events = hear(&frame(NetworkMessage::NotFound(list)));
    assert_eq!(
        events,
        vec![Event::NotFound {
            txids: vec![txid(1), txid(2)],
            blocks: vec![block(3), block(4)]
        }]
    );
}

#[test]
fn nr_04_a_transaction_is_handed_on_as_the_bytes_the_peer_sent() {
    let tx = simple_tx();
    let events = hear(&frame(NetworkMessage::Tx(tx.clone())));
    assert_eq!(events, vec![Event::Tx(serialize(&tx))]);
}

#[test]
fn nr_04_bytes_that_are_not_a_transaction_are_dropped_not_handed_on() {
    let mut s = ready_session();
    let d = s
        .receive(&raw_frame("tx", &[1, 2, 3, 4]), START)
        .unwrap_err();
    assert_eq!(d, Drop::Malformed { command: "tx" });
    let mut s = ready_session();
    let mut payload = serialize(&simple_tx());
    payload.push(0);
    let d = s.receive(&raw_frame("tx", &payload), START).unwrap_err();
    assert_eq!(d, Drop::Malformed { command: "tx" });
}

#[test]
fn nr_04_a_block_is_handed_on_as_bytes_without_being_parsed() {
    let bytes: Vec<u8> = (0..=255u8).cycle().take(3_000).collect();
    let events = hear(&raw_frame("block", &bytes));
    assert_eq!(events, vec![Event::Block(bytes)]);
}

#[test]
fn nr_04_a_filter_of_type_0_becomes_an_event() {
    let filter = CFilter {
        filter_type: 0,
        block_hash: block(9),
        filter: vec![1, 2, 3, 4, 5],
    };
    let events = hear(&frame(NetworkMessage::CFilter(filter)));
    assert_eq!(
        events,
        vec![Event::Filter {
            block: block(9),
            filter: vec![1, 2, 3, 4, 5]
        }]
    );
}

#[test]
fn nr_04_a_filter_of_another_type_ends_the_session() {
    let filter = CFilter {
        filter_type: 1,
        block_hash: block(9),
        filter: vec![],
    };
    let mut s = ready_session();
    let d = s
        .receive(&frame(NetworkMessage::CFilter(filter)), START)
        .unwrap_err();
    assert_eq!(d, Drop::WrongFilterType { filter_type: 1 });
}

#[test]
fn nr_04_addr_becomes_addresses() {
    let v4 = SocketAddr::from(([1, 2, 3, 4], 8333));
    let v6 = SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 18444));
    let onion = Address {
        services: ServiceFlags::NETWORK,
        address: [0xFD87, 0xD87E, 0xEB43, 1, 2, 3, 4, 5],
        port: 9050,
    };
    let list = vec![
        (
            111u32,
            Address::new(&v4, ServiceFlags::NETWORK | ServiceFlags::WITNESS),
        ),
        (222, onion),
        (333, Address::new(&v6, ServiceFlags::NONE)),
    ];
    let events = hear(&frame(NetworkMessage::Addr(list)));
    assert_eq!(
        events,
        vec![Event::Addresses(vec![
            PeerAddress {
                addr: AddrV2::Ipv4(Ipv4Addr::new(1, 2, 3, 4)),
                port: 8333,
                services: ServiceFlags::NETWORK | ServiceFlags::WITNESS,
                time: 111
            },
            PeerAddress {
                addr: AddrV2::Ipv6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
                port: 18444,
                services: ServiceFlags::NONE,
                time: 333
            },
        ])]
    );
}

#[test]
fn nr_04_addrv2_becomes_addresses_of_every_kind() {
    let list = vec![
        AddrV2Message {
            time: 5,
            services: ServiceFlags::NETWORK,
            addr: AddrV2::Ipv4(Ipv4Addr::new(9, 8, 7, 6)),
            port: 8333,
        },
        AddrV2Message {
            time: 6,
            services: ServiceFlags::NETWORK_LIMITED,
            addr: AddrV2::TorV3([5; 32]),
            port: 8333,
        },
        AddrV2Message {
            time: 7,
            services: ServiceFlags::NONE,
            addr: AddrV2::I2p([6; 32]),
            port: 0,
        },
    ];
    let events = hear(&frame(NetworkMessage::AddrV2(list.clone())));
    let expected: Vec<_> = list
        .into_iter()
        .map(|m| PeerAddress {
            addr: m.addr,
            port: m.port,
            services: m.services,
            time: m.time,
        })
        .collect();
    assert_eq!(events, vec![Event::Addresses(expected)]);
}

#[test]
fn nr_04_other_known_messages_are_ignored() {
    for stream in [
        raw_frame("sendcmpct", &[0; 9]),
        raw_frame("feefilter", &[0; 8]),
        frame(NetworkMessage::WtxidRelay),
        frame(NetworkMessage::SendAddrV2),
        frame(NetworkMessage::SendHeaders),
        raw_frame("reject", &[0; 20]),
        raw_frame("cmpctblock", &[0; 20]),
        raw_frame("merkleblock", &[0; 20]),
        raw_frame("cfheaders", &[0; 20]),
    ] {
        assert!(hear(&stream).is_empty());
    }
}

#[test]
fn nr_04_several_messages_in_one_read_make_events_in_order() {
    let mut stream = frame(NetworkMessage::Headers(vec![header(1)]));
    stream.extend(frame(NetworkMessage::Ping(4)));
    stream.extend(frame(NetworkMessage::Inv(vec![Inventory::Transaction(
        txid(1),
    )])));
    stream.extend(raw_frame("block", &[1, 2, 3]));
    let mut s = ready_session();
    let events = s.receive(&stream, START).unwrap();
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], Event::Headers(_)));
    assert!(matches!(events[1], Event::Announced { .. }));
    assert!(matches!(events[2], Event::Block(_)));
    assert_eq!(outgoing(&mut s), vec![NetworkMessage::Pong(4)]);
}

/// One `addrv2` entry as BIP155 writes it: time, services, network id,
/// address length, address, port.
fn addrv2_entry(network: u8, addr: &[u8], port: u16) -> Vec<u8> {
    let mut out = 5u32.to_le_bytes().to_vec();
    // The services, as a compact size: the bits are claims.
    out.push(0x09);
    out.push(network);
    out.push(u8::try_from(addr.len()).unwrap());
    out.extend_from_slice(addr);
    out.extend_from_slice(&port.to_be_bytes());
    out
}

#[test]
fn nr_95_a_tor_v3_entry_of_addrv2_is_read_and_the_book_keeps_it() {
    let key = [0x42u8; 32];
    let mut payload = vec![1u8];
    payload.extend(addrv2_entry(4, &key, 8333));
    let events = hear(&raw_frame("addrv2", &payload));
    let Some(Event::Addresses(list)) = events.first() else {
        panic!("{events:?}");
    };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].addr, AddrV2::TorV3(key));
    assert_eq!(list[0].port, 8333);
    let mut book = crate::AddressBook::new();
    let added = book.add(
        &list[0].addr,
        list[0].port,
        list[0].services,
        crate::Source::Seed,
        0,
    );
    assert_eq!(added, crate::Added::New);
    assert!(book.get(&AddrV2::TorV3(key), 8333).is_some());
}

#[test]
fn nr_95_a_tor_v3_entry_with_a_key_of_another_length_is_not_an_address() {
    // BIP155 fixes the length of a Tor v3 key at 32 bytes.
    let mut payload = vec![1u8];
    payload.extend(addrv2_entry(4, &[0x42u8; 31], 8333));
    let mut s = ready_session();
    match s.receive(&raw_frame("addrv2", &payload), START) {
        Err(Drop::Malformed { command }) => assert_eq!(command, "addrv2"),
        Ok(events) => {
            let kept = events.iter().any(|e| {
                matches!(e, Event::Addresses(l)
                    if l.iter().any(|a| matches!(a.addr, AddrV2::TorV3(_))))
            });
            assert!(!kept, "{events:?}");
        }
        other => panic!("{other:?}"),
    }
}
