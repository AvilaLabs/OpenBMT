//! Section 5 of `docs/SHOP-LIGHTNING-RULES.md`, SL-27 and SL-40, at the
//! counter: a Lightning payment through a channel whose funding has no
//! confirmation is decided as a swap's is (CR-221), and what becomes of the
//! funding is told to the sales that rest on it.

use super::kit::*;
use crate::sale::Sale;
use crate::{LightningPaid, LightningRefused, Opened, PaidFunding, Role, Screen, State, Why};
use openbmt_core::policy::PauseStatus;

const FUNDING: [u8; 32] = [0xf1; 32];
const OTHER: [u8; 32] = [0xf2; 32];

fn paid(hash: u8, sats: u64) -> LightningPaid {
    LightningPaid {
        payment_hash: [hash; 32],
        amount_msat: sats * 1_000,
        settled_at: Some(1_700_000_000),
    }
}

impl World {
    /// The funding is lost: the sales named and what is reported.
    fn lose(&mut self, txid: &[u8; 32]) -> (Vec<u32>, Vec<crate::Report>) {
        self.c
            .lightning_funding_lost(txid, self.now, &mut self.m)
            .unwrap()
    }

    fn funded(
        &mut self,
        o: &Opened,
        hash: u8,
        funding: PaidFunding,
    ) -> Result<Option<Screen>, LightningRefused<&'static str>> {
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c.lightning_paid_funded(
            o.index,
            paid(hash, o.needs_sats),
            funding,
            &facts,
            &mut self.m,
        )
    }
}

fn unconfirmed(txid: [u8; 32]) -> PaidFunding {
    PaidFunding::Unconfirmed {
        txids: vec![txid],
        more: false,
    }
}

fn several(txids: &[[u8; 32]]) -> PaidFunding {
    PaidFunding::Unconfirmed {
        txids: txids.to_vec(),
        more: false,
    }
}

fn lost(txid: [u8; 32]) -> PaidFunding {
    PaidFunding::Lost {
        txids: vec![txid],
        more: false,
    }
}

#[test]
fn sl_40_within_the_instant_limits_the_sale_is_paid_at_once_and_the_ledger_holds_it() {
    let mut w = World::new();
    let o = w.open_five();
    let screen = w.funded(&o, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(screen.why, Why::PaidByLightning);
    // The amount is in the exposure ledger until the funding confirms.
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.books_final() && !sale.is_live());
    assert_eq!(sale.lightning_fundings(), &[FUNDING]);
    assert_eq!(sale.lightning().unwrap().sats(), 5_000);
    assert!(!sale.paid_via_swap());
    // A restart reads it back, with its funding and its ledger.
    w.restart();
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    assert_eq!(w.c.sale(o.index).unwrap().lightning_fundings(), &[FUNDING]);
    assert_eq!(w.judge(o.index).state, State::PaidLightning);
}

#[test]
fn sl_40_a_payment_with_a_confirmed_funding_is_final_at_once_and_counts_nothing() {
    let mut w = World::new();
    let o = w.open_five();
    let screen = w.funded(&o, 1, PaidFunding::Confirmed).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w.c.sale(o.index).unwrap().lightning_fundings().is_empty());
}

#[test]
fn sl_40_above_the_instant_limit_the_sale_waits_and_says_the_funding_is_not_confirmed() {
    let mut w = World::new();
    // $20 is 20,000 sats: above the $10 limit of the light check.
    let big = w.open(2_000);
    assert!(w.funded(&big, 1, unconfirmed(FUNDING)).unwrap().is_none());
    // No ledger and no payment on the sale: only what it waits for is kept.
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w.c.sale(big.index).unwrap().lightning().is_none());
    let screen = w.judge(big.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::FundingNotConfirmed);
    // It does not expire while it waits.
    w.tick(10_000);
    assert_eq!(w.judge(big.index).state, State::Waiting);
    // When the funding is confirmed the payment is final, with nothing counted.
    let screen = w.funded(&big, 1, PaidFunding::Confirmed).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(w.c.exposure_outstanding(), 0);
}

#[test]
fn sl_40_a_full_ledger_makes_the_sale_wait_too() {
    let mut w = World::new();
    // The slice of the light check is 50,000 sats: ten $5 sales fill it.
    for n in 0..10 {
        let o = w.open_five();
        assert!(w.funded(&o, n, unconfirmed(FUNDING)).unwrap().is_some());
    }
    assert_eq!(w.c.exposure_outstanding(), 50_000);
    let o = w.open_five();
    assert!(w.funded(&o, 99, unconfirmed(FUNDING)).unwrap().is_none());
    assert_eq!(w.judge(o.index).why, Why::FundingNotConfirmed);
    // Room comes back when the funding is confirmed, and the payment is then final.
    assert_eq!(
        w.c.lightning_funding_confirmed(&FUNDING, &mut w.m)
            .unwrap()
            .len(),
        10
    );
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w.funded(&o, 99, unconfirmed(FUNDING)).unwrap().is_some());
}

#[test]
fn cr_20_a_manager_can_accept_a_sale_that_waits_for_a_funding_and_its_payment_is_then_its_own() {
    let mut w = World::new();
    let big = w.open(2_000);
    assert!(w.funded(&big, 1, unconfirmed(FUNDING)).unwrap().is_none());
    // A cashier is refused above the per-sale limit (CR-21); a manager may.
    assert!(w.override_sale(big.index, Role::Cashier, "ann").is_err());
    w.override_sale(big.index, Role::Manager, "max").unwrap();
    assert_eq!(w.judge(big.index).state, State::Overridden);
    assert_eq!(w.c.exposure_outstanding(), 20_000);
    // The payment comes again: it is the sale's own, and its amount is the
    // one the acceptance counted.
    let screen = w.funded(&big, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(w.c.exposure_outstanding(), 20_000);
    assert_eq!(
        w.c.sale(big.index).unwrap().lightning_fundings(),
        &[FUNDING]
    );
    // And it leaves the ledger when the funding confirms.
    assert_eq!(
        w.c.lightning_funding_confirmed(&FUNDING, &mut w.m).unwrap(),
        vec![big.index]
    );
    assert_eq!(w.c.exposure_outstanding(), 0);
}

#[test]
fn sl_40_a_funding_that_confirms_releases_the_ledger_once() {
    let mut w = World::new();
    let a = w.open_five();
    let b = w.open_five();
    let c = w.open_five();
    w.funded(&a, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    w.funded(&b, 2, unconfirmed(FUNDING)).unwrap().unwrap();
    w.funded(&c, 3, unconfirmed(OTHER)).unwrap().unwrap();
    assert_eq!(w.c.exposure_outstanding(), 15_000);
    assert_eq!(
        w.c.lightning_funding_confirmed(&FUNDING, &mut w.m).unwrap(),
        vec![a.index, b.index]
    );
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    // Again, and for a funding no sale rests on: nothing changes.
    assert!(w
        .c
        .lightning_funding_confirmed(&FUNDING, &mut w.m)
        .unwrap()
        .is_empty());
    assert!(w
        .c
        .lightning_funding_confirmed(&[0; 32], &mut w.m)
        .unwrap()
        .is_empty());
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    // The release is stored.
    w.restart();
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    assert!(w.lose(&FUNDING).0.is_empty());
}

#[test]
fn sl_27_a_funding_that_is_lost_names_the_sales_paid_through_it() {
    let mut w = World::new();
    let a = w.open_five();
    let b = w.open_five();
    let c = w.open_five();
    let d = w.open_five();
    w.funded(&a, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    w.funded(&b, 2, unconfirmed(OTHER)).unwrap().unwrap();
    w.funded(&c, 3, unconfirmed(FUNDING)).unwrap().unwrap();
    w.funded(&d, 4, PaidFunding::Confirmed).unwrap().unwrap();
    assert_eq!(w.lose(&FUNDING).0, vec![a.index, c.index]);
    assert_eq!(w.lose(&OTHER).0, vec![b.index]);
    assert!(w.lose(&[0; 32]).0.is_empty());
    // The money may never come: what the sales put in the ledger stays there
    // until the owner releases it.
    assert_eq!(w.c.exposure_outstanding(), 15_000);
    // A funding that confirmed is no longer lost.
    w.c.lightning_funding_confirmed(&OTHER, &mut w.m).unwrap();
    assert!(w.lose(&OTHER).0.is_empty());
}

#[test]
fn sl_40_a_payment_is_refused_as_any_lightning_payment_is() {
    let mut w = World::new();
    let o = w.open_five();
    let facts = crate::Facts {
        chain: &w.chain,
        health: w.health,
        heartbeat: heartbeat(),
        remote_order: w.remote_order,
        now: w.now,
    };
    let err =
        w.c.lightning_paid_funded(
            o.index,
            paid(1, 4_999),
            unconfirmed(FUNDING),
            &facts,
            &mut w.m,
        )
        .unwrap_err();
    assert!(matches!(
        err,
        LightningRefused::TooLittle { least: 5_000, .. }
    ));
    assert!(matches!(
        w.funded(
            &Opened {
                index: 99,
                ..o.clone()
            },
            1,
            PaidFunding::Confirmed
        ),
        Err(LightningRefused::UnknownSale(99))
    ));
    // Paid already by this invoice: its screen, and nothing counted twice.
    w.funded(&o, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    w.funded(&o, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn sl_40_the_stored_sale_is_version_7_and_older_versions_still_read() {
    let mut w = World::new();
    let o = w.open_five();
    w.funded(&o, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    let bytes = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(bytes[4], 7);
    let read = Sale::from_bytes(&bytes).unwrap();
    assert_eq!(read.lightning_fundings(), &[FUNDING]);
    assert_eq!(read.to_bytes(), bytes);
    // The funding is the last 32 bytes, and only version 7 has it.
    assert!(Sale::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    let mut long = bytes.clone();
    long.push(0);
    assert!(Sale::from_bytes(&long).is_err());
    let mut v6 = bytes[..bytes.len() - 35].to_vec();
    v6[4] = 6;
    let old = Sale::from_bytes(&v6).unwrap();
    assert!(old.lightning_fundings().is_empty());
    assert!(old.paid_via_swap());
    let mut v6_long = bytes.clone();
    v6_long[4] = 6;
    assert!(Sale::from_bytes(&v6_long).is_err());
    // Version 7 with no payment is refused.
    let unpaid = w.open_five();
    let mut none = w.c.sale(unpaid.index).unwrap().to_bytes();
    none[4] = 7;
    none.extend_from_slice(&[1, 0, 1]);
    none.extend_from_slice(&FUNDING);
    assert!(Sale::from_bytes(&none).is_err());
    // A sale paid with a confirmed funding is written as before.
    let p = w.open_five();
    w.funded(&p, 2, PaidFunding::Confirmed).unwrap().unwrap();
    assert_eq!(w.c.sale(p.index).unwrap().to_bytes()[4], 3);
}

#[test]
fn cr_66_sl_40_a_version_7_sale_keeps_the_reopened_byte_and_the_refund_latch_apart() {
    let mut w = World::new();
    let o = w.open_five();
    w.funded(&o, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    let base = w.c.sale(o.index).unwrap().clone();
    let form = |reopened: bool, latch: bool| {
        let mut s = base.clone();
        s.reopened = reopened;
        s.refund_reopened = latch;
        s.to_bytes()
    };
    // The four codes of the byte, each with the funding behind them.
    for (reopened, latch) in [(false, false), (true, true), (false, true), (true, false)] {
        let bytes = form(reopened, latch);
        assert_eq!(bytes[4], 7);
        let read = Sale::from_bytes(&bytes).unwrap();
        assert_eq!((read.reopened, read.refund_reopened), (reopened, latch));
        assert_eq!(read.lightning_fundings(), &[FUNDING]);
        assert_eq!(read.to_bytes(), bytes);
    }
    // A code past 3 is refused in version 7 as in any other.
    let (off, on) = (form(false, false), form(true, true));
    let at = off.iter().zip(&on).position(|(a, b)| a != b).unwrap();
    let mut bad = off.clone();
    bad[at] = 4;
    assert!(Sale::from_bytes(&bad).is_err());
}

const THIRD: [u8; 32] = [0xf3; 32];

// F3: every funding of the payment's channels is kept and tracked.

#[test]
fn sl_40_a_payment_through_several_fundings_is_released_only_when_every_one_has_confirmed() {
    let mut w = World::new();
    let o = w.open_five();
    let screen = w
        .funded(&o, 1, several(&[FUNDING, OTHER]))
        .unwrap()
        .unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(
        w.c.sale(o.index).unwrap().lightning_fundings(),
        &[FUNDING, OTHER]
    );
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    // The first confirms: the second is still not, so nothing leaves the ledger.
    assert!(w
        .c
        .lightning_funding_confirmed(&FUNDING, &mut w.m)
        .unwrap()
        .is_empty());
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    assert_eq!(w.c.sale(o.index).unwrap().lightning_fundings(), &[OTHER]);
    // What is left is kept: a restart reads it back.
    w.restart();
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    assert_eq!(w.c.sale(o.index).unwrap().lightning_fundings(), &[OTHER]);
    // The last confirms: the exposure is released, once.
    assert_eq!(
        w.c.lightning_funding_confirmed(&OTHER, &mut w.m).unwrap(),
        vec![o.index]
    );
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w
        .c
        .lightning_funding_confirmed(&OTHER, &mut w.m)
        .unwrap()
        .is_empty());
}

#[test]
fn sl_27_any_one_funding_lost_makes_the_sale_lost_and_the_others_do_not_undo_it() {
    let mut w = World::new();
    let o = w.open_five();
    w.funded(&o, 1, several(&[FUNDING, OTHER]))
        .unwrap()
        .unwrap();
    // One confirms, the other is lost: the sale is named and cancelled.
    w.c.lightning_funding_confirmed(&FUNDING, &mut w.m).unwrap();
    let (named, reports) = w.lose(&OTHER);
    assert_eq!(named, vec![o.index]);
    assert!(reports.contains(&crate::Report::Alarm { index: o.index }));
    assert_eq!(w.judge(o.index).state, State::DoNotHandOver);
    // The exposure stays counted until the owner releases it.
    assert_eq!(w.c.exposure_outstanding(), 5_000);
    // The reverse order: lost first, then the other confirms.
    let mut w = World::new();
    let p = w.open_five();
    w.funded(&p, 2, several(&[THIRD, [0xf4; 32]]))
        .unwrap()
        .unwrap();
    assert_eq!(w.lose(&THIRD).0, vec![p.index]);
    w.c.lightning_funding_confirmed(&[0xf4; 32], &mut w.m)
        .unwrap();
    assert_eq!(w.judge(p.index).state, State::DoNotHandOver);
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn sl_40_a_payment_through_more_than_eight_fundings_is_never_taken_at_once() {
    let mut w = World::new();
    let o = w.open_five();
    // The node lists eight and says more came.
    let eight: Vec<[u8; 32]> = (1..=8).map(|i| [i; 32]).collect();
    let more = PaidFunding::Unconfirmed {
        txids: eight.clone(),
        more: true,
    };
    assert!(w.funded(&o, 1, more.clone()).unwrap().is_none());
    assert_eq!(w.judge(o.index).why, Why::FundingNotConfirmed);
    assert_eq!(w.c.exposure_outstanding(), 0);
    // A longer list is cut to eight and is more.
    let nine: Vec<[u8; 32]> = (1..=9).map(|i| [i; 32]).collect();
    let p = w.open_five();
    let long = PaidFunding::Unconfirmed {
        txids: nine,
        more: false,
    };
    assert!(w.funded(&p, 2, long).unwrap().is_none());
    assert_eq!(w.c.sale(p.index).unwrap().lightning_fundings().len(), 8);
    assert!(w.c.sale(p.index).unwrap().lightning_fundings_more());
    // Exactly eight, all listed, is taken at once.
    let q = w.open_five();
    assert!(w.funded(&q, 3, several(&eight)).unwrap().is_some());
    // A manager may accept the sale that waits; its exposure is never
    // released by the confirmation of the eight listed, for more came.
    w.override_sale(o.index, Role::Manager, "max").unwrap();
    w.funded(&o, 1, more).unwrap().unwrap();
    for t in &eight {
        w.c.lightning_funding_confirmed(t, &mut w.m).unwrap();
    }
    assert!(w.c.sale(o.index).unwrap().lightning_fundings().is_empty());
    assert!(w.c.sale(o.index).unwrap().lightning_fundings_more());
    assert_eq!(w.c.exposure_outstanding(), 5_000);
}

#[test]
fn sl_40_the_stored_sale_holds_the_set_strictly() {
    let mut w = World::new();
    let o = w.open_five();
    w.funded(&o, 1, several(&[FUNDING, OTHER, THIRD]))
        .unwrap()
        .unwrap();
    let bytes = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(bytes[4], 7);
    // wait 0, more 0, count 3, then the three fundings, at the end.
    let tail = bytes.len() - (3 + 3 * 32);
    assert_eq!(&bytes[tail..tail + 3], &[0, 0, 3]);
    assert_eq!(&bytes[tail + 3..tail + 35], &FUNDING);
    assert_eq!(&bytes[bytes.len() - 32..], &THIRD);
    let read = Sale::from_bytes(&bytes).unwrap();
    assert_eq!(read.lightning_fundings(), &[FUNDING, OTHER, THIRD]);
    assert_eq!(read.to_bytes(), bytes);
    // A count past 8, a count past the bytes, a funding twice, a flag or a
    // wait that is not known, and a count of 0 without more are refused.
    let with = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut b = bytes.clone();
        edit(&mut b);
        Sale::from_bytes(&b)
    };
    assert!(with(&|b| b[tail + 2] = 9).is_err());
    assert!(with(&|b| b[tail + 2] = 4).is_err());
    assert!(with(&|b| {
        let (a, c) = (tail + 3, tail + 35);
        let first: Vec<u8> = b[a..a + 32].to_vec();
        b[c..c + 32].copy_from_slice(&first);
    })
    .is_err());
    assert!(with(&|b| b[tail + 1] = 2).is_err());
    assert!(with(&|b| b[tail] = 3).is_err());
    assert!(with(&|b| b[tail] = 1).is_err(), "a payment does not wait");
    assert!(with(&|b| {
        b.truncate(tail + 3);
        b[tail + 2] = 0;
    })
    .is_err());
    // More with fewer listed is a sale that rests on more than it lists.
    let mut more = w.c.sale(o.index).unwrap().clone();
    more.funding.truncate(1);
    more.funding_more = true;
    let b = more.to_bytes();
    let back = Sale::from_bytes(&b).unwrap();
    assert!(back.lightning_fundings_more() && back.lightning_fundings() == [FUNDING]);
    assert_eq!(back.to_bytes(), b);
}

// F1: a funding already lost.

#[test]
fn sl_27_a_payment_through_a_funding_already_lost_waits_and_a_manager_can_accept_it() {
    let mut w = World::new();
    let o = w.open_five();
    // Within the instant limits and with room: through a funding not lost it
    // would be taken at once. Through a lost one it waits.
    assert!(w.funded(&o, 1, lost(FUNDING)).unwrap().is_none());
    assert_eq!(w.c.exposure_outstanding(), 0);
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::Waiting, Why::FundingLostWait)
    );
    // It is named with the lost sales of the funding, and the lost cause stays.
    assert_eq!(w.lose(&FUNDING).0, vec![o.index]);
    assert_eq!(w.judge(o.index).why, Why::FundingLostWait);
    // It does not expire, and a restart finds it waiting for the same cause.
    w.tick(10_000);
    assert_eq!(w.judge(o.index).state, State::Waiting);
    w.restart();
    assert_eq!(w.judge(o.index).why, Why::FundingLostWait);
    // A manager may accept it; the payment is then the sale's own, and it is
    // named as a sale of a lost funding.
    w.override_sale(o.index, Role::Manager, "max").unwrap();
    let screen = w.funded(&o, 1, lost(FUNDING)).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(w.c.sale(o.index).unwrap().lightning_fundings(), &[FUNDING]);
    assert_eq!(w.lose(&FUNDING).0, vec![o.index]);
}

#[test]
fn sl_27_a_funding_that_is_lost_while_a_sale_waits_gives_it_the_lost_cause() {
    let mut w = World::new();
    let big = w.open(2_000);
    assert!(w.funded(&big, 1, unconfirmed(FUNDING)).unwrap().is_none());
    assert_eq!(w.judge(big.index).why, Why::FundingNotConfirmed);
    let (named, reports) = w.lose(&FUNDING);
    assert_eq!(named, vec![big.index]);
    // A sale that was never paid is no cancel.
    assert!(reports.is_empty());
    assert_eq!(w.judge(big.index).why, Why::FundingLostWait);
    assert!(!w.c.alarm());
}

// F2: a lost funding is a cancel.

#[test]
fn sl_27_a_sale_paid_at_once_through_a_funding_that_is_lost_is_a_cancel() {
    let mut w = World::new();
    let o = w.open_five();
    w.funded(&o, 1, unconfirmed(FUNDING)).unwrap().unwrap();
    assert_eq!(w.judge(o.index).state, State::PaidLightning);
    let (named, reports) = w.lose(&FUNDING);
    assert_eq!(named, vec![o.index]);
    // As CR-14 has it for a conflict on a sale accepted at once: the alarm,
    // the pause, the POS told the sale is not paid.
    assert!(reports.contains(&crate::Report::Alarm { index: o.index }));
    assert!(reports.contains(&crate::Report::PosUnpaid { index: o.index }));
    assert!(w.c.alarm());
    assert_eq!(
        w.c.pause_status(w.now),
        PauseStatus::PausedFor { secs_left: 3_600 }
    );
    let screen = w.judge(o.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::DoNotHandOver, Why::FundingLost)
    );
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, crate::Event::Replaced { index, .. } if *index == o.index)));
    // Once only: asking again reports nothing, and a restart keeps it.
    let (named, reports) = w.lose(&FUNDING);
    assert_eq!(named, vec![o.index]);
    assert!(reports.is_empty());
    w.restart();
    assert_eq!(w.judge(o.index).why, Why::FundingLost);
    // The pause was counted once, so one more cancel is not a lock.
    assert!(matches!(
        w.c.pause_status(w.now),
        PauseStatus::PausedFor { .. }
    ));
    // The pause stops the next payment through a funding from being taken
    // at once.
    let next = w.open_five();
    assert!(w.funded(&next, 2, unconfirmed(OTHER)).unwrap().is_none());
}

#[test]
fn sl_27_a_sale_paid_through_a_confirmed_funding_is_no_cancel() {
    let mut w = World::new();
    let o = w.open_five();
    w.funded(&o, 1, PaidFunding::Confirmed).unwrap().unwrap();
    let (named, reports) = w.lose(&FUNDING);
    assert!(named.is_empty() && reports.is_empty());
    assert_eq!(w.judge(o.index).state, State::PaidLightning);
}

// F9: a sale waiting for its fundings is restored waiting.

#[test]
fn sl_40_a_sale_waiting_for_its_funding_is_restored_waiting_after_a_start() {
    let mut w = World::new();
    let big = w.open(2_000);
    assert!(w
        .funded(&big, 1, several(&[FUNDING, OTHER]))
        .unwrap()
        .is_none());
    let bytes = w.c.sale(big.index).unwrap().to_bytes();
    assert_eq!(bytes[4], 7);
    assert!(Sale::from_bytes(&bytes).unwrap().lightning().is_none());
    w.restart();
    assert_eq!(
        w.c.sale(big.index).unwrap().lightning_fundings(),
        &[FUNDING, OTHER]
    );
    // It does not expire, as before the start.
    w.tick(10_000);
    let screen = w.judge(big.index);
    assert_eq!(
        (screen.state, screen.why),
        (State::Waiting, Why::FundingNotConfirmed)
    );
    // Asking again with the same facts changes nothing on disk.
    assert!(w
        .funded(&big, 1, several(&[FUNDING, OTHER]))
        .unwrap()
        .is_none());
    assert_eq!(w.c.sale(big.index).unwrap().to_bytes(), bytes);
    // When the funding set confirms, the payment is final with nothing counted.
    let screen = w.funded(&big, 1, PaidFunding::Confirmed).unwrap().unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert!(w.c.sale(big.index).unwrap().lightning_fundings().is_empty());
    assert_eq!(w.c.sale(big.index).unwrap().to_bytes()[4], 3);
    assert_eq!(w.c.exposure_outstanding(), 0);
}
