//! Section 7: the merchant's own node, a source apart from the peers.
//!
//! The driver reaches the node over Bitcoin's peer protocol at a Tor v3
//! address the owner set (`docs/NETWORK-RULES.md`, NR-98). This library
//! never sees an address: the driver says whether the node is connected,
//! how far it has got and when it was last heard, and which transactions it
//! reports. [`check_own_node`] turns that into [`OwnNodeHealth`], and
//! [`SaleWatch::status_with_own_node`](crate::SaleWatch::status_with_own_node)
//! reads it.
//!
//! The node is never a peer: what it reports is kept apart from what the
//! random peers report (WR-60), and it can only add to what the light client
//! saw (WR-62). When it is down or behind, the counter falls back to the
//! light check and says why (WR-63, WR-64).

use crate::{OWN_NODE_LAG_BLOCKS, OWN_NODE_LAG_SECS, OWN_NODE_SILENT_SECS};

/// How the driver's connection to the own node stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnNodeLink {
    /// The handshake is done and the connection is open.
    Connected,
    /// There is no connection. `since` is when the last one ended, or when
    /// the driver started if there has been none.
    Down {
        /// Seconds, on the counter's clock.
        since: u64,
    },
    /// The node broke a rule of the peer protocol (NR-25) and the driver
    /// does not dial it again until the app asks (NR-100).
    Refused {
        /// Seconds, on the counter's clock.
        since: u64,
    },
}

/// What the driver tells about the own node, sent with the peers (NR-75).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnNodeInput {
    /// The connection.
    pub link: OwnNodeLink,
    /// The best height the node has shown: its own statement at the
    /// handshake, and every block it has announced that the counter's chain
    /// holds. `None` before the handshake.
    pub best_height: Option<u32>,
    /// When anything last came from the node, in seconds. `None` if it never
    /// has.
    pub last_heard: Option<u64>,
}

/// The first rule of WR-63 that does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnNodeProblem {
    /// The node broke a rule of the peer protocol and is not dialed again.
    Refused {
        /// Seconds, on the counter's clock.
        since: u64,
    },
    /// There is no connection to the node.
    Unreachable {
        /// Seconds, on the counter's clock.
        since: u64,
    },
    /// Connected, but nothing has been heard for more than 10 minutes.
    Silent {
        /// When it was last heard, in seconds.
        since: u64,
    },
    /// Connected, but the counter's tip is 2 or more blocks above the
    /// node's best height, or 1 block above it for more than 5 minutes.
    Behind {
        /// How many blocks the node is behind the counter.
        blocks: u32,
    },
    /// Connected, but the node has not yet shown a height, or nothing has
    /// been heard from it.
    TipUnknown,
}

impl OwnNodeProblem {
    /// The rule this problem belongs to.
    pub fn rule(self) -> &'static str {
        "WR-63"
    }
}

/// The answer of [`check_own_node`]: the health line for the own node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnNodeHealth {
    /// An own node is set. False makes every other field empty.
    pub configured: bool,
    /// The node may be used as a second check: it is configured and no rule
    /// of WR-63 fails.
    pub usable: bool,
    /// The first rule that fails, in the order of [`OwnNodeProblem`].
    pub problem: Option<OwnNodeProblem>,
    /// The best height the node has shown.
    pub own_height: Option<u32>,
    /// The height of the counter's tip.
    pub our_height: u32,
    /// When anything last came from the node, in seconds.
    pub last_heard: Option<u64>,
}

impl OwnNodeHealth {
    /// No own node is set. Nothing is shown, and no payment is ever
    /// counted as seen by an own node.
    pub fn not_configured() -> Self {
        Self {
            configured: false,
            usable: false,
            problem: None,
            own_height: None,
            our_height: 0,
            last_heard: None,
        }
    }
}

/// Remembers since when the own node has been one block behind, which
/// [`check_own_node`] needs (WR-63). The app keeps one and calls
/// [`OwnNodeTracker::check`] each time it checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnNodeTracker {
    one_behind_since: Option<u64>,
}

impl OwnNodeTracker {
    /// A tracker that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// [`check_own_node`], with the time the lag began kept between calls.
    /// Catching up, or leaving the connected state, starts the count over.
    pub fn check(
        &mut self,
        input: Option<&OwnNodeInput>,
        tip_height: u32,
        now: u64,
    ) -> OwnNodeHealth {
        let lag = input.is_some_and(|i| {
            i.link == OwnNodeLink::Connected
                && i.best_height
                    .is_some_and(|b| tip_height.saturating_sub(b) == OWN_NODE_LAG_BLOCKS)
        });
        if lag {
            self.one_behind_since.get_or_insert(now);
        } else {
            self.one_behind_since = None;
        }
        check_own_node(input, tip_height, now, self.one_behind_since)
    }
}

/// WR-63: whether the own node can be used as a second check right now.
/// `input` is `None` when the owner has set no own node. `one_behind_since`
/// is when the node was first seen exactly one block behind, without a break;
/// [`OwnNodeTracker`] keeps it. With `None`, one block behind passes.
pub fn check_own_node(
    input: Option<&OwnNodeInput>,
    tip_height: u32,
    now: u64,
    one_behind_since: Option<u64>,
) -> OwnNodeHealth {
    let Some(input) = input else {
        return OwnNodeHealth::not_configured();
    };
    let problem = match input.link {
        OwnNodeLink::Refused { since } => Some(OwnNodeProblem::Refused { since }),
        OwnNodeLink::Down { since } => Some(OwnNodeProblem::Unreachable { since }),
        OwnNodeLink::Connected => connected_problem(input, tip_height, now, one_behind_since),
    };
    OwnNodeHealth {
        configured: true,
        usable: problem.is_none(),
        problem,
        own_height: input.best_height,
        our_height: tip_height,
        last_heard: input.last_heard,
    }
}

fn connected_problem(
    input: &OwnNodeInput,
    tip_height: u32,
    now: u64,
    one_behind_since: Option<u64>,
) -> Option<OwnNodeProblem> {
    let (Some(best), Some(heard)) = (input.best_height, input.last_heard) else {
        return Some(OwnNodeProblem::TipUnknown);
    };
    if now.saturating_sub(heard) > OWN_NODE_SILENT_SECS {
        return Some(OwnNodeProblem::Silent { since: heard });
    }
    let behind = tip_height.saturating_sub(best);
    let lagging = behind == OWN_NODE_LAG_BLOCKS
        && one_behind_since.is_some_and(|t| now.saturating_sub(t) > OWN_NODE_LAG_SECS);
    (behind > OWN_NODE_LAG_BLOCKS || lagging).then_some(OwnNodeProblem::Behind { blocks: behind })
}
