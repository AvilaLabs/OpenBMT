//! The OpenBMT counter's judgment of a sale: which address a sale gets, when
//! Bitcoin may be offered, and which of the cashier's screens is shown at each
//! moment.
//!
//! The rules are in `docs/COUNTER-RULES.md`; each has an ID (CR-01 and on),
//! and the refusals and the tests name the rule they belong to. The library
//! joins what already exists: what has arrived (`openbmt-watch`), whether an
//! unconfirmed payment may be accepted at once and the exposure ledger
//! (`openbmt_core::policy`), and the signed address batches, settings and
//! records (`openbmt_core`). It adds no check of its own to them.
//!
//! It has no network, no clock, no file access and no randomness. The caller
//! passes in the facts and the time in seconds, hands every change to a
//! [`Store`] before it takes effect, and draws what it is told.
//!
//! The parts are:
//!
//! 1. [`money`]: amounts, rates, quotes and the two conversions.
//! 2. [`store`]: the [`Store`] the caller supplies and the [`Slot`]s in it.
//! 3. [`events`]: the body of every record the counter writes.
//! 4. [`desk`]: the counter's own state (`OBMG`).
//! 5. [`sale`]: one sale, its valuation and its stored form (`OBML`).
//! 6. [`screen`]: the states, their two texts and what is reported outward.
//! 7. [`refusal`]: the refusals, each naming its rule.
//! 8. [`heartbeat`]: the judgment of the owner's heartbeat (CR-51 to CR-53).
//! 9. [`counter`]: the [`Counter`] itself.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod counter;
pub mod desk;
pub mod events;
pub mod heartbeat;
pub mod money;
pub mod refusal;
pub mod sale;
pub mod screen;
pub mod store;

#[cfg(test)]
mod tests;

pub use counter::{
    sale_payment_id, Actor, BlockEffect, BlockTaken, Counter, Facts, OpenFacts, Opened, Overridden,
    Pairing, Sightings, Stored,
};
pub use desk::{Desk, OverrideEntry, Role};
pub use events::Event;
pub use heartbeat::{judge_heartbeat, HeartbeatView, NotVerified};
pub use money::{sats_for, value_of, Amount, Currency, Quote, Rate, QUOTE_SECS};
pub use refusal::{
    BlockError, CancelRefused, CommandRefused, HeartbeatRefused, JudgeError, LoadError,
    OpenRefused, OverrideRefused, RequoteRefused, SightingError,
};
pub use sale::{End, OverrideMark, PosState, Sale, Segment};
pub use screen::{Color, Next, Report, Screen, State, Why};
pub use store::{Slot, Store, StoreFault};

/// CR-04: most new addresses handed out in one hour **[to tune]**.
pub const MAX_ADDRESSES_PER_HOUR: usize = 120;
/// CR-07: seconds after which a sale with nothing received expires **[to
/// tune]**.
pub const EXPIRY_SECS: u64 = 900;
/// CR-17: what has arrived for a sale and is below this many satoshis in
/// total is dust **[to tune]**.
pub const DUST_SATS: u64 = 1_000;
/// CR-18: seconds a seen payment may stay unconfirmed before the sale is
/// shown as stuck **[to tune]**.
pub const STUCK_SECS: u64 = 7_200;
/// CR-05: most quotes one sale keeps. Past it the last quote stays in force
/// and a new quote is refused.
pub const MAX_QUOTES: usize = 64;
/// CR-21: the window, in seconds, over which a person's overrides are added
/// up.
pub const OVERRIDE_WINDOW_SECS: u64 = 86_400;
/// CR-22: longest name of a person who overrides, in bytes.
pub const MAX_NAME_BYTES: usize = 64;
/// CR-06: most valuations one sale records. Past it, newly arrived satoshis
/// stay unvalued and are counted at the quote in force each time.
pub const MAX_SEGMENTS: usize = 128;
/// CR-19: most late payments one sale records. Later ones are still flagged
/// (the sale is marked late) but not listed.
pub const MAX_LATE_PAYMENTS: usize = 20;
/// CR-21: most override entries the desk keeps inside the window. Past it,
/// overrides are refused until older ones leave the window.
pub const MAX_OVERRIDE_ENTRIES: usize = 1_000;
/// The longest a stored sale watch may be, in bytes (twenty transactions at
/// the watcher's size limit, and room for the rest).
pub const MAX_WATCH_BYTES: usize = 16 * 1024 * 1024;
