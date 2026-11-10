//! The OpenBMT Key app's transaction signer: the rules under which key 2
//! signs.
//!
//! The rules are in `docs/SIGNER-RULES.md`; each has an ID (SR-01 and on),
//! and the code, the refusals and the tests name the rule they belong to.
//! The library has no network, no clock and no storage. The caller passes
//! the time in seconds and a `persist` closure that stores the new state
//! before anything is released.
//!
//! The steps are:
//!
//! 1. [`SignerState::register`] reads the wallet description once.
//! 2. [`SignerState::review`] reads a PSBT file and checks every rule that
//!    needs no key. It changes nothing and returns a [`Review`].
//! 3. [`SignerState::sign`] takes the review itself, checks the rules again
//!    against the state as it is then, signs, stores the new state and
//!    returns the signed file.
//!
//! Changes the owner makes on the Key app's own screen (allowlist,
//! ceilings, the two marks, clock correction) are separate calls on the
//! state.
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod analysis;
mod crypto;
mod refusal;
mod review;
mod sign;
mod snapshot;
mod state;

#[cfg(test)]
mod tests;

pub use refusal::{Ceiling, ChangeError, ChangeRefusal, Refusal, RegisterError, SignError};
pub use review::{OutputKind, Remaining, Review, ReviewedOutput};
pub use sign::Signed;
pub use state::{AllowlistEntry, Ceilings, FeeCeilings, LogEntry, SignerState, Totals};

/// SR-38: first ceilings for other destinations, in satoshis.
pub const FIRST_OTHER: Ceilings = Ceilings {
    per_tx: 100_000,
    per_day: 300_000,
    per_week: 1_000_000,
};
/// SR-38: first ceilings for allowlisted destinations, in satoshis.
pub const FIRST_ALLOWLISTED: Ceilings = Ceilings {
    per_tx: 5_000_000,
    per_day: 10_000_000,
    per_week: 30_000_000,
};
/// SR-40, SR-41: first fee ceilings.
pub const FIRST_FEE: FeeCeilings = FeeCeilings {
    sat_per_vb: 150,
    absolute_sats: 200_000,
};
/// SR-40: the highest fee-rate ceiling the owner can set, in sat/vB.
pub const MAX_FEE_RATE_SAT_PER_VB: u64 = 1_000;
/// SR-41: the highest absolute fee ceiling the owner can set, in satoshis.
pub const MAX_FEE_SATS: u64 = 2_000_000;
/// SR-30: seconds a new destination waits before it is allowlisted.
pub const ALLOWLIST_WAIT_SECS: u64 = 86_400;
/// SR-32: seconds in the day window.
pub const DAY_SECS: u64 = 86_400;
/// SR-32: seconds in the week window.
pub const WEEK_SECS: u64 = 604_800;
/// SR-34: how far past the change mark a change index may be.
pub const CHANGE_LOOKAHEAD: u32 = 20;
/// SR-02: largest PSBT file in bytes.
pub const MAX_PSBT_BYTES: usize = 8 * 1024 * 1024;
/// SR-02: most inputs in a transaction.
pub const MAX_INPUTS: usize = 1_000;
/// SR-02: most outputs in a transaction.
pub const MAX_OUTPUTS: usize = 100;
/// SR-30: most allowlist entries.
pub const MAX_ALLOWLIST: usize = 64;
/// SR-30: most characters in an allowlist label.
pub const MAX_LABEL_CHARS: usize = 64;
/// Section 1: most entries in the spend log.
pub const MAX_LOG_ENTRIES: usize = 10_000;
/// Section 1: most inputs in the spend log, over all entries.
pub const MAX_LOG_INPUTS: usize = 100_000;
/// SR-15, SR-24, SR-37: 21,000,000 BTC in satoshis.
pub const MAX_MONEY_SATS: u64 = 21_000_000 * 100_000_000;
/// SR-43: most weight units of the fully signed transaction.
pub const MAX_WEIGHT: u64 = 400_000;
/// Section 7: bytes of one signature in the size estimate, with the
/// hash-type byte. A low-S ECDSA signature in DER is at most 71 bytes.
pub const SIGNATURE_BYTES: usize = 72;
/// Bytes of the 2-of-3 witness script.
pub const WITNESS_SCRIPT_BYTES: usize = 105;
