//! WR-41: network groups.

use std::collections::BTreeSet;
use std::net::IpAddr;

use crate::{network_group, NetGroup, PeerAddr};

fn ip(text: &str) -> PeerAddr {
    PeerAddr::Ip(text.parse::<IpAddr>().unwrap())
}

/// Every address with the label of the group it must be in. Addresses with
/// one label share a group; addresses with different labels do not.
const TABLE: &[(&str, &str)] = &[
    // IPv4 by the first 16 bits.
    ("8.8.8.8", "v4 8.8"),
    ("8.8.4.4", "v4 8.8"),
    ("8.8.255.255", "v4 8.8"),
    ("8.9.0.1", "v4 8.9"),
    ("1.2.3.4", "v4 1.2"),
    ("1.2.200.200", "v4 1.2"),
    ("203.0.113.5", "v4 203.0"),
    // Local, private, link-local, unspecified, CGNAT, never on the internet.
    ("127.0.0.1", "local"),
    ("127.9.9.9", "local"),
    ("10.1.2.3", "local"),
    ("172.16.0.1", "local"),
    ("172.31.255.255", "local"),
    ("192.168.1.1", "local"),
    ("169.254.1.1", "local"),
    ("100.64.0.1", "local"),
    ("100.127.255.255", "local"),
    ("0.0.0.0", "local"),
    ("224.0.0.1", "local"),
    ("255.255.255.255", "local"),
    // The edges of those ranges are public.
    ("172.15.255.255", "v4 172.15"),
    ("172.32.0.1", "v4 172.32"),
    ("192.169.0.1", "v4 192.169"),
    ("100.63.255.255", "v4 100.63"),
    ("100.128.0.1", "v4 100.128"),
    ("11.0.0.1", "v4 11.0"),
    // IPv6 by the first 32 bits.
    ("2001:db8::1", "v6 2001:0db8"),
    ("2001:db8:ffff::2", "v6 2001:0db8"),
    ("2001:db9::1", "v6 2001:0db9"),
    ("2606:4700::1", "v6 2606:4700"),
    ("2606:4700:ffff::1", "v6 2606:4700"),
    ("2001:1::1", "v6 2001:0001"),
    // IPv6 that is local.
    ("::1", "local"),
    ("::", "local"),
    ("fc00::1", "local"),
    ("fd12:3456:789a::1", "local"),
    ("fe80::1", "local"),
    ("ff02::1", "local"),
    // IPv4 inside IPv6: mapped.
    ("::ffff:8.8.8.8", "v4 8.8"),
    ("::ffff:8.8.4.4", "v4 8.8"),
    ("::ffff:8.9.0.1", "v4 8.9"),
    ("::ffff:10.0.0.1", "local"),
    ("::ffff:127.0.0.1", "local"),
    ("::ffff:100.64.0.1", "local"),
    // 6to4: the next 32 bits after 2002.
    ("2002:0808:0808::1", "v4 8.8"),
    ("2002:0808:0404:1::1", "v4 8.8"),
    ("2002:0a00:0001::1", "local"),
    ("2002:c0a8:0101::", "local"),
    // Teredo: the last 32 bits, inverted. The first is the example of
    // RFC 4380, whose client is 192.0.2.45.
    ("2001:0:4136:e378:8000:63bf:3fff:fdd2", "v4 192.0"),
    ("2001:0:4136:e378:8000:63bf:f7f7:f7f7", "v4 8.8"),
    ("2001:0:1:2:3:4:f7f7:fbfb", "v4 8.8"),
    ("2001:0:4136:e378:8000:63bf:f5ff:fffe", "local"),
];

#[test]
fn wr_41_addresses_fall_in_the_groups_the_rule_names() {
    assert!(TABLE.len() >= 20);
    for (a, label_a) in TABLE {
        for (b, label_b) in TABLE {
            let same = network_group(&ip(a)) == network_group(&ip(b));
            assert_eq!(
                same,
                label_a == label_b,
                "{a} ({label_a}) and {b} ({label_b})"
            );
        }
    }
    let labels: BTreeSet<_> = TABLE.iter().map(|(_, l)| l).collect();
    let groups: BTreeSet<_> = TABLE.iter().map(|(a, _)| network_group(&ip(a))).collect();
    assert_eq!(labels.len(), groups.len());
}

#[test]
fn wr_41_every_name_is_in_one_single_group() {
    let named = network_group(&PeerAddr::Named);
    assert_eq!(named, network_group(&PeerAddr::Named));
    for (a, _) in TABLE {
        assert_ne!(named, network_group(&ip(a)), "{a}");
    }
}

#[test]
fn wr_41_the_rfc_4380_teredo_example_is_the_client_address() {
    let teredo = network_group(&ip("2001:0:4136:e378:8000:63bf:3fff:fdd2"));
    assert_eq!(teredo, network_group(&ip("192.0.55.55")));
    // The server and flags in the address do not matter; the client does.
    assert_ne!(
        teredo,
        network_group(&ip("2001:0:4136:e378:8000:63bf:fcff:fdd2"))
    );
}

#[test]
fn wr_41_groups_can_be_counted_and_ordered() {
    let set: BTreeSet<_> = [
        ip("8.8.8.8"),
        ip("8.8.1.1"),
        ip("9.9.9.9"),
        PeerAddr::Named,
        PeerAddr::Named,
        ip("10.0.0.1"),
        ip("192.168.0.1"),
    ]
    .iter()
    .map(network_group)
    .collect();
    assert_eq!(set.len(), 4);
}

#[test]
fn wr_41_a_group_comes_back_from_its_key() {
    let mut groups: BTreeSet<_> = TABLE.iter().map(|(a, _)| network_group(&ip(a))).collect();
    groups.insert(network_group(&PeerAddr::Named));
    assert!(groups.len() > 10);
    let mut kinds = BTreeSet::new();
    for g in groups {
        assert_eq!(NetGroup::from_key(g.key()), Some(g));
        kinds.insert(g.key()[0]);
    }
    assert_eq!(kinds, BTreeSet::from([0, 1, 2, 3]));
}

#[test]
fn wr_41_a_key_that_is_not_one_groups_own_is_refused() {
    // an unknown kind
    for kind in [4u8, 9, 255] {
        assert_eq!(NetGroup::from_key([kind, 8, 8, 0, 0]), None);
    }
    // padding that is not zero
    assert_eq!(NetGroup::from_key([0, 1, 0, 0, 0]), None);
    assert_eq!(NetGroup::from_key([1, 0, 0, 0, 1]), None);
    assert_eq!(NetGroup::from_key([2, 8, 8, 0, 1]), None);
    assert_eq!(NetGroup::from_key([2, 8, 8, 1, 0]), None);
    // prefixes that no peer is grouped by: they belong to the local group,
    // or are read as the IPv4 address they carry
    assert_eq!(NetGroup::from_key([2, 10, 0, 0, 0]), None);
    assert_eq!(NetGroup::from_key([2, 192, 168, 0, 0]), None);
    assert_eq!(NetGroup::from_key([2, 224, 0, 0, 0]), None);
    assert_eq!(NetGroup::from_key([3, 0xfd, 0, 0, 0]), None);
    assert_eq!(NetGroup::from_key([3, 0xff, 0x02, 0, 0]), None);
    assert_eq!(NetGroup::from_key([3, 0x20, 0x02, 8, 8]), None);
    assert_eq!(NetGroup::from_key([3, 0x20, 0x01, 0, 0]), None);
    // ordinary ones are fine
    assert!(NetGroup::from_key([2, 8, 8, 0, 0]).is_some());
    assert!(NetGroup::from_key([3, 0x20, 0x01, 0x0d, 0xb8]).is_some());
}
