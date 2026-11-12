//! Section 7, continued: the shop's own node alone (WR-66 to WR-68).
//!
//! With `node_only` on (`docs/NETWORK-RULES.md`, NR-105) the own node is the
//! only source: it is the driver's one peer (NR-106), and "seen by the light
//! client" (WR-44) is not used. What this module adds to [`crate::own_node`]
//! is what the driver can say about such a node beyond WR-63: its services
//! do not suit (NR-107), a block it would have to serve is older than it
//! keeps (NR-108), and whether the random peers of NR-110 are running. Like
//! the rest of the library it has no network and no clock.
//!
//! - [`NodeOnlyInput`] is what the driver tells; [`NodeOnlyTracker::check`]
//!   turns it into a [`NodeOnlyHealth`].
//! - [`SaleWatch::status_node_only`](crate::SaleWatch::status_node_only)
//!   reads it: a payment the node holds, while it can be used, has the mode
//!   `CheckMode::OwnNodeOnly` (WR-66); with `light_fallback` on, what the
//!   random peers report gets the light check's mode (WR-68).
//! - [`NodeOnlyHealth::line`] is the health line of WR-67.

use crate::own_node::{OwnNodeHealth, OwnNodeInput, OwnNodeLink, OwnNodeProblem, OwnNodeTracker};

/// What is wrong with a node that is connected or was, besides the rules of
/// WR-63. The driver tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOnlyFault {
    /// NR-107: the node's `version` does not advertise compact filters
    /// (`NODE_COMPACT_FILTERS`) together with blocks (`NODE_NETWORK` or
    /// `NODE_NETWORK_LIMITED`).
    MissingFilters {
        /// Seconds, on the counter's clock: when the node last showed it.
        since: u64,
    },
    /// NR-108: a block that is needed is older than the node serves.
    LacksBlock {
        /// The height of the oldest block that the node does not serve.
        height: u32,
        /// Seconds, on the counter's clock: when it was found out.
        since: u64,
    },
}

/// What the driver tells the app about the own node in this mode
/// (NR-103).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeOnlyInput {
    /// The connection, as in the mode with the light client (NR-102).
    pub node: OwnNodeInput,
    /// A fault of NR-107 or NR-108, if there is one.
    pub fault: Option<NodeOnlyFault>,
    /// The setting `light_fallback` (NR-105).
    pub light_fallback: bool,
    /// The random peers of NR-110 are running now.
    pub fallback_running: bool,
}

/// The first thing that makes the own node unusable in this mode: the rules
/// of WR-63 and the two of NR-107 and NR-108.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOnlyProblem {
    /// A rule of WR-63: broken protocol, connecting, not reachable, silent,
    /// behind, or not yet known.
    Node(OwnNodeProblem),
    /// NR-107: the node does not offer compact filters and blocks.
    MissingFilters {
        /// Seconds, on the counter's clock.
        since: u64,
    },
    /// NR-108: the node does not keep a block that is needed.
    LacksBlock {
        /// The height of the block.
        height: u32,
        /// Seconds, on the counter's clock.
        since: u64,
    },
}

impl NodeOnlyProblem {
    /// The rule the health line of this mode belongs to.
    pub fn rule(self) -> &'static str {
        "WR-67"
    }
}

/// The health line of the own node in this mode (WR-67).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOnlyLine {
    /// The node can be used.
    Usable,
    /// The node cannot be used and no one else is asked: sales wait and
    /// nothing is accepted at once (NR-110).
    NotUsable(NodeOnlyProblem),
    /// The node cannot be used and the counter is checking with the random
    /// peers meanwhile (NR-110, WR-68).
    CheckingWithOthers(NodeOnlyProblem),
}

/// The answer of [`NodeOnlyTracker::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeOnlyHealth {
    /// The node's health as [`SaleWatch::status_with_own_node`] reads it:
    /// `usable` is false for every problem of this mode too.
    ///
    /// [`SaleWatch::status_with_own_node`]: crate::SaleWatch::status_with_own_node
    pub own: OwnNodeHealth,
    /// The first rule that fails, in the order of
    /// [`NodeOnlyTracker::check`].
    pub problem: Option<NodeOnlyProblem>,
    /// The setting `light_fallback`.
    pub light_fallback: bool,
    /// The random peers of NR-110 are running.
    pub fallback_running: bool,
}

impl NodeOnlyHealth {
    /// WR-67: the state of the line.
    pub fn line(&self) -> NodeOnlyLine {
        match self.problem {
            None => NodeOnlyLine::Usable,
            Some(p) if self.fallback_running => NodeOnlyLine::CheckingWithOthers(p),
            Some(p) => NodeOnlyLine::NotUsable(p),
        }
    }
}

/// [`OwnNodeTracker`] for this mode: it adds the faults of NR-107 and NR-108.
/// The app keeps one and calls [`NodeOnlyTracker::check`] each time it checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NodeOnlyTracker {
    inner: OwnNodeTracker,
}

impl NodeOnlyTracker {
    /// A tracker that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// As [`OwnNodeTracker::retry`].
    pub fn retry(&mut self, now: u64) {
        self.inner.retry(now);
    }

    /// WR-63 with NR-107 and NR-108, checked in this order:
    /// 1. the node broke a rule of the peer protocol;
    /// 2. its services do not suit (NR-107), whether or not it is connected:
    ///    the driver drops such a node and the line goes on saying why;
    /// 3. there is no connection (connecting, or not reachable);
    /// 4. a block that is needed is older than it serves (NR-108);
    /// 5. silent, not yet known, behind (WR-63, in that order).
    ///
    /// `input` is `None` when no own node is set; the answer then says so
    /// and has no problem.
    pub fn check(
        &mut self,
        input: Option<&NodeOnlyInput>,
        tip_height: u32,
        now: u64,
    ) -> NodeOnlyHealth {
        let base = self.inner.check(input.map(|i| &i.node), tip_height, now);
        let Some(input) = input else {
            return NodeOnlyHealth {
                own: base,
                problem: None,
                light_fallback: false,
                fallback_running: false,
            };
        };
        let node_problem = base.problem.map(NodeOnlyProblem::Node);
        let link_problem = matches!(
            base.problem,
            Some(OwnNodeProblem::Connecting { .. } | OwnNodeProblem::Unreachable { .. })
        );
        let refused = matches!(input.node.link, OwnNodeLink::Refused { .. });
        let problem = match input.fault {
            _ if refused => node_problem,
            Some(NodeOnlyFault::MissingFilters { since }) => {
                Some(NodeOnlyProblem::MissingFilters { since })
            }
            _ if link_problem => node_problem,
            Some(NodeOnlyFault::LacksBlock { height, since }) => {
                Some(NodeOnlyProblem::LacksBlock { height, since })
            }
            None => node_problem,
        };
        let mut own = base;
        own.usable = problem.is_none();
        NodeOnlyHealth {
            own,
            problem,
            light_fallback: input.light_fallback,
            fallback_running: input.fallback_running,
        }
    }
}
