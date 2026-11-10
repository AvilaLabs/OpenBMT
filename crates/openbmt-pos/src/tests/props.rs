//! Property-style tests with a fixed-seed generator: a few thousand random
//! shops, each checked against the properties the rules promise.

use std::collections::BTreeSet;
use std::convert::Infallible;

use super::kit::*;
use crate::*;

/// One random shop: orders and counter sales.
fn scenario(rng: &mut Rng, refs: bool) -> (Vec<PosOrder>, Vec<SaleFact>) {
    let mut sales = Vec::new();
    for i in 0..u32::try_from(rng.below(12)).unwrap() {
        let c = [3u8, 3, 4, 5, 9][usize::try_from(rng.below(5)).unwrap()];
        let opened = T0 + rng.below(3_000);
        let mut s = sale(c, i, 100 * (1 + rng.below(3)), opened);
        match rng.below(8) {
            0 => {}
            1 => {
                s.paid_at = Some(opened + rng.below(2_000));
                s.unpaid_after = true;
            }
            _ => {
                s.paid_at = Some(opened + rng.below(2_000));
                s.is_final = rng.below(3) == 0;
            }
        }
        sales.push(s);
    }
    let mut orders = Vec::new();
    for j in 0..rng.below(14) {
        let at = T0 + rng.below(5_000);
        let mut tenders = Vec::new();
        for k in 0..1 + rng.below(3) {
            let mut t = tender(
                &format!("t{k}"),
                100 * (1 + rng.below(3)),
                at + rng.below(60),
            );
            match rng.below(6) {
                0 => t.name = text("  bitcoin "),
                1 => {
                    t.name = text("Crypto");
                    t.kind = TenderKind::Other;
                }
                2 => {
                    t.name = text("Cash");
                    t.kind = TenderKind::Cash;
                }
                _ => {}
            }
            match rng.below(8) {
                0 => t.state = TenderState::Voided,
                1 => t.state = TenderState::Refunded { amount_minor: 50 },
                _ => {}
            }
            if rng.below(6) == 0 {
                t.tip_minor = 25;
            }
            if refs && rng.below(4) == 0 && !sales.is_empty() {
                let s = &sales
                    [usize::try_from(rng.below(u64::try_from(sales.len()).unwrap())).unwrap()];
                t.reference = Some(text(&format!("ref {}", s.reference.unwrap())));
            } else if refs && rng.below(10) == 0 {
                t.reference = Some(text("no code here"));
            }
            tenders.push(t);
        }
        let loc = ["Main", "Main", "Back", "Elsewhere"][usize::try_from(rng.below(4)).unwrap()];
        let mut o = order(&format!("o{j}"), loc, tenders);
        if rng.below(10) == 0 {
            o.state = OrderState::Voided;
        }
        if rng.below(12) == 0 {
            o.currency = eur();
        }
        orders.push(o);
    }
    (orders, sales)
}

const NOW: u64 = T0 + 100_000;

fn tender_keys_to_account_for(orders: &[PosOrder]) -> BTreeSet<TenderKey> {
    let s = settings();
    orders
        .iter()
        .filter(|o| o.state != OrderState::Voided)
        .flat_map(|o| {
            o.tenders
                .iter()
                .filter(|t| t.state == TenderState::Paid && s.is_bitcoin(t))
                .map(|t| TenderKey {
                    order: o.id.clone(),
                    tender: t.id.clone(),
                })
        })
        .collect()
}

#[test]
fn ps_30_prop_the_result_does_not_depend_on_input_order() {
    let mut rng = Rng(0x0b17_c0de);
    for _ in 0..300 {
        let (orders, sales) = scenario(&mut rng, true);
        let mut a = Ledger::new();
        step(&mut a, &orders, &sales, NOW);
        for _ in 0..3 {
            let (mut o, mut s) = (orders.clone(), sales.clone());
            rng.shuffle(&mut o);
            rng.shuffle(&mut s);
            for ord in &mut o {
                rng.shuffle(&mut ord.tenders);
            }
            let mut b = Ledger::new();
            step(&mut b, &o, &s, NOW);
            assert_eq!(a, b);
        }
    }
}

#[test]
fn ps_34_prop_no_sale_or_tender_is_in_two_matches() {
    let mut rng = Rng(0x1234_5678);
    for _ in 0..300 {
        let (orders, sales) = scenario(&mut rng, true);
        let mut l = Ledger::new();
        step(&mut l, &orders, &sales, NOW);
        let sale_keys: BTreeSet<SaleKey> = l.matches().values().map(|m| m.sale).collect();
        assert_eq!(
            sale_keys.len(),
            l.matches().len(),
            "a sale is in two matches"
        );
        // A tender is the key of the map, so it is in one match at most; check
        // that every match names a tender that exists.
        for k in l.matches().keys() {
            let o = &l.orders()[&k.order];
            assert!(o.tender(&k.tender).is_some());
        }
    }
}

#[test]
fn ps_40_prop_every_paid_bitcoin_tender_is_matched_or_has_an_alert() {
    let mut rng = Rng(0xfeed_f00d);
    for _ in 0..300 {
        let (orders, sales) = scenario(&mut rng, true);
        let mut l = Ledger::new();
        step(&mut l, &orders, &sales, NOW);
        for key in tender_keys_to_account_for(&orders) {
            let matched = l.matches().contains_key(&key);
            let alerted = l
                .open_alerts()
                .iter()
                .any(|a| a.key.subject == Subject::Tender(key.clone()));
            assert!(matched || alerted, "{key:?} is neither");
        }
    }
}

#[test]
fn ps_74_prop_replaying_the_same_pages_changes_nothing_and_raises_nothing() {
    let mut rng = Rng(0xabad_1dea);
    for _ in 0..300 {
        let (orders, sales) = scenario(&mut rng, true);
        let mut l = Ledger::new();
        step(&mut l, &orders, &sales, NOW);
        let before = l.to_bytes();
        for later in [NOW, NOW + 1, NOW + 5_000] {
            let d = step(&mut l, &orders, &sales, later);
            assert_eq!(d, Delta::default());
            assert_eq!(l.to_bytes(), before);
        }
    }
}

#[test]
fn ps_74_prop_two_polls_end_where_one_poll_of_the_lot_does_for_alerts() {
    // Without references, the same orders split over two polls give the same
    // pairs and alerts as one poll, once the later poll has run. (A reference
    // match is kept as made, so it can depend on what was seen first.)
    let mut rng = Rng(0x0dd5_eed5);
    for _ in 0..200 {
        let (orders, sales) = scenario(&mut rng, false);
        let mut whole = Ledger::new();
        step(&mut whole, &orders, &sales, NOW);
        let cut = orders.len() / 2;
        let mut split = Ledger::new();
        step(&mut split, &orders[..cut], &sales, NOW - 1);
        step(&mut split, &orders[cut..], &sales, NOW);
        let keys = |l: &Ledger| -> BTreeSet<AlertKey> {
            l.open_alerts().iter().map(|a| a.key.clone()).collect()
        };
        assert_eq!(keys(&whole), keys(&split));
        let pairs = |l: &Ledger| -> Vec<(TenderKey, SaleKey, bool)> {
            l.matches()
                .iter()
                .map(|(k, m)| (k.clone(), m.sale, m.by_order))
                .collect()
        };
        assert_eq!(pairs(&whole), pairs(&split));
    }
}

#[test]
fn ps_71_prop_the_stored_ledger_round_trips_and_damage_is_refused() {
    let mut rng = Rng(0x0571_04e5);
    for round in 0..40 {
        let (orders, sales) = scenario(&mut rng, true);
        let mut l = Ledger::new();
        l.update_stored(
            &orders,
            Cursor::new(vec![u8::try_from(round).unwrap(); 5]).unwrap(),
            &sales,
            &settings(),
            NOW,
            |_| Ok::<(), Infallible>(()),
        )
        .unwrap();
        let keys: Vec<AlertKey> = l
            .open_alerts()
            .iter()
            .take(2)
            .map(|a| a.key.clone())
            .collect();
        for k in keys {
            l.acknowledge_stored(&k, NOW, |_| Ok::<(), Infallible>(()))
                .unwrap();
        }
        let bytes = l.to_bytes();
        let back = Ledger::from_bytes(&bytes).unwrap();
        assert_eq!(back, l);
        assert_eq!(back.to_bytes(), bytes);
        for n in 0..bytes.len() {
            assert!(Ledger::from_bytes(&bytes[..n]).is_err());
        }
        let step_by = (bytes.len() / 150).max(1);
        for i in (0..bytes.len()).step_by(step_by) {
            let mut b = bytes.clone();
            b[i] ^= 0x10;
            match Ledger::from_bytes(&b) {
                Err(_) => {}
                Ok(read) => assert_eq!(read, l),
            }
        }
    }
}
