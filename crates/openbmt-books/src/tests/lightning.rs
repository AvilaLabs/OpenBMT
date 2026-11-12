//! BK-32 to BK-34: a sale paid over Lightning.

use openbmt_counter::{Amount, Event};

use super::fixture::*;
use crate::{Books, Counting, PaymentMode, RevealsPayments, SaleState, Settlement, Value};

const HASH: [u8; 32] = [0x5a; 32];
const MSAT: u64 = 4_321_987_654;

fn lightning_event(index: u32, at: u64, msat: u64) -> Event {
    Event::PaidByLightning {
        index,
        at,
        payment_hash: HASH,
        amount_msat: msat,
        settled_at: Some(at - 3),
        valued: Amount(FINAL_VALUED),
    }
}

/// Sale 0 paid over Lightning, with a POS record; sale 1 paid on-chain.
fn mixed() -> Kit {
    kit_with(&[
        opened(0, FEB_1_0100, 2500),
        lightning_event(0, FEB_1_0100 + 20, MSAT),
        Event::PosPaid {
            index: 0,
            at: FEB_1_0100 + 20,
        },
        opened(1, FEB_1_0100 + 40, 1850),
        final_event(1, FEB_2_NOON, SATS_PAID),
    ])
}

#[test]
fn bk_32_a_lightning_sale_is_final_at_once_and_says_how_it_was_paid() {
    let kit = mixed();
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.state, SaleState::Final);
    assert_eq!(s.counting(), Counting::Counted);
    assert_eq!(s.first_funds_at, Some(FEB_1_0100 + 20));
    // The satoshis are the millisatoshis rounded down.
    assert_eq!(s.sats_received, Value::Known(MSAT / 1_000));
    assert_eq!(s.value_at_first_seen_minor, Value::Known(FINAL_VALUED));
    assert_eq!(s.payment_mode.text(), "lightning");
    let p = s.payment_mode.lightning().expect("a Lightning payment");
    assert_eq!(p.payment_hash, HASH);
    assert_eq!(p.amount_msat, MSAT);
    assert_eq!(p.settled_at, Some(FEB_1_0100 + 17));
    // It has no transaction, and says so rather than "unknown".
    match &s.payments {
        Value::Known(list) => assert!(list.payments.is_empty() && !list.more),
        Value::Unknown(f) => panic!("unknown: {f:?}"),
    }
    // A sale paid on the chain is on-chain.
    let other = books.sale(&kit.key(1)).expect("sale");
    assert_eq!(other.payment_mode, PaymentMode::OnChain);
    assert_eq!(other.payment_mode.text(), "on-chain");
    assert!(other.payment_mode.lightning().is_none());
}

#[test]
fn bk_32_the_ledger_has_the_mode_the_hash_the_amount_and_the_time() {
    let kit = mixed();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let header = ledger
        .lines()
        .find(|l| l.starts_with("Type,"))
        .expect("header");
    assert!(header
        .contains("Payment mode,Payment hash,Lightning amount (msat),Lightning settled (UTC)"));
    let rows: Vec<&str> = ledger.lines().filter(|l| l.starts_with("sale,")).collect();
    assert_eq!(rows.len(), 2);
    let hash_hex = "5a".repeat(32);
    let ln = rows
        .iter()
        .find(|r| r.contains("lightning"))
        .expect("a row");
    assert!(ln.contains(&hash_hex), "{ln}");
    assert!(ln.contains(&MSAT.to_string()));
    assert!(
        ln.ends_with(&format!(
            ",lightning,{hash_hex},{MSAT},2025-02-01T01:00:17Z,,"
        )),
        "{ln}"
    );
    assert!(ln.contains("paid over Lightning"));
    assert!(!ln.contains("the final record names"));
    let chain = rows.iter().find(|r| r.contains("on-chain")).expect("a row");
    assert!(chain.ends_with(",on-chain,,,,,"), "{chain}");
}

#[test]
fn bk_32_the_accounting_export_counts_it_with_fixed_words_only() {
    let kit = mixed();
    let files = kit.books().accounting_export().expect("export");
    let all: String = files.iter().map(|(_, f)| f.as_str()).collect();
    // The month of the Lightning sale has it as a counted sale.
    assert!(all.contains("25.00"), "{all}");
    // No hash, no millisatoshis, no mode word.
    assert!(!all.contains(&"5a".repeat(8)));
    assert!(!all.contains(&MSAT.to_string()));
    assert!(!all.to_lowercase().contains("lightning"));
}

#[test]
fn bk_32_a_late_payment_on_the_address_is_added_and_listed() {
    let events = vec![
        opened(0, FEB_1_0100, 2500),
        lightning_event(0, FEB_1_0100 + 20, MSAT),
        Event::SurplusOwed {
            index: 0,
            at: FEB_1_0100 + 20,
            sats: 3_000,
        },
        Event::LatePayment {
            index: 0,
            at: FEB_2_NOON,
            txid: TXID_LATE,
            sats: SATS_LATE,
        },
    ];
    let kit = kit_with(&events);
    let s = kit.books().sale(&kit.key(0)).expect("sale").clone();
    assert_eq!(s.state, SaleState::Final);
    assert_eq!(s.surplus_owed_sats, 3_000);
    assert_eq!(s.late_payments.len(), 1);
    assert_eq!(s.sats_received, Value::Known(MSAT / 1_000 + SATS_LATE));
    assert!(matches!(
        s.settlement(),
        Some(Settlement::Overpaid {
            surplus_sats: 3_000
        })
    ));
}

#[test]
fn bk_32_an_invoice_paid_late_is_listed_as_a_late_payment_by_its_hash() {
    let events = vec![
        opened(0, FEB_1_0100, 2500),
        Event::Expired {
            index: 0,
            at: FEB_1_0100 + 900,
        },
        Event::LateLightning {
            index: 0,
            at: FEB_2_NOON,
            payment_hash: [0x5a; 32],
            amount_msat: 2_500_400,
        },
    ];
    let kit = kit_with(&events);
    let s = kit.books().sale(&kit.key(0)).expect("sale").clone();
    assert_eq!(s.state, SaleState::LatePayment);
    assert_eq!(s.late_payments.len(), 1);
    assert_eq!(s.late_payments[0].txid, [0x5a; 32]);
    assert_eq!(s.late_payments[0].sats, 2_500);
    assert_eq!(s.payment_mode, PaymentMode::OnChain);
}

#[test]
fn bk_33_the_bundle_is_version_two_only_with_a_lightning_sale() {
    let kit = mixed();
    let books = kit.books();
    let bytes = books.to_bundle();
    assert_eq!(&bytes[..4], b"OBM6");
    assert_eq!(bytes[4], 2);
    let back = Books::from_bundle(&bytes).expect("bundle");
    assert_eq!(back.to_bundle(), bytes);
    assert_eq!(back.sales()[0].payment_mode, books.sales()[0].payment_mode);
    for n in 0..bytes.len() {
        assert!(Books::from_bundle(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Books::from_bundle(&longer).is_err());
    // No sale paid over Lightning: the bundle is the one earlier builds wrote.
    let plain = kit_with(&[opened(0, FEB_1_0100, 2500), final_event(0, FEB_2_NOON, 5)]);
    let plain_bytes = plain.books().to_bundle();
    assert_eq!(plain_bytes[4], 1);
    assert_eq!(
        Books::from_bundle(&plain_bytes).expect("bundle").sales()[0].payment_mode,
        PaymentMode::OnChain
    );
}

#[test]
fn bk_33_a_version_two_bundle_without_a_lightning_sale_is_refused() {
    let plain = kit_with(&[opened(0, FEB_1_0100, 2500), final_event(0, FEB_2_NOON, 5)]);
    let v1 = plain.books().to_bundle();
    // Rebuild the same sale as version 2 by hand: the mode byte 0 at the end
    // of the sale, which is the end of the sales' part.
    let lightning = mixed().books().to_bundle();
    assert!(Books::from_bundle(&lightning).is_ok());
    // A mode byte that is not known is refused.
    let mut bad = lightning.clone();
    let at = bad
        .windows(32)
        .position(|w| w == [0x5a; 32])
        .expect("the hash")
        - 1;
    bad[at] = 2;
    assert!(Books::from_bundle(&bad).is_err());
    // Zeroing the amount makes a payment for nothing.
    let mut bad = lightning.clone();
    let hash_at = bad
        .windows(32)
        .position(|w| w == [0x5a; 32])
        .expect("the hash");
    for b in bad.iter_mut().skip(hash_at + 32).take(8) {
        *b = 0;
    }
    assert!(Books::from_bundle(&bad).is_err());
    // A version byte that is none of 1, 2 and 3 is refused.
    let mut bad = v1.clone();
    bad[4] = 4;
    assert!(Books::from_bundle(&bad).is_err());
}

#[test]
fn bk_34_a_lightning_sale_is_not_read_against_the_last_quote() {
    // The invoice was made at the first quote; a later quote asks more, and
    // the counter took the payment as paid in full.
    let kit = kit_with(&[
        opened(0, FEB_1_0100, 2500),
        Event::Requoted {
            index: 0,
            at: FEB_1_0100 + 10,
            needs_sats: SATS_PAID * 2,
            minor_per_btc: 1_000_000,
            source: source("later-rate", FEB_1_0100 + 10),
        },
        lightning_event(0, FEB_1_0100 + 20, SATS_PAID * 1_000),
    ]);
    let s = kit.books().sale(&kit.key(0)).expect("sale").clone();
    assert!(s.sats_received.known().copied().unwrap_or(0) < s.last_quote_sats);
    assert_eq!(s.settlement(), None);
    // The same amounts paid on-chain read as below the last quote.
    let chain = kit_with(&[
        opened(0, FEB_1_0100, 2500),
        Event::Requoted {
            index: 0,
            at: FEB_1_0100 + 10,
            needs_sats: SATS_PAID * 2,
            minor_per_btc: 1_000_000,
            source: source("later-rate", FEB_1_0100 + 10),
        },
        final_event(0, FEB_1_0100 + 20, SATS_PAID),
    ]);
    assert!(matches!(
        chain
            .books()
            .sale(&chain.key(0))
            .expect("sale")
            .settlement(),
        Some(Settlement::BelowLastQuote { .. })
    ));
}
