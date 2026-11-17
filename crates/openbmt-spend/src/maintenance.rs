//! Advisory UTXO maintenance readiness; this module never constructs or signs a transaction.

use std::collections::HashSet;

use bitcoin::OutPoint;

use crate::coin::{Coin, CoinError};
use crate::kind::{WalletKind, WalletRef};
use crate::plan::{consolidation_size, consolidation_size_single, MAX_CONSOLIDATION_INPUTS};

/// Why a candidate consolidation batch should be deferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceDeferReason {
    /// The estimated fee exceeds the unused portion of the caller's budget.
    FeeBudget,
    /// The estimated fee is greater than or equal to the batch's gross value.
    FeeConsumesValue,
    /// After the estimated fee, the output would be below the wallet output's dust limit.
    Dust,
}

/// One deterministic, oldest-first candidate batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceBatch {
    /// Outpoints in the order the ordinary spend planner uses.
    pub outpoints: Vec<OutPoint>,
    /// Sum of these inputs, before any future transaction fee.
    pub gross_sats: u64,
    /// Estimated fee at the explicitly supplied rate; not an actual fee paid.
    pub estimated_fee_sats: u64,
    /// Gross value less estimated fee, when representable.
    pub estimated_output_sats: Option<u64>,
    /// None means this batch fits the supplied budget and produces a non-dust output.
    pub deferred: Option<MaintenanceDeferReason>,
}

/// Readiness estimate for confirmed wallet coins at one explicit fee scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceAssessment {
    /// Total value of every supplied coin, including ineligible coins.
    pub supplied_balance_sats: u64,
    /// Value of coins meeting `min_confirmations`.
    pub eligible_balance_sats: u64,
    /// Value of coins below `min_confirmations`.
    pub ineligible_balance_sats: u64,
    /// Sum of estimated future batch fees, independent of available budget.
    pub estimated_future_cost_sats: u64,
    /// Candidate batches, with budget and dust deferrals made explicit.
    pub batches: Vec<MaintenanceBatch>,
}

/// Assess bounded UTXO maintenance candidates without creating a spend plan.
///
/// All coins are validated against the supplied wallet, including coins below
/// the confirmation threshold. `min_confirmations` must be at least 1 so this
/// maintenance operation never proposes spending unconfirmed receipts.
/// Eligible coins are sorted by confirmation depth
/// descending and then by outpoint, matching SP-11. Batches contain at most
/// `MAX_CONSOLIDATION_INPUTS`. Fee rate uses the planner's satoshis-per-1000-vB
/// units. Budget is cumulative and considered in batch order. This is advisory:
/// callers must obtain fresh chain state and use [`crate::plan`] to prepare any
/// actual, owner-authorized consolidation.
pub fn assess_maintenance<'a>(
    wallet: impl Into<WalletRef<'a>>,
    coins: &[Coin],
    min_confirmations: u32,
    fee_rate: u64,
    fee_budget_sats: u64,
) -> Result<MaintenanceAssessment, MaintenanceError> {
    if min_confirmations == 0 {
        return Err(MaintenanceError::UnconfirmedNotAllowed);
    }
    let wallet = wallet.into();
    let mut seen = HashSet::new();
    let mut eligible = Vec::new();
    let mut supplied_balance_sats = 0u64;
    let mut eligible_balance_sats = 0u64;
    for coin in coins {
        coin.check(wallet).map_err(MaintenanceError::Coin)?;
        if !seen.insert(coin.outpoint) {
            return Err(MaintenanceError::DuplicateCoin(coin.outpoint));
        }
        let value = coin
            .output()
            .ok_or(MaintenanceError::Coin(CoinError::OutputMissing {
                outpoint: coin.outpoint,
            }))?
            .value
            .to_sat();
        supplied_balance_sats = supplied_balance_sats
            .checked_add(value)
            .ok_or(MaintenanceError::Overflow)?;
        if coin.confirmations >= min_confirmations {
            eligible_balance_sats = eligible_balance_sats
                .checked_add(value)
                .ok_or(MaintenanceError::Overflow)?;
            eligible.push((coin, value));
        }
    }
    eligible.sort_by(|(a, _), (b, _)| {
        b.confirmations
            .cmp(&a.confirmations)
            .then_with(|| a.outpoint_key().cmp(&b.outpoint_key()))
    });
    let ineligible_balance_sats = supplied_balance_sats
        .checked_sub(eligible_balance_sats)
        .ok_or(MaintenanceError::Overflow)?;
    let mut batches = Vec::new();
    let mut estimated_future_cost_sats = 0u64;
    let mut budget_used = 0u64;
    let dust = match wallet.kind() {
        WalletKind::Multisig => 330,
        WalletKind::Single => 294,
    };
    for group in eligible.chunks(MAX_CONSOLIDATION_INPUTS) {
        let (_, vsize) = match wallet.kind() {
            WalletKind::Multisig => consolidation_size(group.len()),
            WalletKind::Single => consolidation_size_single(group.len()),
        }
        .ok_or(MaintenanceError::Overflow)?;
        let fee = vsize
            .checked_mul(fee_rate)
            .ok_or(MaintenanceError::Overflow)?
            .div_ceil(1000);
        estimated_future_cost_sats = estimated_future_cost_sats
            .checked_add(fee)
            .ok_or(MaintenanceError::Overflow)?;
        let gross = group.iter().try_fold(0u64, |sum, (_, value)| {
            sum.checked_add(*value).ok_or(MaintenanceError::Overflow)
        })?;
        let output = gross.checked_sub(fee);
        let deferred = if fee >= gross {
            Some(MaintenanceDeferReason::FeeConsumesValue)
        } else if budget_used
            .checked_add(fee)
            .ok_or(MaintenanceError::Overflow)?
            > fee_budget_sats
        {
            Some(MaintenanceDeferReason::FeeBudget)
        } else if output.is_some_and(|value| value < dust) {
            Some(MaintenanceDeferReason::Dust)
        } else {
            budget_used += fee;
            None
        };
        batches.push(MaintenanceBatch {
            outpoints: group.iter().map(|(coin, _)| coin.outpoint).collect(),
            gross_sats: gross,
            estimated_fee_sats: fee,
            estimated_output_sats: output,
            deferred,
        });
    }
    Ok(MaintenanceAssessment {
        supplied_balance_sats,
        eligible_balance_sats,
        ineligible_balance_sats,
        estimated_future_cost_sats,
        batches,
    })
}

/// Why a maintenance assessment could not be computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceError {
    /// A supplied coin did not validate against this wallet.
    Coin(CoinError),
    /// An outpoint was supplied more than once.
    DuplicateCoin(OutPoint),
    /// A balance or fee calculation overflowed.
    Overflow,
    /// Maintenance requires confirmed inputs; pass a threshold of at least one.
    UnconfirmedNotAllowed,
}

impl std::fmt::Display for MaintenanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Coin(error) => write!(f, "{error}"),
            Self::DuplicateCoin(outpoint) => write!(f, "SP-03: coin {outpoint} is given twice"),
            Self::Overflow => write!(f, "SP-34: maintenance estimate overflowed"),
            Self::UnconfirmedNotAllowed => write!(
                f,
                "SP-88: maintenance assessment requires at least one confirmation"
            ),
        }
    }
}
impl std::error::Error for MaintenanceError {}
