//! The judgment of the owner's heartbeat (CR-51 to CR-53).
//!
//! [`judge_heartbeat`] is a pure function of the heartbeat, the counter's own
//! header chain and the counter's clock. It says whether the heartbeat can
//! count towards instant acceptance, and if not, why. Who signed the
//! heartbeat, which counter it names and its number are checked before this
//! (CR-50, CR-54), and its age against the signed setting is checked by the
//! instant decision (CR-51), which is given the age by
//! [`Counter::heartbeat_for_policy`](crate::Counter::heartbeat_for_policy).

use std::fmt;

use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_watch::HeaderChain;

/// CR-51: the most the heartbeat's own time may be ahead of the counter's
/// clock, in seconds **[to tune]**.
pub const MAX_AHEAD_SECS: u64 = 120;
/// CR-53: how far the heartbeat's tip may be from the counter's tip, in
/// blocks, in either direction **[to tune]**.
pub const TIP_SLACK_BLOCKS: u32 = 2;

/// Why a heartbeat does not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotVerified {
    /// CR-51: its time is more than [`MAX_AHEAD_SECS`] ahead of the
    /// counter's clock.
    FromTheFuture {
        /// How far ahead, in seconds.
        ahead_secs: u64,
    },
    /// CR-52: the owner's watcher says its own check is not healthy.
    OwnerUnhealthy,
    /// CR-53: the tip is not one the counter holds yet, at a height above
    /// the counter's own tip by at most [`TIP_SLACK_BLOCKS`]. The counter
    /// may be the one that is behind. Not an alarm.
    TipNotYetHeld,
    /// CR-53: the tip is not one the counter holds, at a height more than
    /// [`TIP_SLACK_BLOCKS`] above its own tip. Either side may be far
    /// behind; the counter's own health rules say if it is. Not an alarm.
    TipTooFarAhead,
    /// CR-53: the tip is on the counter's chain but more than
    /// [`TIP_SLACK_BLOCKS`] below its tip: the owner's watcher is behind.
    TipBehind {
        /// How many blocks the owner's tip is below the counter's.
        blocks: u32,
    },
    /// CR-53: the counter holds another block at that height, or the same
    /// block at another height. The two watchers see different chains. This
    /// is logged.
    TipDisagrees,
}

impl NotVerified {
    /// Whether the two watchers see different chains. The caller logs it.
    pub fn is_disagreement(&self) -> bool {
        matches!(self, Self::TipDisagrees)
    }

    /// The rule the answer belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::FromTheFuture { .. } => "CR-51",
            Self::OwnerUnhealthy => "CR-52",
            _ => "CR-53",
        }
    }
}

impl fmt::Display for NotVerified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FromTheFuture { ahead_secs } => write!(
                f,
                "the heartbeat is timed {ahead_secs} seconds ahead of this counter's clock"
            ),
            Self::OwnerUnhealthy => {
                write!(f, "the owner's watcher says its own check is unhealthy")
            }
            Self::TipNotYetHeld => write!(
                f,
                "the owner's watcher sees a block this counter does not hold yet"
            ),
            Self::TipTooFarAhead => write!(
                f,
                "the owner's watcher sees a block far above this counter's tip"
            ),
            Self::TipBehind { blocks } => {
                write!(
                    f,
                    "the owner's watcher is {blocks} blocks behind this counter"
                )
            }
            Self::TipDisagrees => write!(
                f,
                "the owner's watcher and this counter hold different blocks at that height"
            ),
        }
    }
}

/// What the counter makes of a heartbeat it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatView {
    /// The heartbeat counts. `sent_at` is its own time; its age is the
    /// counter's time minus this.
    Verified {
        /// The time the heartbeat carries, in seconds.
        sent_at: u64,
    },
    /// The heartbeat does not count.
    NotVerified(NotVerified),
}

impl HeartbeatView {
    /// Whether the heartbeat counts.
    pub fn is_verified(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }
}

/// CR-51 to CR-53: whether a heartbeat that has been checked for signature,
/// counter and number counts now. The first of these that fails is the
/// answer:
///
/// 1. its time is not more than [`MAX_AHEAD_SECS`] ahead of `now` (CR-51);
/// 2. the owner's watcher says it is healthy (CR-52);
/// 3. its tip is a block the counter holds at that height, at most
///    [`TIP_SLACK_BLOCKS`] below the counter's tip (CR-53).
///
/// A tip is held when the chain gives that hash that height. A height above
/// the counter's tip cannot be held yet. A height at or below it that does
/// not hold that hash is a disagreement.
pub fn judge_heartbeat<C: HeaderChain + ?Sized>(
    hb: &WatcherHeartbeat,
    chain: &C,
    now: u64,
) -> HeartbeatView {
    let ahead = hb.time.saturating_sub(now);
    if ahead > MAX_AHEAD_SECS {
        return HeartbeatView::NotVerified(NotVerified::FromTheFuture { ahead_secs: ahead });
    }
    if !hb.healthy {
        return HeartbeatView::NotVerified(NotVerified::OwnerUnhealthy);
    }
    let tip = chain.tip_height();
    if hb.tip_height > tip {
        let problem = if holds(chain, hb) {
            // The chain gives the hash a height above its own tip: it does
            // not agree with itself, so this is a disagreement.
            NotVerified::TipDisagrees
        } else if hb.tip_height - tip <= TIP_SLACK_BLOCKS {
            NotVerified::TipNotYetHeld
        } else {
            NotVerified::TipTooFarAhead
        };
        return HeartbeatView::NotVerified(problem);
    }
    if !holds(chain, hb) {
        return HeartbeatView::NotVerified(NotVerified::TipDisagrees);
    }
    let blocks = tip - hb.tip_height;
    if blocks > TIP_SLACK_BLOCKS {
        return HeartbeatView::NotVerified(NotVerified::TipBehind { blocks });
    }
    HeartbeatView::Verified { sent_at: hb.time }
}

/// The chain gives the heartbeat's hash the heartbeat's height.
fn holds<C: HeaderChain + ?Sized>(chain: &C, hb: &WatcherHeartbeat) -> bool {
    chain.height_of(&BlockHash::from_byte_array(hb.tip_hash)) == Some(hb.tip_height)
}
