//! A fixed-seed property test: long random runs of the owner phone's calls
//! against the real Key app and key 1, with the invariants checked after
//! every step (AS-50 to AS-54, AS-61, AS-64, AS-65).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::OutPoint;
use common::*;
use openbmt_approver::Choice;
use openbmt_owner::{
    CashAmount, Delivery, FeePayer, Options, Owner, OwnerError, RefundPacking, Stage,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            items.get(self.below(items.len() as u64) as usize)
        }
    }
}

fn committed(o: &Owner) -> BTreeMap<u32, u64> {
    // For each sale (by index), what is signed, sent or paid.
    let mut m = BTreeMap::new();
    for r in o.refunds() {
        if matches!(
            r.stage,
            Stage::Signed { .. } | Stage::Sent { .. } | Stage::Paid { .. }
        ) {
            *m.entry(r.sale.index).or_insert(0) += r.sats;
        }
    }
    m
}

/// The invariants that must hold after every step.
fn check(o: &Owner, step: usize) {
    // AS-65: the snapshot restores to itself.
    let bytes = o.snapshot().unwrap();
    let again = restore_owner(&bytes).unwrap_or_else(|e| panic!("step {step}: {e}"));
    assert_eq!(again.snapshot().unwrap(), bytes, "step {step}");
    // AS-52: what is counted against a sale is at most what was received
    // (the run only ever raises the facts), and what is signed, sent or
    // paid is part of it.
    for n in 1..=6 {
        if let Some(f) = o.sale_facts(&sale(n)) {
            assert!(
                o.refunded_sats(&sale(n)) <= f.received_sats,
                "step {step}: sale {n}"
            );
        }
    }
    for (n, sats) in committed(o) {
        assert!(sats <= o.refunded_sats(&sale(n)), "step {step}");
    }
    // AS-64: every signed or sent refund is held by exactly one waiting
    // transaction, and no refund is in two.
    let mut held: BTreeSet<u32> = BTreeSet::new();
    for d in o.outbox() {
        if let Delivery::Transaction {
            refunds, cash_out, ..
        } = &d.what
        {
            for id in refunds.iter().chain(cash_out.iter()) {
                assert!(held.insert(*id), "step {step}: {id} is in two transactions");
            }
        }
    }
    let waiting: BTreeSet<u32> = o
        .refunds()
        .iter()
        .map(|r| (r.id, &r.stage))
        .chain(o.cash_outs().iter().map(|c| (c.id, &c.stage)))
        .filter(|(_, s)| matches!(s, Stage::Signed { .. } | Stage::Sent { .. }))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(waiting, held, "step {step}");
    // AS-50: the bounds.
    assert!(o.refunds().len() <= openbmt_owner::MAX_REFUNDS);
    assert!(o.open_sessions().len() <= openbmt_owner::MAX_OPEN_SESSIONS);
    assert!(o.outbox().len() <= openbmt_owner::MAX_OUTBOX);
}

#[test]
fn as_65_long_random_runs_keep_every_invariant() {
    for seed in [
        0x9e37_79b9_7f4a_7c15u64,
        0x1234_5678_9abc_def1,
        0xdead_beef_cafe_f00d,
    ] {
        run(seed);
    }
}

fn run(seed: u64) {
    let mut rng = Rng(seed);
    let options = Options {
        fee_payer: if rng.below(2) == 0 {
            FeePayer::Shop
        } else {
            FeePayer::Customer
        },
        packing: if rng.below(2) == 0 {
            RefundPacking::OneTransaction
        } else {
            RefundPacking::OneEach
        },
        min_confirmations: 1,
    };
    let mut rig = Rig::with(options, true);
    // The Key app knows the counter, so that batches can be approved.
    ok(rig
        .owner
        .queue_settings(settings_with(1, &[1]), |b| rig.owner_store.put(b)));
    rig.round(&[], 0);
    let mut tag = 1000u32;
    let mut change = 0u32;
    let mut batch_start = 0u32;
    let mut version = 2u32;
    for step in 0..90 {
        match rng.below(11) {
            0 | 1 => {
                let n = rng.below(6) as u32 + 1;
                let counted = rig.owner.refunded_sats(&sale(n));
                let received = counted + 5_000 + rng.below(40_000);
                set_facts(
                    &mut rig.owner,
                    sale(n),
                    if rng.below(4) == 0 {
                        shallow(received)
                    } else {
                        deep(received)
                    },
                );
            }
            2..=4 => {
                let n = rng.below(6) as u32 + 1;
                let sats = 500 + rng.below(20_000);
                let before = rig.owner.refunded_sats(&sale(n));
                let r =
                    rig.owner
                        .queue_refund(sale(n), "Mia", external(n as u8), sats, None, |b| {
                            rig.owner_store.put(b)
                        });
                match r {
                    Ok(_) => assert_eq!(rig.owner.refunded_sats(&sale(n)), before + sats),
                    Err(OwnerError::Refused(_)) => {
                        assert_eq!(rig.owner.refunded_sats(&sale(n)), before)
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            5..=7 => {
                let coins: Vec<_> = (0..3)
                    .map(|i| {
                        tag += 1;
                        coin(tag, tag % 50 + i, 200_000)
                    })
                    .collect();
                let built = rig.build(&coins, change);
                change += 8;
                let seen: BTreeSet<OutPoint> = BTreeSet::new();
                let mut seen = seen;
                for tx in spends_of(&built.request) {
                    for i in &tx.input {
                        assert!(seen.insert(i.previous_output), "step {step}: coin reused");
                    }
                }
                let flip = rng.next();
                let response = rig.answer(&built.request, |l| {
                    if (flip >> (l.item % 60)) & 7 == 0 {
                        Choice::Decline("random".to_string())
                    } else {
                        Choice::Approve
                    }
                });
                ok(rig.take(&response));
            }
            8 => {
                let txids: Vec<_> = rig
                    .owner
                    .outbox()
                    .iter()
                    .filter_map(|d| match &d.what {
                        Delivery::Transaction { txid, .. } => Some(*txid),
                        _ => None,
                    })
                    .collect();
                if let Some(txid) = rng.pick(&txids).copied() {
                    match rng.below(3) {
                        0 => ok(rig.owner.mark_broadcast(&txid, |b| rig.owner_store.put(b))),
                        1 => ok(rig.owner.mark_confirmed(&txid, |b| rig.owner_store.put(b))),
                        _ => ok(rig.owner.mark_dropped(&txid, |b| rig.owner_store.put(b))),
                    }
                }
            }
            9 => {
                let returned: Vec<u32> = rig
                    .owner
                    .refunds()
                    .iter()
                    .filter(|r| matches!(r.stage, Stage::Returned { .. }))
                    .map(|r| r.id)
                    .collect();
                if let Some(id) = rng.pick(&returned).copied() {
                    if rng.below(2) == 0 {
                        ok(rig.owner.requeue(id, |b| rig.owner_store.put(b)));
                    } else {
                        // An entry that kept outpoints cannot be cancelled
                        // (AS-68).
                        let kept = rig
                            .owner
                            .refunds()
                            .iter()
                            .any(|r| r.id == id && !r.conflict.is_empty());
                        let r = rig.owner.cancel(id, |b| rig.owner_store.put(b));
                        assert_eq!(r.is_err(), kept);
                    }
                }
                ok(rig
                    .owner
                    .archive_paid(&std::collections::BTreeSet::new(), |b| {
                        rig.owner_store.put(b)
                    }));
            }
            _ => match rng.below(3) {
                0 => {
                    let _ = rig.owner.queue_cash_out(
                        external(9),
                        if rng.below(2) == 0 {
                            CashAmount::All
                        } else {
                            CashAmount::Sats(5_000 + rng.below(30_000))
                        },
                        "Bank",
                        |b| rig.owner_store.put(b),
                    );
                }
                1 => {
                    let _ = rig
                        .owner
                        .queue_batch(batch(1, batch_start, 3), |b| rig.owner_store.put(b));
                    batch_start += 3;
                }
                _ => {
                    // A failing store changes nothing.
                    let before = rig.owner.snapshot().unwrap();
                    let failing = Store::default();
                    failing.fail_next(true);
                    version += 1;
                    let r = rig
                        .owner
                        .queue_settings(settings_with(version, &[1]), |b| failing.put(b));
                    assert!(matches!(r, Err(OwnerError::Store(_))));
                    assert_eq!(rig.owner.snapshot().unwrap(), before, "step {step}");
                }
            },
        }
        check(&rig.owner, step);
        // Now and then both sides are restarted from what they stored.
        if step % 17 == 16 {
            rig.owner = restore_owner(&rig.owner.snapshot().unwrap()).unwrap();
            check(&rig.owner, step);
        }
    }
}

fn spends_of(request: &[u8]) -> Vec<bitcoin::Transaction> {
    let request = openbmt_session::Request::from_bytes(request).unwrap();
    request
        .items()
        .iter()
        .filter_map(|i| match &i.item {
            openbmt_session::Item::Spend { psbt, .. } => {
                Some(bitcoin::psbt::Psbt::deserialize(psbt).unwrap().unsigned_tx)
            }
            _ => None,
        })
        .collect()
}
