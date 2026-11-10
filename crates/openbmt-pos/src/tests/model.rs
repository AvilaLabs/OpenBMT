//! PS-01 to PS-04 and PS-10 to PS-14: the model, what counts as a Bitcoin
//! tender, and the settings.

use super::kit::*;
use crate::*;

#[test]
fn ps_01_an_order_id_is_one_to_sixty_four_printable_ascii_bytes() {
    assert!(Id::new("A").is_ok());
    assert!(Id::new(&"x".repeat(64)).is_ok());
    assert!(Id::new("with space ~!").is_ok());
    assert_eq!(Id::new(""), Err(ModelError::BadId));
    assert_eq!(Id::new(&"x".repeat(65)), Err(ModelError::BadId));
    assert_eq!(Id::new("tab\t"), Err(ModelError::BadId));
    assert_eq!(Id::new("caf\u{e9}"), Err(ModelError::BadId));
    assert_eq!(Id::new("del\u{7f}"), Err(ModelError::BadId));
}

#[test]
fn ps_01_an_order_over_its_bounds_is_refused_whole() {
    let tenders: Vec<PosTender> = (0..17).map(|i| tender(&format!("t{i}"), 100, T0)).collect();
    let o = order("o1", "Main", tenders);
    assert_eq!(o.validate(), Err(ModelError::TooManyTenders));
    let sixteen: Vec<PosTender> = (0..16).map(|i| tender(&format!("t{i}"), 100, T0)).collect();
    assert_eq!(order("o1", "Main", sixteen).validate(), Ok(()));
}

#[test]
fn ps_01_a_page_with_one_bad_order_is_refused_whole() {
    let bad_tenders: Vec<PosTender> = (0..17).map(|i| tender(&format!("t{i}"), 100, T0)).collect();
    let page = Page {
        orders: vec![
            simple("good", "Main", 500, T0),
            order("bad", "Main", bad_tenders),
        ],
        next: Cursor::new(vec![1]).unwrap(),
        more: false,
    };
    let mut ledger = Ledger::new();
    let mut pos = MemoryPos::new([page]);
    let before = ledger.clone();
    let r = sync(&mut pos, &mut ledger, &[], &settings(), T0 + 10, ok);
    assert_eq!(
        r,
        Err(SyncFailed {
            error: SyncError::Ledger(LedgerError::Order(ModelError::TooManyTenders)),
            pages_kept: 0
        })
    );
    assert_eq!(ledger, before);
}

#[test]
fn ps_02_tender_ids_are_unique_and_refunds_fit() {
    let dup = order(
        "o1",
        "Main",
        vec![tender("t", 100, T0), tender("t", 200, T0)],
    );
    assert_eq!(dup.validate(), Err(ModelError::DuplicateTender));

    let mut t = tender("t", 100, T0);
    t.tip_minor = 20;
    t.state = TenderState::Refunded { amount_minor: 120 };
    assert_eq!(order("o1", "Main", vec![t.clone()]).validate(), Ok(()));
    t.state = TenderState::Refunded { amount_minor: 121 };
    assert_eq!(
        order("o1", "Main", vec![t]).validate(),
        Err(ModelError::RefundTooLarge)
    );
}

#[test]
fn ps_03_text_is_one_to_sixty_four_bytes_without_control_characters() {
    assert!(Text::new("Main street").is_ok());
    assert!(Text::new("Caf\u{e9}").is_ok());
    assert!(Text::new(&"y".repeat(64)).is_ok());
    assert_eq!(Text::new(""), Err(ModelError::BadText));
    assert_eq!(Text::new(&"y".repeat(65)), Err(ModelError::BadText));
    assert_eq!(Text::new("a\nb"), Err(ModelError::BadText));
    assert_eq!(Text::new("a\u{0}b"), Err(ModelError::BadText));
}

#[test]
fn ps_04_the_two_clocks_are_not_otherwise_corrected() {
    // The tender is 121 seconds before the sale opened: past the slack.
    let s = sale(3, 1, 500, T0 + 200);
    let o = simple("o1", "Main", 500, T0 + 79);
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s], T0 + 1_000);
    assert!(l.matches().is_empty());
    assert_eq!(kinds(&l), vec![AlertKind::TenderWithoutPayment]);
}

#[test]
fn ps_10_a_bitcoin_tender_is_its_kind_or_its_exact_name() {
    let s = settings();
    let mut t = tender("t", 100, T0);
    assert!(s.is_bitcoin(&t));
    t.name = text("  bItCoIn ");
    assert!(s.is_bitcoin(&t));
    t.name = text("BITCOIN");
    assert!(s.is_bitcoin(&t));
    t.name = text("Bitcoin Cash");
    assert!(!s.is_bitcoin(&t));
    t.name = text("Bit coin");
    assert!(!s.is_bitcoin(&t));
    t.name = text("Bitcoin\u{ff}");
    assert!(!s.is_bitcoin(&t));
    // The POS's own kind counts whatever the name.
    t.kind = TenderKind::Bitcoin;
    t.name = text("Crypto");
    assert!(s.is_bitcoin(&t));
}

#[test]
fn ps_11_another_name_is_never_guessed_to_be_bitcoin() {
    let s = settings();
    for name in ["BTC", "Crypto", "Gift card", "Sat", "Lightning"] {
        let mut t = tender("t", 100, T0);
        t.name = text(name);
        t.kind = TenderKind::Other;
        assert!(!s.is_bitcoin(&t), "{name}");
    }
    // Named like the setting but of another kind: counted, the safe side.
    let mut t = tender("t", 100, T0);
    t.kind = TenderKind::Card;
    assert!(s.is_bitcoin(&t));
}

#[test]
fn ps_12_only_paid_tenders_on_orders_not_voided_are_candidates() {
    let s = sale(3, 1, 500, T0);
    let s = paid(s, T0 + 60);
    let mut voided_tender = tender("v", 500, T0 + 90);
    voided_tender.state = TenderState::Voided;
    let mut refunded = tender("r", 500, T0 + 90);
    refunded.state = TenderState::Refunded { amount_minor: 500 };
    let mut voided_order = simple("vo", "Main", 500, T0 + 90);
    voided_order.state = OrderState::Voided;
    let cash_order = order("c", "Main", vec![cash("c1", 500, T0 + 90)]);
    let orders = vec![
        order("o1", "Main", vec![voided_tender]),
        order("o2", "Main", vec![refunded]),
        voided_order,
        cash_order,
    ];
    let mut l = Ledger::new();
    step(&mut l, &orders, &[s], T0 + 5_000);
    // Nothing matched, nothing alerted about a tender; the sale is alone.
    assert!(l.matches().is_empty());
    assert_eq!(kinds(&l), vec![AlertKind::PaymentWithoutTender]);
}

#[test]
fn ps_12_split_tenders_compare_only_the_bitcoin_amount() {
    let s = paid(sale(3, 1, 300, T0), T0 + 60);
    let o = order(
        "o1",
        "Main",
        vec![cash("c", 700, T0 + 70), tender("b", 300, T0 + 80)],
    );
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s], T0 + 100);
    assert_eq!(l.matches().len(), 1);
    assert!(l.matches().contains_key(&tkey("o1", "b")));
    assert!(l.open_alerts().is_empty());
}

#[test]
fn ps_13_a_candidate_in_another_currency_is_not_matched_and_alerts() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut o = simple("o1", "Main", 500, T0 + 70);
    o.currency = eur();
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s], T0 + 100);
    assert!(l.matches().is_empty());
    assert!(kinds(&l).contains(&AlertKind::UnknownCurrency));
}

#[test]
fn ps_14_settings_map_locations_exactly_and_hold_their_bounds() {
    let base = settings();
    assert_eq!(base.validate(), Ok(()));
    assert_eq!(base.counters_at(&text("Main")), &[counter(3)]);
    assert!(base.counters_at(&text("main")).is_empty());
    assert!(base.is_no_pos(&counter(9)));

    let mut s = base.clone();
    s.locations[1].counters.push(counter(3));
    assert_eq!(s.validate(), Err(SettingsError::Locations));
    let mut s = base.clone();
    s.no_pos.push(counter(3));
    assert_eq!(s.validate(), Err(SettingsError::Locations));
    let mut s = base.clone();
    s.locations[1].location = text("Main");
    assert_eq!(s.validate(), Err(SettingsError::Locations));
    let mut s = base.clone();
    s.bitcoin_tenders.clear();
    assert_eq!(s.validate(), Err(SettingsError::TenderNames));
    let mut s = base.clone();
    s.bitcoin_tenders = vec![text("   ")];
    assert_eq!(s.validate(), Err(SettingsError::TenderNames));
    let mut s = base.clone();
    s.currencies = vec![usd(), usd()];
    assert_eq!(s.validate(), Err(SettingsError::Currencies));
    let mut s = base.clone();
    s.timing.grace_secs = 0;
    assert_eq!(s.validate(), Err(SettingsError::Timing));
    let mut s = base;
    s.locations[0].counters = (10..27).map(counter).collect();
    assert_eq!(s.validate(), Err(SettingsError::Locations));
}

#[test]
fn ps_14_a_location_the_settings_do_not_name_cannot_match() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let o = simple("o1", "Elsewhere", 500, T0 + 70);
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s], T0 + 1_000);
    assert!(l.matches().is_empty());
    let k = kinds(&l);
    assert!(k.contains(&AlertKind::TenderWithoutPayment));
    assert!(k.contains(&AlertKind::PaymentWithoutTender));
}

#[test]
fn ps_15_the_tenders_kept_are_those_of_the_settings_when_the_order_was_taken() {
    let mut crypto = tender("t", 500, T0);
    crypto.name = text("Crypto");
    let o = order("o", "Main", vec![crypto, tender("b", 100, T0)]);
    // First seen when only "Bitcoin" counts: the other tender is not kept.
    let l = Ledger::new()
        .updated(&[o.clone()], Cursor::start(), &[], &settings(), T0 + 10)
        .unwrap()
        .0;
    assert_eq!(l.orders()[&oid("o")].tenders.len(), 1);
    // Later the shop adds the name. The stored order is not read again...
    let mut more = settings();
    more.bitcoin_tenders.push(text("Crypto"));
    let (l2, _) = l
        .updated(&[], Cursor::start(), &[], &more, T0 + 20)
        .unwrap();
    assert_eq!(l2.orders()[&oid("o")].tenders.len(), 1);
    // ...until the POS hands the order in again.
    let (l3, _) = l2
        .updated(&[o], Cursor::start(), &[], &more, T0 + 30)
        .unwrap();
    assert_eq!(l3.orders()[&oid("o")].tenders.len(), 2);
}
