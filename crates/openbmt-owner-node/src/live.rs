//! The owner's live watch and the deep check of the addresses left out of
//! it (`docs/OWNER-COINS-RULES.md`, OC-22 to OC-27).
//!
//! The network manager watches at most `MAX_WATCHED_SCRIPTS` scripts
//! (NR-41). A shop that issues addresses for years would pass that, so the
//! owner's device keeps a smaller live watch ([`OWNER_LIVE_WATCH`]) and, when
//! the wallet's scripts do not fit, sets the oldest quiet receive scripts
//! aside. Those are looked at again by a deep check every
//! [`DEEP_CHECK_SECS`], and when the owner asks.
//!
//! This module holds the numbers, the compact set of receive indices that
//! were set aside ([`Spans`]), the plan that picks them ([`plan_release`])
//! and the three states the owner is told about ([`DeepLine`]). The coin
//! view stores the set and the schedule; the node does the work.

use std::collections::{BTreeMap, BTreeSet};

use openbmt_net_io::MAX_WATCHED_SCRIPTS;
use openbmt_owner::IssuedRange;

use crate::coins::INDEX_LIMIT;

/// OC-22: scripts the owner's live watch holds, change and receive
/// together, before quiet receive scripts are set aside. Leaves room under
/// the driver's limit (NR-41) for growth **[to tune]**.
pub const OWNER_LIVE_WATCH: usize = 90_000;
/// OC-23: a receive script is quiet when its batch was signed at least this
/// long ago (60 days) **[to tune]**.
pub const QUIET_SECS: u64 = 60 * 24 * 3_600;
/// OC-24: seconds between two deep checks (30 days) **[to tune]**.
pub const DEEP_CHECK_SECS: u64 = 30 * 24 * 3_600;
/// OC-26: seconds before a failed deep check is tried again (1 hour)
/// **[to tune]**.
pub const DEEP_RETRY_SECS: u64 = 3_600;
/// OC-26: seconds without a block or a new height from the deep check's
/// driver, after which the check counts as failed (1 hour) **[to tune]**.
pub const DEEP_STALL_SECS: u64 = 3_600;
/// Most separate runs of set-aside indices the view keeps.
pub const MAX_RELEASED_SPANS: usize = 1 << 20;
/// How often the node looks again for scripts to set aside while the live
/// watch does not fit and nothing was quiet enough, in seconds.
pub const REBALANCE_SECS: u64 = 600;

/// The three numbers of the live watch. [`WatchLimits::default`] holds the
/// constants above; a check against a real node shrinks them to see the
/// rules at work without issuing 90,000 addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchLimits {
    /// The live watch's bound ([`OWNER_LIVE_WATCH`]).
    pub live: usize,
    /// The quiet time ([`QUIET_SECS`]).
    pub quiet_secs: u64,
    /// The time between deep checks ([`DEEP_CHECK_SECS`]).
    pub deep_secs: u64,
}

impl Default for WatchLimits {
    fn default() -> Self {
        Self {
            live: OWNER_LIVE_WATCH,
            quiet_secs: QUIET_SECS,
            deep_secs: DEEP_CHECK_SECS,
        }
    }
}

impl WatchLimits {
    /// Whether the numbers can work: a bound of at least one script and no
    /// more than the driver takes.
    pub fn check(&self) -> Result<(), String> {
        if self.live == 0 || self.live > MAX_WATCHED_SCRIPTS {
            return Err(format!(
                "the live watch must hold from 1 to {MAX_WATCHED_SCRIPTS} scripts, not {}",
                self.live
            ));
        }
        Ok(())
    }
}

/// A set of receive indices kept as sorted runs, so that the stored form
/// and the memory stay small when a long stretch of the oldest indices is
/// set aside. Runs never touch or overlap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spans(BTreeMap<u32, u32>);

impl Spans {
    /// No index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether no index is held.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many runs are held.
    pub fn runs(&self) -> usize {
        self.0.len()
    }

    /// How many indices are held.
    pub fn len(&self) -> u64 {
        self.0
            .iter()
            .fold(0u64, |n, (s, e)| n.saturating_add(u64::from(e - s)))
    }

    /// How many indices below `limit` are held.
    pub fn count_below(&self, limit: u32) -> u64 {
        self.0
            .iter()
            .take_while(|(s, _)| **s < limit)
            .fold(0u64, |n, (s, e)| {
                n.saturating_add(u64::from((*e).min(limit) - s))
            })
    }

    /// Whether the index is held.
    pub fn contains(&self, index: u32) -> bool {
        self.0
            .range(..=index)
            .next_back()
            .is_some_and(|(_, end)| index < *end)
    }

    /// Hold an index. Returns whether it was new.
    pub fn insert(&mut self, index: u32) -> bool {
        if index >= INDEX_LIMIT || self.contains(index) {
            return false;
        }
        let mut start = index;
        let mut end = index + 1;
        if let Some((s, e)) = self.0.range(..index).next_back().map(|(s, e)| (*s, *e)) {
            if e == index {
                self.0.remove(&s);
                start = s;
            }
        }
        if let Some(e) = self.0.remove(&end) {
            end = e;
        }
        self.0.insert(start, end);
        true
    }

    /// Let an index go. Returns whether it was held.
    pub fn remove(&mut self, index: u32) -> bool {
        let Some((s, e)) = self.0.range(..=index).next_back().map(|(s, e)| (*s, *e)) else {
            return false;
        };
        if index >= e {
            return false;
        }
        self.0.remove(&s);
        if s < index {
            self.0.insert(s, index);
        }
        if index + 1 < e {
            self.0.insert(index + 1, e);
        }
        true
    }

    /// Every index held, ascending.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().flat_map(|(s, e)| *s..*e)
    }

    /// The runs as (first, one past the last), ascending.
    pub fn pairs(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.0.iter().map(|(s, e)| (*s, *e))
    }

    /// Build from runs, strictly: ascending, not empty, not touching, not
    /// past the last index, and no more than [`MAX_RELEASED_SPANS`].
    pub fn from_pairs(pairs: &[(u32, u32)]) -> Option<Self> {
        if pairs.len() > MAX_RELEASED_SPANS {
            return None;
        }
        let mut map = BTreeMap::new();
        let mut floor: Option<u32> = None;
        for (start, end) in pairs {
            if start >= end || *end > INDEX_LIMIT || floor.is_some_and(|f| *start <= f) {
                return None;
            }
            floor = Some(*end);
            map.insert(*start, *end);
        }
        Some(Self(map))
    }
}

/// OC-22, OC-23: which receive indices to set aside, oldest issued first.
/// An index is chosen when its batch was signed at least `quiet_secs` before
/// `now`, it is not set aside already, and `held` (the receive indices with
/// an unspent coin) does not hold it. Change scripts are never chosen: they
/// are not among `ranges`. At most `need` indices are returned, in the
/// order they were chosen.
pub fn plan_release(
    ranges: &[IssuedRange],
    released: &Spans,
    held: &BTreeSet<u32>,
    now: u64,
    quiet_secs: u64,
    need: u64,
) -> Vec<u32> {
    let mut order: Vec<&IssuedRange> = ranges
        .iter()
        .filter(|r| r.signed_at.saturating_add(quiet_secs) <= now)
        .collect();
    order.sort_by_key(|r| (r.signed_at, r.start));
    let mut chosen = Vec::new();
    for range in order {
        let end = u32::try_from(range.end()).unwrap_or(u32::MAX);
        for index in range.start..end {
            if u64::try_from(chosen.len()).unwrap_or(u64::MAX) >= need {
                return chosen;
            }
            if !released.contains(index) && !held.contains(&index) {
                chosen.push(index);
            }
        }
    }
    chosen
}

/// OC-25: what the owner is told while some receive scripts are set aside.
/// Each state has its own words (`crate::say`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepLine {
    /// Addresses are set aside and no deep check has run yet.
    FirstCheckWaiting {
        /// How many addresses are set aside.
        count: u64,
        /// When the first check is due, on the owner's clock.
        next: u64,
    },
    /// Addresses are set aside; the last check finished whole days ago.
    Monthly {
        /// How many addresses are set aside.
        count: u64,
        /// Whole days since the last check finished.
        days_ago: u64,
        /// When the next check is due, on the owner's clock.
        next: u64,
    },
    /// A deep check is running.
    Checking {
        /// How many addresses are set aside.
        count: u64,
    },
    /// The last deep check did not finish.
    Failed {
        /// How many addresses are set aside.
        count: u64,
        /// When it is tried again, on the owner's clock.
        next: u64,
    },
}
