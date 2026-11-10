//! Planning a transaction (sections 2 to 4 of the rules).

use std::collections::HashSet;
use std::fmt;

use bitcoin::absolute::LockTime;
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};
use openbmt_core::wallet::WalletDescription;
use openbmt_signer::{MAX_FEE_RATE_SAT_PER_VB, MAX_FEE_SATS, MAX_INPUTS, MAX_OUTPUTS, MAX_WEIGHT};

use crate::coin::{Coin, CoinError};
use crate::common::{shape_weight, template_weight, MAX_MONEY, SEQUENCE, TX_VERSION};

/// What to spend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendRequest {
    /// The payments, in the order they appear in the transaction. With
    /// `send_all` there is exactly one, and its amount is 0 (SP-12).
    pub payments: Vec<(ScriptBuf, u64)>,
    /// The fee rate in satoshis per 1000 virtual bytes. The caller supplies
    /// it; this library estimates nothing.
    pub fee_rate: u64,
    /// The index on the change keychain that change goes to (SP-21).
    pub change_index: u32,
    /// The fewest confirmations a coin needs to be spent (SP-10). 1 spends
    /// confirmed coins only; 0 also spends unconfirmed ones.
    pub min_confirmations: u32,
    /// Spend every eligible coin to the one payment, with no change.
    pub send_all: bool,
}

/// An input of a plan: the coin and its amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInput {
    /// The coin, as it was handed in.
    pub coin: Coin,
    /// The coin's amount in satoshis.
    pub amount_sats: u64,
}

/// A payment of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPayment {
    /// The script paid.
    pub script: ScriptBuf,
    /// The amount in satoshis.
    pub sats: u64,
    /// Whether this is the one output of a consolidation (SP-24), paid to
    /// the wallet's own change script.
    pub to_own_change: bool,
}

/// The change output of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedChange {
    /// The index on the change keychain.
    pub index: u32,
    /// The wallet's change script at that index.
    pub script: ScriptBuf,
    /// The amount in satoshis.
    pub sats: u64,
}

/// Why no plan could be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// SP-01 or SP-02: a coin is refused.
    Coin(CoinError),
    /// SP-03: the same outpoint twice.
    DuplicateCoin(OutPoint),
    /// SP-25: no payment.
    NoPayments,
    /// SP-12: `send_all` needs exactly one payment.
    SendAllNeedsOnePayment,
    /// SP-12: `send_all` takes the amount from the coins; the one given
    /// must be 0.
    SendAllAmountGiven,
    /// SP-32: a fee rate of 0.
    ZeroFeeRate,
    /// The change script at `change_index` could not be derived.
    ChangeScript,
    /// SP-22: a payment's script is not a standard kind the signer allows.
    PaymentScriptKind {
        /// Position in the request.
        payment: usize,
    },
    /// SP-22: a payment below the dust limit of its script, or zero.
    PaymentBelowDust {
        /// Position in the request.
        payment: usize,
        /// The amount asked for.
        sats: u64,
        /// The limit for its script.
        dust: u64,
    },
    /// SP-22, SP-34: an amount above 21,000,000 BTC, alone or in total.
    AmountTooLarge,
    /// SP-24: a payment to one of the wallet's own scripts.
    PaymentToOwnScript {
        /// Position in the request.
        payment: usize,
    },
    /// SP-25: more outputs than the signer takes.
    TooManyOutputs,
    /// SP-33: more inputs than the signer takes.
    TooManyInputs,
    /// SP-10: no coin is eligible.
    NoEligibleCoins,
    /// The eligible coins do not pay for the request and its fee.
    InsufficientFunds {
        /// The total of the eligible coins.
        available: u64,
        /// What payments plus the fee would take with all of them.
        needed: u64,
    },
    /// SP-12: after the fee, the amount of a `send_all` is below dust.
    SendAllBelowDust {
        /// What is left after the fee.
        sats: u64,
        /// The dust limit of the script.
        dust: u64,
    },
    /// SP-32: the fee is above the fixed bound.
    FeeTooHigh {
        /// The fee in satoshis.
        fee: u64,
        /// The bound.
        limit: u64,
    },
    /// SP-32: the fee rate is above the fixed bound.
    FeeRateTooHigh {
        /// The fee in satoshis.
        fee: u64,
        /// The virtual size.
        vsize: u64,
        /// The bound in satoshis per virtual byte.
        limit: u64,
    },
    /// SP-33: the weight is above the standard limit.
    TooHeavy {
        /// The weight in weight units.
        weight: u64,
        /// The limit.
        limit: u64,
    },
    /// SP-34: an amount overflowed.
    Overflow,
    /// A check the library makes of its own work failed.
    Internal(&'static str),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coin(e) => write!(f, "{e}"),
            Self::DuplicateCoin(o) => write!(f, "SP-03: coin {o} is given twice"),
            Self::NoPayments => write!(f, "SP-25: there is no payment"),
            Self::SendAllNeedsOnePayment => {
                write!(f, "SP-12: sending everything takes exactly one payment")
            }
            Self::SendAllAmountGiven => write!(
                f,
                "SP-12: sending everything takes its amount from the coins; the amount given \
                 must be 0"
            ),
            Self::ZeroFeeRate => write!(f, "SP-32: the fee rate is 0"),
            Self::ChangeScript => write!(f, "SP-21: the change script could not be derived"),
            Self::PaymentScriptKind { payment } => write!(
                f,
                "SP-22: payment {payment} is not to a P2WPKH, P2WSH, P2TR, P2PKH or P2SH script"
            ),
            Self::PaymentBelowDust {
                payment,
                sats,
                dust,
            } => write!(
                f,
                "SP-22: payment {payment} of {sats} sat is below the dust limit of {dust} sat"
            ),
            Self::AmountTooLarge => write!(f, "SP-22: an amount is above 21,000,000 BTC"),
            Self::PaymentToOwnScript { payment } => write!(
                f,
                "SP-24: payment {payment} is to one of the wallet's own scripts"
            ),
            Self::TooManyOutputs => write!(f, "SP-25: more than {MAX_OUTPUTS} outputs"),
            Self::TooManyInputs => write!(f, "SP-33: more than {MAX_INPUTS} inputs are needed"),
            Self::NoEligibleCoins => write!(f, "SP-10: no coin has enough confirmations"),
            Self::InsufficientFunds { available, needed } => write!(
                f,
                "the eligible coins hold {available} sat; the payments and the fee need {needed} sat"
            ),
            Self::SendAllBelowDust { sats, dust } => write!(
                f,
                "SP-12: {sats} sat is left after the fee, below the dust limit of {dust} sat"
            ),
            Self::FeeTooHigh { fee, limit } => {
                write!(f, "SP-32: the fee of {fee} sat is above {limit} sat")
            }
            Self::FeeRateTooHigh { fee, vsize, limit } => write!(
                f,
                "SP-32: a fee of {fee} sat over {vsize} vB is above {limit} sat/vB"
            ),
            Self::TooHeavy { weight, limit } => write!(
                f,
                "SP-33: the transaction weighs {weight} units; the limit is {limit}"
            ),
            Self::Overflow => write!(f, "SP-34: an amount overflowed"),
            Self::Internal(what) => write!(f, "internal check failed: {what}"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<CoinError> for PlanError {
    fn from(e: CoinError) -> Self {
        Self::Coin(e)
    }
}

/// A transaction worked out, with everything a person must be shown
/// (SP-36). Make one with [`plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub(crate) inputs: Vec<PlanInput>,
    pub(crate) payments: Vec<PlannedPayment>,
    pub(crate) change: Option<PlannedChange>,
    pub(crate) change_index: u32,
    pub(crate) dropped_change: Option<u64>,
    pub(crate) fee: u64,
    pub(crate) weight: u64,
    pub(crate) vsize: u64,
    pub(crate) requested_rate: u64,
    pub(crate) total_in: u64,
    pub(crate) total_out: u64,
    pub(crate) tx: Transaction,
}

impl Plan {
    /// The coins spent, in the order of the transaction's inputs.
    pub fn inputs(&self) -> &[PlanInput] {
        &self.inputs
    }

    /// The payments, in the order of the transaction's outputs.
    pub fn payments(&self) -> &[PlannedPayment] {
        &self.payments
    }

    /// The change output, last in the transaction, if there is one.
    pub fn change(&self) -> Option<&PlannedChange> {
        self.change.as_ref()
    }

    /// SP-23: the change that was below dust and went into the fee.
    pub fn dropped_change_sats(&self) -> Option<u64> {
        self.dropped_change
    }

    /// The fee in satoshis: inputs minus outputs.
    pub fn fee_sats(&self) -> u64 {
        self.fee
    }

    /// The weight of the fully signed transaction, counting every signature
    /// at its longest (SP-30).
    pub fn weight(&self) -> u64 {
        self.weight
    }

    /// The virtual size of the fully signed transaction, signatures at their
    /// longest.
    pub fn vsize(&self) -> u64 {
        self.vsize
    }

    /// The rate that was asked for, in satoshis per 1000 virtual bytes.
    pub fn requested_rate(&self) -> u64 {
        self.requested_rate
    }

    /// The fee over the planned size, in satoshis per 1000 virtual bytes,
    /// rounded down. It is at least the requested rate unless a dust
    /// change made the fee larger (then it is above).
    pub fn fee_rate_per_kvb(&self) -> u64 {
        self.fee.saturating_mul(1000) / self.vsize.max(1)
    }

    /// The total of the inputs in satoshis.
    pub fn total_in_sats(&self) -> u64 {
        self.total_in
    }

    /// The total of the outputs (payments and change) in satoshis.
    pub fn total_out_sats(&self) -> u64 {
        self.total_out
    }

    /// The unsigned transaction.
    pub fn unsigned_tx(&self) -> &Transaction {
        &self.tx
    }

    /// The transaction ID. It does not change when signatures are added.
    pub fn txid(&self) -> Txid {
        self.tx.compute_txid()
    }
}

/// The dust limit of a script of one of the standard kinds, in satoshis,
/// at the default dust relay rate of 3 sat/vB: three times the size of the
/// output plus the input that spends it. `None` for any other script.
pub fn dust_limit(script: &bitcoin::Script) -> Option<u64> {
    if script.is_p2wpkh() {
        Some(294)
    } else if script.is_p2wsh() || script.is_p2tr() {
        Some(330)
    } else if script.is_p2pkh() {
        Some(546)
    } else if script.is_p2sh() {
        Some(540)
    } else {
        None
    }
}

/// The fee for `inputs` coins and outputs with scripts of `lens` bytes at
/// `rate` satoshis per 1000 virtual bytes: `(weight, vsize, fee)`.
fn costs(inputs: usize, lens: &[usize], rate: u64) -> Result<(u64, u64, u64), PlanError> {
    let inputs = u64::try_from(inputs).map_err(|_| PlanError::Overflow)?;
    let weight = shape_weight(inputs, lens).ok_or(PlanError::Overflow)?;
    let vsize = weight.div_ceil(4);
    let fee = vsize
        .checked_mul(rate)
        .ok_or(PlanError::Overflow)?
        .div_ceil(1000);
    Ok((weight, vsize, fee))
}

/// Plan a transaction from `coins` (SP-01 to SP-36).
///
/// Every coin is checked first, eligible or not. Coins are then taken oldest
/// first until they pay for the payments and the fee. The result is the same
/// for the same arguments.
pub fn plan(
    wallet: &WalletDescription,
    coins: &[Coin],
    request: &SpendRequest,
) -> Result<Plan, PlanError> {
    if request.fee_rate == 0 {
        return Err(PlanError::ZeroFeeRate);
    }
    if request.payments.is_empty() {
        return Err(PlanError::NoPayments);
    }
    if request.send_all {
        match request.payments.as_slice() {
            [(_, 0)] => {}
            [_] => return Err(PlanError::SendAllAmountGiven),
            _ => return Err(PlanError::SendAllNeedsOnePayment),
        }
    }
    if request.payments.len() > MAX_OUTPUTS {
        return Err(PlanError::TooManyOutputs);
    }

    // SP-01, SP-02, SP-03: every coin, eligible or not.
    let mut seen: HashSet<OutPoint> = HashSet::new();
    let mut own_scripts: Vec<ScriptBuf> = Vec::new();
    for coin in coins {
        coin.check(wallet)?;
        if !seen.insert(coin.outpoint) {
            return Err(PlanError::DuplicateCoin(coin.outpoint));
        }
        if let Some(output) = coin.output() {
            own_scripts.push(output.script_pubkey.clone());
        }
    }
    let change_script = wallet
        .change_script(request.change_index)
        .map_err(|_| PlanError::ChangeScript)?;
    own_scripts.push(change_script.clone());

    // SP-22, SP-24
    let mut payments: Vec<PlannedPayment> = Vec::with_capacity(request.payments.len());
    for (n, (script, sats)) in request.payments.iter().enumerate() {
        let consolidation = request.send_all && *script == change_script;
        if !consolidation && own_scripts.contains(script) {
            return Err(PlanError::PaymentToOwnScript { payment: n });
        }
        let dust = dust_limit(script).ok_or(PlanError::PaymentScriptKind { payment: n })?;
        if !request.send_all && *sats < dust {
            return Err(PlanError::PaymentBelowDust {
                payment: n,
                sats: *sats,
                dust,
            });
        }
        payments.push(PlannedPayment {
            script: script.clone(),
            sats: *sats,
            to_own_change: consolidation,
        });
    }
    let mut pay_total: u64 = 0;
    for p in &payments {
        pay_total = pay_total.checked_add(p.sats).ok_or(PlanError::Overflow)?;
    }
    if pay_total > MAX_MONEY {
        return Err(PlanError::AmountTooLarge);
    }

    // SP-10, SP-11
    let mut eligible: Vec<&Coin> = coins
        .iter()
        .filter(|c| c.confirmations >= request.min_confirmations)
        .collect();
    eligible.sort_by(|a, b| {
        b.confirmations
            .cmp(&a.confirmations)
            .then_with(|| a.outpoint_key().cmp(&b.outpoint_key()))
    });
    if eligible.is_empty() {
        return Err(PlanError::NoEligibleCoins);
    }
    let mut amounts: Vec<u64> = Vec::with_capacity(eligible.len());
    for coin in &eligible {
        let sats = coin
            .output()
            .ok_or(PlanError::Internal("a checked coin has no output"))?
            .value
            .to_sat();
        if sats > MAX_MONEY {
            return Err(PlanError::AmountTooLarge);
        }
        amounts.push(sats);
    }

    let payment_lens: Vec<usize> = payments.iter().map(|p| p.script.len()).collect();
    let mut change_lens = payment_lens.clone();
    change_lens.push(change_script.len());
    let may_have_change = !request.send_all && payments.len() < MAX_OUTPUTS;
    let rate = request.fee_rate;

    // Choose the coins.
    let mut taken = 0usize;
    let mut total: u64 = 0;
    let mut finish: Option<Finish> = None;
    for (n, sats) in amounts.iter().enumerate() {
        total = total.checked_add(*sats).ok_or(PlanError::Overflow)?;
        taken = n.checked_add(1).ok_or(PlanError::Overflow)?;
        if taken > MAX_INPUTS {
            return Err(PlanError::TooManyInputs);
        }
        let (weight, vsize, fee) = costs(taken, &payment_lens, rate)?;
        if weight > MAX_WEIGHT {
            return Err(PlanError::TooHeavy {
                weight,
                limit: MAX_WEIGHT,
            });
        }
        if request.send_all {
            // Everything is spent; the last coin decides.
            if taken == amounts.len() {
                finish = Some(Finish::All { weight, vsize, fee });
            }
            continue;
        }
        let Some(spare) = total.checked_sub(pay_total) else {
            continue;
        };
        if spare < fee {
            continue;
        }
        if may_have_change {
            let (c_weight, c_vsize, c_fee) = costs(taken, &change_lens, rate)?;
            if c_weight > MAX_WEIGHT {
                return Err(PlanError::TooHeavy {
                    weight: c_weight,
                    limit: MAX_WEIGHT,
                });
            }
            if let Some(change) = spare.checked_sub(c_fee) {
                let dust =
                    dust_limit(&change_script).ok_or(PlanError::Internal("change script kind"))?;
                if change >= dust {
                    finish = Some(Finish::WithChange {
                        weight: c_weight,
                        vsize: c_vsize,
                        fee: c_fee,
                        change,
                    });
                    break;
                }
                // SP-23: below dust, into the fee.
                finish = Some(Finish::NoChange {
                    weight,
                    vsize,
                    dropped: Some(change),
                });
                break;
            }
        }
        finish = Some(Finish::NoChange {
            weight,
            vsize,
            dropped: None,
        });
        break;
    }
    let Some(finish) = finish else {
        let (_, _, fee) = costs(amounts.len().max(1), &payment_lens, rate)?;
        let needed = if request.send_all {
            fee
        } else {
            pay_total.checked_add(fee).ok_or(PlanError::Overflow)?
        };
        return Err(PlanError::InsufficientFunds {
            available: total,
            needed,
        });
    };

    // Work out the outputs and the fee that is paid.
    let (weight, vsize, fee, change) = match finish {
        Finish::All { weight, vsize, fee } => {
            let sats = total.checked_sub(fee).ok_or(PlanError::InsufficientFunds {
                available: total,
                needed: fee,
            })?;
            let only = payments
                .first_mut()
                .ok_or(PlanError::Internal("send_all without a payment"))?;
            let dust = dust_limit(&only.script).ok_or(PlanError::Internal("payment kind"))?;
            if sats < dust {
                return Err(PlanError::SendAllBelowDust { sats, dust });
            }
            only.sats = sats;
            (weight, vsize, fee, None)
        }
        Finish::WithChange {
            weight,
            vsize,
            fee,
            change,
        } => (
            weight,
            vsize,
            fee,
            Some(PlannedChange {
                index: request.change_index,
                script: change_script.clone(),
                sats: change,
            }),
        ),
        Finish::NoChange { weight, vsize, .. } => {
            let paid = total.checked_sub(pay_total).ok_or(PlanError::Overflow)?;
            (weight, vsize, paid, None)
        }
    };
    let dropped_change = match finish {
        Finish::NoChange { dropped, .. } => dropped,
        _ => None,
    };

    // SP-32
    if fee > MAX_FEE_SATS {
        return Err(PlanError::FeeTooHigh {
            fee,
            limit: MAX_FEE_SATS,
        });
    }
    if fee > MAX_FEE_RATE_SAT_PER_VB.saturating_mul(vsize) {
        return Err(PlanError::FeeRateTooHigh {
            fee,
            vsize,
            limit: MAX_FEE_RATE_SAT_PER_VB,
        });
    }
    if weight > MAX_WEIGHT {
        return Err(PlanError::TooHeavy {
            weight,
            limit: MAX_WEIGHT,
        });
    }

    // The transaction (SP-20, SP-35).
    let chosen: Vec<&Coin> = eligible.into_iter().take(taken).collect();
    let mut tx = Transaction {
        version: Version(TX_VERSION),
        lock_time: LockTime::ZERO,
        input: chosen
            .iter()
            .map(|c| TxIn {
                previous_output: c.outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence(SEQUENCE),
                witness: Witness::new(),
            })
            .collect(),
        output: payments
            .iter()
            .map(|p| TxOut {
                value: Amount::from_sat(p.sats),
                script_pubkey: p.script.clone(),
            })
            .collect(),
    };
    if let Some(c) = &change {
        tx.output.push(TxOut {
            value: Amount::from_sat(c.sats),
            script_pubkey: c.script.clone(),
        });
    }
    if tx.input.len() > MAX_INPUTS || tx.output.len() > MAX_OUTPUTS {
        return Err(PlanError::TooManyOutputs);
    }
    // SP-30: the worked-out weight against the library's own count.
    if template_weight(&tx) != weight {
        return Err(PlanError::Internal("the weight does not match the shape"));
    }

    // SP-34
    let mut total_in: u64 = 0;
    let mut inputs = Vec::with_capacity(chosen.len());
    for (coin, sats) in chosen.iter().zip(&amounts) {
        total_in = total_in.checked_add(*sats).ok_or(PlanError::Overflow)?;
        inputs.push(PlanInput {
            coin: (*coin).clone(),
            amount_sats: *sats,
        });
    }
    let mut total_out: u64 = 0;
    for out in &tx.output {
        total_out = total_out
            .checked_add(out.value.to_sat())
            .ok_or(PlanError::Overflow)?;
    }
    if total_out > MAX_MONEY || total_in.checked_sub(total_out) != Some(fee) {
        return Err(PlanError::Internal("inputs are not outputs plus fee"));
    }

    Ok(Plan {
        inputs,
        payments,
        change,
        change_index: request.change_index,
        dropped_change,
        fee,
        weight,
        vsize,
        requested_rate: rate,
        total_in,
        total_out,
        tx,
    })
}

/// How the coin choice ended.
#[derive(Clone, Copy)]
enum Finish {
    /// `send_all`: one payment, every coin.
    All { weight: u64, vsize: u64, fee: u64 },
    /// Payments and a change output.
    WithChange {
        weight: u64,
        vsize: u64,
        fee: u64,
        change: u64,
    },
    /// Payments only; the rest of the coins is the fee. `dropped` is the
    /// change that was below dust, if there was one.
    NoChange {
        weight: u64,
        vsize: u64,
        dropped: Option<u64>,
    },
}
