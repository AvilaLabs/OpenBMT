//! Valuing satoshis (CR-06, CR-08), the tolerance (CR-16) and dust (CR-17).

use openbmt_watch::SaleWatch;

use super::kit::*;
use crate::money::{is_enough, tolerance, MAX_SATS};
use crate::{sats_for, value_of, Amount, Currency, Rate, Report, Segment, State, DUST_SATS};

fn rate_of(minor_per_btc: u64) -> Rate {
    Rate::new(minor_per_btc).unwrap()
}

// The two conversions.

#[test]
fn cr_08_satoshis_for_an_amount_round_up_to_a_whole_satoshi() {
    // 1000 cents at 30,000,000 cents per bitcoin is 3333.33 sats.
    assert_eq!(sats_for(Amount(1_000), rate_of(30_000_000)), Some(3_334));
    // An exact division does not round.
    assert_eq!(sats_for(Amount(500), rate_of(10_000_000)), Some(5_000));
    assert_eq!(sats_for(Amount(0), rate_of(10_000_000)), Some(0));
    assert_eq!(sats_for(Amount(1), rate_of(u64::MAX)), Some(1));
}

#[test]
fn cr_08_more_than_all_the_bitcoin_has_no_answer() {
    // At one cent per bitcoin, 21,000,000 cents is all the bitcoin there is.
    let rate = rate_of(1);
    assert_eq!(sats_for(Amount(21_000_000), rate), Some(MAX_SATS));
    assert_eq!(sats_for(Amount(21_000_001), rate), None);
    assert_eq!(sats_for(Amount(u64::MAX), rate), None);
}

#[test]
fn cr_06_satoshis_are_valued_by_rounding_down_and_saturate() {
    assert_eq!(value_of(3_333, rate_of(30_000_000)), Amount(999));
    assert_eq!(value_of(4_000, rate_of(10_000_000)), Amount(400));
    assert_eq!(value_of(u64::MAX, rate_of(u64::MAX)), Amount(u64::MAX));
    assert_eq!(value_of(0, rate_of(10_000_000)), Amount(0));
}

#[test]
fn cr_05_a_currency_is_three_upper_case_letters() {
    assert!(Currency::new(*b"USD").is_some());
    assert!(Currency::new(*b"usd").is_none());
    assert!(Currency::new(*b"U1D").is_none());
    assert_eq!(Currency::new(*b"EUR").unwrap().to_string(), "EUR");
    assert!(Rate::new(0).is_none());
}

// CR-06 and CR-08 with a sale.

#[test]
fn cr_06_cr_08_a_part_payment_is_valued_at_the_quote_that_was_current() {
    let mut w = World::new();
    let o = w.open(1_000); // $10: 10,000 sats at 1000 sats per dollar
    assert_eq!(o.needs_sats, 10_000);
    let part = w.pay(&o, 4_000);
    w.see(&part, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PartPaid);
    assert_eq!(screen.needs_sats, 10_000);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(
        sale.segments(),
        &[Segment {
            sats: 4_000,
            quote: 0
        }]
    );

    // The quote runs out; the price of bitcoin doubles; the sale is quoted again.
    w.tick(301);
    w.requote(o.index, 20_000_000).unwrap();
    let screen = w.judge(o.index);
    // 4,000 sats stay worth 400 cents; the other 600 cents need 3,000 sats.
    assert_eq!(screen.needs_sats, 4_000 + 3_000);
    assert_eq!(screen.state, State::PartPaid);

    // The rest arrives under the second quote.
    let rest = w.pay(&o, 3_000);
    w.see(&rest, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.received_sats, 7_000);
    assert_eq!(screen.needs_sats, 7_000);
    assert_eq!(screen.state, State::Waiting, "enough has arrived");
    assert_eq!(w.c.sale(o.index).unwrap().frozen_need(), Some(7_000));
}

#[test]
fn cr_06_a_payment_after_the_quote_ran_out_uses_the_last_quote() {
    let mut w = World::new();
    let o = w.open(1_000);
    w.tick(10_000);
    let part = w.pay(&o, 4_000);
    w.see(&part, 1);
    w.judge(o.index);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(
        sale.segments(),
        &[Segment {
            sats: 4_000,
            quote: 0
        }]
    );
    assert_eq!(sale.needs_sats(), 10_000);
}

#[test]
fn cr_06_a_valuation_never_changes_when_a_later_quote_comes() {
    let mut w = World::new();
    let o = w.open(1_000);
    let part = w.pay(&o, 4_000);
    w.see(&part, 1);
    w.judge(o.index);
    w.tick(301);
    w.requote(o.index, 20_000_000).unwrap();
    w.tick(301);
    w.requote(o.index, 5_000_000).unwrap();
    w.judge(o.index);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(
        sale.segments(),
        &[Segment {
            sats: 4_000,
            quote: 0
        }]
    );
}

#[test]
fn cr_06_when_what_arrived_shrinks_the_newest_valuations_go_first() {
    let mut w = World::new();
    let o = w.open(1_000);
    let first = w.pay(&o, 4_000);
    w.see(&first, 1);
    w.judge(o.index);
    w.tick(301);
    w.requote(o.index, 20_000_000).unwrap();
    // Coin 99 pays 2,000 now.
    let second = w.pay_from(&o, 99, 2_000);
    w.see(&second, 1);
    w.judge(o.index);
    assert_eq!(
        w.c.sale(o.index).unwrap().segments(),
        &[
            Segment {
                sats: 4_000,
                quote: 0
            },
            Segment {
                sats: 2_000,
                quote: 1
            }
        ]
    );
    // A conflicting spend of coin 99 that pays only 500: only the smallest
    // of the two counts, so what arrived shrinks by 1,500.
    let mut smaller = w.pay_from(&o, 99, 500);
    smaller.lock_time = bitcoin::absolute::LockTime::from_height(7).unwrap();
    w.see(&smaller, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.received_sats, 4_500);
    assert_eq!(
        w.c.sale(o.index).unwrap().segments(),
        &[
            Segment {
                sats: 4_000,
                quote: 0
            },
            Segment {
                sats: 500,
                quote: 1
            }
        ]
    );
}

#[test]
fn cr_08_once_enough_has_arrived_the_sale_takes_no_new_quote() {
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 5_000);
    w.see(&tx, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    w.tick(301);
    let err = w.requote(o.index, 20_000_000).unwrap_err();
    assert_eq!(err, crate::RequoteRefused::AlreadyEnough);
    assert_eq!(err.rule(), "CR-08");
    assert_eq!(w.c.sale(o.index).unwrap().needs_sats(), 5_000);
}

#[test]
fn cr_08_the_frozen_need_does_not_move_with_a_new_clock() {
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 5_000);
    w.see(&tx, 1);
    w.judge(o.index);
    w.tick(10_000);
    assert_eq!(w.judge(o.index).needs_sats, 5_000);
}

#[test]
fn cr_08_a_frozen_need_is_dropped_when_what_arrived_falls_short() {
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 5_000);
    w.see(&tx, 1);
    w.judge(o.index);
    assert_eq!(w.c.sale(o.index).unwrap().frozen_need(), Some(5_000));
    // A smaller payment on the same coin takes the place of the group.
    let mut smaller = w.pay_from(&o, 1, 3_000);
    smaller.lock_time = bitcoin::absolute::LockTime::from_height(7).unwrap();
    w.see(&smaller, 1);
    let screen = w.judge(o.index);
    // The conflict also shows (CR-12 rule 3); the need is no longer frozen.
    assert_eq!(screen.state, State::DoNotHandOver);
    assert_eq!(screen.received_sats, 3_000);
    assert_eq!(w.c.sale(o.index).unwrap().frozen_need(), None);
}

// CR-16.

#[test]
fn cr_16_enough_is_what_the_sale_needs_less_the_tolerance() {
    // 50 basis points of 5,000 is 25.
    assert_eq!(tolerance(5_000, 50), 25);
    assert!(is_enough(4_975, 5_000, 50));
    assert!(!is_enough(4_974, 5_000, 50));
    assert!(!is_enough(0, 0, 50), "nothing is never enough");

    let mut w = World::new();
    let o = w.open_five();
    let short = w.pay(&o, 4_974);
    w.see(&short, 1);
    assert_eq!(w.judge(o.index).state, State::PartPaid);

    let mut w = World::new();
    let o = w.open_five();
    let near = w.pay(&o, 4_975);
    w.see(&near, 1);
    assert_eq!(w.judge(o.index).state, State::Waiting);
}

#[test]
fn cr_16_the_counters_tolerance_agrees_with_the_watchers_one_satoshi_either_side() {
    for need in [5_000u64, 12_345, 999_999, 1_000, 77_777_777] {
        let tol = tolerance(need, 50);
        for (paid, confirmed) in [(need - tol, true), (need - tol - 1, false)] {
            let script = p2wsh(1);
            let mut watch = SaleWatch::new(script.clone(), need, 50);
            let tx = spend(1, &[(&script, paid)]);
            let block = mined(1, &[tx]);
            let mut chain = TestChain::new(100);
            chain.add(block.block_hash(), 100);
            let wf = openbmt_watch::WellFormedBlock::parse(&bitcoin::consensus::encode::serialize(
                &block,
            ))
            .unwrap();
            for p in wf.payments(&chain, &script).unwrap() {
                watch.confirmed(p).unwrap();
            }
            let status = watch.status(&chain, T0);
            assert_eq!(
                status.min_confirmations >= 1,
                confirmed,
                "need {need}, paid {paid}"
            );
            assert_eq!(
                is_enough(paid, need, 50),
                confirmed,
                "need {need}, paid {paid}"
            );
        }
    }
}

#[test]
fn cr_16_more_than_is_needed_is_paid_in_full_and_the_surplus_is_owed() {
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 6_000);
    w.tick(10);
    w.see(&tx, 3);
    w.tick(8);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(screen.needs_sats, 5_000);
    assert_eq!(screen.received_sats, 6_000);
    assert!(screen.reports.contains(&Report::SurplusOwed {
        index: 0,
        sats: 1_000
    }));
    // Once only.
    assert!(!w
        .judge(o.index)
        .reports
        .iter()
        .any(|r| matches!(r, Report::SurplusOwed { .. })));
    // The exposure is the larger of the two.
    assert_eq!(w.c.exposure_outstanding(), 6_000);
}

#[test]
fn cr_16_the_instant_decision_is_asked_with_the_larger_of_need_and_arrival() {
    // 11,000 arrived for a 5,000 sale: above the 10,000 sat limit per sale.
    let mut w = World::new();
    let o = w.open_five();
    let tx = w.pay(&o, 11_000);
    w.tick(10);
    w.see(&tx, 3);
    w.tick(8);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(
        screen.why,
        crate::Why::Wait(openbmt_core::policy::WaitReason::OverSaleLimit)
    );
    assert_eq!(w.c.exposure_outstanding(), 0);
}

// CR-17.

#[test]
fn cr_17_what_arrived_below_1000_sats_is_dust_logged_and_changes_no_state() {
    let mut w = World::new();
    let o = w.open_five();
    let dust = w.pay(&o, DUST_SATS - 1);
    w.see(&dust, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::NotPaidYet);
    assert_eq!(screen.received_sats, 0);
    assert_eq!(
        screen.reports,
        vec![Report::Dust {
            index: 0,
            sats: DUST_SATS - 1
        }]
    );
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, crate::Event::Dust { sats: 999, .. })));
    // Logged once.
    assert!(w.judge(o.index).reports.is_empty());
    assert!(w.c.sale(o.index).unwrap().segments().is_empty());
}

#[test]
fn cr_17_dust_in_total_counts_once_it_reaches_the_line() {
    let mut w = World::new();
    let o = w.open_five();
    let a = w.pay(&o, 600);
    w.see(&a, 1);
    assert_eq!(w.judge(o.index).state, State::NotPaidYet);
    let b = w.pay(&o, 600);
    w.see(&b, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PartPaid);
    assert_eq!(screen.received_sats, 1_200);
}

#[test]
fn cr_17_a_sale_with_only_dust_still_expires() {
    let mut w = World::new();
    let o = w.open_five();
    let dust = w.pay(&o, 500);
    w.see(&dust, 1);
    w.tick(crate::EXPIRY_SECS);
    assert_eq!(w.judge(o.index).state, State::Expired);
}

#[test]
fn cr_17_a_sale_below_the_dust_line_is_not_offered() {
    let mut w = World::new();
    let facts = w.open_facts();
    // 40 cents is 400 sats.
    let err =
        w.c.open(
            Amount(40),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(err, crate::OpenRefused::BelowDust { needs_sats: 400 });
    assert_eq!(err.rule(), "CR-17");
}
