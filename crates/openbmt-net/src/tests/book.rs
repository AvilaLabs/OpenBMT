//! NR-22 to NR-25: the address book.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bitcoin::p2p::address::AddrV2;
use bitcoin::p2p::ServiceFlags;
use openbmt_core::wire::DecodeError;
use openbmt_watch::{network_group, NetGroup, PeerAddr};

use crate::book::{
    Added, AddressBook, BookError, Refusal, Source, BAN_SECS, MAX_ENTRIES, MAX_FROM_SEEDS,
    MAX_PER_GROUP, MAX_PER_PEER_GROUP, RETRY_SECS,
};

const PORT: u16 = 8333;

fn v4(a: u8, b: u8, c: u8, d: u8) -> AddrV2 {
    AddrV2::Ipv4(Ipv4Addr::new(a, b, c, d))
}

/// An address in the `idx`-th network group of 16,000 routable ones.
fn gv4(idx: u16, host: u8) -> AddrV2 {
    let a = 20 + u8::try_from(idx / 200).unwrap();
    let b = 1 + u8::try_from(idx % 200).unwrap();
    v4(a, b, 0, host)
}

fn group_of(addr: &AddrV2) -> NetGroup {
    match addr {
        AddrV2::Ipv4(a) => network_group(&PeerAddr::Ip(IpAddr::V4(*a))),
        AddrV2::Ipv6(a) => network_group(&PeerAddr::Ip(IpAddr::V6(*a))),
        _ => network_group(&PeerAddr::Named),
    }
}

fn svc() -> ServiceFlags {
    ServiceFlags::NETWORK | ServiceFlags::WITNESS
}

fn filters() -> ServiceFlags {
    svc() | ServiceFlags::COMPACT_FILTERS
}

fn add(book: &mut AddressBook, addr: &AddrV2, source: Source) -> Added {
    book.add(addr, PORT, svc(), source, 0)
}

fn none() -> BTreeSet<NetGroup> {
    BTreeSet::new()
}

/// A book with `n` single-address groups, from `Own`.
fn own_book(n: u16) -> AddressBook {
    let mut book = AddressBook::new();
    for i in 0..n {
        assert_eq!(add(&mut book, &gv4(i, 1), Source::Own), Added::New);
    }
    book
}

fn v6(text: &str) -> AddrV2 {
    AddrV2::Ipv6(text.parse::<Ipv6Addr>().unwrap())
}

#[test]
fn nr_22_refuses_addresses_that_are_not_publicly_routable() {
    let private_v4 = [
        [10, 1, 2, 3],
        [172, 16, 0, 1],
        [172, 31, 255, 255],
        [192, 168, 1, 1],
        [127, 0, 0, 1],
        [0, 0, 0, 0],
        [0, 1, 2, 3],
        [224, 0, 0, 1],
        [239, 255, 255, 250],
        [255, 255, 255, 255],
        [169, 254, 1, 1],
        [100, 64, 0, 1],
        [192, 0, 2, 1],
        [198, 51, 100, 7],
        [203, 0, 113, 9],
    ];
    let mut book = AddressBook::new();
    for o in private_v4 {
        let addr = v4(o[0], o[1], o[2], o[3]);
        assert_eq!(
            add(&mut book, &addr, Source::Own),
            Added::Refused(Refusal::NotRoutable),
            "{addr:?}"
        );
    }
    for text in [
        "::",
        "::1",
        "fe80::1",
        "fc00::1",
        "fd12:3456::1",
        "ff02::1",
        "2001:db8::1",
        "::ffff:1.2.3.4",
        "2002:0a00:0001::1",
    ] {
        assert_eq!(
            add(&mut book, &v6(text), Source::Own),
            Added::Refused(Refusal::NotRoutable),
            "{text}"
        );
    }
    assert!(book.is_empty());
}

#[test]
fn nr_22_keeps_only_ipv4_ipv6_and_tor_v3() {
    let mut book = AddressBook::new();
    for addr in [
        AddrV2::I2p([7; 32]),
        AddrV2::Cjdns("fc00::1".parse().unwrap()),
        AddrV2::TorV2([3; 10]),
        AddrV2::Unknown(99, vec![1, 2, 3]),
    ] {
        assert_eq!(
            add(&mut book, &addr, Source::Own),
            Added::Refused(Refusal::KindNotKept),
            "{addr:?}"
        );
    }
    assert_eq!(
        book.add(&v4(8, 8, 8, 8), 0, svc(), Source::Own, 0),
        Added::Refused(Refusal::PortZero)
    );
    assert!(book.is_empty());
}

#[test]
fn nr_22_accepts_routable_v4_v6_and_tor_v3() {
    let mut book = AddressBook::new();
    for addr in [v4(8, 8, 8, 8), v6("2606:4700::1"), AddrV2::TorV3([5; 32])] {
        assert_eq!(add(&mut book, &addr, Source::Seed), Added::New, "{addr:?}");
        let e = book.get(&addr, PORT).unwrap();
        assert_eq!(e.group, group_of(&addr));
        assert_eq!(e.services, svc());
    }
    assert_eq!(book.len(), 3);
}

#[test]
fn nr_22_takes_at_most_eight_per_network_group() {
    let mut book = AddressBook::new();
    for host in 1..=8 {
        assert_eq!(add(&mut book, &v4(30, 1, 0, host), Source::Own), Added::New);
    }
    // Another address of the same /16, and another port of a known one.
    assert_eq!(
        add(&mut book, &v4(30, 1, 77, 9), Source::Own),
        Added::Refused(Refusal::GroupFull)
    );
    assert_eq!(
        book.add(&v4(30, 1, 0, 1), 8334, svc(), Source::Own, 0),
        Added::Refused(Refusal::GroupFull)
    );
    // A known one is still known, and another group still takes addresses.
    assert_eq!(add(&mut book, &v4(30, 1, 0, 1), Source::Own), Added::Known);
    assert_eq!(add(&mut book, &v4(30, 2, 0, 1), Source::Own), Added::New);
    assert_eq!(MAX_PER_GROUP, 8);
    // All Tor addresses are one group.
    for i in 0..8 {
        assert_eq!(
            add(&mut book, &AddrV2::TorV3([i + 1; 32]), Source::Own),
            Added::New
        );
    }
    assert_eq!(
        add(&mut book, &AddrV2::TorV3([99; 32]), Source::Own),
        Added::Refused(Refusal::GroupFull)
    );
}

#[test]
fn nr_22_takes_at_most_64_from_the_peers_of_one_network_group() {
    let told_by = Source::Peer(group_of(&v4(77, 7, 7, 7)));
    let other_peer = Source::Peer(group_of(&v4(88, 8, 8, 8)));
    let mut book = AddressBook::new();
    for i in 0..64 {
        assert_eq!(
            add(&mut book, &gv4(i / 8, i as u8 % 8 + 1), told_by),
            Added::New
        );
    }
    assert_eq!(
        add(&mut book, &gv4(100, 1), told_by),
        Added::Refused(Refusal::SourceFull)
    );
    assert_eq!(add(&mut book, &gv4(100, 1), other_peer), Added::New);
    assert_eq!(add(&mut book, &gv4(101, 1), Source::Seed), Added::New);
    assert_eq!(add(&mut book, &gv4(102, 1), Source::Own), Added::New);
    assert_eq!(MAX_PER_PEER_GROUP, 64);
}

#[test]
fn nr_22_takes_at_most_256_from_seeds() {
    let mut book = AddressBook::new();
    for i in 0..256 {
        let addr = gv4(i / 8, i as u8 % 8 + 1);
        assert_eq!(add(&mut book, &addr, Source::Seed), Added::New, "{i}");
    }
    assert_eq!(
        add(&mut book, &gv4(500, 1), Source::Seed),
        Added::Refused(Refusal::SourceFull)
    );
    assert_eq!(add(&mut book, &gv4(500, 1), Source::Own), Added::New);
    assert_eq!(MAX_FROM_SEEDS, 256);
}

#[test]
fn nr_22_takes_at_most_2000_in_all() {
    let mut book = AddressBook::new();
    for i in 0..250 {
        for host in 1..=8 {
            assert_eq!(add(&mut book, &gv4(i, host), Source::Own), Added::New);
        }
    }
    assert_eq!(book.len(), MAX_ENTRIES);
    assert_eq!(
        add(&mut book, &gv4(300, 1), Source::Own),
        Added::Refused(Refusal::BookFull)
    );
    assert_eq!(add(&mut book, &gv4(0, 1), Source::Own), Added::Known);
    assert_eq!(book.len(), MAX_ENTRIES);
}

#[test]
fn nr_22_a_full_book_drops_only_entries_that_failed_three_times() {
    let mut book = AddressBook::new();
    for i in 0..250 {
        for host in 1..=8 {
            add(&mut book, &gv4(i, host), Source::Own);
        }
    }
    // Two failures are not enough, and nothing else is dropped.
    book.failed(&gv4(0, 1), PORT, 10);
    book.failed(&gv4(0, 1), PORT, 11);
    assert_eq!(
        add(&mut book, &gv4(300, 1), Source::Own),
        Added::Refused(Refusal::BookFull)
    );
    // Two anchors fail three times, then stop being anchors. The one tried
    // longest ago goes first.
    let (a, b) = (gv4(1, 1), gv4(2, 1));
    book.set_anchors(&[(a.clone(), PORT), (b.clone(), PORT)]);
    assert_eq!(book.pick(&none(), false, 0, 100).unwrap().addr, a);
    assert_eq!(book.pick(&none(), false, 0, 200).unwrap().addr, b);
    for addr in [&a, &b] {
        for t in 0..3 {
            book.failed(addr, PORT, 300 + t);
        }
        assert_eq!(book.get(addr, PORT).unwrap().failures, 3);
    }
    book.set_anchors(&[]);
    assert_eq!(book.len(), MAX_ENTRIES);
    assert_eq!(add(&mut book, &gv4(300, 1), Source::Own), Added::New);
    assert_eq!(book.len(), MAX_ENTRIES);
    assert!(book.get(&a, PORT).is_none());
    assert!(book.get(&b, PORT).is_some());
    assert!(book.get(&gv4(0, 1), PORT).is_some());
    assert_eq!(add(&mut book, &gv4(301, 1), Source::Own), Added::New);
    assert!(book.get(&b, PORT).is_none());
    assert_eq!(
        add(&mut book, &gv4(302, 1), Source::Own),
        Added::Refused(Refusal::BookFull)
    );
}

#[test]
fn nr_22_a_known_address_keeps_its_ban_and_failures() {
    let mut book = AddressBook::new();
    let addr = v4(9, 9, 9, 9);
    let told_by = Source::Peer(group_of(&v4(77, 7, 7, 7)));
    assert_eq!(book.add(&addr, PORT, svc(), told_by, 0), Added::New);
    book.failed(&addr, PORT, 5);
    book.misbehaved(&addr, PORT, 10);
    // A peer's claim does not change the services; the owner's does.
    assert_eq!(book.add(&addr, PORT, filters(), told_by, 20), Added::Known);
    assert_eq!(book.get(&addr, PORT).unwrap().services, svc());
    assert_eq!(
        book.add(&addr, PORT, filters(), Source::Own, 20),
        Added::Known
    );
    assert_eq!(book.get(&addr, PORT).unwrap().services, filters());
    let e = book.get(&addr, PORT).unwrap();
    assert_eq!(e.failures, 1);
    assert_eq!(e.banned_until, Some(10 + BAN_SECS));
    assert_eq!(e.source, told_by);
    assert!(book.pick(&none(), false, 0, 30).is_none());
}

#[test]
fn nr_22_one_peer_cannot_fill_the_book_with_made_up_addresses() {
    let flooder = Source::Peer(group_of(&v4(77, 7, 7, 7)));
    let mut book = AddressBook::new();
    for i in 0..10_000u16 {
        // 5,000 groups, two addresses in each.
        let _ = add(
            &mut book,
            &gv4(i % 5_000, 1 + u8::try_from(i / 5_000).unwrap()),
            flooder,
        );
    }
    let from_flooder = book.iter().filter(|e| e.source == flooder).count();
    assert_eq!(from_flooder, 64);
    assert_eq!(book.len(), 64);
    // Honest sources still add.
    assert_eq!(add(&mut book, &gv4(6_000, 1), Source::Seed), Added::New);
}

#[test]
fn nr_23_never_returns_a_connected_banned_or_recently_tried_entry() {
    let mut book = AddressBook::new();
    let (a, b, c, d) = (gv4(1, 1), gv4(2, 1), gv4(3, 1), gv4(4, 1));
    for addr in [&a, &b, &c, &d] {
        add(&mut book, addr, Source::Own);
    }
    book.misbehaved(&b, PORT, 1_000);
    let connected = BTreeSet::from([group_of(&a)]);
    // Only c and d are candidates; whichever is drawn is then tried.
    let first = book.pick(&connected, false, 0, 1_000).unwrap();
    let second = book.pick(&connected, false, 0, 1_001).unwrap();
    assert_ne!(first.addr, second.addr);
    for addr in [&first.addr, &second.addr] {
        assert!(*addr == c || *addr == d);
    }
    // Both were tried less than 600 seconds ago.
    assert!(book
        .pick(&connected, false, 0, 1_000 + RETRY_SECS - 1)
        .is_none());
    assert!(book
        .pick(&connected, false, 0, 1_000 + RETRY_SECS)
        .is_some());
    // The connected group is free again when it is no longer connected.
    for random in 0..64 {
        let mut copy = book.clone();
        let p = copy.pick(&none(), false, random, 1_000 + RETRY_SECS + 5);
        assert_ne!(p.map(|p| p.addr), Some(b.clone()));
    }
}

#[test]
fn nr_23_is_a_pure_function_of_its_inputs() {
    let mut book = own_book(30);
    for host in 2..=6 {
        add(&mut book, &gv4(3, host), Source::Own);
    }
    let connected = BTreeSet::from([group_of(&gv4(5, 1))]);
    for random in [0, 1, 7, u64::MAX, 0xdead_beef_cafe_f00d] {
        let mut x = book.clone();
        let mut y = book.clone();
        assert_eq!(
            x.pick(&connected, true, random, 99),
            y.pick(&connected, true, random, 99)
        );
        assert_eq!(x, y);
    }
}

#[test]
fn nr_23_a_group_of_eight_is_no_more_likely_than_a_group_of_one() {
    let mut book = AddressBook::new();
    let many = group_of(&v4(30, 1, 0, 1));
    for host in 1..=8 {
        add(&mut book, &v4(30, 1, 0, host), Source::Own);
    }
    add(&mut book, &v4(40, 1, 0, 1), Source::Own);
    let n: u64 = 1_024;
    let mut in_many = 0;
    let mut per_entry = [0u32; 8];
    for i in 0..n {
        let mut copy = book.clone();
        let p = copy.pick(&none(), false, i | ((i / 2) << 32), 0).unwrap();
        if p.group == many {
            in_many += 1;
            if let AddrV2::Ipv4(a) = p.addr {
                per_entry[usize::from(a.octets()[3] - 1)] += 1;
            }
        }
    }
    assert_eq!(in_many, n / 2);
    assert_eq!(per_entry, [64; 8]);
}

#[test]
fn nr_21_want_filters_restricts_when_such_peers_exist_and_falls_back() {
    let mut book = AddressBook::new();
    let with = gv4(1, 1);
    for i in 2..10 {
        add(&mut book, &gv4(i, 1), Source::Own);
    }
    book.add(&with, PORT, filters(), Source::Own, 0);
    for random in 0..50 {
        let mut copy = book.clone();
        assert_eq!(copy.pick(&none(), true, random, 0).unwrap().addr, with);
    }
    let mut seen = BTreeSet::new();
    for random in 0..50 {
        let mut copy = book.clone();
        seen.insert(copy.pick(&none(), false, random, 0).unwrap().group);
    }
    assert!(seen.len() > 1);
    // When the filter peer is in a connected group, any other is drawn.
    let connected = BTreeSet::from([group_of(&with)]);
    let mut copy = book.clone();
    let p = copy.pick(&connected, true, 3, 0).unwrap();
    assert_ne!(p.addr, with);
}

#[test]
fn nr_21_a_peers_claim_that_another_serves_filters_earns_no_preference() {
    // Otherwise one peer could make up addresses that all claim the
    // service and be drawn first whenever filter peers are wanted.
    let mut book = AddressBook::new();
    let teller = Source::Peer(group_of(&gv4(200, 1)));
    for i in 1..9 {
        book.add(&gv4(i, 1), PORT, filters(), teller, 0);
    }
    let honest = gv4(50, 1);
    book.add(&honest, PORT, filters(), Source::Seed, 0);
    for random in 0..50 {
        let mut copy = book.clone();
        assert_eq!(copy.pick(&none(), true, random, 0).unwrap().addr, honest);
    }
    // Once a claimed one has shown the service itself, it counts.
    let proven = gv4(3, 1);
    book.connected_ok(&proven, PORT, filters(), 5);
    let mut seen = BTreeSet::new();
    for random in 0..50 {
        let mut copy = book.clone();
        seen.insert(copy.pick(&none(), true, random, 10_000).unwrap().group);
    }
    assert_eq!(seen, BTreeSet::from([group_of(&honest), group_of(&proven)]));
}

#[test]
fn nr_23_an_empty_book_gives_nothing() {
    let mut book = AddressBook::new();
    assert_eq!(book.pick(&none(), false, 0, 0), None);
    assert_eq!(book.pick(&none(), true, u64::MAX, u64::MAX), None);
    let mut book = own_book(1);
    let connected = BTreeSet::from([group_of(&gv4(0, 1))]);
    assert_eq!(book.pick(&connected, false, 0, 0), None);
}

#[test]
fn nr_23_marks_the_entry_as_tried() {
    let mut book = own_book(1);
    let p = book.pick(&none(), false, 0, 1_000).unwrap();
    assert_eq!(book.get(&p.addr, p.port).unwrap().last_try, Some(1_000));
    // A clock that went back does not make it a candidate.
    assert_eq!(book.pick(&none(), false, 0, 500), None);
}

#[test]
fn nr_24_an_anchor_is_returned_first() {
    let mut book = own_book(20);
    let (a, b) = (gv4(7, 1), gv4(9, 1));
    book.set_anchors(&[(a.clone(), PORT), (b.clone(), PORT)]);
    assert_eq!(book.anchors(), vec![(a.clone(), PORT), (b.clone(), PORT)]);
    for random in 0..10 {
        let mut copy = book.clone();
        assert_eq!(copy.pick(&none(), false, random, 5).unwrap().addr, a);
    }
    // After a is tried, b is next; then the draw takes over.
    assert_eq!(book.pick(&none(), true, 1, 5).unwrap().addr, a);
    assert_eq!(book.pick(&none(), true, 1, 6).unwrap().addr, b);
    let third = book.pick(&none(), true, 1, 7).unwrap();
    assert!(third.addr != a && third.addr != b);
}

#[test]
fn nr_24_an_anchor_in_a_connected_group_is_skipped() {
    let mut book = own_book(5);
    let (a, b) = (gv4(1, 1), gv4(2, 1));
    book.set_anchors(&[(a.clone(), PORT), (b.clone(), PORT)]);
    let connected = BTreeSet::from([group_of(&a)]);
    assert_eq!(book.pick(&connected, false, 0, 0).unwrap().addr, b);
}

#[test]
fn nr_24_keeps_two_anchors_and_ignores_addresses_not_in_the_book() {
    let mut book = own_book(5);
    let stranger = gv4(900, 1);
    book.set_anchors(&[
        (stranger.clone(), PORT),
        (gv4(1, 1), PORT),
        (gv4(1, 1), 9999),
        (AddrV2::I2p([1; 32]), PORT),
        (gv4(2, 1), PORT),
        (gv4(3, 1), PORT),
    ]);
    assert_eq!(book.anchors(), vec![(gv4(1, 1), PORT), (gv4(2, 1), PORT)]);
    book.set_anchors(&[(gv4(2, 1), PORT), (gv4(2, 1), PORT)]);
    assert_eq!(book.anchors(), vec![(gv4(2, 1), PORT)]);
    book.set_anchors(&[]);
    assert!(book.anchors().is_empty());
}

#[test]
fn nr_24_a_misbehaving_anchor_stops_being_one() {
    let mut book = own_book(5);
    let (a, b) = (gv4(1, 1), gv4(2, 1));
    book.set_anchors(&[(a.clone(), PORT), (b.clone(), PORT)]);
    book.misbehaved(&a, PORT, 100);
    assert_eq!(book.anchors(), vec![(b.clone(), PORT)]);
    assert_eq!(book.pick(&none(), false, 0, 101).unwrap().addr, b);
}

#[test]
fn nr_24_anchors_survive_the_stored_form() {
    let mut book = own_book(5);
    book.set_anchors(&[(gv4(3, 1), PORT), (gv4(1, 1), PORT)]);
    let back = AddressBook::from_bytes(&book.to_bytes(), 0).unwrap();
    assert_eq!(back, book);
    assert_eq!(back.anchors(), vec![(gv4(3, 1), PORT), (gv4(1, 1), PORT)]);
}

#[test]
fn nr_25_a_ban_lasts_exactly_24_hours() {
    let mut book = own_book(1);
    let addr = gv4(0, 1);
    book.misbehaved(&addr, PORT, 1_000);
    assert_eq!(BAN_SECS, 86_400);
    assert_eq!(book.pick(&none(), false, 0, 1_000 + 86_399), None);
    assert_eq!(book.pick(&none(), false, 0, 1_000), None);
    assert_eq!(
        book.pick(&none(), false, 0, 1_000 + 86_400).unwrap().addr,
        addr
    );
    // The ban is remembered in the book.
    assert_eq!(add(&mut book, &addr, Source::Own), Added::Known);
}

#[test]
fn nr_25_a_clock_that_goes_back_does_not_end_a_ban() {
    let mut book = own_book(1);
    book.misbehaved(&gv4(0, 1), PORT, 1_000_000);
    for now in [0, 1, 999_999, 1_000_000, 1_086_399] {
        assert_eq!(book.pick(&none(), false, 0, now), None, "{now}");
    }
    // The end of a ban at the top of the range does not overflow.
    let mut book = own_book(1);
    book.misbehaved(&gv4(0, 1), PORT, u64::MAX - 5);
    assert_eq!(book.pick(&none(), false, 0, u64::MAX - 1), None);
    assert!(book.pick(&none(), false, 0, u64::MAX).is_some());
}

#[test]
fn nr_25_a_banned_entry_is_not_dropped_for_failing_or_for_room() {
    let mut book = own_book(1);
    let addr = gv4(0, 1);
    book.misbehaved(&addr, PORT, 10);
    for t in 0..5 {
        book.failed(&addr, PORT, 20 + t);
    }
    assert_eq!(book.get(&addr, PORT).unwrap().failures, 5);
    assert_eq!(
        book.get(&addr, PORT).unwrap().banned_until,
        Some(10 + BAN_SECS)
    );
}

#[test]
fn nr_25_three_failures_in_a_row_drop_an_entry_and_a_success_resets_the_count() {
    let mut book = own_book(2);
    let (a, b) = (gv4(0, 1), gv4(1, 1));
    book.failed(&a, PORT, 1);
    book.failed(&a, PORT, 2);
    book.connected_ok(&a, PORT, filters(), 3);
    let e = book.get(&a, PORT).unwrap();
    assert_eq!(
        (e.failures, e.last_good, e.services),
        (0, Some(3), filters())
    );
    book.failed(&a, PORT, 4);
    book.failed(&a, PORT, 5);
    assert_eq!(book.get(&a, PORT).unwrap().failures, 2);
    book.failed(&a, PORT, 6);
    assert!(book.get(&a, PORT).is_none());
    assert!(book.get(&b, PORT).is_some());
    // Unknown addresses are ignored.
    book.failed(&a, PORT, 7);
    book.connected_ok(&a, PORT, svc(), 7);
    book.misbehaved(&a, PORT, 7);
    book.failed(&AddrV2::I2p([1; 32]), PORT, 7);
    assert_eq!(book.len(), 1);
}

#[test]
fn nr_24_an_anchor_is_not_dropped_for_failing() {
    let mut book = own_book(2);
    let a = gv4(0, 1);
    book.set_anchors(&[(a.clone(), PORT)]);
    for t in 0..5 {
        book.failed(&a, PORT, t);
    }
    assert_eq!(book.get(&a, PORT).unwrap().failures, 5);
    assert_eq!(book.anchors().len(), 1);
}

/// A busy book: every kind, every source, bans, failures and anchors.
fn busy_book() -> AddressBook {
    let mut book = AddressBook::new();
    let sources = [
        Source::Seed,
        Source::Own,
        Source::Peer(group_of(&v4(77, 7, 7, 7))),
        Source::Peer(group_of(&v6("2606:4700::1"))),
        Source::Peer(group_of(&AddrV2::TorV3([1; 32]))),
        Source::Peer(network_group(&PeerAddr::Ip(IpAddr::V4(
            Ipv4Addr::LOCALHOST,
        )))),
    ];
    for i in 0..60u16 {
        let source = sources[usize::from(i) % sources.len()];
        let flags = if i % 3 == 0 { filters() } else { svc() };
        let addr = match i % 3 {
            0 => gv4(i / 2, 1 + u8::try_from(i % 2).unwrap()),
            1 => v6(&format!("2a0{}:{:x}::{:x}", i % 7, 1 + i, i)),
            _ => AddrV2::TorV3([u8::try_from(i).unwrap(); 32]),
        };
        book.add(&addr, PORT + i, flags, source, 0);
    }
    let all: Vec<(AddrV2, u16)> = book.iter().map(|e| (e.addr.clone(), e.port)).collect();
    book.misbehaved(&all[3].0, all[3].1, 5_000);
    book.failed(&all[4].0, all[4].1, 5);
    book.failed(&all[4].0, all[4].1, 6);
    book.connected_ok(&all[5].0, all[5].1, filters(), 77);
    book.pick(&none(), false, 12_345, 4_000);
    book.set_anchors(&[all[8].clone(), all[2].clone()]);
    book
}

#[test]
fn nr_22_the_stored_form_round_trips_and_is_canonical() {
    let book = busy_book();
    assert!(book.len() >= 40, "{}", book.len());
    let bytes = book.to_bytes();
    assert_eq!(&bytes[..5], b"OBMA\x01");
    let back = AddressBook::from_bytes(&bytes, 10).unwrap();
    assert_eq!(back, book);
    assert_eq!(back.to_bytes(), bytes);
    let empty = AddressBook::new();
    assert_eq!(
        AddressBook::from_bytes(&empty.to_bytes(), 0).unwrap(),
        empty
    );
}

#[test]
fn nr_22_every_truncation_and_extension_of_the_stored_form_is_refused() {
    let bytes = busy_book().to_bytes();
    for len in 0..bytes.len() {
        assert!(
            AddressBook::from_bytes(&bytes[..len], 0).is_err(),
            "prefix of {len}"
        );
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        AddressBook::from_bytes(&longer, 0),
        Err(BookError::Malformed(DecodeError::TrailingBytes))
    );
}

#[test]
fn nr_22_a_wrong_magic_or_version_is_refused() {
    let mut bytes = busy_book().to_bytes();
    bytes[0] = b'X';
    assert_eq!(
        AddressBook::from_bytes(&bytes, 0),
        Err(BookError::Malformed(DecodeError::BadMagic))
    );
    let mut bytes = busy_book().to_bytes();
    bytes[4] = 2;
    assert_eq!(
        AddressBook::from_bytes(&bytes, 0),
        Err(BookError::Malformed(DecodeError::UnsupportedVersion(2)))
    );
}

/// One entry of the stored form, an IPv4 address from a seed.
fn raw(addr: [u8; 4], port: u16) -> Vec<u8> {
    let mut e = vec![1];
    e.extend_from_slice(&addr);
    e.extend_from_slice(&port.to_le_bytes());
    e.extend_from_slice(&1u64.to_le_bytes());
    e.push(0); // source: seed
    e.push(0); // never tried
    e.extend_from_slice(&0u32.to_le_bytes());
    e.push(0); // never good
    e.push(0); // never banned
    e
}

fn file(entries: &[Vec<u8>], anchors: &[u32]) -> Vec<u8> {
    let mut out = b"OBMA\x01".to_vec();
    out.extend_from_slice(&u32::try_from(entries.len()).unwrap().to_le_bytes());
    for e in entries {
        out.extend_from_slice(e);
    }
    out.extend_from_slice(&u32::try_from(anchors.len()).unwrap().to_le_bytes());
    for a in anchors {
        out.extend_from_slice(&a.to_le_bytes());
    }
    out
}

#[test]
fn nr_22_the_stored_form_is_read_under_the_same_rules() {
    let ok = file(&[raw([30, 1, 0, 1], PORT), raw([30, 1, 0, 2], PORT)], &[1]);
    let book = AddressBook::from_bytes(&ok, 0).unwrap();
    assert_eq!(book.len(), 2);
    assert_eq!(book.anchors(), vec![(v4(30, 1, 0, 2), PORT)]);
    assert_eq!(book.to_bytes(), ok);
}

#[test]
fn nr_22_a_stored_entry_that_is_not_routable_or_has_port_zero_refuses_the_file() {
    for addr in [
        [10, 0, 0, 1],
        [127, 0, 0, 1],
        [0, 0, 0, 0],
        [224, 1, 1, 1],
        [192, 0, 2, 5],
    ] {
        assert_eq!(
            AddressBook::from_bytes(&file(&[raw(addr, PORT)], &[]), 0),
            Err(BookError::Refused {
                index: 0,
                why: Refusal::NotRoutable
            }),
            "{addr:?}"
        );
    }
    assert_eq!(
        AddressBook::from_bytes(&file(&[raw([30, 1, 0, 1], 0)], &[]), 0),
        Err(BookError::Refused {
            index: 0,
            why: Refusal::PortZero
        })
    );
    // A kind the book does not keep.
    let mut e = raw([30, 1, 0, 1], PORT);
    e[0] = 4;
    assert_eq!(
        AddressBook::from_bytes(&file(&[e], &[]), 0),
        Err(BookError::Refused {
            index: 0,
            why: Refusal::KindNotKept
        })
    );
}

#[test]
fn nr_22_a_ninth_stored_entry_in_one_group_refuses_the_file() {
    let mut entries: Vec<Vec<u8>> = (1..=8).map(|h| raw([30, 1, 0, h], PORT)).collect();
    assert!(AddressBook::from_bytes(&file(&entries, &[]), 0).is_ok());
    entries.push(raw([30, 1, 0, 9], PORT));
    assert_eq!(
        AddressBook::from_bytes(&file(&entries, &[]), 0),
        Err(BookError::Refused {
            index: 8,
            why: Refusal::GroupFull
        })
    );
}

#[test]
fn nr_22_more_than_256_stored_seed_entries_refuse_the_file() {
    let entries: Vec<Vec<u8>> = (0..257u16)
        .map(|i| {
            raw(
                [
                    20 + u8::try_from(i / 8).unwrap(),
                    1,
                    0,
                    1 + u8::try_from(i % 8).unwrap(),
                ],
                PORT,
            )
        })
        .collect();
    // The entries must be in order: the first byte of the address rises
    // with the group, and the host within a group.
    let mut sorted = entries.clone();
    sorted.sort();
    assert_eq!(sorted, entries);
    assert_eq!(
        AddressBook::from_bytes(&file(&entries[..256], &[]), 0).map(|b| b.len()),
        Ok(256)
    );
    assert_eq!(
        AddressBook::from_bytes(&file(&entries, &[]), 0),
        Err(BookError::Refused {
            index: 256,
            why: Refusal::SourceFull
        })
    );
}

#[test]
fn nr_22_more_than_2000_stored_entries_refuse_the_file() {
    let mut bytes = b"OBMA\x01".to_vec();
    bytes.extend_from_slice(&2_001u32.to_le_bytes());
    bytes.resize(bytes.len() + 2_001 * 23, 0);
    assert_eq!(
        AddressBook::from_bytes(&bytes, 0),
        Err(BookError::Malformed(DecodeError::TooLarge))
    );
}

#[test]
fn nr_22_duplicate_or_unsorted_stored_entries_refuse_the_file() {
    let a = raw([30, 1, 0, 1], PORT);
    let b = raw([30, 1, 0, 2], PORT);
    assert_eq!(
        AddressBook::from_bytes(&file(&[a.clone(), a.clone()], &[]), 0),
        Err(BookError::OutOfOrder { index: 1 })
    );
    assert_eq!(
        AddressBook::from_bytes(&file(&[b.clone(), a.clone()], &[]), 0),
        Err(BookError::OutOfOrder { index: 1 })
    );
    // The same address with a lower port after a higher one.
    assert_eq!(
        AddressBook::from_bytes(
            &file(&[raw([30, 1, 0, 1], 9000), raw([30, 1, 0, 1], 8000)], &[]),
            0
        ),
        Err(BookError::OutOfOrder { index: 1 })
    );
}

#[test]
fn nr_24_a_stored_anchor_that_is_not_in_the_book_refuses_the_file() {
    let a = raw([30, 1, 0, 1], PORT);
    let b = raw([30, 1, 0, 2], PORT);
    let c = raw([30, 1, 0, 3], PORT);
    let entries = [a, b, c];
    assert!(AddressBook::from_bytes(&file(&entries, &[2, 0]), 0).is_ok());
    for anchors in [&[3u32][..], &[u32::MAX], &[1, 1]] {
        assert_eq!(
            AddressBook::from_bytes(&file(&entries, anchors), 0),
            Err(BookError::BadAnchor),
            "{anchors:?}"
        );
    }
    // Three anchors are more than the book keeps.
    assert_eq!(
        AddressBook::from_bytes(&file(&entries, &[0, 1, 2]), 0),
        Err(BookError::Malformed(DecodeError::TooLarge))
    );
}

#[test]
fn nr_22_a_stored_source_that_is_not_one_the_book_writes_refuses_the_file() {
    let mut e = raw([30, 1, 0, 1], PORT);
    let at = 1 + 4 + 2 + 8;
    e[at] = 3;
    assert_eq!(
        AddressBook::from_bytes(&file(&[e], &[]), 0),
        Err(BookError::BadSource { index: 0 })
    );
    // A peer group that the book cannot have written: a private /16.
    let mut e = raw([30, 1, 0, 1], PORT);
    e[at] = 2;
    e.splice(at + 1..at + 1, [2, 10, 0, 0, 0]);
    assert_eq!(
        AddressBook::from_bytes(&file(&[e], &[]), 0),
        Err(BookError::BadSource { index: 0 })
    );
    // A flag that is neither 0 nor 1.
    let mut e = raw([30, 1, 0, 1], PORT);
    e[at + 1] = 2;
    assert!(matches!(
        AddressBook::from_bytes(&file(&[e], &[]), 0),
        Err(BookError::Malformed(DecodeError::NonCanonical(_)))
    ));
}

#[test]
fn nr_22_every_kind_of_source_group_survives_the_stored_form() {
    let groups = [
        group_of(&v4(77, 7, 7, 7)),
        group_of(&v6("2606:4700::1")),
        group_of(&AddrV2::TorV3([1; 32])),
        network_group(&PeerAddr::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST))),
    ];
    let mut book = AddressBook::new();
    for (i, g) in groups.iter().enumerate() {
        let i = u16::try_from(i).unwrap();
        assert_eq!(add(&mut book, &gv4(i, 1), Source::Peer(*g)), Added::New);
    }
    let back = AddressBook::from_bytes(&book.to_bytes(), 0).unwrap();
    assert_eq!(back, book);
    for (e, g) in back.iter().zip(groups) {
        assert_eq!(e.source, Source::Peer(g));
    }
}

#[test]
fn nr_22_is_routable_agrees_with_what_the_book_keeps() {
    use crate::book::is_routable;
    let cases = [
        v4(8, 8, 8, 8),
        v4(10, 1, 2, 3),
        v4(127, 0, 0, 1),
        v4(0, 0, 0, 0),
        v4(192, 168, 1, 1),
        v4(203, 0, 113, 5),
        v6("::"),
        v6("::1"),
        v6("fe80::1"),
        v6("fc00::1"),
        v6("2001:db8::1"),
        v6("2606:4700::1"),
    ];
    for addr in cases {
        let mut book = AddressBook::new();
        let kept = add(&mut book, &addr, Source::Seed) == Added::New;
        assert_eq!(is_routable(&addr), kept, "{addr:?}");
    }
    assert!(!is_routable(&AddrV2::Cjdns(Ipv6Addr::from([0xfc; 16]))));
}

/// The next number of a small xorshift generator.
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn nr_21_has_filter_candidate_agrees_with_pick_on_random_books() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    for round in 0..300u16 {
        let mut book = AddressBook::new();
        let mut keys = Vec::new();
        for i in 0..(next(&mut state) % 12) as u16 {
            let addr = gv4(round % 50 * 3 + i % 6, 1 + (i % 200) as u8);
            let services = if next(&mut state).is_multiple_of(2) {
                filters()
            } else {
                svc()
            };
            let source = match next(&mut state) % 3 {
                0 => Source::Seed,
                1 => Source::Own,
                _ => Source::Peer(group_of(&v4(77, 7, 7, 7))),
            };
            if book.add(&addr, PORT, services, source, 0) == Added::New {
                keys.push(addr.clone());
            }
            match next(&mut state) % 5 {
                0 => book.connected_ok(&addr, PORT, services, 5),
                1 => book.misbehaved(&addr, PORT, 5),
                _ => {}
            }
        }
        let now = 10 + next(&mut state) % 1_000;
        let mut connected = BTreeSet::new();
        for addr in &keys {
            if next(&mut state).is_multiple_of(4) {
                connected.insert(group_of(addr));
            }
        }
        let expect = book.has_filter_candidate(&connected, now);
        let random = next(&mut state);
        let mut copy = book.clone();
        let picked = copy.pick(&connected, true, random, now);
        let trusted_filter = picked.is_some_and(|p| {
            let e = book.get(&p.addr, p.port).unwrap();
            e.services.has(ServiceFlags::COMPACT_FILTERS)
                && (e.last_good.is_some() || !matches!(e.source, Source::Peer(_)))
        });
        // With a candidate that offers filters, pick takes one; without,
        // it never takes one that offers filters.
        assert_eq!(expect, trusted_filter, "round {round}");
    }
}

#[test]
fn nr_21_has_filter_candidate_skips_connected_groups_and_recent_tries() {
    let mut book = AddressBook::new();
    let a = gv4(1, 1);
    book.add(&a, PORT, filters(), Source::Seed, 0);
    assert!(book.has_filter_candidate(&none(), 0));
    let mut connected = none();
    connected.insert(group_of(&a));
    assert!(!book.has_filter_candidate(&connected, 0));
    book.pick(&none(), true, 0, 100);
    assert!(!book.has_filter_candidate(&none(), 100));
    assert!(book.has_filter_candidate(&none(), 100 + RETRY_SECS));
    // A peer's word is not enough.
    let mut book = AddressBook::new();
    let told_by = Source::Peer(group_of(&v4(77, 7, 7, 7)));
    book.add(&gv4(2, 1), PORT, filters(), told_by, 0);
    assert!(!book.has_filter_candidate(&none(), 0));
}
