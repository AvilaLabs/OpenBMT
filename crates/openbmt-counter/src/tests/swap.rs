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
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(screen.why, Why::PaidByLightning);
    // The amount at risk is counted in the ledger, as for any instant sale.
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.paid_via_swap() && sale.books_final() && !sale.is_live());
    // What the sale keeps is what the claim pays its address.
    assert_eq!(sale.lightning().unwrap().sats(), 4_300);
    assert!(screen.reports.iter().any(|r| matches!(
        r,
        Report::BooksFinal {
            received_sats: 4_300,
            ..
        }
    )));
    // The records: the payment and the POS.
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::PaidByLightning {
            amount_msat: 4_300_000,
            ..
        }
    )));
    // The exposure leaves the ledger when the lockup is in a block, once.
    w.c.swap_lockup_confirmed(o.index, &mut w.m).unwrap();
    assert_eq!(w.c.exposure_outstanding(), 0);
    w.c.swap_lockup_confirmed(o.index, &mut w.m).unwrap();
    // A restart reads the sale back as a swap payment.
    w.restart();
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.paid_via_swap() && sale.books_final());
    assert_eq!(w.judge(o.index).state, State::PaidLightning);
}

#[test]
fn cr_221_a_sale_above_the_instant_limit_or_a_full_ledger_waits_for_the_block() {
    let mut w = World::new();
    // $20 is 20,000 sats: above the $10 limit of the light check.
    let big = w.open(2_000);
    let st = w.lockup_status(3, 9);
    assert_eq!(w.swap_decide(&big, &st), None);
    // With nothing decided at once the call takes the payment as one whose
    // lockup is in a block: paid, and nothing counted.
    let screen = w.swap_paid(&big, None).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w.c.sale(big.index).unwrap().paid_via_swap());
    // A slice that is full: five $5 sales are 25,000 sats, the slice is
    // 50,000, so ten of them fill it and the eleventh waits.
    let mut w = World::new();
    for _ in 0..10 {
        let o = w.open_five();
        let st = w.lockup_status(3, 9);
        let screen = w.swap_paid(&o, Some(&st)).unwrap().unwrap();
        assert_eq!(screen.state, State::PaidLightning);
    }
    assert_eq!(w.c.exposure_outstanding(), 50_000);
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
    w.swap_paid(&o, None).unwrap().unwrap();
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
    // Once only; and a sale paid with its lockup in a block has no cancel.
    assert!(w
        .c
        .swap_cancelled(o.index, w.now, &mut w.m)
        .unwrap()
        .is_empty());
    let mut w = World::new();
    let o = w.open_five();
    w.swap_paid(&o, None).unwrap().unwrap();
    assert!(w
        .c
        .swap_cancelled(o.index, w.now, &mut w.m)
        .unwrap()
        .is_empty());
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
    w.swap_paid(&o, None).unwrap().unwrap();
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
    w.swap_paid(&o, None).unwrap().unwrap();
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
