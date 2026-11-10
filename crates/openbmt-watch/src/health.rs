//! Section 6: whether the check is working.
//!
//! Instant acceptance needs a check that is working. [`check_health`] says
//! "healthy" only when every rule holds, and names the first that does not.
//! The result is the `check_healthy` fact of
//! `openbmt_core::policy::Inputs`.
//!
//! The library keeps no memory, so WR-52 needs a hand-off. The result has a
//! `behind` flag, true when some peer announces a height more than 2 above
//! the tip. The caller stores the time at which `behind` first became true
//! (and clears it when `behind` is false again) and passes it back in
//! [`HealthInput::behind_since`]. WR-52 fails when `behind` is true and more
//! than 10 minutes have passed since that time. If `behind` is true and
//! `behind_since` is `None`, the clock has not started and WR-52 holds for
//! now.

use std::collections::BTreeSet;

use crate::netgroup::{network_group, PeerAddr};
use crate::{
    BEHIND_BLOCKS, MAX_BEHIND_SECS, MAX_CLOCK_BEHIND_SECS, MAX_TIP_AGE_SECS, MIN_FILTER_GROUPS,
    MIN_PEER_GROUPS,
};

/// A peer the light client is connected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// Where the peer was reached.
    pub addr: PeerAddr,
    /// Whether the peer serves compact block filters.
    pub serves_filters: bool,
    /// The best height the peer announced.
    pub best_height: u32,
}

/// What the health rules read.
#[derive(Debug, Clone, Copy)]
pub struct HealthInput<'a> {
    /// The connected peers.
    pub peers: &'a [Peer],
    /// Height of the counter's tip.
    pub tip_height: u32,
    /// The time in the tip's header, in seconds.
    pub tip_time: u64,
    /// The counter's clock, in seconds.
    pub now: u64,
    /// When `behind` first became true, as the caller stored it from an
    /// earlier answer; `None` if it is not true now or was not stored.
    pub behind_since: Option<u64>,
}

/// The first rule that does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// WR-50: peers from fewer than 3 network groups.
    TooFewPeerGroups,
    /// WR-50: fewer than 2 network groups with a peer that serves filters.
    TooFewFilterPeers,
    /// WR-51: the tip's header time is more than 2 hours behind the clock.
    TipIsOld,
    /// WR-52: a peer has announced a height more than 2 above the tip for
    /// more than 10 minutes.
    HeadersBehind,
    /// WR-53: the clock is more than 2 hours behind the tip's header time.
    ClockMismatch,
}

impl Problem {
    /// The rule this problem belongs to.
    pub fn rule(self) -> &'static str {
        match self {
            Self::TooFewPeerGroups | Self::TooFewFilterPeers => "WR-50",
            Self::TipIsOld => "WR-51",
            Self::HeadersBehind => "WR-52",
            Self::ClockMismatch => "WR-53",
        }
    }
}

/// The answer of [`check_health`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    /// Every rule holds.
    pub healthy: bool,
    /// The first rule that does not, in the order WR-50 to WR-53.
    pub problem: Option<Problem>,
    /// Some peer announces a height more than 2 above the tip. The caller
    /// stores the time this first became true (see the module docs).
    pub behind: bool,
}

/// WR-50 to WR-53.
pub fn check_health(input: &HealthInput<'_>) -> Health {
    let behind = input
        .peers
        .iter()
        .any(|p| u64::from(p.best_height) > u64::from(input.tip_height) + u64::from(BEHIND_BLOCKS));
    let problem = first_problem(input, behind);
    Health {
        healthy: problem.is_none(),
        problem,
        behind,
    }
}

fn first_problem(input: &HealthInput<'_>, behind: bool) -> Option<Problem> {
    // WR-50.
    let groups: BTreeSet<_> = input.peers.iter().map(|p| network_group(&p.addr)).collect();
    if groups.len() < MIN_PEER_GROUPS {
        return Some(Problem::TooFewPeerGroups);
    }
    let filter_groups: BTreeSet<_> = input
        .peers
        .iter()
        .filter(|p| p.serves_filters)
        .map(|p| network_group(&p.addr))
        .collect();
    if filter_groups.len() < MIN_FILTER_GROUPS {
        return Some(Problem::TooFewFilterPeers);
    }
    // WR-51.
    if input.now.saturating_sub(input.tip_time) > MAX_TIP_AGE_SECS {
        return Some(Problem::TipIsOld);
    }
    // WR-52.
    if behind {
        if let Some(since) = input.behind_since {
            if input.now.saturating_sub(since) > MAX_BEHIND_SECS {
                return Some(Problem::HeadersBehind);
            }
        }
    }
    // WR-53.
    if input.tip_time.saturating_sub(input.now) > MAX_CLOCK_BEHIND_SECS {
        return Some(Problem::ClockMismatch);
    }
    None
}
