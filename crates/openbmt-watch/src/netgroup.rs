//! WR-41: peers are counted by network group, not by address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Where a peer was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerAddr {
    /// An IP address.
    Ip(IpAddr),
    /// A name that anyone can make without cost: I2P, CJDNS, or a host name.
    Named,
    /// A Tor v3 peer, by the public key of its address. Its group is the
    /// first 4 bits of the key (NR-93).
    Onion([u8; 32]),
}

/// A network group. Two peers in one group count once. The value can be
/// compared and ordered and says nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetGroup(Repr);

impl NetGroup {
    /// The group as five bytes, for storing: a kind byte (0 local, 1 named,
    /// 2 IPv4, 3 IPv6) and the prefix, padded with zeros. Two groups have
    /// the same bytes only if they are the same group.
    pub fn key(self) -> [u8; 5] {
        match self.0 {
            Repr::Local => [0, 0, 0, 0, 0],
            Repr::Named => [1, 0, 0, 0, 0],
            Repr::Host([a, b, c, d]) => [4, a, b, c, d],
            Repr::V4([a, b]) => [2, a, b, 0, 0],
            Repr::V6([a, b, c, d]) => [3, a, b, c, d],
            Repr::Onion(n) => [5, n, 0, 0, 0],
        }
    }

    /// The group stored by [`NetGroup::key`]. `None` for an unknown kind
    /// byte, for padding that is not zero, and for a prefix that no peer can
    /// have (for example a private IPv4 prefix, which is the local group
    /// instead), so that each group has exactly one key.
    pub fn from_key(key: [u8; 5]) -> Option<Self> {
        let [kind, a, b, c, d] = key;
        let group = match kind {
            0 if [a, b, c, d] == [0; 4] => Self(Repr::Local),
            1 if [a, b, c, d] == [0; 4] => Self(Repr::Named),
            2 if [c, d] == [0, 0] => Self(Repr::V4([a, b])),
            3 => Self(Repr::V6([a, b, c, d])),
            5 if a < 16 && [b, c, d] == [0; 3] => Self(Repr::Onion(a)),
            // A host group (kind 4) is for a check on one machine and is
            // never stored.
            _ => return None,
        };
        // A prefix is canonical if a peer with that prefix is put in the
        // group by `network_group`.
        let probe = match group.0 {
            Repr::V4([a, b]) => Some(PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(a, b, 0, 1)))),
            Repr::V6([a, b, c, d]) => {
                let mut octets = [0u8; 16];
                octets[..4].copy_from_slice(&[a, b, c, d]);
                octets[15] = 2;
                Some(PeerAddr::Ip(IpAddr::V6(Ipv6Addr::from(octets))))
            }
            Repr::Local | Repr::Named | Repr::Onion(_) | Repr::Host(_) => None,
        };
        match probe {
            Some(addr) if network_group(&addr) != group => None,
            _ => Some(group),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Repr {
    /// Local, private, link-local, unspecified, CGNAT and addresses that no
    /// peer on the internet can have.
    Local,
    /// Every peer reached by a name.
    Named,
    /// The first 16 bits of an IPv4 address.
    V4([u8; 2]),
    /// The first 32 bits of an IPv6 address.
    V6([u8; 4]),
    /// The first 4 bits of the public key of an onion address, as a number
    /// from 0 to 15.
    Onion(u8),
    /// One address on this machine, for a check against several nodes that
    /// all run here. The four bytes are the last four of the address.
    Host([u8; 4]),
}

/// The network group of a peer (WR-41).
///
/// - An IPv4 address is grouped by its first 16 bits.
/// - An IPv6 address is grouped by its first 32 bits. An IPv4 address carried
///   in IPv6 (mapped `::ffff:a.b.c.d`, 6to4 `2002::/16`, Teredo `2001:0::/32`)
///   is grouped by the IPv4 rule on the address it carries.
/// - Loopback, private (RFC 1918, RFC 4193), link-local, unspecified and
///   CGNAT (`100.64.0.0/10`) addresses are one single group. So are
///   multicast and reserved addresses, which no peer can have.
/// - Every peer reached by a name is in one single group.
/// - An onion peer is grouped by the first 4 bits of its public key: 16
///   groups, as Bitcoin Core groups Tor addresses (NR-93).
pub fn network_group(addr: &PeerAddr) -> NetGroup {
    NetGroup(match addr {
        PeerAddr::Named => Repr::Named,
        PeerAddr::Onion(key) => Repr::Onion(key[0] >> 4),
        PeerAddr::Ip(IpAddr::V4(v4)) => v4_group(*v4),
        PeerAddr::Ip(IpAddr::V6(v6)) => v6_group(*v6),
    })
}

/// The group of a peer when the caller allows peers on this machine: an
/// address that [`network_group`] puts in the local group is a group of its
/// own, by its whole address, so that several nodes on one machine count as
/// several groups. Every other address is grouped as [`network_group`]
/// does. Only a check against nodes on one machine has a use for it; the
/// health rules and the address book never see these groups.
pub fn host_group(addr: &PeerAddr) -> NetGroup {
    let group = network_group(addr);
    match (group, addr) {
        (NetGroup(Repr::Local), PeerAddr::Ip(ip)) => {
            let tail = match ip {
                IpAddr::V4(v4) => v4.octets(),
                IpAddr::V6(v6) => {
                    let o = v6.octets();
                    [o[12], o[13], o[14], o[15]]
                }
            };
            NetGroup(Repr::Host(tail))
        }
        _ => group,
    }
}

fn v4_group(addr: Ipv4Addr) -> Repr {
    let [a, b, _, _] = addr.octets();
    let local = a == 0 // this network, including unspecified
        || a == 10
        || a == 127
        || (a == 100 && (64..128).contains(&b)) // CGNAT
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || a >= 224; // multicast, reserved, broadcast
    if local {
        Repr::Local
    } else {
        Repr::V4([a, b])
    }
}

fn v6_group(addr: Ipv6Addr) -> Repr {
    if let Some(v4) = addr.to_ipv4_mapped() {
        return v4_group(v4);
    }
    let octets = addr.octets();
    // 6to4: the IPv4 address follows the 16-bit prefix.
    if octets[0] == 0x20 && octets[1] == 0x02 {
        return v4_group(Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]));
    }
    // Teredo: the client's IPv4 address is the last 32 bits, inverted.
    if octets[..4] == [0x20, 0x01, 0x00, 0x00] {
        return v4_group(Ipv4Addr::new(
            !octets[12],
            !octets[13],
            !octets[14],
            !octets[15],
        ));
    }
    let local = addr.is_unspecified()
        || addr.is_loopback()
        || (octets[0] & 0xfe) == 0xfc // fc00::/7, RFC 4193
        || (octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80) // fe80::/10
        || (octets[0] == 0xfe && (octets[1] & 0xc0) == 0xc0) // fec0::/10
        || octets[0] == 0xff; // multicast
    if local {
        Repr::Local
    } else {
        Repr::V6([octets[0], octets[1], octets[2], octets[3]])
    }
}
