//! The review (SR-50): what the Key app shows before it signs, and the
//! checks that need the state and the time (SR-20 to SR-25, SR-33 to SR-36
//! and section 7).

use bitcoin::{Address, ScriptBuf, Txid};

use crate::analysis::{analyze, signed_weight, Parsed};
use crate::refusal::{Ceiling, Refusal};
use crate::state::{is_payment_script, sums, LogEntry, LogMap, SignerState};
use crate::{CHANGE_LOOKAHEAD, MAX_LOG_ENTRIES, MAX_LOG_INPUTS};

/// What an output is, in the sense of SR-20 to SR-22.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputKind {
    /// Change back to the wallet (SR-20), at this change index.
    Change {
        /// The change index.
        index: u32,
    },
    /// A destination on the allowlist that is active (SR-21).
    Allowlisted {
        /// The owner's label for it.
        label: String,
    },
    /// Any other destination (SR-22).
    Other,
}

/// One output as the review shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewedOutput {
    /// Position in the transaction.
    pub index: usize,
    /// Amount in satoshis.
    pub amount: u64,
    /// The output script.
    pub script: ScriptBuf,
    /// The address, in the registered wallet's network.
    pub address: Option<Address>,
    /// What the output is.
    pub kind: OutputKind,
}

/// What would be left under each of the six ceilings after this
/// transaction, in satoshis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remaining {
    /// Per transaction, other destinations.
    pub other_per_tx: u64,
    /// Per day, other destinations and fees.
    pub other_day: u64,
    /// Per week, other destinations and fees.
    pub other_week: u64,
    /// Per transaction, allowlisted destinations.
    pub allowlisted_per_tx: u64,
    /// Per day, allowlisted destinations.
    pub allowlisted_day: u64,
    /// Per week, allowlisted destinations.
    pub allowlisted_week: u64,
}

/// A transaction that passed every rule that needs no key, with what the
/// owner is shown. Signing takes the review itself (SR-50), so what was
/// shown is what is signed.
#[derive(Debug)]
pub struct Review {
    parsed: Parsed,
    outputs: Vec<ReviewedOutput>,
    change: Option<(u32, u64)>,
    fee: u64,
    vsize: u64,
    weight: u64,
    other_total: u64,
    allowlisted_total: u64,
    remaining: Remaining,
    clock_behind_by: Option<u64>,
    already_counted: bool,
}

impl Review {
    /// The transaction ID.
    pub fn txid(&self) -> Txid {
        self.parsed.txid
    }

    /// Every output, in transaction order, change included.
    pub fn outputs(&self) -> &[ReviewedOutput] {
        &self.outputs
    }

    /// The change index and amount, only if SR-20 passed.
    pub fn change(&self) -> Option<(u32, u64)> {
        self.change
    }

    /// Whether the only output is change (SR-25).
    pub fn moves_coins_inside_wallet(&self) -> bool {
        self.change.is_some() && self.outputs.len() == 1
    }

    /// The fee in satoshis.
    pub fn fee_sats(&self) -> u64 {
        self.fee
    }

    /// The size of the fully signed transaction in virtual bytes (section 7).
    pub fn vsize(&self) -> u64 {
        self.vsize
    }

    /// The weight of the fully signed transaction.
    pub fn weight(&self) -> u64 {
        self.weight
    }

    /// The fee rate in satoshis per virtual byte, rounded up.
    pub fn fee_rate_ceil_sat_per_vb(&self) -> u64 {
        self.fee.div_ceil(self.vsize)
    }

    /// The number of inputs.
    pub fn input_count(&self) -> usize {
        self.parsed.tx.input.len()
    }

    /// The total of the inputs in satoshis.
    pub fn input_total_sats(&self) -> u64 {
        self.parsed.input_total
    }

    /// The total sent to other destinations.
    pub fn other_total_sats(&self) -> u64 {
        self.other_total
    }

    /// The total sent to allowlisted destinations.
    pub fn allowlisted_total_sats(&self) -> u64 {
        self.allowlisted_total
    }

    /// What would remain under each ceiling after this transaction.
    pub fn remaining(&self) -> Remaining {
        self.remaining
    }

    /// If the clock passed in is behind the latest time seen, by how many
    /// seconds (SR-31).
    pub fn clock_behind_by(&self) -> Option<u64> {
        self.clock_behind_by
    }

    /// Whether this transaction ID is already in the spend log, so signing
    /// it again counts nothing more (SR-33).
    pub fn already_counted(&self) -> bool {
        self.already_counted
    }

    /// The kind of every output, for the comparison of SR-51.
    pub(crate) fn kinds(&self) -> impl Iterator<Item = &OutputKind> {
        self.outputs.iter().map(|o| &o.kind)
    }

    pub(crate) fn parsed(&self) -> &Parsed {
        &self.parsed
    }
}

/// The kind of every output at one time, and the totals by kind.
pub(crate) struct Classified {
    pub kinds: Vec<OutputKind>,
    /// Position and change index of the change output.
    pub change: Option<(usize, u32)>,
    pub other_total: u64,
    pub allowlisted_total: u64,
}

/// What the ceilings and the log say about a transaction.
pub(crate) struct Limits {
    pub fee: u64,
    pub vsize: u64,
    pub weight: u64,
    pub next_log: LogMap,
    pub already_counted: bool,
    pub remaining: Remaining,
}

impl SignerState {
    /// Work out the kind of every output (SR-20 to SR-23) at `effective`.
    pub(crate) fn classify(&self, p: &Parsed, effective: u64) -> Result<Classified, Refusal> {
        let limit = u64::from(self.change_mark) + u64::from(CHANGE_LOOKAHEAD);
        let mut kinds = Vec::with_capacity(p.tx.output.len());
        let mut change = None;
        let mut other_total = 0u64;
        let mut allowlisted_total = 0u64;
        for (j, (txout, candidate)) in p.tx.output.iter().zip(&p.change_candidates).enumerate() {
            let amount = txout.value.to_sat();
            if let Some(index) = *candidate {
                // SR-34 holds for every output that claims to be change, not
                // only for the one that is taken as change.
                if u64::from(index) >= limit {
                    return Err(Refusal::ChangeIndexTooFar {
                        output: j,
                        index,
                        limit,
                    });
                }
                if change.is_none() {
                    change = Some((j, index));
                    kinds.push(OutputKind::Change { index });
                    continue;
                }
            }
            if let Some(entry) = self.active_entry(&txout.script_pubkey, effective) {
                allowlisted_total = allowlisted_total
                    .checked_add(amount)
                    .ok_or(Refusal::OutputTotalOverflow)?;
                kinds.push(OutputKind::Allowlisted {
                    label: entry.label.clone(),
                });
                continue;
            }
            if !is_payment_script(&txout.script_pubkey) {
                return Err(Refusal::OutputScriptKind { output: j });
            }
            other_total = other_total
                .checked_add(amount)
                .ok_or(Refusal::OutputTotalOverflow)?;
            kinds.push(OutputKind::Other);
        }
        Ok(Classified {
            kinds,
            change,
            other_total,
            allowlisted_total,
        })
    }

    /// Check the fee (section 7), the ceilings (SR-35, SR-36) and the room
    /// in the spend log, and work out the log as it would be after signing.
    pub(crate) fn limits(
        &self,
        p: &Parsed,
        c: &Classified,
        effective: u64,
    ) -> Result<Limits, Refusal> {
        let fee = p.input_total - p.output_total;
        let weight = signed_weight(&p.tx);
        let vsize = weight.div_ceil(4);
        // SR-40 without division.
        if fee > self.fee.sat_per_vb.saturating_mul(vsize) {
            return Err(Refusal::FeeRateTooHigh {
                fee,
                vsize,
                limit: self.fee.sat_per_vb,
            });
        }
        // SR-41
        if fee > self.fee.absolute_sats {
            return Err(Refusal::FeeTooHigh {
                fee,
                limit: self.fee.absolute_sats,
            });
        }
        // SR-43
        if weight > crate::MAX_WEIGHT {
            return Err(Refusal::WeightTooHigh {
                weight,
                limit: crate::MAX_WEIGHT,
            });
        }

        let mut inputs =
            p.tx.input
                .iter()
                .map(|i| i.previous_output)
                .collect::<Vec<_>>();
        inputs.sort_by_key(crate::state::outpoint_key);
        let plan = self.plan_log(
            LogEntry {
                txid: p.txid,
                time: effective,
                other_sats: c.other_total,
                allowlisted_sats: c.allowlisted_total,
                fee_sats: fee,
                inputs,
            },
            effective,
        );
        let s = sums(plan.log.values(), effective);
        // Signing the same transaction again needs no room (SR-33). A
        // transaction whose only output is change is checked like any
        // other: nothing but its fee counts (SR-25).
        let checked = !plan.already_counted;
        let fit = |v: u128| -> Result<u64, Refusal> {
            match u64::try_from(v) {
                Ok(v) => Ok(v),
                Err(_) if checked => Err(Refusal::TotalsOverflow),
                Err(_) => Ok(u64::MAX),
            }
        };
        let other_day = fit(s.other_day)?;
        let other_week = fit(s.other_week)?;
        let allowlisted_day = fit(s.allowlisted_day)?;
        let allowlisted_week = fit(s.allowlisted_week)?;
        if checked {
            let rows = [
                (Ceiling::OtherPerTx, c.other_total, self.other.per_tx),
                (
                    Ceiling::AllowlistedPerTx,
                    c.allowlisted_total,
                    self.allowlisted.per_tx,
                ),
                (Ceiling::OtherDay, other_day, self.other.per_day),
                (Ceiling::OtherWeek, other_week, self.other.per_week),
                (
                    Ceiling::AllowlistedDay,
                    allowlisted_day,
                    self.allowlisted.per_day,
                ),
                (
                    Ceiling::AllowlistedWeek,
                    allowlisted_week,
                    self.allowlisted.per_week,
                ),
            ];
            for (ceiling, amount, limit) in rows {
                if amount > limit {
                    return Err(Refusal::OverCeiling {
                        ceiling,
                        amount,
                        limit,
                    });
                }
            }
        }
        if !plan.already_counted {
            let inputs_in_log: usize = plan.log.values().map(|e| e.inputs.len()).sum();
            if plan.log.len() > MAX_LOG_ENTRIES || inputs_in_log > MAX_LOG_INPUTS {
                return Err(Refusal::SpendLogFull);
            }
        }
        let remaining = Remaining {
            other_per_tx: self.other.per_tx.saturating_sub(c.other_total),
            other_day: self.other.per_day.saturating_sub(other_day),
            other_week: self.other.per_week.saturating_sub(other_week),
            allowlisted_per_tx: self.allowlisted.per_tx.saturating_sub(c.allowlisted_total),
            allowlisted_day: self.allowlisted.per_day.saturating_sub(allowlisted_day),
            allowlisted_week: self.allowlisted.per_week.saturating_sub(allowlisted_week),
        };
        Ok(Limits {
            fee,
            vsize,
            weight,
            next_log: plan.log,
            already_counted: plan.already_counted,
            remaining,
        })
    }

    /// Read a transaction file and check everything that needs no key
    /// (SR-50). This changes nothing.
    pub fn review(&self, psbt_bytes: &[u8], now: u64) -> Result<Review, Refusal> {
        let parsed = analyze(self, psbt_bytes)?;
        let effective = self.effective_time(now);
        let classified = self.classify(&parsed, effective)?;
        let limits = self.limits(&parsed, &classified, effective)?;
        let outputs = parsed
            .tx
            .output
            .iter()
            .zip(classified.kinds)
            .enumerate()
            .map(|(index, (txout, kind))| ReviewedOutput {
                index,
                amount: txout.value.to_sat(),
                script: txout.script_pubkey.clone(),
                address: Address::from_script(&txout.script_pubkey, self.wallet.network()).ok(),
                kind,
            })
            .collect();
        let change = classified
            .change
            .map(|(pos, index)| (index, parsed.tx.output[pos].value.to_sat()));
        Ok(Review {
            outputs,
            change,
            fee: limits.fee,
            vsize: limits.vsize,
            weight: limits.weight,
            other_total: classified.other_total,
            allowlisted_total: classified.allowlisted_total,
            remaining: limits.remaining,
            clock_behind_by: self.latest_time_seen.checked_sub(now).filter(|d| *d > 0),
            already_counted: limits.already_counted,
            parsed,
        })
    }
}
