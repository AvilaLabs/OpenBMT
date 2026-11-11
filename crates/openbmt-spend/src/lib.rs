//! The OpenBMT spend path: plan a transaction from the wallet's coins,
//! write it as a PSBT, join two signers' signatures and finish it.
//!
//! The rules are in `docs/SPEND-RULES.md`; each has an ID (SP-01 and on),
//! and the tests name the rule they prove. The library has no network, no
//! clock, no storage and no randomness. The steps are:
//!
//! 1. [`plan`] chooses coins and works out the transaction ([`Plan`]).
//! 2. [`Plan::to_psbt`] writes it as a PSBT that the signer
//!    (`openbmt-signer`) accepts.
//! 3. [`combine`] joins the signatures two signers made on copies of it,
//!    verifying each one first.
//! 4. [`finalize`] (or [`finalize_plan`]) turns two signatures per input
//!    into a transaction, and checks that transaction again from scratch.
//!
//! 5. [`psbt_for_signer`], [`take_signer_signatures`], [`check_finished`]
//!    and [`transaction_of_final_file`] are the steps for a signer that
//!    someone else made: the file it is shown and what it returns.
//!
//! The final check is this library's own. There is no consensus library
//! here; Bitcoin Core's `testmempoolaccept` is the independent check (see
//! `scripts/spend-e2e-core.sh`).
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod coin;
mod combine;
mod common;
mod finalize;
mod plan;
mod psbt;
pub mod say;
mod signer_file;

pub use coin::{Coin, CoinError};
pub use combine::{combine, CombineError, Source};
pub use finalize::{finalize, finalize_plan, FinalizeError};
pub use plan::{
    consolidation_cost_sats, consolidation_size, dust_limit, plan, receipt_spend_cost_sats,
    ChangePosition, Plan, PlanError, PlanInput, PlanOutput, PlannedChange, PlannedPayment,
    SpendRequest, MAX_CONSOLIDATION_INPUTS,
};
pub use psbt::PsbtError;
pub use signer_file::{
    check_finished, psbt_for_signer, take_signer_signatures, transaction_of_final_file,
    SignerFileError,
};
