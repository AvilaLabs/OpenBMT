//! Building a session (AS-53 to AS-61).

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::consensus::deserialize;
use bitcoin::psbt::Psbt;
use bitcoin::{OutPoint, ScriptBuf};
use openbmt_core::identity::tagged_hash;
use openbmt_core::wallet::WalletDescription;
use openbmt_core::wire::DecodeError;
use openbmt_session::{Item, Label, OutputLabel, Request, RequestItem};
use openbmt_spend::{dust_limit, plan, ChangePosition, Coin, Plan, PlanOutput, SpendRequest};

use crate::error::{OwnerError, Refusal};
use crate::say::{RESERVED_NO_ELIGIBLE, RESERVED_PARTIAL, RESERVED_REQUIRED_INPUTS};
use crate::state::{Frozen, OpenSession, Owner, State};
use crate::types::{
    Built, CashAmount, Delivery, FeePayer, Options, Refund, RefundPacking, Skipped, Stage,
};
use crate::{MAX_ITEMS_PER_SESSION, MAX_OPEN_SESSIONS, MAX_REFUNDS_PER_TX, MAX_TXS_PER_SESSION};

/// AS-61: the tag of the hash that gives each transaction of a session its
/// own seed for the change position.
const TAG_CHANGE_SEED: &str = "OpenBMT/owner-change-seed/v1";

/// AS-56: rounds of looking for the fee the shares must cover.
const FEE_ROUNDS: usize = 8;
/// AS-56: rounds after which the shares cover the largest fee seen.
const FIXED_ROUNDS: usize = 4;

/// AS-53, AS-52: a refund is ready when it is queued, the sale's facts say
/// it reached the refund depth, and they cover everything counted against
/// the sale.
pub(crate) fn is_ready(st: &State, r: &Refund) -> bool {
    if r.stage != Stage::Queued {
        return false;
    }
    match st.sales.get(&r.sale) {
        Some(entry) => {
            entry.facts.reached_refund_depth && entry.facts.received_sats >= st.counted(&r.sale)
        }
        None => false,
    }
}

/// A refund transaction worked out (AS-56).
struct PlannedTx {
    plan: Plan,
    /// Positions in the group, in output order.
    included: Vec<usize>,
    /// Positions held back, with the reason.
    held: Vec<(usize, String)>,
}

/// What every transaction of a session shares: the fee rate, the lock
/// height and the seed of the change position (AS-61).
#[derive(Clone, Copy)]
struct Shape {
    fee_rate: u64,
    lock_height: u32,
    change_seed: [u8; 32],
}

fn spend_request(
    payments: Vec<(ScriptBuf, u64)>,
    shape: Shape,
    change_index: u32,
    options: Options,
    send_all: bool,
    must: &[OutPoint],
) -> SpendRequest {
    SpendRequest {
        payments,
        fee_rate: shape.fee_rate,
        change_index,
        min_confirmations: options.min_confirmations,
        send_all,
        must_spend: must.to_vec(),
        lock_height: shape.lock_height,
        change_position: ChangePosition::Seed(shape.change_seed),
    }
}

/// The positions of the payments among the outputs of `plan`, in the order
/// of the payments, and the position of the change (AS-61).
fn output_positions(plan: &Plan) -> (Vec<u32>, Option<u32>) {
    let mut payments = Vec::new();
    let mut change = None;
    for (at, out) in plan.outputs().iter().enumerate() {
        let at = u32::try_from(at).unwrap_or(u32::MAX);
        match out {
            PlanOutput::Payment(_) => payments.push(at),
            PlanOutput::Change(_) => change = Some(at),
        }
    }
    (payments, change)
}

/// AS-55, AS-56: one transaction for `group`.
fn plan_refunds(
    wallet: &WalletDescription,
    pool: &[Coin],
    group: &[Refund],
    shape: Shape,
    change_index: u32,
    options: Options,
    must: &[OutPoint],
) -> Result<PlannedTx, String> {
    let mut included: Vec<usize> = (0..group.len()).collect();
    let mut held: Vec<(usize, String)> = Vec::new();
    if options.fee_payer == FeePayer::Shop {
        let payments = group.iter().map(|r| (r.script.clone(), r.sats)).collect();
        let request = spend_request(payments, shape, change_index, options, false, must);
        let plan = plan(wallet, pool, &request).map_err(|e| e.to_string())?;
        return Ok(PlannedTx {
            plan,
            included,
            held,
        });
    }
    'again: loop {
        let n = u64::try_from(included.len()).unwrap_or(u64::MAX);
        if n == 0 {
            return Err(
                "AS-56: every refund is below the dust limit once it pays its share of the fee"
                    .to_string(),
            );
        }
        let mut fee_used: u64 = 0;
        let mut max_seen: u64 = 0;
        for round in 0..FEE_ROUNDS {
            let base = fee_used / n;
            let rem = usize::try_from(fee_used % n).unwrap_or(0);
            let mut payments: Vec<(ScriptBuf, u64)> = Vec::with_capacity(included.len());
            let mut low: Vec<usize> = Vec::new();
            for (pos, &gi) in included.iter().enumerate() {
                let Some(r) = group.get(gi) else { continue };
                let share = base + u64::from(pos < rem);
                let out = r.sats.saturating_sub(share);
                let dust = dust_limit(&r.script).unwrap_or(u64::MAX);
                if r.sats < share || out < dust {
                    low.push(gi);
                }
                payments.push((r.script.clone(), out));
            }
            if !low.is_empty() {
                for gi in low {
                    held.push((
                        gi,
                        "AS-56: its amount after its share of the fee is below the dust limit"
                            .to_string(),
                    ));
                    included.retain(|x| *x != gi);
                }
                continue 'again;
            }
            let request = spend_request(payments, shape, change_index, options, false, must);
            let planned = plan(wallet, pool, &request).map_err(|e| e.to_string())?;
            let fee = planned.fee_sats();
            max_seen = max_seen.max(fee);
            if fee == fee_used || (round >= FIXED_ROUNDS && fee <= fee_used) {
                return Ok(PlannedTx {
                    plan: planned,
                    included,
                    held,
                });
            }
            fee_used = if round + 1 >= FIXED_ROUNDS {
                max_seen
            } else {
                fee
            };
        }
        return Err("AS-56: the fee did not settle".to_string());
    }
}

const BLOCKED: &str = "AS-68: the outpoints its dropped transaction spent are used by another \
                       transaction of this session; it waits for the next";

/// AS-68: one of `must` was among the coins handed in and an earlier
/// transaction of this session has taken it. The new transaction could then
/// not spend it, so it would not conflict with the dropped transaction that
/// did, and both could confirm.
fn blocked(original: &[OutPoint], pool: &[Coin], must: &[OutPoint]) -> bool {
    must.iter()
        .any(|o| original.contains(o) && !pool.iter().any(|c| c.outpoint == *o))
}

/// The coins left after a plan used some.
fn take_coins(pool: &mut Vec<Coin>, plan: &Plan) {
    let used: Vec<OutPoint> = plan.inputs().iter().map(|i| i.coin.outpoint).collect();
    pool.retain(|c| !used.contains(&c.outpoint));
}

/// Inputs already committed to a transaction the owner has handed out or
/// signed. Node callers also remove these through their coin view, but the
/// owner library accepts caller-supplied coins and must be safe when that
/// list is stale or shared across open sessions.
fn reserved_outpoints(state: &State) -> Result<BTreeSet<OutPoint>, Refusal> {
    let mut reserved = BTreeSet::new();
    for session in &state.sessions {
        let request = Request::from_bytes(&session.request).map_err(Refusal::Frame)?;
        for item in request.items() {
            if let Item::Spend { psbt, .. } = &item.item {
                let psbt = Psbt::deserialize(psbt).map_err(|_| {
                    Refusal::Frame(DecodeError::NonCanonical(
                        "an open spend's transaction does not parse",
                    ))
                })?;
                reserved.extend(
                    psbt.unsigned_tx
                        .input
                        .into_iter()
                        .map(|input| input.previous_output),
                );
            }
        }
    }
    for deliverable in &state.outbox {
        if let Delivery::Transaction { tx, .. } = &deliverable.what {
            let tx = deserialize::<bitcoin::Transaction>(tx).map_err(|_| {
                Refusal::Frame(DecodeError::NonCanonical(
                    "a waiting spend's transaction does not parse",
                ))
            })?;
            reserved.extend(tx.input.into_iter().map(|input| input.previous_output));
        }
    }
    Ok(reserved)
}

/// Explain planner failures when eligible supplied coins were withheld for
/// another approval. Other planner failures keep their original detail.
fn reservation_reason(
    why: &str,
    reserved_eligible: bool,
    available_eligible: bool,
) -> Option<&'static str> {
    if !reserved_eligible {
        return None;
    }
    if why.starts_with("SP-10:") && !available_eligible {
        Some(RESERVED_NO_ELIGIBLE)
    } else if why.contains("the eligible coins hold") && available_eligible {
        Some(RESERVED_PARTIAL)
    } else {
        None
    }
}

fn thirds(random: &[u8; 96]) -> ([u8; 32], [u8; 32], [u8; 32]) {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    let mut c = [0u8; 32];
    for (dst, src) in a.iter_mut().zip(random.iter()) {
        *dst = *src;
    }
    for (dst, src) in b.iter_mut().zip(random.iter().skip(32)) {
        *dst = *src;
    }
    for (dst, src) in c.iter_mut().zip(random.iter().skip(64)) {
        *dst = *src;
    }
    (a, b, c)
}

/// AS-61: the seed of the change position of the transaction in place `k`
/// of the session.
fn seed_for(change_seed: &[u8; 32], k: u32) -> [u8; 32] {
    let mut msg = change_seed.to_vec();
    msg.extend_from_slice(&k.to_le_bytes());
    tagged_hash(TAG_CHANGE_SEED, &msg)
}

impl Owner {
    /// AS-61: freeze what is ready into one session.
    ///
    /// `coins` are the wallet's coins and `fee_rate` is in satoshis per 1000
    /// virtual bytes, both the caller's. Each transaction takes a change
    /// index from `change_index` upwards. `lock_height` is the chain's tip
    /// height as the caller knows it: every transaction's lock time (SP-37).
    /// `random` is 96 bytes from the caller's random source: the session ID,
    /// the health check's number and the seed of the change positions, each
    /// transaction taking its own seed from it. The session and what it
    /// froze are stored before the request bytes are returned. An entry
    /// that does not fit stays queued and is reported in
    /// [`Built::skipped`].
    #[allow(clippy::too_many_arguments)]
    pub fn build_request<E>(
        &mut self,
        coins: &[Coin],
        fee_rate: u64,
        change_index: u32,
        lock_height: u32,
        now: u64,
        random: &[u8; 96],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Built, OwnerError<E>> {
        if fee_rate == 0 {
            return Err(Refusal::ZeroFeeRate.into());
        }
        let mut next = self.state.clone();
        if next.sessions.len() >= MAX_OPEN_SESSIONS {
            return Err(Refusal::Full("the list of open sessions").into());
        }
        let (session_id, nonce, change_seed) = thirds(random);
        if next.sessions.iter().any(|s| s.id == session_id) {
            return Err(Refusal::SessionIdUsed.into());
        }
        let options = next.options;
        let mut items: Vec<(Item, Frozen)> = Vec::new();
        let mut skipped: Vec<Skipped> = Vec::new();
        let original: Vec<OutPoint> = coins.iter().map(|c| c.outpoint).collect();
        let reserved = reserved_outpoints(&next)?;
        let mut pool: Vec<Coin> = coins
            .iter()
            .filter(|coin| !reserved.contains(&coin.outpoint))
            .cloned()
            .collect();
        let reserved_eligible = coins.iter().any(|coin| {
            reserved.contains(&coin.outpoint) && coin.confirmations >= options.min_confirmations
        });
        let available_eligible = pool
            .iter()
            .any(|coin| coin.confirmations >= options.min_confirmations);
        let mut txs: usize = 0;
        let mut k: u32 = 0;
        // One place is kept for the health check (AS-60).
        let room = |items: &Vec<(Item, Frozen)>| items.len() + 1 < MAX_ITEMS_PER_SESSION;
        let full = "AS-50: the session holds no more items";
        // The refunds (AS-53, AS-55).
        let ready: Vec<Refund> = next
            .refunds
            .iter()
            .filter(|r| is_ready(&next, r))
            .cloned()
            .collect();
        // AS-68: refunds that were in a dropped transaction are planned first,
        // each set of them together in one transaction of its own.
        let mut constrained: BTreeMap<Vec<OutPoint>, Vec<Refund>> = BTreeMap::new();
        let mut free: Vec<Refund> = Vec::new();
        for r in ready {
            if r.conflict.is_empty() {
                free.push(r);
            } else {
                constrained.entry(r.conflict.clone()).or_default().push(r);
            }
        }
        let mut groups: Vec<Vec<Refund>> = Vec::new();
        for (_, set) in constrained {
            groups.extend(set.chunks(MAX_REFUNDS_PER_TX).map(<[Refund]>::to_vec));
        }
        match options.packing {
            RefundPacking::OneEach => groups.extend(free.iter().map(|r| vec![r.clone()])),
            RefundPacking::OneTransaction => {
                groups.extend(free.chunks(MAX_REFUNDS_PER_TX).map(<[Refund]>::to_vec));
            }
        }
        for group in groups {
            let skip_all = |why: String, skipped: &mut Vec<Skipped>| {
                for r in &group {
                    skipped.push(Skipped {
                        id: r.id,
                        reason: why.clone(),
                    });
                }
            };
            if txs >= MAX_TXS_PER_SESSION || !room(&items) {
                skip_all(full.to_string(), &mut skipped);
                continue;
            }
            let Some(index) = change_index.checked_add(k) else {
                skip_all("SP-21: no change index is left".to_string(), &mut skipped);
                continue;
            };
            let must: Vec<OutPoint> = group
                .first()
                .map(|r| r.conflict.clone())
                .unwrap_or_default();
            if blocked(&original, &pool, &must) {
                let reason = if must.iter().any(|outpoint| reserved.contains(outpoint)) {
                    RESERVED_REQUIRED_INPUTS
                } else {
                    BLOCKED
                };
                skip_all(reason.to_string(), &mut skipped);
                continue;
            }
            let shape = Shape {
                fee_rate,
                lock_height,
                change_seed: seed_for(&change_seed, k),
            };
            let planned =
                match plan_refunds(&self.wallet, &pool, &group, shape, index, options, &must) {
                    Ok(p) => p,
                    Err(why) => {
                        skip_all(
                            reservation_reason(&why, reserved_eligible, available_eligible)
                                .unwrap_or(&why)
                                .to_string(),
                            &mut skipped,
                        );
                        continue;
                    }
                };
            let psbt = match planned.plan.to_psbt(&self.wallet) {
                Ok(p) => p.serialize(),
                Err(e) => {
                    skip_all(e.to_string(), &mut skipped);
                    continue;
                }
            };
            for (gi, why) in &planned.held {
                if let Some(r) = group.get(*gi) {
                    skipped.push(Skipped {
                        id: r.id,
                        reason: why.clone(),
                    });
                }
            }
            let (places, change_position) = output_positions(&planned.plan);
            let mut labels: Vec<OutputLabel> = Vec::new();
            let mut ids: Vec<u32> = Vec::new();
            for (pos, gi) in planned.included.iter().enumerate() {
                let Some(r) = group.get(*gi) else { continue };
                labels.push(OutputLabel {
                    output: places.get(pos).copied().unwrap_or(u32::MAX),
                    label: Label::Refund {
                        counter: r.sale.counter,
                        sale_index: r.sale.index,
                        cashier: r.cashier.clone(),
                        shop_value: r.shop_value,
                    },
                });
                ids.push(r.id);
            }
            let payments: Vec<(ScriptBuf, u64)> = planned
                .plan
                .payments()
                .iter()
                .map(|p| (p.script.clone(), p.sats))
                .collect();
            if let Some(at) = change_position {
                labels.push(OutputLabel {
                    output: at,
                    label: Label::Change,
                });
            }
            labels.sort_by_key(|l| l.output);
            take_coins(&mut pool, &planned.plan);
            items.push((
                Item::Spend { psbt, labels },
                Frozen::Spend {
                    txid: planned.plan.txid(),
                    refunds: ids,
                    cash_out: None,
                    payments,
                    change_index: index,
                    change_position,
                },
            ));
            txs += 1;
            k += 1;
        }

        // The cash-outs (AS-57): with the coins the refunds left.
        let cash: Vec<_> = next
            .cash_outs
            .iter()
            .filter(|c| c.stage == Stage::Queued)
            .cloned()
            .collect();
        for c in cash {
            let skip = |why: String, skipped: &mut Vec<Skipped>| {
                skipped.push(Skipped {
                    id: c.id,
                    reason: why,
                });
            };
            if txs >= MAX_TXS_PER_SESSION || !room(&items) {
                skip(full.to_string(), &mut skipped);
                continue;
            }
            let Some(index) = change_index.checked_add(k) else {
                skip("SP-21: no change index is left".to_string(), &mut skipped);
                continue;
            };
            let (amount, send_all) = match c.amount {
                CashAmount::Sats(n) => (n, false),
                CashAmount::All => (0, true),
            };
            if blocked(&original, &pool, &c.conflict) {
                let reason = if c
                    .conflict
                    .iter()
                    .any(|outpoint| reserved.contains(outpoint))
                {
                    RESERVED_REQUIRED_INPUTS
                } else {
                    BLOCKED
                };
                skip(reason.to_string(), &mut skipped);
                continue;
            }
            let shape = Shape {
                fee_rate,
                lock_height,
                change_seed: seed_for(&change_seed, k),
            };
            let request = spend_request(
                vec![(c.script.clone(), amount)],
                shape,
                index,
                options,
                send_all,
                &c.conflict,
            );
            let planned = match plan(&self.wallet, &pool, &request) {
                Ok(p) => p,
                Err(e) => {
                    let why = e.to_string();
                    skip(
                        reservation_reason(&why, reserved_eligible, available_eligible)
                            .unwrap_or(&why)
                            .to_string(),
                        &mut skipped,
                    );
                    continue;
                }
            };
            let psbt = match planned.to_psbt(&self.wallet) {
                Ok(p) => p.serialize(),
                Err(e) => {
                    skip(e.to_string(), &mut skipped);
                    continue;
                }
            };
            let (places, change_position) = output_positions(&planned);
            let mut labels = vec![OutputLabel {
                output: places.first().copied().unwrap_or(u32::MAX),
                label: Label::CashOut {
                    destination: c.label.clone(),
                },
            }];
            let payments: Vec<(ScriptBuf, u64)> = planned
                .payments()
                .iter()
                .map(|p| (p.script.clone(), p.sats))
                .collect();
            if let Some(at) = change_position {
                labels.push(OutputLabel {
                    output: at,
                    label: Label::Change,
                });
            }
            labels.sort_by_key(|l| l.output);
            take_coins(&mut pool, &planned);
            items.push((
                Item::Spend { psbt, labels },
                Frozen::Spend {
                    txid: planned.txid(),
                    refunds: Vec::new(),
                    cash_out: Some(c.id),
                    payments,
                    change_index: index,
                    change_position,
                },
            ));
            txs += 1;
            k += 1;
        }

        // Pairings, settings, batches (AS-58), in that order.
        let in_force = next.known.as_ref().map_or(0, |s| s.settings.version);
        let pairings: Vec<_> = next
            .pairings
            .iter()
            .filter(|p| p.stage == Stage::Queued)
            .cloned()
            .collect();
        for p in pairings {
            if !room(&items) {
                skipped.push(Skipped {
                    id: p.id,
                    reason: full.to_string(),
                });
                continue;
            }
            items.push((
                Item::Pairing {
                    request: p.request.clone(),
                    settings_version: in_force,
                },
                Frozen::Pairing(p.id),
            ));
        }
        let settings: Vec<_> = next
            .settings
            .iter()
            .filter(|s| s.stage == Stage::Queued)
            .cloned()
            .collect();
        for s in settings {
            if !room(&items) {
                skipped.push(Skipped {
                    id: s.id,
                    reason: full.to_string(),
                });
                continue;
            }
            items.push((
                Item::Settings {
                    signed: s.signed.clone(),
                },
                Frozen::Settings(s.id),
            ));
        }
        let batches: Vec<_> = next
            .batches
            .iter()
            .filter(|b| b.stage == Stage::Queued)
            .cloned()
            .collect();
        for b in batches {
            if !room(&items) {
                skipped.push(Skipped {
                    id: b.id,
                    reason: full.to_string(),
                });
                continue;
            }
            items.push((
                Item::BatchRefill {
                    batch: b.batch.clone(),
                    owner_sig: b.owner_sig,
                },
                Frozen::Batch(b.id),
            ));
        }

        // The health check is last (AS-60).
        items.push((Item::HealthCheck { nonce }, Frozen::Health));

        let mut request_items: Vec<RequestItem> = Vec::with_capacity(items.len());
        let mut frozen: Vec<(u32, Frozen)> = Vec::with_capacity(items.len());
        for (n, (item, f)) in items.into_iter().enumerate() {
            let number = u32::try_from(n).unwrap_or(u32::MAX);
            request_items.push(RequestItem { number, item });
            frozen.push((number, f));
        }
        let request = Request::new(&self.identity, session_id, self.wallet_id, request_items)
            .map_err(Refusal::Encode)?;

        for (_, f) in &frozen {
            let ids: Vec<u32> = match f {
                Frozen::Spend {
                    refunds, cash_out, ..
                } => refunds.iter().copied().chain(*cash_out).collect(),
                Frozen::Batch(n) | Frozen::Settings(n) | Frozen::Pairing(n) => vec![*n],
                Frozen::Health => Vec::new(),
            };
            for id in ids {
                if let Some(stage) = next.stage_mut(id) {
                    *stage = Stage::InSession {
                        session: session_id,
                    };
                }
            }
        }
        let bytes = request.as_bytes().to_vec();
        next.sessions.push(OpenSession {
            id: session_id,
            created_at: now,
            request: bytes.clone(),
            frozen,
        });
        self.commit(next, persist)?;
        Ok(Built {
            session_id,
            request: bytes,
            skipped,
        })
    }
}
