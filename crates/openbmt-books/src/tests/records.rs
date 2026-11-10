//! What the counter's newer records give the books: the rate sources, the
//! last quote, and the final record's transactions, first-seen time and
//! value (BK-06, BK-09, BK-18, BK-19, BK-28).

use openbmt_counter::{Amount, Event, MAX_FINAL_PAYMENTS};

use super::fixture::*;
use crate::{
    Books, Payment, PaymentList, RevealsPayments, SaleState, Settlement, UnknownField, Value,
};

const T: u64 = FEB_1_0100;

fn requoted(index: u32, at: u64, needs_sats: u64, rate: u64, label: &str) -> Event {
    Event::Requoted {
        index,
        at,
        needs_sats,
        minor_per_btc: rate,
        source: source(label, at - 3),
    }
}

fn final_with(
    index: u32,
    at: u64,
    received: u64,
    payments: Vec<([u8; 32], u64)>,
    more: bool,
) -> Event {
    Event::Final {
        index,
        at,
        received_sats: received,
        depth: 6,
        first_seen_at: at - 500,
        valued: Amount(FINAL_VALUED),
        payments,
        more,
    }
}

fn late(index: u32, at: u64, txid: [u8; 32], sats: u64) -> Event {
    Event::LatePayment {
        index,
        at,
        txid,
        sats,
    }
}

#[test]
fn bk_06_the_rate_sources_and_the_last_quote_come_from_opened_and_requoted() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        requoted(0, T + 400, 3_000_000, 6_166_666, "second-feed"),
        requoted(0, T + 800, 2_900_000, 6_379_310, "third.feed+1"),
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.first_quote_sats, 4_321_987);
    assert_eq!(s.quote_rate_minor_per_btc, 4_281_700);
    let first = s.rate_source.known().expect("known");
    assert_eq!((first.label(), first.fetched_at()), ("fixture-rate", T));
    assert_eq!(s.last_quote_sats, 2_900_000);
    assert_eq!(s.last_quote_rate_minor_per_btc, 6_379_310);
    let last = s.last_rate_source.known().expect("known");
    assert_eq!((last.label(), last.fetched_at()), ("third.feed+1", T + 797));
}

#[test]
fn bk_06_a_sale_never_requoted_has_its_first_quote_as_the_last() {
    let kit = kit_with(&[opened(0, T, 1850)]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.last_quote_sats, s.first_quote_sats);
    assert_eq!(s.last_quote_rate_minor_per_btc, s.quote_rate_minor_per_btc);
    assert_eq!(s.last_rate_source, s.rate_source);
}

#[test]
fn bk_06_underpaid_is_read_against_the_last_quote() {
    // The last quote needs fewer satoshis than the first: 4,000,000 sats are
    // enough for it and short for the first.
    let kit = kit_with(&[
        opened(0, T, 1850),
        requoted(0, T + 400, 3_900_000, 4_743_000, "second-feed"),
        final_event(0, T + 900, 4_000_000),
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.settlement(), None);
    // Without the requote the same payment is short.
    let kit = kit_with(&[opened(0, T, 1850), final_event(0, T + 900, 4_000_000)]);
    let books = kit.books();
    assert_eq!(
        books.sale(&kit.key(0)).expect("sale").settlement(),
        Some(Settlement::BelowLastQuote {
            short_sats: 321_987
        })
    );
    // A last quote that needs more makes a payment short that met the first.
    let kit = kit_with(&[
        opened(0, T, 1850),
        requoted(0, T + 400, 5_000_000, 3_700_000, "second-feed"),
        final_event(0, T + 900, 4_500_000),
    ]);
    assert_eq!(
        kit.books().sale(&kit.key(0)).expect("sale").settlement(),
        Some(Settlement::BelowLastQuote {
            short_sats: 500_000
        })
    );
}

#[test]
fn bk_06_the_final_record_makes_the_value_the_time_and_the_transactions_known() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        Event::PaidAtOnce {
            index: 0,
            at: T + 10,
            at_risk_sats: 100,
            label: openbmt_core::policy::InstantLabel::LightCheck,
            mode: openbmt_core::policy::CheckMode::Light,
        },
        final_with(
            0,
            T + 1_000,
            4_321_987,
            vec![([1; 32], 4_000_000), ([2; 32], 321_987)],
            false,
        ),
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.value_at_first_seen_minor, Value::Known(FINAL_VALUED));
    assert_eq!(s.first_seen_at, Some(T + 500));
    assert_eq!(
        s.payments,
        Value::Known(PaymentList {
            payments: vec![
                Payment {
                    txid: [1; 32],
                    sats: 4_000_000
                },
                Payment {
                    txid: [2; 32],
                    sats: 321_987
                },
            ],
            more: false,
        })
    );
    assert_eq!(s.sats_received, Value::Known(4_321_987));
    assert!(s.own_payment);
}

#[test]
fn bk_06_a_sale_that_is_not_final_keeps_the_transactions_and_the_value_unknown() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        Event::Confirmed {
            index: 0,
            at: T + 100,
            received_sats: 4_321_987,
            depth: 1,
        },
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.payments, Value::Unknown(UnknownField::Txid));
    assert_eq!(
        s.value_at_first_seen_minor,
        Value::Unknown(UnknownField::ValueAtFirstSeen)
    );
    assert_eq!(s.first_seen_at, None);
    let txid = UnknownField::Txid.explain();
    assert_eq!(
        txid.why,
        "the sale is not final; the final record names its transactions"
    );
    assert!(txid.next_step.contains("final"));
    let value = UnknownField::ValueAtFirstSeen.explain();
    assert!(value.why.contains("not final") && value.next_step.contains("final"));
    // The rate source is never unknown for a counter that writes it.
    assert!(s.rate_source.known().is_some() && s.last_rate_source.known().is_some());
}

#[test]
fn bk_06_a_conflict_that_replaces_the_payment_takes_the_final_fields_back() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        final_event(0, T + 900, 4_321_987),
        Event::Replaced {
            index: 0,
            at: T + 1_200,
        },
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.state, SaleState::Replaced);
    assert_eq!(s.payments, Value::Unknown(UnknownField::Txid));
    assert_eq!(s.first_seen_at, None);
    assert!(!s.own_payment);
    assert_eq!(
        s.value_at_first_seen_minor,
        Value::Unknown(UnknownField::ValueAtFirstSeen)
    );
}

#[test]
fn bk_09_a_late_payment_among_the_final_transactions_is_already_in_the_amount() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        final_with(
            0,
            T + 900,
            5_000,
            vec![([1; 32], 3_000), ([7; 32], 2_000)],
            false,
        ),
        late(0, T + 1_000, [7; 32], 2_000),
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.sats_received, Value::Known(5_000));
}

#[test]
fn bk_09_a_late_payment_that_is_not_among_them_is_added() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        final_with(0, T + 900, 5_000, vec![([1; 32], 5_000)], false),
        late(0, T + 1_000, [9; 32], 700),
        late(0, T + 1_100, [8; 32], 30),
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.sats_received, Value::Known(5_730));
    assert_eq!(s.state, SaleState::Final);
}

#[test]
fn bk_09_a_cut_short_final_record_settles_only_what_it_names() {
    let many: Vec<([u8; 32], u64)> = (1..=MAX_FINAL_PAYMENTS as u8)
        .map(|i| ([i; 32], 100))
        .collect();
    let total = 100 * MAX_FINAL_PAYMENTS as u64;
    // A late payment among the 32 named is in the amount.
    let named = kit_with(&[
        opened(0, T, 1850),
        final_with(0, T + 900, total + 100, many.clone(), true),
        late(0, T + 1_000, [5; 32], 100),
    ]);
    assert_eq!(
        named
            .books()
            .sale(&named.key(0))
            .expect("sale")
            .sats_received,
        Value::Known(total + 100)
    );
    // One that is not named may be among the left out: unknown, and the
    // unknown explains itself.
    let unnamed = kit_with(&[
        opened(0, T, 1850),
        final_with(0, T + 900, total + 100, many, true),
        late(0, T + 1_000, [200; 32], 100),
    ]);
    let books = unnamed.books();
    let s = books.sale(&unnamed.key(0)).expect("sale");
    assert_eq!(
        s.sats_received,
        Value::Unknown(UnknownField::SatsLateAmbiguous)
    );
    let why = UnknownField::SatsLateAmbiguous.explain();
    assert!(why.why.contains("final record") && why.next_step.contains("final"));
}

#[test]
fn bk_09_without_a_final_record_a_late_payment_still_leaves_the_sum_unknown() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        Event::Confirmed {
            index: 0,
            at: T + 100,
            received_sats: 5_000,
            depth: 1,
        },
        late(0, T + 1_000, [9; 32], 700),
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(
        s.sats_received,
        Value::Unknown(UnknownField::SatsLateAmbiguous)
    );
    assert_eq!(s.payments, Value::Unknown(UnknownField::Txid));
}

#[test]
fn bk_18_the_accounting_export_has_three_columns_and_no_txid_or_address_column() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        requoted(0, T + 60, 3_000_000, 6_166_666, "second-feed"),
        final_with(0, T + 900, 5_000, vec![([0x5a; 32], 5_000)], false),
        opened(1, T, 500),
        final_with(
            1,
            T + 900,
            6_000,
            vec![([0x6b; 32], 4_000), ([0x7c; 32], 2_000)],
            false,
        ),
    ]);
    let files = kit.books().accounting_export().expect("export");
    assert_eq!(files.len(), 1);
    let text = &files[0].1;
    let mut rows = text.split("\r\n").filter(|r| !r.is_empty());
    assert_eq!(rows.next(), Some("Date,Description,Amount"));
    let mut n = 0;
    for row in rows {
        n += 1;
        assert_eq!(row.matches(',').count(), 2, "{row}");
        assert!(row.starts_with("2025-02-01,Sale "), "{row}");
    }
    assert_eq!(n, 2);
    // What the final record carries is not in it, in any form.
    for byte in [0x5au8, 0x6b, 0x7c] {
        let hex: String = format!("{byte:02x}").repeat(32);
        assert!(!text.contains(&hex[..16]), "a txid is in the export");
    }
    for word in [
        "fixture-rate",
        "second-feed",
        "txid",
        "Txid",
        "address",
        "Address",
        "bc1",
    ] {
        assert!(!text.contains(word), "{word}");
    }
    // The full ledger does hold them.
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    for byte in [0x5au8, 0x6b, 0x7c] {
        assert!(ledger.contains(&format!("{byte:02x}").repeat(32)));
    }
}

#[test]
fn bk_19_the_full_ledger_shows_the_sources_the_value_the_time_and_the_transactions() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        requoted(0, T + 60, 3_000_000, 6_166_666, "second-feed"),
        final_with(
            0,
            T + 900,
            5_000,
            vec![([0x11; 32], 2_000), ([0x22; 32], 3_000)],
            false,
        ),
        late(0, T + 2_000, [0x33; 32], 44),
        opened(1, T, 500),
        Event::Confirmed {
            index: 1,
            at: T + 10,
            received_sats: 9,
            depth: 1,
        },
    ]);
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let mut lines = ledger.lines();
    let header: Vec<&str> = lines.nth(2).expect("header").split(',').collect();
    let column = |name: &str| header.iter().position(|h| *h == name).expect(name);
    let rows: Vec<Vec<String>> = lines
        .map(|l| l.split(',').map(str::to_string).collect())
        .collect();
    // Sale 0 is a simple row (its Note and Unknown cells are quoted and
    // have commas, so read the cells before them).
    let r0 = &rows[0];
    assert_eq!(r0[column("Rate source")], "fixture-rate");
    assert_eq!(r0[column("Value at first seen")], "17.17");
    assert_eq!(r0[column("Quote rate (minor per BTC)")], "4281700");
    assert_eq!(
        r0[column("Txid")],
        format!(
            "{} {} {}",
            "11".repeat(32),
            "22".repeat(32),
            "33".repeat(32)
        )
    );
    // The cells after the quoted ones are counted from the end.
    let tail = &r0[r0.len() - 4..];
    assert_eq!(tail[0], "2025-02-01T01:06:40Z", "first seen");
    assert_eq!(tail[1], "3000000", "last quote sats");
    assert_eq!(tail[2], "6166666", "last quote rate");
    assert_eq!(tail[3], "second-feed", "last rate source");
    // Sale 1 is confirmed and not final: the value and the txid say so.
    assert!(ledger.contains("txid: the sale is not final; the final record names its transactions"));
    assert!(ledger.contains("value at first seen: the sale is not final"));
    assert!(!ledger.contains("rate source:"));
}

#[test]
fn bk_28_the_bundle_carries_the_new_fields_and_reads_them_back() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        requoted(0, T + 60, 3_000_000, 6_166_666, "second-feed"),
        final_with(
            0,
            T + 900,
            5_000,
            vec![([1; 32], 2_000), ([2; 32], 3_000)],
            false,
        ),
        opened(1, T, 500),
    ]);
    let books = kit.books();
    let bytes = books.to_bundle();
    assert_eq!(&bytes[..5], b"OBM6\x01", "the version stays 1");
    let back = Books::from_bundle(&bytes).expect("reads back");
    assert_eq!(back, books);
    assert_eq!(back.to_bundle(), bytes);
    // Every cut and a byte too many are refused.
    for n in 0..bytes.len() {
        assert!(Books::from_bundle(&bytes[..n]).is_err(), "prefix {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Books::from_bundle(&longer).is_err());
}

#[test]
fn bk_28_a_bundle_that_gives_the_final_fields_in_part_or_out_of_order_is_refused() {
    let kit = kit_with(&[
        opened(0, T, 1850),
        final_with(
            0,
            T + 900,
            5_000,
            vec![([1; 32], 2_000), ([2; 32], 3_000)],
            false,
        ),
    ]);
    let good = kit.books();
    // All three fields are there or none.
    let mut part = good.clone();
    part.sales[0].first_seen_at = None;
    assert!(Books::from_bundle(&part.to_bundle()).is_err());
    let mut part = good.clone();
    part.sales[0].value_at_first_seen_minor = Value::Unknown(UnknownField::ValueAtFirstSeen);
    assert!(Books::from_bundle(&part.to_bundle()).is_err());
    let mut part = good.clone();
    part.sales[0].payments = Value::Unknown(UnknownField::Txid);
    assert!(Books::from_bundle(&part.to_bundle()).is_err());
    // The transactions are in ascending order, and not repeated.
    for (a, b) in [([2u8; 32], [1u8; 32]), ([1; 32], [1; 32])] {
        let mut bad = good.clone();
        bad.sales[0].payments = Value::Known(PaymentList {
            payments: vec![Payment { txid: a, sats: 1 }, Payment { txid: b, sats: 1 }],
            more: false,
        });
        assert!(Books::from_bundle(&bad.to_bundle()).is_err());
    }
    // At most 32, and "more" needs a full list.
    let mut bad = good.clone();
    bad.sales[0].payments = Value::Known(PaymentList {
        payments: (0..33u8)
            .map(|i| Payment {
                txid: [i + 1; 32],
                sats: 1,
            })
            .collect(),
        more: true,
    });
    assert!(Books::from_bundle(&bad.to_bundle()).is_err());
    let mut bad = good.clone();
    bad.sales[0].payments = Value::Known(PaymentList {
        payments: vec![Payment {
            txid: [1; 32],
            sats: 1,
        }],
        more: true,
    });
    assert!(Books::from_bundle(&bad.to_bundle()).is_err());
    // A full list that says more is fine.
    let mut full = good.clone();
    full.sales[0].payments = Value::Known(PaymentList {
        payments: (0..32u8)
            .map(|i| Payment {
                txid: [i + 1; 32],
                sats: 1,
            })
            .collect(),
        more: true,
    });
    assert!(Books::from_bundle(&full.to_bundle()).is_ok());
}

#[test]
fn bk_28_a_rate_source_label_that_is_not_allowed_is_refused_in_a_bundle() {
    let kit = kit_with(&[opened(0, T, 1850)]);
    let bytes = kit.books().to_bundle();
    let label = b"fixture-rate";
    let at = bytes
        .windows(label.len())
        .position(|w| w == label)
        .expect("the label");
    for bad in [b'A', b' ', b'_', 0u8] {
        let mut b = bytes.clone();
        b[at + 3] = bad;
        assert!(Books::from_bundle(&b).is_err(), "byte {bad}");
    }
    // An empty label.
    let mut b = bytes.clone();
    b.splice(at - 1..at + label.len(), [0u8]);
    assert!(Books::from_bundle(&b).is_err());
}
