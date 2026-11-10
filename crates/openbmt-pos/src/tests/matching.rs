//! PS-30 to PS-37: matching.

use super::kit::*;
use crate::*;

fn note(s: &SaleFact) -> Option<Text> {
    Some(text(&format!("btc {}", s.reference.unwrap())))
}

fn only_match(l: &Ledger) -> (&TenderKey, &Match) {
    assert_eq!(l.matches().len(), 1, "{:?}", l.matches());
    l.matches().iter().next().unwrap()
}

#[test]
fn ps_30_the_same_input_gives_the_same_output_in_any_order() {
    let mut rng = Rng(0x5eed);
    for _ in 0..40 {
        let mut sales = Vec::new();
        let mut orders = Vec::new();
        for i in 0..8u32 {
            let cents = 100 * (1 + rng.below(3));
            let c = if rng.below(2) == 0 { 3 } else { 4 };
            let opened = T0 + rng.below(3_000);
            let mut s = sale(c, i, cents, opened);
            if rng.below(4) != 0 {
                s.paid_at = Some(opened + rng.below(300));
            }
            sales.push(s);
        }
        for j in 0..8 {
            let cents = 100 * (1 + rng.below(3));
            let loc = if rng.below(2) == 0 { "Main" } else { "Back" };
            let at = T0 + rng.below(4_000);
            let mut t = tender(&format!("t{j}"), cents, at);
            if rng.below(5) == 0 {
                t.reference = note(&sales[usize::try_from(rng.below(8)).unwrap()]);
            }
            if rng.below(6) == 0 {
                t.tip_minor = 50;
            }
            orders.push(order(
                &format!("o{j}"),
                loc,
                vec![t, cash(&format!("c{j}"), 100, at)],
            ));
        }
        let mut a = Ledger::new();
        step(&mut a, &orders, &sales, T0 + 9_000);
        let mut o2 = orders.clone();
        let mut s2 = sales.clone();
        rng.shuffle(&mut o2);
        rng.shuffle(&mut s2);
        let mut b = Ledger::new();
        step(&mut b, &o2, &s2, T0 + 9_000);
        assert_eq!(a, b);
        assert_eq!(a.to_bytes(), b.to_bytes());
    }
}

#[test]
fn ps_30_ties_are_broken_by_time_then_ids() {
    // Two tenders at the same second for two equal sales: the lower ids go
    // first, whatever the order they are handed in.
    let s1 = paid(sale(3, 1, 500, T0), T0 + 10);
    let s2 = paid(sale(3, 2, 500, T0), T0 + 10);
    let a = simple("a", "Main", 500, T0 + 60);
    let b = simple("b", "Main", 500, T0 + 60);
    for orders in [vec![a.clone(), b.clone()], vec![b, a]] {
        let mut l = Ledger::new();
        step(&mut l, &orders, &[s1.clone(), s2.clone()], T0 + 100);
        assert_eq!(l.matches()[&tkey("a", "t-a")].sale, s1.key());
        assert_eq!(l.matches()[&tkey("b", "t-b")].sale, s2.key());
    }
}

#[test]
fn ps_31_a_reference_in_the_tender_note_matches_exactly() {
    let s1 = paid(sale(3, 1, 500, T0), T0 + 10);
    let s2 = paid(sale(3, 2, 500, T0 + 20), T0 + 30);
    let mut t = tender("t", 500, T0 + 60);
    t.reference = note(&s2);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("o", "Main", vec![t])],
        &[s1.clone(), s2.clone()],
        T0 + 100,
    );
    let (_, m) = only_match(&l);
    assert_eq!(
        (m.sale, m.how, m.by_order),
        (s2.key(), MatchHow::Reference, false)
    );
    // The other sale has no tender yet, and is not an alert before the grace.
    assert!(l.open_alerts().is_empty());
}

#[test]
fn ps_31_a_reference_in_the_order_note_works_too_and_the_tender_wins() {
    let s1 = paid(sale(3, 1, 500, T0), T0 + 10);
    let s2 = paid(sale(3, 2, 500, T0), T0 + 10);
    let mut o = simple("o", "Main", 500, T0 + 60);
    o.reference = note(&s2);
    let mut l = Ledger::new();
    step(&mut l, &[o.clone()], &[s1.clone(), s2.clone()], T0 + 100);
    assert_eq!(only_match(&l).1.sale, s2.key());

    // Both carry one: the tender's is read first.
    o.tenders[0].reference = note(&s1);
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s1.clone(), s2], T0 + 100);
    assert_eq!(only_match(&l).1.sale, s1.key());
}

#[test]
fn ps_31_a_reference_to_another_location_or_no_sale_goes_to_pass_two() {
    let back = paid(sale(4, 1, 500, T0), T0 + 10);
    let main = paid(sale(3, 1, 500, T0), T0 + 10);
    let mut t = tender("t", 500, T0 + 60);
    t.reference = note(&back);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("o", "Main", vec![t.clone()])],
        &[back.clone(), main.clone()],
        T0 + 100,
    );
    let (_, m) = only_match(&l);
    assert_eq!((m.sale, m.how), (main.key(), MatchHow::AmountAndTime));

    // A reference that names no sale at all.
    let ghost = sale(3, 77, 500, T0);
    t.reference = note(&ghost);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("o", "Main", vec![t])],
        &[main.clone()],
        T0 + 100,
    );
    assert_eq!(only_match(&l).1.how, MatchHow::AmountAndTime);
}

#[test]
fn ps_32_a_reference_with_another_amount_is_an_alert_not_a_match() {
    let s = paid(sale(3, 1, 500, T0), T0 + 10);
    let other = paid(sale(3, 2, 700, T0), T0 + 10);
    let mut t = tender("t", 700, T0 + 60);
    t.reference = note(&s);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("o", "Main", vec![t])],
        &[s.clone(), other.clone()],
        T0 + 5_000,
    );
    // Not matched to the named sale, and not to the other of 7.00 either.
    assert!(l.matches().is_empty());
    let alert = l
        .open_alerts()
        .into_iter()
        .find(|a| a.key.kind == AlertKind::AmountDiffers)
        .unwrap();
    assert_eq!(
        (alert.tender_minor, alert.sale_minor, alert.sale),
        (700, 500, Some(s.key()))
    );
    // The named sale is claimed: no second alert for it. The other sale of 7.00
    // has no tender, which is its own alert.
    let k = kinds(&l);
    assert_eq!(
        k,
        vec![AlertKind::PaymentWithoutTender, AlertKind::AmountDiffers]
    );
}

#[test]
fn ps_32_a_second_tender_naming_a_taken_sale_is_a_duplicate() {
    let s = paid(sale(3, 1, 500, T0), T0 + 10);
    let spare = paid(sale(3, 2, 500, T0), T0 + 10);
    let mut a = tender("a", 500, T0 + 60);
    let mut b = tender("b", 500, T0 + 90);
    a.reference = note(&s);
    b.reference = note(&s);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("oa", "Main", vec![a]), order("ob", "Main", vec![b])],
        &[s.clone(), spare],
        T0 + 5_000,
    );
    assert_eq!(l.matches().len(), 1);
    assert_eq!(l.matches()[&tkey("oa", "a")].sale, s.key());
    let dup: Vec<_> = l
        .open_alerts()
        .into_iter()
        .filter(|a| a.key.kind == AlertKind::DuplicateReference)
        .collect();
    assert_eq!(dup.len(), 1);
    assert_eq!(dup[0].key.subject, Subject::Tender(tkey("ob", "b")));
    // The later tender did not take the spare sale by amount.
    assert!(!kinds(&l).contains(&AlertKind::TenderWithoutPayment));
}

#[test]
fn ps_33_the_window_runs_from_opening_to_thirty_minutes_after_paid() {
    let s = paid(sale(3, 1, 500, T0 + 100), T0 + 400);
    for (at, should) in [
        (T0 + 99, false),
        (T0 + 100, true),
        (T0 + 400 + 1_800, true),
        (T0 + 400 + 1_801, false),
    ] {
        let mut l = Ledger::new();
        step(
            &mut l,
            &[simple("o", "Main", 500, at)],
            &[s.clone()],
            T0 + 10_000,
        );
        assert_eq!(!l.matches().is_empty(), should, "at {}", at - T0);
    }
}

#[test]
fn ps_33_a_sale_never_paid_can_be_matched_for_a_shorter_while() {
    let s = sale(3, 1, 500, T0);
    for (at, should) in [(T0 + 900 + 1_800, true), (T0 + 900 + 1_801, false)] {
        let mut l = Ledger::new();
        step(
            &mut l,
            &[simple("o", "Main", 500, at)],
            &[s.clone()],
            T0 + 10_000,
        );
        assert_eq!(!l.matches().is_empty(), should);
    }
}

#[test]
fn ps_33_amount_and_currency_must_be_equal() {
    let s = paid(sale(3, 1, 500, T0), T0 + 10);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[simple("o", "Main", 501, T0 + 60)],
        &[s.clone()],
        T0 + 100,
    );
    assert!(l.matches().is_empty());
    let mut e = SaleFact::opened(counter(3), 1, openbmt_counter::Amount(500), eur(), T0);
    e.paid_at = Some(T0 + 10);
    let mut l = Ledger::new();
    step(&mut l, &[simple("o", "Main", 500, T0 + 60)], &[e], T0 + 100);
    assert!(l.matches().is_empty());
}

#[test]
fn ps_34_two_equal_amounts_minutes_apart_pair_in_time_order() {
    let s1 = paid(sale(3, 1, 500, T0), T0 + 60);
    let s2 = paid(sale(3, 2, 500, T0 + 300), T0 + 360);
    // The order ids are the wrong way round on purpose.
    let early = simple("z-early", "Main", 500, T0 + 400);
    let late = simple("a-late", "Main", 500, T0 + 500);
    let mut l = Ledger::new();
    step(&mut l, &[late, early], &[s2.clone(), s1.clone()], T0 + 700);
    let first = &l.matches()[&tkey("z-early", "t-z-early")];
    let second = &l.matches()[&tkey("a-late", "t-a-late")];
    assert_eq!(first.sale, s1.key());
    assert_eq!(second.sale, s2.key());
    assert!(first.by_order && second.by_order);
    assert_eq!(l.summary().matches_by_order, 2);
    assert!(l.open_alerts().is_empty());
}

#[test]
fn ps_34_an_unambiguous_pair_is_not_marked_by_order() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[simple("o", "Main", 500, T0 + 90)], &[s], T0 + 100);
    assert!(!only_match(&l).1.by_order);
}

#[test]
fn ps_34_one_sale_matches_one_tender_and_the_other_way_round() {
    // Two tenders, one sale.
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[
            simple("a", "Main", 500, T0 + 90),
            simple("b", "Main", 500, T0 + 95),
        ],
        &[s],
        T0 + 2_000,
    );
    assert_eq!(l.matches().len(), 1);
    assert_eq!(kinds(&l), vec![AlertKind::TenderWithoutPayment]);
    // Two sales, one tender.
    let s1 = paid(sale(3, 1, 500, T0), T0 + 60);
    let s2 = paid(sale(3, 2, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[simple("a", "Main", 500, T0 + 90)],
        &[s1, s2],
        T0 + 2_000,
    );
    assert_eq!(l.matches().len(), 1);
    assert_eq!(kinds(&l), vec![AlertKind::PaymentWithoutTender]);
}

#[test]
fn ps_35_a_tip_is_not_part_of_the_amount_and_is_reported() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut t = tender("t", 500, T0 + 90);
    t.tip_minor = 75;
    let mut l = Ledger::new();
    step(&mut l, &[order("o", "Main", vec![t])], &[s], T0 + 100);
    assert_eq!(l.matches().len(), 1);
    let alerts = l.open_alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(
        (alerts[0].key.kind, alerts[0].tender_minor),
        (AlertKind::BitcoinTip, 75)
    );
}

#[test]
fn ps_36_a_late_tender_uncrosses_a_pairing_made_by_order() {
    let s1 = paid(sale(3, 1, 500, T0), T0 + 60);
    let s2 = paid(sale(3, 2, 500, T0 + 300), T0 + 360);
    let late = simple("b", "Main", 500, T0 + 500);
    let early = simple("a", "Main", 500, T0 + 400);
    let mut l = Ledger::new();
    step(&mut l, &[late], &[s1.clone(), s2.clone()], T0 + 510);
    assert_eq!(l.matches()[&tkey("b", "t-b")].sale, s1.key());
    let d = step(&mut l, &[early], &[s1.clone(), s2.clone()], T0 + 520);
    assert_eq!(l.matches()[&tkey("a", "t-a")].sale, s1.key());
    assert_eq!(l.matches()[&tkey("b", "t-b")].sale, s2.key());
    assert_eq!(d.new_matches.len(), 2);
}

#[test]
fn ps_36_a_match_by_reference_never_changes() {
    let s1 = paid(sale(3, 1, 500, T0), T0 + 60);
    let s2 = paid(sale(3, 2, 500, T0), T0 + 60);
    let mut t = tender("t", 500, T0 + 90);
    t.reference = note(&s2);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("o", "Main", vec![t])],
        &[s1.clone(), s2.clone()],
        T0 + 100,
    );
    // A new tender without a reference cannot take the sale.
    step(
        &mut l,
        &[simple("p", "Main", 500, T0 + 110)],
        &[s1.clone(), s2.clone()],
        T0 + 120,
    );
    assert_eq!(l.matches()[&tkey("o", "t")].sale, s2.key());
    assert_eq!(l.matches()[&tkey("p", "t-p")].sale, s1.key());
}

#[test]
fn ps_36_a_match_stays_when_its_tender_is_voided_and_the_sale_stays_taken() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let o = simple("o", "Main", 500, T0 + 90);
    let mut l = Ledger::new();
    step(&mut l, &[o.clone()], &[s.clone()], T0 + 100);
    assert_eq!(l.matches().len(), 1);

    let mut voided = o;
    voided.tenders[0].state = TenderState::Voided;
    voided.updated_at = T0 + 200;
    step(&mut l, &[voided], &[s.clone()], T0 + 210);
    assert_eq!(l.matches().len(), 1);
    assert_eq!(kinds(&l), vec![AlertKind::VoidedAfterPayment]);

    // The cashier rings it again as a new order: the sale is already taken.
    step(
        &mut l,
        &[simple("o2", "Main", 500, T0 + 300)],
        &[s.clone()],
        T0 + 1_000,
    );
    assert_eq!(l.matches().len(), 1);
    assert!(kinds(&l).contains(&AlertKind::TenderWithoutPayment));
    assert!(!kinds(&l).contains(&AlertKind::PaymentWithoutTender));
}

#[test]
fn ps_36_a_match_stays_when_its_sale_leaves_the_facts() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[simple("o", "Main", 500, T0 + 90)], &[s], T0 + 100);
    step(&mut l, &[], &[], T0 + 50_000);
    assert_eq!(l.matches().len(), 1);
    assert!(l.open_alerts().is_empty());
}

#[test]
fn ps_37_a_paid_sale_of_a_counter_nobody_set_up_is_seen() {
    let stray = paid(sale(5, 1, 500, T0), T0 + 60);
    let quiet = paid(sale(9, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[], &[stray.clone(), quiet], T0 + 2_000);
    let alerts = l.open_alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].key.subject, Subject::Sale(stray.key()));
}
