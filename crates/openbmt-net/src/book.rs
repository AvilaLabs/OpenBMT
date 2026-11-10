//! The counter's address book of Bitcoin peers (NR-22 to NR-24, and the
//! 24 hours of NR-25).
//!
//! The book holds peers the counter may dial, where it heard of each, how
//! often dialing failed and whether the peer is banned. It has no sockets,
//! no clock and no randomness of its own: the caller passes in the time in
//! seconds and a random `u64`. The same inputs always give the same result.
//!
//! The book keeps only publicly routable IPv4, IPv6 and Tor v3 addresses.
//! It holds at most [`MAX_ENTRIES`] addresses, at most
//! [`MAX_PER_GROUP`] in one network group (WR-41), and caps how many it
//! learned from one source, so that one peer cannot fill it with addresses
//! it made up. The stored form is read back under the same rules.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bitcoin::p2p::address::AddrV2;
use bitcoin::p2p::ServiceFlags;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_watch::{network_group, NetGroup, PeerAddr};

/// NR-22: magic of the stored form.
pub const MAGIC_ADDRESS_BOOK: [u8; 4] = *b"OBMA";
/// NR-22: most addresses in the book.
pub const MAX_ENTRIES: usize = 2_000;
/// NR-22: most addresses in the book from one network group.
pub const MAX_PER_GROUP: usize = 8;
/// NR-22: most addresses in the book learned from the peers of any one
/// network group.
pub const MAX_PER_PEER_GROUP: usize = 64;
/// NR-22: most addresses in the book learned from DNS seeds and the
/// compiled-in list.
pub const MAX_FROM_SEEDS: usize = 256;
/// NR-23: seconds before an address that was tried may be drawn again.
pub const RETRY_SECS: u64 = 600;
/// NR-24: most anchors kept.
pub const MAX_ANCHORS: usize = 2;
/// NR-22: consecutive failures after which an address is dropped.
pub const MAX_FAILURES: u32 = 3;
/// NR-25: seconds a peer that broke a rule is not dialed.
pub const BAN_SECS: u64 = 86_400;
/// NR-81: seconds after which a peer that did not speak the encrypted
/// transport is tried with it again **[to tune]**.
pub const V2_RETRY_SECS: u64 = 86_400;

/// Where the counter heard of an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A DNS seed or the compiled-in list.
    Seed,
    /// A connected peer in this network group.
    Peer(NetGroup),
    /// The counter's own stored anchors, or an address the owner entered.
    Own,
}

/// Why `add` did not keep an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// NR-22: the address is not publicly routable.
    NotRoutable,
    /// NR-22: the address is of a kind the book does not keep (not IPv4,
    /// IPv6 or Tor v3).
    KindNotKept,
    /// NR-22: the port is 0.
    PortZero,
    /// NR-22: the book holds 8 addresses of this network group.
    GroupFull,
    /// NR-22: the book holds the most it takes from this source.
    SourceFull,
    /// NR-22: the book holds 2,000 addresses and none may be dropped.
    BookFull,
}

/// What `add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// The address is new and is kept.
    New,
    /// The address was in the book already.
    Known,
    /// The address is not kept.
    Refused(Refusal),
}

/// The address chosen by `pick`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    /// The address.
    pub addr: AddrV2,
    /// The port.
    pub port: u16,
    /// The address's network group.
    pub group: NetGroup,
}

/// One address of the book and what the counter knows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The address.
    pub addr: AddrV2,
    /// The port, not 0.
    pub port: u16,
    /// The services claimed for it. A claim.
    pub services: ServiceFlags,
    /// Where the counter heard of it.
    pub source: Source,
    /// The network group of the address itself.
    pub group: NetGroup,
    /// When it was last drawn or dialed.
    pub last_try: Option<u64>,
    /// Failed tries in a row.
    pub failures: u32,
    /// When a handshake with it last completed.
    pub last_good: Option<u64>,
    /// The time its ban ends (NR-25).
    pub banned_until: Option<u64>,
    /// NR-81: the time until which the peer is dialed with the old
    /// transport at once, because it did not speak the encrypted one. Kept
    /// in memory: the stored form does not hold it, so a start forgets it.
    pub v2_failed_until: Option<u64>,
}

/// Why a stored form was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    /// The bytes are cut short, too long, or not in the framed form.
    Malformed(DecodeError),
    /// An entry would be refused by `add` today.
    Refused {
        /// The entry's place in the file.
        index: u32,
        /// Why.
        why: Refusal,
    },
    /// An entry is not after the one before it, or repeats it.
    OutOfOrder {
        /// The entry's place in the file.
        index: u32,
    },
    /// The source of an entry is not one the counter writes.
    BadSource {
        /// The entry's place in the file.
        index: u32,
    },
    /// An anchor is not an entry of the book, or is named twice.
    BadAnchor,
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "NR-22: address book file: {e}"),
            Self::Refused { index, why } => {
                write!(f, "NR-22: address book entry {index} is refused: {why:?}")
            }
            Self::OutOfOrder { index } => {
                write!(f, "NR-22: address book entry {index} is out of order")
            }
            Self::BadSource { index } => {
                write!(f, "NR-22: address book entry {index} has a bad source")
            }
            Self::BadAnchor => write!(f, "NR-24: an anchor is not an entry of the book"),
        }
    }
}

impl std::error::Error for BookError {}

/// The kept kinds of address, in the order of the stored form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Addr {
    V4([u8; 4]),
    V6([u8; 16]),
    Tor([u8; 32]),
}

impl Addr {
    fn from_v2(addr: &AddrV2) -> Option<Self> {
        match addr {
            AddrV2::Ipv4(a) => Some(Self::V4(a.octets())),
            AddrV2::Ipv6(a) => Some(Self::V6(a.octets())),
            AddrV2::TorV3(b) => Some(Self::Tor(*b)),
            _ => None,
        }
    }

    fn to_v2(self) -> AddrV2 {
        match self {
            Self::V4(b) => AddrV2::Ipv4(Ipv4Addr::from(b)),
            Self::V6(b) => AddrV2::Ipv6(Ipv6Addr::from(b)),
            Self::Tor(b) => AddrV2::TorV3(b),
        }
    }

    /// The kind byte of the stored form, then the address bytes.
    fn write(self, out: &mut Vec<u8>) {
        match self {
            Self::V4(b) => {
                out.push(1);
                out.extend_from_slice(&b);
            }
            Self::V6(b) => {
                out.push(2);
                out.extend_from_slice(&b);
            }
            Self::Tor(b) => {
                out.push(3);
                out.extend_from_slice(&b);
            }
        }
    }

    /// Read a kind byte and the address; `None` for a kind not kept.
    fn read(r: &mut Reader<'_>) -> Result<Option<Self>, DecodeError> {
        Ok(match r.u8()? {
            1 => Some(Self::V4(r.array()?)),
            2 => Some(Self::V6(r.array()?)),
            3 => Some(Self::Tor(r.array()?)),
            _ => None,
        })
    }

    /// NR-22: the address's network group, or why it is not kept.
    fn classify(self) -> Result<NetGroup, Refusal> {
        let group = match self {
            Self::V4(b) => {
                if v4_documentation(b) {
                    return Err(Refusal::NotRoutable);
                }
                network_group(&PeerAddr::Ip(IpAddr::V4(Ipv4Addr::from(b))))
            }
            Self::V6(b) => {
                let a = Ipv6Addr::from(b);
                // BIP155 does not allow an IPv4 address in IPv6 form, and
                // 2001:db8::/32 is for documentation.
                if a.to_ipv4_mapped().is_some() || b[..4] == [0x20, 0x01, 0x0d, 0xb8] {
                    return Err(Refusal::NotRoutable);
                }
                network_group(&PeerAddr::Ip(IpAddr::V6(a)))
            }
            Self::Tor(key) => network_group(&PeerAddr::Onion(key)),
        };
        if group == local_group() {
            Err(Refusal::NotRoutable)
        } else {
            Ok(group)
        }
    }
}

/// NR-22: whether the book would keep this address as publicly routable.
/// False for a kind the book does not keep.
pub fn is_routable(addr: &AddrV2) -> bool {
    Addr::from_v2(addr).is_some_and(|a| a.classify().is_ok())
}

/// NR-92, NR-94: how the draw treats the Tor v3 addresses of the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnionDraw {
    /// Every kind of address is drawn, with no preference.
    Any,
    /// A Tor v3 address is never drawn: there is no proxy to dial it
    /// through, or the onion peers are as many as NR-94 allows.
    Never,
    /// With a proxy and room for another onion peer: when the book has
    /// candidates of both kinds, the kind is chosen with equal chance.
    Mixed,
}

/// Whether the entry is a Tor v3 address.
fn is_onion(e: &Entry) -> bool {
    matches!(e.addr, AddrV2::TorV3(_))
}

/// NR-23: an entry that may be dialed now: not banned, not in a connected
/// group and not tried in the last [`RETRY_SECS`].
fn is_candidate(e: &Entry, connected: &BTreeSet<NetGroup>, now: u64) -> bool {
    !banned(e, now)
        && !connected.contains(&e.group)
        && e.last_try.is_none_or(|t| now >= t && now - t >= RETRY_SECS)
}

/// NR-21: an entry that offers the compact-filter service. A peer's word
/// about another peer's services is not used to prefer it: only what a
/// seed, the owner or the peer itself said.
fn offers_filters(e: &Entry) -> bool {
    e.services.has(ServiceFlags::COMPACT_FILTERS)
        && (e.last_good.is_some() || !matches!(e.source, Source::Peer(_)))
}

/// The group of local and private addresses (WR-41).
fn local_group() -> NetGroup {
    network_group(&PeerAddr::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)))
}

/// Documentation ranges of RFC 5737, which the watcher's groups do not
/// class as local.
fn v4_documentation(b: [u8; 4]) -> bool {
    matches!(b, [192, 0, 2, _] | [198, 51, 100, _] | [203, 0, 113, _])
}

/// The network group as five bytes for the stored form.
fn group_key(group: NetGroup) -> [u8; 5] {
    group.key()
}

/// The group a five-byte key names, if the key is exactly what `group_key`
/// writes for it.
fn group_from_key(k: [u8; 5]) -> Option<NetGroup> {
    let group = match k {
        [0, ..] => local_group(),
        [1, ..] => network_group(&PeerAddr::Named),
        [5, n, ..] if n < 16 => network_group(&PeerAddr::Onion([n << 4; 32])),
        [2, a, b, ..] => network_group(&PeerAddr::Ip(IpAddr::V4(Ipv4Addr::new(a, b, 0, 0)))),
        [3, a, b, c, d] => {
            let o = [a, b, c, d, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            network_group(&PeerAddr::Ip(IpAddr::V6(Ipv6Addr::from(o))))
        }
        _ => return None,
    };
    (group_key(group) == k).then_some(group)
}

/// An address and port: the identity of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    addr: Addr,
    port: u16,
}

impl Key {
    fn of(addr: &AddrV2, port: u16) -> Option<Self> {
        Some(Self {
            addr: Addr::from_v2(addr)?,
            port,
        })
    }
}

/// The counter's address book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddressBook {
    entries: BTreeMap<Key, Entry>,
    anchors: Vec<Key>,
}

/// NR-25: whether the ban is in force. It ends only when `now` reaches the
/// end time, so a clock that goes back does not end it.
fn banned(entry: &Entry, now: u64) -> bool {
    matches!(entry.banned_until, Some(until) if now < until)
}

/// An index from `random` into `len` items.
fn index(random: u64, len: usize) -> Option<usize> {
    let len = u64::try_from(len).ok().filter(|n| *n > 0)?;
    usize::try_from(random % len).ok()
}

impl AddressBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many addresses the book holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry for an address and port, if the book has it.
    pub fn get(&self, addr: &AddrV2, port: u16) -> Option<&Entry> {
        self.entries.get(&Key::of(addr, port)?)
    }

    /// Every entry, in the order of the stored form.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// NR-24: the anchors, the one to dial first first.
    pub fn anchors(&self) -> Vec<(AddrV2, u16)> {
        self.anchors
            .iter()
            .map(|k| (k.addr.to_v2(), k.port))
            .collect()
    }

    fn is_anchor(&self, key: &Key) -> bool {
        self.anchors.contains(key)
    }

    /// NR-22: the limit that stops a new address of this group and source,
    /// if there is one. The total is checked by the caller.
    fn limit_hit(&self, group: NetGroup, source: Source) -> Option<Refusal> {
        let in_group = self.entries.values().filter(|e| e.group == group).count();
        if in_group >= MAX_PER_GROUP {
            return Some(Refusal::GroupFull);
        }
        let cap = match source {
            Source::Seed => MAX_FROM_SEEDS,
            Source::Peer(_) => MAX_PER_PEER_GROUP,
            Source::Own => return None,
        };
        let from_source = self.entries.values().filter(|e| e.source == source).count();
        (from_source >= cap).then_some(Refusal::SourceFull)
    }

    /// NR-22: drop the entry with 3 or more failures that was tried longest
    /// ago, other than an anchor or a banned one. Whether one was dropped.
    fn make_room(&mut self, now: u64) -> bool {
        let victim = self
            .entries
            .iter()
            .filter(|(k, e)| e.failures >= MAX_FAILURES && !self.is_anchor(k) && !banned(e, now))
            .min_by_key(|(k, e)| (e.last_try.unwrap_or(0), **k))
            .map(|(k, _)| *k);
        match victim {
            Some(k) => self.entries.remove(&k).is_some(),
            None => false,
        }
    }

    /// NR-22: add an address that was heard of from `source`.
    ///
    /// An address already in the book is `Known`: its claimed services are
    /// replaced only when the source is `Own`, and its ban and failure count
    /// are never touched. A full book never drops a good address for a new
    /// one; it drops addresses that failed 3 times, oldest tried first, and
    /// then refuses.
    pub fn add(
        &mut self,
        addr: &AddrV2,
        port: u16,
        services: ServiceFlags,
        source: Source,
        now: u64,
    ) -> Added {
        let Some(kept) = Addr::from_v2(addr) else {
            return Added::Refused(Refusal::KindNotKept);
        };
        if port == 0 {
            return Added::Refused(Refusal::PortZero);
        }
        let group = match kept.classify() {
            Ok(g) => g,
            Err(why) => return Added::Refused(why),
        };
        let key = Key { addr: kept, port };
        if let Some(entry) = self.entries.get_mut(&key) {
            if source == Source::Own {
                entry.services = services;
            }
            return Added::Known;
        }
        if let Some(why) = self.limit_hit(group, source) {
            return Added::Refused(why);
        }
        if self.entries.len() >= MAX_ENTRIES && !self.make_room(now) {
            return Added::Refused(Refusal::BookFull);
        }
        self.entries.insert(
            key,
            Entry {
                addr: kept.to_v2(),
                port,
                services,
                source,
                group,
                last_try: None,
                failures: 0,
                last_good: None,
                banned_until: None,
                v2_failed_until: None,
            },
        );
        Added::New
    }

    /// NR-23, NR-24, NR-21: choose the next address to dial.
    ///
    /// Candidates are not banned, not in a connected group and not tried in
    /// the last 600 seconds. The first candidate anchor wins. Otherwise,
    /// when `want_filters` is set and some candidates claim the
    /// compact-filter service, only those are left. A network group is then
    /// chosen with equal chance among the groups with candidates, and an
    /// address within it with equal chance. The chosen address is marked as
    /// tried at `now`.
    pub fn pick(
        &mut self,
        connected: &BTreeSet<NetGroup>,
        want_filters: bool,
        random: u64,
        now: u64,
    ) -> Option<Picked> {
        self.pick_with(connected, want_filters, random, now, OnionDraw::Any)
    }

    /// [`pick`](Self::pick) with the rule for Tor v3 addresses of NR-92 and
    /// NR-94. With `Never` they are not candidates. With `Mixed`, when the
    /// candidates are of both kinds (after the filter preference), the top
    /// bit of `random` chooses the kind, and the draw goes on within it.
    pub fn pick_with(
        &mut self,
        connected: &BTreeSet<NetGroup>,
        want_filters: bool,
        random: u64,
        now: u64,
        onion: OnionDraw,
    ) -> Option<Picked> {
        let mut candidates: Vec<Key> = self
            .entries
            .iter()
            .filter(|(_, e)| is_candidate(e, connected, now))
            .filter(|(_, e)| onion != OnionDraw::Never || !is_onion(e))
            .map(|(k, _)| *k)
            .collect();
        let anchor = self.anchors.iter().find(|a| candidates.contains(a));
        let chosen = if let Some(anchor) = anchor {
            *anchor
        } else {
            if want_filters {
                let filtered: Vec<Key> = candidates
                    .iter()
                    .filter(|k| self.entries.get(k).is_some_and(offers_filters))
                    .copied()
                    .collect();
                if !filtered.is_empty() {
                    candidates = filtered;
                }
            }
            if onion == OnionDraw::Mixed {
                let (tor, clear): (Vec<Key>, Vec<Key>) = candidates
                    .iter()
                    .partition(|k| matches!(k.addr, Addr::Tor(_)));
                if !tor.is_empty() && !clear.is_empty() {
                    candidates = if random >> 63 == 1 { tor } else { clear };
                }
            }
            let mut groups: BTreeMap<NetGroup, Vec<Key>> = BTreeMap::new();
            for k in &candidates {
                if let Some(e) = self.entries.get(k) {
                    groups.entry(e.group).or_default().push(*k);
                }
            }
            let list = groups.values().nth(index(random, groups.len())?)?;
            *list.get(index(random >> 32, list.len())?)?
        };
        let entry = self.entries.get_mut(&chosen)?;
        entry.last_try = Some(now);
        Some(Picked {
            addr: chosen.addr.to_v2(),
            port: chosen.port,
            group: entry.group,
        })
    }

    /// Whether [`pick`](Self::pick) with `want_filters` would choose among
    /// addresses that offer filters: some entry that is a candidate now
    /// (see `pick`) offers the compact-filter service as a seed, the owner
    /// or the peer itself said. It shares both tests with `pick`.
    pub fn has_filter_candidate(&self, connected: &BTreeSet<NetGroup>, now: u64) -> bool {
        self.has_filter_candidate_with(connected, now, OnionDraw::Any)
    }

    /// [`has_filter_candidate`](Self::has_filter_candidate) for a draw by
    /// [`pick_with`](Self::pick_with): with `Never`, a Tor v3 address is
    /// not a candidate.
    pub fn has_filter_candidate_with(
        &self,
        connected: &BTreeSet<NetGroup>,
        now: u64,
        onion: OnionDraw,
    ) -> bool {
        self.entries.values().any(|e| {
            is_candidate(e, connected, now)
                && offers_filters(e)
                && (onion != OnionDraw::Never || !is_onion(e))
        })
    }

    /// A handshake with the peer completed: failures go to 0, the time is
    /// kept, and the claimed services become what the peer itself stated.
    pub fn connected_ok(
        &mut self,
        addr: &AddrV2,
        port: u16,
        services_seen: ServiceFlags,
        now: u64,
    ) {
        if let Some(entry) = Key::of(addr, port).and_then(|k| self.entries.get_mut(&k)) {
            entry.failures = 0;
            entry.last_good = Some(now);
            entry.services = services_seen;
        }
    }

    /// A dial or handshake did not complete. At 3 failures in a row the
    /// address is dropped, unless it is an anchor or is banned (the ban is
    /// kept in the book).
    pub fn failed(&mut self, addr: &AddrV2, port: u16, now: u64) {
        let Some(key) = Key::of(addr, port) else {
            return;
        };
        let anchor = self.is_anchor(&key);
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= MAX_FAILURES && !anchor && !banned(entry, now) {
            self.entries.remove(&key);
        }
    }

    /// NR-25: the peer broke a rule. It is not dialed for 24 hours, and it
    /// stops being an anchor. The entry stays so that the ban is
    /// remembered.
    pub fn misbehaved(&mut self, addr: &AddrV2, port: u16, now: u64) {
        let Some(key) = Key::of(addr, port) else {
            return;
        };
        if let Some(entry) = self.entries.get_mut(&key) {
            let until = now.saturating_add(BAN_SECS);
            entry.banned_until = Some(entry.banned_until.map_or(until, |old| old.max(until)));
            self.anchors.retain(|a| *a != key);
        }
    }

    /// NR-81: the peer did not speak the encrypted transport. It is dialed
    /// with the old one for [`V2_RETRY_SECS`].
    pub fn v2_failed(&mut self, addr: &AddrV2, port: u16, now: u64) {
        if let Some(entry) = Key::of(addr, port).and_then(|k| self.entries.get_mut(&k)) {
            entry.v2_failed_until = Some(now.saturating_add(V2_RETRY_SECS));
        }
    }

    /// NR-80: whether to try the encrypted transport first with this peer.
    /// Yes, unless NR-81 says it did not speak it lately, or the peer itself
    /// (not a seed, not another peer) stated services without
    /// `NODE_P2P_V2`. An address that is not in the book is tried.
    pub fn v2_first(&self, addr: &AddrV2, port: u16, now: u64) -> bool {
        let Some(entry) = self.get(addr, port) else {
            return true;
        };
        if matches!(entry.v2_failed_until, Some(until) if now < until) {
            return false;
        }
        entry.last_good.is_none() || entry.services.has(ServiceFlags::P2P_V2)
    }

    /// NR-24: set the anchors: the first two of these that are entries of
    /// the book. Others are ignored.
    pub fn set_anchors(&mut self, anchors: &[(AddrV2, u16)]) {
        let mut kept: Vec<Key> = Vec::new();
        for (addr, port) in anchors {
            if kept.len() >= MAX_ANCHORS {
                break;
            }
            if let Some(key) = Key::of(addr, *port) {
                if self.entries.contains_key(&key) && !kept.contains(&key) {
                    kept.push(key);
                }
            }
        }
        self.anchors = kept;
    }

    /// The stored form: every entry in order, then the anchors as indexes
    /// into the entries.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_ADDRESS_BOOK, 8 + self.entries.len() * 64);
        out.extend_from_slice(&wire::len_u32(self.entries.len()).to_le_bytes());
        for (key, e) in &self.entries {
            key.addr.write(&mut out);
            out.extend_from_slice(&key.port.to_le_bytes());
            out.extend_from_slice(&u64::from(e.services).to_le_bytes());
            match e.source {
                Source::Seed => out.push(0),
                Source::Own => out.push(1),
                Source::Peer(g) => {
                    out.push(2);
                    out.extend_from_slice(&group_key(g));
                }
            }
            wire::put_opt_u64(&mut out, e.last_try);
            out.extend_from_slice(&e.failures.to_le_bytes());
            wire::put_opt_u64(&mut out, e.last_good);
            wire::put_opt_u64(&mut out, e.banned_until);
        }
        out.extend_from_slice(&wire::len_u32(self.anchors.len()).to_le_bytes());
        for anchor in &self.anchors {
            let at = self.entries.keys().position(|k| k == anchor).unwrap_or(0);
            out.extend_from_slice(&wire::len_u32(at).to_le_bytes());
        }
        out
    }

    /// Read a stored form. Every rule of `add` is applied again to every
    /// entry, and entries must be in strictly ascending order. Any failure
    /// refuses the whole file. The time is not used: a ban or a try in the
    /// future is kept as written, so that a clock that went back does not
    /// lose them.
    pub fn from_bytes(bytes: &[u8], _now: u64) -> Result<Self, BookError> {
        let bad = BookError::Malformed;
        let mut r = Reader::framed(bytes, &MAGIC_ADDRESS_BOOK).map_err(bad)?;
        let count = r.count(MAX_ENTRIES, 23).map_err(bad)?;
        let mut book = Self::new();
        let mut previous: Option<Key> = None;
        for i in 0..count {
            let index = u32::try_from(i).unwrap_or(u32::MAX);
            let refused = |why| BookError::Refused { index, why };
            // The length of an address of an unknown kind is not known, so
            // the file ends here.
            let Some(kept) = Addr::read(&mut r).map_err(bad)? else {
                return Err(refused(Refusal::KindNotKept));
            };
            let port = r.u16_le().map_err(bad)?;
            let services = ServiceFlags::from(r.u64_le().map_err(bad)?);
            let source = match r.u8().map_err(bad)? {
                0 => Source::Seed,
                1 => Source::Own,
                2 => {
                    let key: [u8; 5] = r.array().map_err(bad)?;
                    match group_from_key(key) {
                        Some(g) => Source::Peer(g),
                        None => return Err(BookError::BadSource { index }),
                    }
                }
                _ => return Err(BookError::BadSource { index }),
            };
            let last_try = r.opt_u64().map_err(bad)?;
            let failures = r.u32_le().map_err(bad)?;
            let last_good = r.opt_u64().map_err(bad)?;
            let banned_until = r.opt_u64().map_err(bad)?;
            if port == 0 {
                return Err(refused(Refusal::PortZero));
            }
            let group = kept.classify().map_err(refused)?;
            let key = Key { addr: kept, port };
            if previous.is_some_and(|p| p >= key) {
                return Err(BookError::OutOfOrder { index });
            }
            previous = Some(key);
            if let Some(why) = book.limit_hit(group, source) {
                return Err(refused(why));
            }
            book.entries.insert(
                key,
                Entry {
                    addr: kept.to_v2(),
                    port,
                    services,
                    source,
                    group,
                    last_try,
                    failures,
                    last_good,
                    banned_until,
                    v2_failed_until: None,
                },
            );
        }
        let anchors = r.count(MAX_ANCHORS, 4).map_err(bad)?;
        for _ in 0..anchors {
            let at = usize::try_from(r.u32_le().map_err(bad)?).map_err(|_| BookError::BadAnchor)?;
            let key = book
                .entries
                .keys()
                .nth(at)
                .copied()
                .ok_or(BookError::BadAnchor)?;
            if book.anchors.contains(&key) {
                return Err(BookError::BadAnchor);
            }
            book.anchors.push(key);
        }
        r.finish().map_err(bad)?;
        Ok(book)
    }
}
