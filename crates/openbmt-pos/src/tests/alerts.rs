//! PS-40 to PS-52 and PS-60 to PS-62: the alerts and what leaves.

use std::convert::Infallible;

use super::kit::*;
use crate::*;

fn alert_of(l: &Ledger, kind: AlertKind) -> Alert {
    l.open_alerts()
        .into_iter()
        .find(|a| a.key.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} in {:?}", kinds(l)))
        .clone()
}

#[test]
fn ps_40_a_tender_with_no_sale_alerts_after_ten_minutes() {
    let o = simple("o", "Main", 500, T0);
    let mut l = Ledger::new();
    step(&mut l, &[o.clone()], &[], T0 + 599);
    assert!(l.open_alerts().is_empty());
    let d = step(&mut l, &[], &[], T0 + 600);
    assert_eq!(kinds(&l), vec![AlertKind::TenderWithoutPayment]);
    assert_eq!(d.new_alerts.len(), 1);
    assert_eq!(
        alert_of(&l, AlertKind::TenderWithoutPayment).tender_minor,
        500
    );
}

#[test]
fn ps_41_an_order_closed_more_than_two_minutes_before_paid_alerts() {
    let s = paid(sale(3, 1, 500, T0), T0 + 300);
    for (at, alert) in [
        (T0 + 100, true),
        (T0 + 179, true),
        (T0 + 180, false),
        (T0 + 400, false),
    ] {
        let mut l = Ledger::new();
        step(
            &mut l,
            &[simple("o", "Main", 500, at)],
            &[s.clone()],
            T0 + 500,
        );
        assert_eq!(l.matches().len(), 1);
        assert_eq!(
            kinds(&l) == vec![AlertKind::ClosedBeforePaid],
            alert,
            "at {}",
            at - T0
        );
        if alert {
            assert_eq!(
                alert_of(&l, AlertKind::ClosedBeforePaid).secs,
                T0 + 300 - at
            );
        }
    }
}

#[test]
fn ps_42_a_paid_sale_with_no_tender_alerts_after_ten_minutes() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[], &[s.clone()], T0 + 60 + 599);
    assert!(l.open_alerts().is_empty());
    step(&mut l, &[], &[s.clone()], T0 + 60 + 600);
    assert_eq!(kinds(&l), vec![AlertKind::PaymentWithoutTender]);
    assert_eq!(
        alert_of(&l, AlertKind::PaymentWithoutTender).sale_minor,
        500
    );
}

#[test]
fn ps_42_not_for_no_pos_unpaid_or_unpaid_again_or_very_old_sales() {
    let quiet = paid(sale(9, 1, 500, T0), T0 + 60);
    let never = sale(3, 2, 500, T0);
    let mut again = paid(sale(3, 3, 500, T0), T0 + 60);
    again.unpaid_after = true;
    let mut l = Ledger::new();
    step(&mut l, &[], &[quiet, never, again], T0 + 5_000);
    assert!(l.open_alerts().is_empty());

    // Reported paid 8 days ago and never alerted: not newly raised.
    let old = paid(sale(3, 4, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[], &[old.clone()], T0 + 60 + 8 * 86_400);
    assert!(l.open_alerts().is_empty());
    // But one raised while young stays while its condition holds.
    let mut l = Ledger::new();
    step(&mut l, &[], &[old.clone()], T0 + 60 + 700);
    step(&mut l, &[], &[old], T0 + 60 + 8 * 86_400);
    assert_eq!(kinds(&l), vec![AlertKind::PaymentWithoutTender]);
}

#[test]
fn ps_43_an_amount_that_differs_names_both_amounts() {
    let s = paid(sale(3, 1, 500, T0), T0 + 10);
    let mut t = tender("t", 450, T0 + 60);
    t.reference = Some(text(&s.reference.unwrap().to_string()));
    let mut l = Ledger::new();
    step(&mut l, &[order("o", "Main", vec![t])], &[s], T0 + 100);
    let a = alert_of(&l, AlertKind::AmountDiffers);
    assert_eq!((a.tender_minor, a.sale_minor), (450, 500));
    assert!(a.why().contains("4.50") && a.why().contains("5.00"));
}

#[test]
fn ps_44_a_tender_on_a_sale_reported_unpaid_alerts() {
    let mut s = paid(sale(3, 1, 500, T0), T0 + 60);
    let o = simple("o", "Main", 500, T0 + 90);
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s.clone()], T0 + 100);
    assert!(l.open_alerts().is_empty());
    // A reorg: the counter reports the sale unpaid after the order closed.
    s.unpaid_after = true;
    let d = step(&mut l, &[], &[s.clone()], T0 + 200);
    assert_eq!(kinds(&l), vec![AlertKind::TenderOnUnpaidSale]);
    assert_eq!(d.new_alerts.len(), 1);
    // Paid again: resolved.
    s.unpaid_after = false;
    let d = step(&mut l, &[], &[s], T0 + 300);
    assert!(l.open_alerts().is_empty());
    assert_eq!(d.cleared.len(), 1);
}

#[test]
fn ps_44_a_tender_on_a_sale_never_paid_alerts_after_ten_minutes() {
    let s = sale(3, 1, 500, T0);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[simple("o", "Main", 500, T0 + 30)],
        &[s.clone()],
        T0 + 100,
    );
    assert!(l.open_alerts().is_empty());
    step(&mut l, &[], &[s], T0 + 30 + 600);
    assert_eq!(kinds(&l), vec![AlertKind::TenderOnUnpaidSale]);
}

#[test]
fn ps_45_voided_or_refunded_or_gone_after_payment_alerts() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let base = simple("o", "Main", 500, T0 + 90);
    let mut voided_order = base.clone();
    voided_order.state = OrderState::Voided;
    let mut voided_tender = base.clone();
    voided_tender.tenders[0].state = TenderState::Voided;
    let mut refunded = base.clone();
    refunded.tenders[0].state = TenderState::Refunded { amount_minor: 200 };
    let mut gone = base.clone();
    gone.tenders = vec![cash("c", 500, T0 + 90)];
    for changed in [voided_order, voided_tender, refunded, gone] {
        let mut l = Ledger::new();
        step(&mut l, &[base.clone()], &[s.clone()], T0 + 100);
        let mut c = changed;
        c.updated_at = T0 + 400;
        step(&mut l, &[c], &[s.clone()], T0 + 410);
        assert_eq!(kinds(&l), vec![AlertKind::VoidedAfterPayment]);
        assert!(l.matches().contains_key(&tkey("o", "t-o")));
    }
}

#[test]
fn ps_45_not_when_the_sale_is_not_paid_any_more() {
    let mut s = paid(sale(3, 1, 500, T0), T0 + 60);
    let base = simple("o", "Main", 500, T0 + 90);
    let mut l = Ledger::new();
    step(&mut l, &[base.clone()], &[s.clone()], T0 + 100);
    s.unpaid_after = true;
    let mut v = base;
    v.state = OrderState::Voided;
    v.updated_at = T0 + 400;
    step(&mut l, &[v], &[s], T0 + 410);
    assert!(l.open_alerts().is_empty());
}

#[test]
fn ps_46_a_duplicate_reference_alerts_on_the_later_tender() {
    // The scenario is in matching::ps_32; here the alert's own fields.
    let s = paid(sale(3, 1, 500, T0), T0 + 10);
    let mut a = tender("a", 500, T0 + 60);
    let mut b = tender("b", 500, T0 + 70);
    a.reference = Some(text(&s.reference.unwrap().to_string()));
    b.reference = a.reference.clone();
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("oa", "Main", vec![a]), order("ob", "Main", vec![b])],
        &[s.clone()],
        T0 + 100,
    );
    let al = alert_of(&l, AlertKind::DuplicateReference);
    assert_eq!(al.sale, Some(s.key()));
    assert_eq!(al.key.subject, Subject::Tender(tkey("ob", "b")));
}

#[test]
fn ps_47_a_tip_alerts_matched_or_not() {
    let mut t = tender("t", 500, T0);
    t.tip_minor = 100;
    let mut l = Ledger::new();
    step(&mut l, &[order("o", "Main", vec![t])], &[], T0 + 10);
    assert_eq!(kinds(&l), vec![AlertKind::BitcoinTip]);
}

#[test]
fn ps_48_another_currency_alerts_at_once() {
    let mut o = simple("o", "Main", 500, T0);
    o.currency = eur();
    let mut l = Ledger::new();
    step(&mut l, &[o], &[], T0 + 1);
    assert_eq!(kinds(&l), vec![AlertKind::UnknownCurrency]);
    // Not also a tender without payment.
    step(&mut l, &[], &[], T0 + 5_000);
    assert_eq!(kinds(&l), vec![AlertKind::UnknownCurrency]);
}

#[test]
fn ps_49_every_alert_explains_itself_in_plain_words() {
    let s = paid(sale(3, 1, 500, T0), T0 + 300);
    let mut t = tender("t", 500, T0 + 30);
    t.tip_minor = 25;
    let mut eur_order = simple("e", "Main", 100, T0);
    eur_order.currency = eur();
    let mut l = Ledger::new();
    step(
        &mut l,
        &[
            order("o", "Main", vec![t]),
            simple("w", "Main", 900, T0),
            eur_order,
            simple("p", "Main", 123, T0),
        ],
        &[s, paid(sale(3, 2, 777, T0), T0)],
        T0 + 700,
    );
    let all: Vec<Alert> = l.open_alerts().into_iter().cloned().collect();
    assert!(all.len() >= 5);
    for a in &all {
        let why = a.why();
        let next = a.next();
        assert!(why.len() > 30 && next.len() > 30, "{:?}", a.key.kind);
        assert!(why.ends_with('.') && next.ends_with('.'));
        assert!(!a.key.kind.name().is_empty());
        for word in ["sats", "satoshi", "txid", "address", "bc1"] {
            assert!(!why.to_lowercase().contains(word) && !next.to_lowercase().contains(word));
        }
    }
    assert!(all
        .iter()
        .any(|a| a.key.kind == AlertKind::TenderWithoutPayment && a.why().contains("9.00 USD")));
    assert!(alert_of(&l, AlertKind::BitcoinTip)
        .why()
        .contains("0.25 USD"));
    assert!(alert_of(&l, AlertKind::ClosedBeforePaid)
        .why()
        .contains("4 minutes"));
}

#[test]
fn ps_50_an_alert_is_raised_once_with_a_stable_identity() {
    let o = simple("o", "Main", 500, T0);
    let mut l = Ledger::new();
    let d = step(&mut l, &[o.clone()], &[], T0 + 700);
    assert_eq!(d.new_alerts.len(), 1);
    let raised = l.open_alerts()[0].raised_at;
    // The same poll again, and later polls: nothing new, same raised_at.
    for later in [T0 + 710, T0 + 3_000, T0 + 86_400] {
        let d = step(&mut l, &[o.clone()], &[], later);
        assert!(d.new_alerts.is_empty() && d.cleared.is_empty());
        assert_eq!(l.open_alerts().len(), 1);
        assert_eq!(l.open_alerts()[0].raised_at, raised);
    }
}

#[test]
fn ps_51_an_alert_is_cleared_by_a_state_that_resolves_it() {
    let o = simple("o", "Main", 500, T0);
    let mut l = Ledger::new();
    step(&mut l, &[o], &[], T0 + 700);
    assert_eq!(kinds(&l), vec![AlertKind::TenderWithoutPayment]);
    // The sale turns up.
    let s = paid(sale(3, 1, 500, T0), T0 + 100);
    let d = step(&mut l, &[], &[s], T0 + 800);
    assert!(l.open_alerts().is_empty());
    assert_eq!(d.cleared.len(), 1);
    assert_eq!(d.cleared[0].kind, AlertKind::TenderWithoutPayment);
    assert_eq!(l.matches().len(), 1);
}

#[test]
fn ps_51_an_alert_that_cannot_be_worked_out_again_stays_open() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[], &[s.clone()], T0 + 1_000);
    assert_eq!(kinds(&l), vec![AlertKind::PaymentWithoutTender]);
    // The sale is no longer among the facts: not resolved, only unknown.
    let d = step(&mut l, &[], &[], T0 + 2_000);
    assert_eq!(kinds(&l), vec![AlertKind::PaymentWithoutTender]);
    assert!(d.cleared.is_empty());
    // Its tender arrives and the sale is back: resolved.
    let d = step(
        &mut l,
        &[simple("o", "Main", 500, T0 + 900)],
        &[s],
        T0 + 2_100,
    );
    assert!(l.open_alerts().is_empty());
    assert_eq!(d.cleared.len(), 1);
}

#[test]
fn ps_52_an_acknowledgement_moves_an_alert_and_it_is_not_raised_again() {
    let o = simple("o", "Main", 500, T0);
    let mut l = Ledger::new();
    step(&mut l, &[o.clone()], &[], T0 + 700);
    let key = l.open_alerts()[0].key.clone();
    l.acknowledge_stored(&key, T0 + 800, |_| Ok::<(), Infallible>(()))
        .unwrap();
    assert!(l.open_alerts().is_empty());
    assert!(l.is_acknowledged(&key) && !l.is_open(&key));
    assert_eq!(l.summary().acknowledged_alerts, 1);
    let d = step(&mut l, &[o], &[], T0 + 5_000);
    assert!(d.new_alerts.is_empty() && l.open_alerts().is_empty());
    assert!(l.is_acknowledged(&key));
    // When the condition goes, so does the acknowledged alert.
    let s = paid(sale(3, 1, 500, T0), T0 + 100);
    step(&mut l, &[], &[s], T0 + 6_000);
    assert!(!l.is_acknowledged(&key));
}

#[test]
fn ps_52_only_an_open_alert_can_be_acknowledged_and_a_failed_store_changes_nothing() {
    let mut l = Ledger::new();
    step(&mut l, &[simple("o", "Main", 500, T0)], &[], T0 + 700);
    let key = l.open_alerts()[0].key.clone();
    let before = l.clone();
    assert_eq!(
        l.acknowledge_stored(&key, T0, |_| Err::<(), _>("disk full")),
        Err(LedgerWriteError::Persist("disk full"))
    );
    assert_eq!(l, before);
    l.acknowledge_stored(&key, T0, |_| Ok::<(), Infallible>(()))
        .unwrap();
    assert_eq!(
        l.acknowledge_stored(&key, T0, |_| Ok::<(), Infallible>(())),
        Err(LedgerWriteError::Refused(LedgerError::NotOpen))
    );
}

#[test]
fn ps_60_a_poll_returns_what_is_new() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    let d = step(
        &mut l,
        &[
            simple("o", "Main", 500, T0 + 90),
            simple("x", "Main", 999, T0 + 90),
        ],
        &[s],
        T0 + 100,
    );
    assert_eq!(d.new_matches.len(), 1);
    assert_eq!(d.new_matches[0].0, tkey("o", "t-o"));
    assert!(d.new_alerts.is_empty() && d.cleared.is_empty());
    assert_eq!(d.retired_matches, 0);
    let d = step(&mut l, &[], &[], T0 + 100);
    assert_eq!(d, Delta::default());
}

#[test]
fn ps_61_the_owner_sees_open_alerts_oldest_first_and_a_summary() {
    let mut l = Ledger::new();
    step(&mut l, &[simple("b", "Main", 500, T0 + 100)], &[], T0 + 800);
    step(&mut l, &[simple("a", "Main", 600, T0 + 100)], &[], T0 + 900);
    let open = l.open_alerts();
    assert_eq!(open.len(), 2);
    assert!(open[0].raised_at < open[1].raised_at);
    assert_eq!(open[0].key.subject, Subject::Tender(tkey("b", "t-b")));
    let s = l.summary();
    assert_eq!(
        (s.open_alerts, s.acknowledged_alerts, s.tracked_orders),
        (2, 0, 2)
    );
    assert_eq!(
        (s.matches, s.matches_by_order, s.retired_matches),
        (0, 0, 0)
    );
}

#[test]
fn ps_62_the_books_get_a_line_for_each_final_match_and_nothing_else() {
    let mut s1 = paid(sale(3, 1, 500, T0), T0 + 60);
    s1.is_final = true;
    let s2 = paid(sale(3, 2, 700, T0), T0 + 60);
    let mut l = Ledger::new();
    step(
        &mut l,
        &[
            simple("a", "Main", 500, T0 + 90),
            simple("b", "Main", 700, T0 + 90),
        ],
        &[s1.clone(), s2.clone()],
        T0 + 100,
    );
    let lines = l.books_lines(&[s1.clone(), s2.clone()]);
    assert_eq!(
        lines,
        vec![BooksLine {
            order: oid("a"),
            tender: oid("t-a"),
            sale: s1.key(),
            amount_minor: 500,
            currency: usd(),
        }]
    );
    // A voided order is not a sale.
    let mut v = simple("a", "Main", 500, T0 + 90);
    v.state = OrderState::Voided;
    v.updated_at = T0 + 500;
    step(&mut l, &[v], &[s1.clone(), s2], T0 + 510);
    assert!(l.books_lines(&[s1]).is_empty());
}
