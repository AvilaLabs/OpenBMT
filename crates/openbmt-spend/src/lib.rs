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
//! A wallet with one key (`docs/SPEND-RULES.md` section 10) goes through the
//! same four steps with one signature in place of two: every function that
//! takes a wallet takes an `openbmt_core::wallet_single::ImportedWallet`
//! (or, as before, a 2-of-3 `WalletDescription`), and [`combine_wallet`]
//! and [`take_wallet_signature`] are the steps that need to know the
//! wallet.
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
mod kind;
mod plan;
mod psbt;
pub mod say;
mod signer_file;

pub use coin::{Coin, CoinError};
pub use combine::{combine, combine_wallet, CombineError, Source};
pub use finalize::{finalize, finalize_plan, FinalizeError};
pub use kind::{WalletKind, WalletRef};
pub use plan::{
    consolidation_cost_sats, consolidation_cost_sats_single, consolidation_size,
    consolidation_size_single, dust_limit, plan, receipt_spend_cost_sats,
    receipt_spend_cost_sats_single, ChangePosition, Plan, PlanError, PlanInput, PlanOutput,
    PlannedChange, PlannedPayment, SpendRequest, MAX_CONSOLIDATION_INPUTS,
};
pub use psbt::PsbtError;
pub use signer_file::{
    check_finished, psbt_for_signer, take_own_signatures, take_signer_signatures,
    take_wallet_signature, transaction_of_final_file, SignerFileError,
};
