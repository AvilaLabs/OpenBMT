//! Section 22: a sale paid over Lightning through a swap provider
//! (CR-221, CR-224). The setting, the driver that follows the swap, the
//! claim and the transport (CR-218 to CR-220, CR-222, CR-223, CR-225) belong
//! to the settings, the node and the app, and are tested there.

use openbmt_core::policy::CheckMode;
use openbmt_watch::{SaleStatus, SaleWatch};

use super::kit::*;
use crate::refund::{RefundAsk, RefundRequest};
use crate::{
    Actor, Event, LightningRefused, Opened, RefundRefused, Report, Role, Screen, State, SwapPaid,
    Why,
};

fn paid(invoice_sats: u64) -> SwapPaid {
    SwapPaid {
        payment_hash: [9; 32],
        invoice_sats,
        received_sats: invoice_sats - 700,
    }
}

impl World {
    fn facts(&self) -> crate::Facts<'_, TestChain> {
        crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        }
    }

    /// The status of the provider's lockup, as the watch of the node keeps
    /// it: seen by `groups` groups of peers, `secs` seconds ago.
    fn lockup_status(&mut self, groups: u8, secs: u64) -> SaleStatus {
        let lockup = p2wsh(0x77);
        let mut watch = SaleWatch::new(lockup.clone(), 4_300, 0);
        let tx = spend(0x77, &[(&lockup, 4_300)]);
        for n in 20..20 + groups {
            let Source::Peer(addr) = peer(n) else {
                unreachable!()
            };
            watch
                .sighting(&bytes(&tx), Source::Peer(addr), self.now)
                .unwrap();
        }
        self.tick(secs);
        watch.status(&self.chain, self.now)
    }

    fn swap_raw(
        &mut self,
        index: u32,
        paid: SwapPaid,
    ) -> Result<Option<Screen>, LightningRefused<&'static str>> {
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c.swap_paid(index, paid, None, &facts, &mut self.m)
    }

    fn own_node_paid(&mut self, index: u32, paid: crate::LightningPaid) {
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c
            .lightning_paid(index, paid, &facts, &mut self.m)
            .unwrap();
    }

    fn swap_decide(&self, o: &Opened, status: &SaleStatus) -> Option<CheckMode> {
        self.c
            .swap_decide(o.index, o.needs_sats, status, &self.facts())
            .map(|(_, mode)| mode)
    }

    /// The node tells the claim's confirmations.
    fn claim_depth(&mut self, o: &Opened, confirmations: u32) -> Vec<Report> {
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c
            .swap_claim_depth(o.index, confirmations, &facts, &mut self.m)
            .unwrap()
    }

    /// A claim that pays the sale `sats`, known to the counter as a claim of
    /// the swap, in a block of the chain.
    fn mine_claim(&mut self, o: &Opened, sats: u64) -> bitcoin::Transaction {
        let claim = self.pay(o, sats);
        let mut ids = self.c.swap_claim_ids_of(o.index);
        ids.push(claim.compute_txid());
        self.c.set_swap_claims(o.index, &ids);
        self.confirm(std::slice::from_ref(&claim), 100);
        claim
    }

    fn spent(&mut self, o: &Opened, spent: bool) -> Vec<Report> {
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c
            .swap_lockup_spent(o.index, spent, &facts, &mut self.m)
            .unwrap()
    }

    /// A payment taken at the claim's refund depth: the sale is final.
    fn swap_paid_final(&mut self, o: &Opened) -> Screen {
        self.claim_depth(o, 6);
        self.swap_paid(o, None).unwrap().unwrap()
    }

    fn swap_paid(
        &mut self,
        o: &Opened,
        status: Option<&SaleStatus>,
    ) -> Result<Option<Screen>, LightningRefused<&'static str>> {
        let instant =
            status.and_then(|st| self.c.swap_decide(o.index, o.needs_sats, st, &self.facts()));
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c
            .swap_paid(o.index, paid(o.needs_sats), instant, &facts, &mut self.m)
    }
}

use openbmt_watch::Source;

#[test]
fn cr_221_a_lockup_in_the_mempool_within_the_limits_is_paid_and_counted_like_an_instant_sale() {
    let mut w = World::new();
    let o = w.open_five();
    w.tick(10);
    // Seen by one group only: not enough to accept at once.
    let one = w.lockup_status(1, 10);
    assert_eq!(w.swap_decide(&o, &one), None);
    // Seen by three groups for long enough: the policy of CR-13 says yes.
    let st = w.lockup_status(3, 9);
    assert_eq!(w.swap_decide(&o, &st), Some(CheckMode::Light));
    let screen = w.swap_paid(&o, Some(&st)).unwrap().unwrap();
    // Paid at once, as an instant on-chain sale is, until the claim is in a
    // block (CR-30).
    assert_eq!(screen.state, State::PaidInstant);
    assert!(screen.reports.contains(&Report::PosPaid { index: o.index }));
    // The amount at risk is counted in the ledger, as for any instant sale:
    // what the claim pays the shop (4,300), not what the customer paid.
    assert_eq!(w.c.exposure_outstanding(), 4_300);
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.paid_via_swap() && !sale.books_final() && sale.is_live());
    // What the sale keeps is what the claim pays its address.
    assert_eq!(sale.lightning().unwrap().sats(), 4_300);
    // The records: the payment and the POS.
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::PaidByLightning {
            amount_msat: 4_300_000,
            ..
        }
    )));
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::PaidAtOnce {
            at_risk_sats: 4_300,
            ..
        }
    )));
    // The lockup in a block does not release the exposure: the claim can
    // still lose to the provider's refund.
    assert_eq!(w.judge(o.index).state, State::PaidInstant);
    assert_eq!(w.c.exposure_outstanding(), 4_300);
    // The claim's first confirmation releases it, once, and at the shop's
    // depth (1) the sale is confirmed.
    let reports = w.claim_depth(&o, 1);
    assert!(reports.is_empty(), "{reports:?}");
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    w.claim_depth(&o, 1);
    // At the refund depth (6) the sale is final and the books are told.
    let reports = w.claim_depth(&o, 6);
    assert!(
        reports.iter().any(|r| matches!(
            r,
            Report::BooksFinal {
                received_sats: 4_300,
                ..
            }
        )),
        "{reports:?}"
    );
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.books_final() && !sale.is_live());
    assert_eq!(w.judge(o.index).state, State::Final);
    // A restart reads the sale back as a swap payment, and the node's first
    // word after it changes nothing.
    w.restart();
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.paid_via_swap() && sale.books_final());
    assert_eq!(w.judge(o.index).state, State::Final);
}

#[test]
fn cr_221_the_exposure_of_a_swap_payment_stays_until_the_claim_has_a_confirmation() {
    let mut w = World::new();
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    w.swap_paid(&o, Some(&st)).unwrap().unwrap();
    // A restart with the sale accepted at once and the claim not yet in a
    // block: still counted, and the sale still paid at once.
    w.restart();
    assert_eq!(w.c.exposure_outstanding(), 4_300);
    assert_eq!(w.judge(o.index).state, State::PaidInstant);
    // The claim in a block, then out of it again: not released twice, and
    // the sale that was accepted at once is taken back until the claim is
    // back, with nothing counted again (CR-221).
    w.claim_depth(&o, 1);
    assert_eq!(w.c.exposure_outstanding(), 0);
    let reports = w.claim_depth(&o, 0);
    assert!(
        reports.contains(&Report::PosUnpaid { index: o.index }),
        "{reports:?}"
    );
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Reopened)
    );
    assert_eq!(w.c.exposure_outstanding(), 0);
    // Back in a block: paid again, and the POS is told so.
    let reports = w.claim_depth(&o, 1);
    assert!(
        reports.contains(&Report::PosPaid { index: o.index }),
        "{reports:?}"
    );
    assert_eq!(w.judge(o.index).state, State::Confirmed);
}

#[test]
fn cr_221_a_sale_above_the_instant_limit_or_a_full_ledger_waits_for_the_claim() {
    let mut w = World::new();
    // $20 is 20,000 sats: above the $10 limit of the light check.
    let big = w.open(2_000);
    let st = w.lockup_status(3, 9);
    assert_eq!(w.swap_decide(&big, &st), None);
    // With nothing decided at once the call takes the payment as one whose
    // claim is not yet in a block: held, and not paid, nothing counted.
    let screen = w.swap_paid(&big, None).unwrap().unwrap();
    assert_ne!(screen.state, State::PaidInstant);
    assert!(!screen.state.is_paid(), "{screen:?}");
    assert!(!screen
        .reports
        .contains(&Report::PosPaid { index: big.index }));
    assert_eq!(w.c.exposure_outstanding(), 0);
    // The claim at the shop's depth: confirmed, and told to the POS.
    let reports = w.claim_depth(&big, 1);
    assert!(
        reports.contains(&Report::PosPaid { index: big.index }),
        "{reports:?}"
    );
    assert_eq!(w.judge(big.index).state, State::Confirmed);
    assert!(w.c.sale(big.index).unwrap().paid_via_swap());
    // A slice that is full: five $5 sales are 25,000 sats of which the claims
    // pay 4,300 each, the slice is 50,000, so eleven of them fill it (47,300)
    // and the twelfth waits.
    let mut w = World::new();
    for _ in 0..11 {
        let o = w.open_five();
        let st = w.lockup_status(3, 9);
        let screen = w.swap_paid(&o, Some(&st)).unwrap().unwrap();
        assert_eq!(screen.state, State::PaidInstant);
    }
    assert_eq!(w.c.exposure_outstanding(), 47_300);
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    assert_eq!(w.swap_decide(&o, &st), None);
}

#[test]
fn cr_221_a_payment_is_refused_like_any_lightning_payment_and_the_claim_is_not_late() {
    let mut w = World::new();
    let o = w.open_five();
    // For less than the sale asks.
    let small = SwapPaid {
        payment_hash: [9; 32],
        invoice_sats: 4_999,
        received_sats: 4_000,
    };
    let err = w.swap_raw(o.index, small).unwrap_err();
    assert!(matches!(
        err,
        LightningRefused::TooLittle { least: 5_000, .. }
    ));
    assert!(matches!(
        w.swap_paid(
            &crate::Opened {
                index: 99,
                ..o.clone()
            },
            None
        ),
        Err(LightningRefused::UnknownSale(99))
    ));
    // Paid; the claim arrives at the sale's own address: not a late payment.
    w.swap_paid_final(&o);
    let claim = w.pay(&o, 4_300);
    let seen = w.see(&claim, 1);
    assert!(seen.reports.is_empty(), "{:?}", seen.reports);
    let block = w.confirm(&[claim], 100);
    let _ = block;
    assert!(w.c.sale(o.index).unwrap().late_payments().is_empty());
}

#[test]
fn cr_221_a_sale_whose_lockup_is_on_the_way_does_not_expire() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.set_swap_pending(o.index, true);
    w.tick(10_000);
    assert_eq!(w.judge(o.index).state, State::NotPaidYet);
    w.c.set_swap_pending(o.index, false);
    assert_eq!(w.judge(o.index).state, State::Expired);
}

#[test]
fn cr_221_a_lockup_replaced_after_the_sale_was_paid_is_a_cancel() {
    let mut w = World::new();
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    w.swap_paid(&o, Some(&st)).unwrap().unwrap();
    let reports = w.c.swap_cancelled(o.index, w.now, &mut w.m).unwrap();
    assert!(reports
        .iter()
        .any(|r| matches!(r, Report::Alarm { index } if *index == o.index)));
    assert!(reports
        .iter()
        .any(|r| matches!(r, Report::PosUnpaid { .. })));
    assert!(w.c.alarm());
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::DoNotHandOver);
    assert_eq!(screen.why, Why::Replaced);
    // Once only.
    assert!(w
        .c
        .swap_cancelled(o.index, w.now, &mut w.m)
        .unwrap()
        .is_empty());
    // The cancel is stored with the sale and read back, and the node's
    // later word about the claim does not undo it.
    w.restart();
    assert!(w.c.sale(o.index).unwrap().cancel_recorded());
    w.claim_depth(&o, 6);
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
}

#[test]
fn cr_221_a_swap_lost_after_the_sale_was_reported_paid_is_a_cancel() {
    // A sale above the instant limit, paid at the claim's first confirmation
    // and reported to the POS, whose claim then leaves the chain and loses
    // to the provider's refund.
    let mut w = World::new();
    let big = w.open(2_000);
    w.claim_depth(&big, 1);
    let screen = w.swap_paid(&big, None).unwrap().unwrap();
    assert_eq!(screen.state, State::Confirmed);
    assert!(screen
        .reports
        .contains(&Report::PosPaid { index: big.index }));
    assert!(w.c.sale(big.index).unwrap().accepted().is_none());
    // The claim leaves the chain: the sale is taken back while the claim can
    // be sent again, and the POS is told it is unpaid again (CR-30).
    let reports = w.claim_depth(&big, 0);
    assert!(
        reports.contains(&Report::PosUnpaid { index: big.index }),
        "{reports:?}"
    );
    let screen = w.judge(big.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Reopened)
    );
    // Back in a block: paid again, and told so again.
    let reports = w.claim_depth(&big, 1);
    assert!(
        reports.contains(&Report::PosPaid { index: big.index }),
        "{reports:?}"
    );
    assert_eq!(w.judge(big.index).state, State::Confirmed);
    // Then lost: the alarm, the POS told, the screen red. This sale was
    // never accepted at once, so a cancel that needs an acceptance would
    // skip it.
    w.claim_depth(&big, 0);
    let reports = w.c.swap_cancelled(big.index, w.now, &mut w.m).unwrap();
    assert!(
        reports.contains(&Report::Alarm { index: big.index }),
        "{reports:?}"
    );
    assert!(w.c.alarm());
    let screen = w.judge(big.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Replaced)
    );
    assert!(w
        .c
        .swap_cancelled(big.index, w.now, &mut w.m)
        .unwrap()
        .is_empty());
    // The stored sale reads back with the cancel.
    w.restart();
    assert!(w.c.sale(big.index).unwrap().cancel_recorded());
    assert_eq!(w.judge(big.index).state, State::DoNotHandOver);
}

#[test]
fn cr_221_a_swap_lost_before_the_sale_was_reported_paid_is_taken_back_without_an_alarm() {
    // The claim is not in a block yet and the sale was not accepted at once:
    // nothing was reported to the POS. The payment is taken back, and no
    // cancel is raised.
    let mut w = World::new();
    let big = w.open(2_000);
    w.swap_paid(&big, None).unwrap().unwrap();
    let reports = w.c.swap_cancelled(big.index, w.now, &mut w.m).unwrap();
    assert!(reports.is_empty(), "{reports:?}");
    assert!(!w.c.alarm());
    let screen = w.judge(big.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Replaced)
    );
    w.restart();
    assert!(w.c.sale(big.index).unwrap().cancel_recorded());
    assert_eq!(w.judge(big.index).state, State::DoNotHandOver);
    // A sale that has no swap payment at all changes nothing.
    let o = w.open_five();
    assert!(w
        .c
        .swap_cancelled(o.index, w.now, &mut w.m)
        .unwrap()
        .is_empty());
    assert!(!w.c.sale(o.index).unwrap().cancel_recorded());
}

fn request(sats: u64) -> RefundRequest {
    RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: p2wsh(0xa1),
        actor: Actor {
            role: Role::Manager,
            name: "max".to_string(),
        },
        reason: "wrong size".to_string(),
    }
}

#[test]
fn cr_224_a_sale_paid_through_a_swap_can_be_refunded_on_chain() {
    let mut w = World::new();
    let o = w.open_five();
    w.mine_claim(&o, 4_300);
    w.swap_paid_final(&o);
    // The claim is on the chain, so there is no cash-only limit.
    assert_eq!(w.c.refund_onchain_room(o.index, w.now, &w.chain), None);
    w.c.request_refund(o.index, request(3_000), w.now, &w.chain, &mut w.m)
        .unwrap();
    // The most that can be asked is what the claim paid the shop.
    let err: RefundRefused<&str> =
        w.c.request_refund(o.index, request(1_400), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert!(matches!(
        err,
        RefundRefused::MoreThanReceived {
            received: 4_300,
            ..
        }
    ));
    // The sale's own node is another path: its payment is still cash only.
    let mut w = World::new();
    let o = w.open_five();
    let own = crate::LightningPaid {
        payment_hash: [7; 32],
        amount_msat: 5_000_000,
        settled_at: None,
    };
    w.own_node_paid(o.index, own);
    assert_eq!(w.c.refund_onchain_room(o.index, w.now, &w.chain), Some(0));
}

#[test]
fn cr_221_the_stored_sale_of_a_swap_payment_is_version_six_and_read_strictly() {
    let mut w = World::new();
    let o = w.open_five();
    w.swap_paid_final(&o);
    let good = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(good[4], 6);
    let back = crate::Sale::from_bytes(&good).unwrap();
    assert!(back.paid_via_swap());
    assert_eq!(back.to_bytes(), good);
    // Every cut is refused, and so is a byte more.
    for cut in 0..good.len() {
        assert!(crate::Sale::from_bytes(&good[..cut]).is_err(), "{cut}");
    }
    let mut long = good.clone();
    long.push(0);
    assert!(crate::Sale::from_bytes(&long).is_err());
    // Version 6 holds a payment: a sale of version 5 with the Lightning
    // flag at 0, read as version 6, is refused.
    let mut sale = good.clone();
    sale[4] = 5;
    assert!(crate::Sale::from_bytes(&sale).is_err());
    // A sale paid through the shop's own node is still version 3.
    let mut w = World::new();
    let o = w.open_five();
    w.own_node_paid(
        o.index,
        crate::LightningPaid {
            payment_hash: [7; 32],
            amount_msat: 5_000_000,
            settled_at: None,
        },
    );
    let own = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(own[4], 3);
    assert!(!crate::Sale::from_bytes(&own).unwrap().paid_via_swap());
}

#[test]
fn cr_221_a_sale_that_holds_a_swap_payment_cannot_be_cancelled() {
    let mut w = World::new();
    let big = w.open(2_000);
    w.swap_paid(&big, None).unwrap().unwrap();
    // The customer has paid the invoice: the sale holds the payment, with
    // nothing in its watch yet.
    let before = w.c.sale(big.index).unwrap().to_bytes();
    w.m.reset();
    let err = w.c.cancel(big.index, w.now, &mut w.m).unwrap_err();
    assert!(matches!(err, crate::CancelRefused::MoneyArrived), "{err:?}");
    assert_eq!(w.m.writes, 0, "nothing is written");
    let after = w.c.sale(big.index).unwrap().to_bytes();
    assert_eq!(before, after);
    // The stored sale decodes, and a restart holds it as it was.
    assert!(crate::Sale::from_bytes(&after).is_ok());
    w.restart();
    assert!(w.c.sale(big.index).unwrap().paid_via_swap());
    assert!(w.c.sale(big.index).unwrap().is_live());
}

#[test]
fn cr_224_the_refund_limit_and_the_books_are_what_the_claim_in_a_block_paid() {
    // The first claim paid 4,300; a claim built again at a higher fee pays
    // 4,000 and is the one that is mined.
    let mut w = World::new();
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    w.swap_paid(&o, Some(&st)).unwrap().unwrap();
    assert_eq!(
        w.c.sale(o.index).unwrap().lightning().unwrap().sats(),
        4_300
    );
    let facts = crate::Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: w.remote_order,
        now: w.now,
    };
    let _ = facts;
    w.c.swap_claim_amount(o.index, 4_000, &mut w.m).unwrap();
    // The sale holds the newest claim's amount; the exposure stays as it was.
    assert_eq!(
        w.c.sale(o.index).unwrap().lightning().unwrap().sats(),
        4_000
    );
    assert_eq!(w.c.exposure_outstanding(), 4_300);
    // Nothing is in a block: nothing can be refunded on chain.
    let nothing =
        w.c.request_refund(o.index, request(1_000), w.now, &w.chain, &mut w.m);
    assert!(matches!(nothing, Err(RefundRefused::NothingReceived)));
    w.mine_claim(&o, 4_000);
    w.claim_depth(&o, 1);
    let err: RefundRefused<&str> =
        w.c.request_refund(o.index, request(4_100), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert!(
        matches!(
            err,
            RefundRefused::MoreThanReceived {
                received: 4_000,
                ..
            }
        ),
        "{err:?}"
    );
    // The books carry the same amount.
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::Confirmed {
            received_sats: 4_000,
            ..
        }
    )));
    // An earlier, larger claim is the one mined: its amount counts, though
    // the sale holds the newest claim's.
    let mut w = World::new();
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    w.swap_paid(&o, Some(&st)).unwrap().unwrap();
    w.c.swap_claim_amount(o.index, 4_000, &mut w.m).unwrap();
    w.mine_claim(&o, 4_300);
    w.claim_depth(&o, 1);
    let err: RefundRefused<&str> =
        w.c.request_refund(o.index, request(4_400), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert!(
        matches!(
            err,
            RefundRefused::MoreThanReceived {
                received: 4_300,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::Confirmed {
            received_sats: 4_300,
            ..
        }
    )));
}

#[test]
fn cr_221_a_refund_in_a_block_takes_the_sale_back_and_a_reorg_to_the_claim_gives_it_again() {
    let mut w = World::new();
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    w.swap_paid(&o, Some(&st)).unwrap().unwrap();
    assert_eq!(w.judge(o.index).state, State::PaidInstant);
    // The provider's refund is in a block: at once the alarm, the POS told,
    // the screen red.
    let reports = w.spent(&o, true);
    assert!(
        reports.contains(&Report::Alarm { index: o.index }),
        "{reports:?}"
    );
    assert!(
        reports.contains(&Report::PosUnpaid { index: o.index }),
        "{reports:?}"
    );
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::Replaced)
    );
    // Asking again changes nothing.
    assert!(w.spent(&o, true).is_empty());
    // The spend leaves the chain: the sale is paid again, and the POS is told
    // so. The claim in a block then confirms it.
    let reports = w.spent(&o, false);
    assert!(
        reports.contains(&Report::PosPaid { index: o.index }),
        "{reports:?}"
    );
    w.claim_depth(&o, 1);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    // The spend came to the refund depth: the swap is lost for good.
    w.spent(&o, true);
    w.c.swap_cancelled(o.index, w.now, &mut w.m).unwrap();
    assert!(w.c.sale(o.index).unwrap().cancel_recorded());
    w.spent(&o, false);
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    // A sale not reported paid is taken back without an alarm.
    let mut w = World::new();
    let big = w.open(2_000);
    w.swap_paid(&big, None).unwrap().unwrap();
    let reports = w.spent(&big, true);
    assert!(reports.is_empty(), "{reports:?}");
    assert!(!w.c.alarm());
    assert_eq!(w.judge(big.index).state, State::DoNotHandOver);
    w.spent(&big, false);
    assert_eq!(w.judge(big.index).state, State::Waiting);
}

#[test]
fn cr_221_a_manager_override_of_a_held_swap_sale_shows_it_paid() {
    let mut w = World::new();
    let big = w.open(2_000);
    w.swap_paid(&big, None).unwrap().unwrap();
    assert!(!w.judge(big.index).state.is_paid());
    w.override_sale(big.index, Role::Manager, "max").unwrap();
    let screen = w.judge(big.index);
    assert_eq!(screen.state, State::Overridden, "{screen:?}");
    assert!(screen.state.is_paid());
}

#[test]
fn cr_19_a_payment_to_a_swap_sale_that_is_not_one_of_its_claims_is_late() {
    let mut w = World::new();
    let o = w.open_five();
    let claim = w.pay(&o, 4_300);
    w.c.set_swap_claims(o.index, &[claim.compute_txid()]);
    w.swap_paid_final(&o);
    // The claim is the sale's own payment.
    let seen = w.see(&claim, 1);
    assert!(seen.reports.is_empty(), "{:?}", seen.reports);
    // Any other payment to the address is late, as for any sale.
    let other = w.pay(&o, 2_000);
    let seen = w.see(&other, 1);
    assert!(
        seen.reports
            .iter()
            .any(|r| matches!(r, Report::LatePayment { sats: 2_000, .. })),
        "{:?}",
        seen.reports
    );
    assert_eq!(w.c.sale(o.index).unwrap().late_payments().len(), 1);
}

#[test]
fn cr_221_the_claims_of_a_swap_in_the_sale_watch_are_not_a_surplus() {
    // The claim is in a block, in the sale's watch, before the payment is
    // recorded (a retry, or a restart).
    let mut w = World::new();
    let o = w.open_five();
    w.mine_claim(&o, 4_300);
    let screen = w.swap_paid(&o, None).unwrap().unwrap();
    assert!(
        !screen
            .reports
            .iter()
            .any(|r| matches!(r, Report::SurplusOwed { .. })),
        "{:?}",
        screen.reports
    );
    assert!(!w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, Event::SurplusOwed { .. })));
    // A payment that is not a claim still is one.
    let mut w = World::new();
    let o = w.open_five();
    let other = w.pay(&o, 1_000);
    w.confirm(std::slice::from_ref(&other), 100);
    let screen = w.swap_paid(&o, None).unwrap().unwrap();
    assert!(screen
        .reports
        .iter()
        .any(|r| matches!(r, Report::SurplusOwed { sats: 1_000, .. })));
}

#[test]
fn cr_221_a_retry_that_is_not_accepted_at_once_leaves_no_exposure_behind() {
    let mut w = World::new();
    let o = w.open_five();
    let st = w.lockup_status(3, 9);
    // The sale is not stored: the exposure counted for it stays for the
    // retry.
    w.m.reset();
    w.m.fail_on = Some(3);
    let err = w.swap_paid(&o, Some(&st)).unwrap_err();
    assert!(matches!(err, LightningRefused::Store(_)), "{err:?}");
    assert_eq!(w.c.exposure_outstanding(), 4_300);
    // The retry is not decided at once: the sale holds the payment, and no
    // entry is left that nothing will release.
    w.m.reset();
    let screen = w.swap_paid(&o, None).unwrap().unwrap();
    assert!(!screen.state.is_paid(), "{screen:?}");
    assert_eq!(w.c.exposure_outstanding(), 0);
}
