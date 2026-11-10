//! BK-17 to BK-22: the two CSV exports.

use openbmt_counter::{Currency, Event};

use super::fixture::*;
use super::outflow::fee;
use crate::csv::{push_row, render, Cell};
use crate::{
    format_minor, Books, CashReturn, Disposal, DisposalKind, ExportError, PosFacts,
    RevealsPayments, SaleId, YearMonth, LEDGER_WARNING,
};

const ADDRESSES: [&str; 2] = [
    "bc1qzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
    "bc1qyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
];

/// Counted sales on both sides of a month boundary, plus outflows.
fn month_kit() -> (Kit, Books) {
    let last = 1_738_367_999; // 2025-01-31T23:59:59Z
    let first = 1_738_368_000; // 2025-02-01T00:00:00Z
    let fin = |index, at| Event::Final {
        index,
        at,
        received_sats: SATS_PAID,
        depth: 6,
    };
    let kit = kit_with(&[
        opened(0, last - 5, 1850),
        fin(0, last),
        opened(1, first - 5, 2000),
        fin(1, first),
    ]);
    let books = kit.books();
    (kit, books)
}

#[test]
fn bk_17_the_accounting_export_is_one_file_per_utc_month() {
    let (kit, books) = month_kit();
    let files = books.accounting_export().expect("export");
    let months: Vec<YearMonth> = files.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        months,
        vec![
            YearMonth {
                year: 2025,
                month: 1
            },
            YearMonth {
                year: 2025,
                month: 2
            }
        ]
    );
    let id0 = SaleId::for_sale(&kit.key(0));
    let id1 = SaleId::for_sale(&kit.key(1));
    assert_eq!(
        files[0].1,
        format!("Date,Description,Amount\r\n2025-01-31,Sale {id0},18.50\r\n")
    );
    assert_eq!(
        files[1].1,
        format!("Date,Description,Amount\r\n2025-02-01,Sale {id1},20.00\r\n")
    );
}

#[test]
fn bk_17_pending_sales_are_not_in_it_and_a_month_without_lines_has_no_file() {
    let kit = rich_kit();
    let files = kit.books().accounting_export().expect("export");
    // Sales 0 (January) and 5 (February) are counted; 3 and 4 are pending.
    assert_eq!(files.len(), 2);
    assert_eq!(files[1].1.lines().count(), 2);
    assert_eq!(Books::default().accounting_export(), Ok(Vec::new()));
}

#[test]
fn bk_17_refunds_cash_outs_fees_and_returns_are_negative_lines() {
    let kit = rich_kit();
    let books = kit
        .books()
        .with_disposals(vec![
            Disposal {
                at: FEB_2_NOON + 10,
                kind: DisposalKind::Refund { sale: kit.key(0) },
                currency: usd(),
                sats: SATS_REFUND,
                minor_value: Some(500),
                basis: None,
            },
            Disposal {
                kind: DisposalKind::CashOut,
                minor_value: Some(185_000),
                ..fee(FEB_2_NOON + 20, 9, None)
            },
            fee(FEB_2_NOON + 30, 9, Some(7)),
        ])
        .expect("disposals")
        .with_cash_returns(vec![CashReturn {
            at: FEB_2_NOON + 40,
            sale: kit.key(5),
            minor: 100,
        }])
        .expect("returns");
    let feb = books.accounting_export().expect("export").remove(1).1;
    let id0 = SaleId::for_sale(&kit.key(0));
    let id5 = SaleId::for_sale(&kit.key(5));
    assert!(feb.contains(&format!("Refund of sale {id0},-5.00\r\n")));
    assert!(feb.contains("Bitcoin cash-out,-1850.00\r\n"));
    assert!(feb.contains("Network fee paid in bitcoin,-0.07\r\n"));
    assert!(feb.contains(&format!("Cash return for sale {id5},-1.00\r\n")));
}

#[test]
fn bk_17_more_than_one_currency_refuses_the_export() {
    let eur = Currency::new(*b"EUR").expect("currency");
    let mut opened_eur = opened(1, 11, 500);
    if let Event::Opened { currency, .. } = &mut opened_eur {
        *currency = eur;
    }
    let fin = |index| Event::Final {
        index,
        at: FEB_2_NOON,
        received_sats: 1,
        depth: 6,
    };
    let kit = kit_with(&[opened(0, 10, 100), fin(0), opened_eur, fin(1)]);
    assert_eq!(
        kit.books().accounting_export(),
        Err(ExportError::MixedCurrencies)
    );
}

#[test]
fn bk_18_the_accounting_export_never_holds_a_txid_an_address_or_sats() {
    let kit = rich_kit();
    let mut facts = PosFacts::new();
    facts.insert_tax(kit.key(0), 148);
    let books = kit
        .books()
        .with_pos_facts(&facts)
        .expect("facts")
        .with_disposals(vec![
            Disposal {
                at: FEB_2_NOON + 10,
                kind: DisposalKind::Refund { sale: kit.key(0) },
                currency: usd(),
                sats: SATS_REFUND,
                minor_value: Some(500),
                basis: None,
            },
            fee(FEB_2_NOON + 20, SATS_DUST + 1, Some(7)),
        ])
        .expect("disposals");
    let accounting: String = books
        .accounting_export()
        .expect("export")
        .into_iter()
        .map(|(_, s)| s)
        .collect();
    let txid_hex: String = TXID_LATE.iter().map(|b| format!("{b:02x}")).collect();
    let mut secrets: Vec<String> = vec![txid_hex.clone()];
    secrets.extend(ADDRESSES.iter().map(|a| a.to_string()));
    for sats in [
        SATS_PAID,
        SATS_LATE,
        SATS_SURPLUS,
        SATS_DUST,
        SATS_DUST + 1,
        SATS_REFUND,
        SATS_PAID + SATS_SURPLUS,
        4_321_987,
        4_281_700,
    ] {
        secrets.push(sats.to_string());
    }
    for s in &secrets {
        assert!(!accounting.contains(s), "the accounting file holds {s}");
    }
    // The scan can see: the full ledger holds the txid and the sats.
    let ledger = books
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    assert!(ledger.contains(&txid_hex));
    assert!(ledger.contains(&SATS_LATE.to_string()));
    assert!(ledger.contains(&SATS_REFUND.to_string()));
}

#[test]
fn bk_18_a_line_that_is_not_about_a_sale_holds_fixed_words_only() {
    let kit = rich_kit();
    let books = kit
        .books()
        .with_disposals(vec![fee(FEB_2_NOON, 5, Some(7))])
        .expect("disposals");
    let feb = books.accounting_export().expect("export").remove(1).1;
    assert!(feb.contains("Network fee paid in bitcoin,-0.07"));
}

#[test]
fn bk_19_the_full_ledger_starts_with_the_warning_and_the_high_water_mark() {
    let kit = rich_kit();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let mut lines = ledger.lines();
    assert_eq!(lines.next(), Some(format!("# {LEDGER_WARNING}").as_str()));
    assert_eq!(
        LEDGER_WARNING,
        "This file reveals the shop's payments on the blockchain. Share it only with people you trust with that."
    );
    let second = lines.next().expect("second row");
    assert!(second.starts_with("# High-water mark"));
    assert!(second.ends_with(&format!("{}=7", crate::error::hex_id(&kit.counter_id()))));
}

#[test]
fn bk_19_the_full_ledger_has_every_sale_and_every_unknown_with_its_next_step() {
    let kit = rich_kit();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    // Two comment rows, the header and eight sales.
    assert_eq!(ledger.lines().count(), 3 + 8);
    for state in [
        "expired",
        "cancelled",
        "late payment",
        "confirmed",
        "final",
        "replaced",
        "open",
    ] {
        assert!(ledger.contains(&format!(",{state},")), "no {state}");
    }
    assert!(ledger.contains("rate source: no event records where the quote's rate came from"));
    assert!(ledger.contains("Next: pass the tax of the POS match as PosFacts"));
    assert!(ledger.contains("overpaid: 1111117 sats owed to the customer"));
    assert!(ledger.contains("accepted at once with 4321987 sats at risk"));
    assert!(ledger.contains("manager override for"));
    assert!(ledger.contains("the POS was told paid"));
    assert!(ledger.contains("1111117"));
    assert!(ledger.contains("sats arrived below the dust line"));
}

#[test]
fn bk_19_the_ledger_shows_tax_when_the_pos_facts_gave_it() {
    let kit = rich_kit();
    let mut facts = PosFacts::new();
    facts.insert_tax(kit.key(0), 148);
    let ledger = kit
        .books()
        .with_pos_facts(&facts)
        .expect("facts")
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let row = ledger
        .lines()
        .find(|l| l.starts_with("sale,") && l.contains(",final,counted,2025-01-31T"))
        .expect("sale 0 row");
    assert_eq!(row.split(',').nth(10), Some("1.48"));
}

#[test]
fn bk_20_fields_are_quoted_as_rfc_4180_says() {
    assert_eq!(render(&Cell::text("plain")), "plain");
    assert_eq!(render(&Cell::text("a,b")), "\"a,b\"");
    assert_eq!(render(&Cell::text("say \"hi\"")), "\"say \"\"hi\"\"\"");
    assert_eq!(render(&Cell::text("two\nlines")), "\"two\nlines\"");
    assert_eq!(render(&Cell::text("cr\rhere")), "\"cr\rhere\"");
    assert_eq!(render(&Cell::text("héllo ₿")), "héllo ₿");
    assert_eq!(render(&Cell::Empty), "");
}

#[test]
fn bk_20_rows_end_in_crlf_and_the_text_has_no_byte_order_mark() {
    let mut out = String::new();
    push_row(&mut out, &[Cell::text("a"), Cell::text("b,c"), Cell::Empty]);
    assert_eq!(out, "a,\"b,c\",\r\n");
    let kit = rich_kit();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    assert!(!ledger.starts_with('\u{feff}') && ledger.as_bytes().first() == Some(&b'#'));
    assert!(ledger.ends_with("\r\n"));
    assert_eq!(ledger.matches('\n').count(), ledger.matches("\r\n").count());
    let (_, books) = month_kit();
    for (_, file) in books.accounting_export().expect("export") {
        assert_eq!(file.matches('\n').count(), file.matches("\r\n").count());
    }
}

#[test]
fn bk_21_a_text_field_that_could_be_a_formula_gets_a_quote_in_front() {
    for start in ["=", "+", "-", "@", "\t", "\r"] {
        let text = format!("{start}SUM(A1)");
        let out = render(&Cell::text(text.clone()));
        let guarded = format!("'{text}");
        let want = if text.contains('\r') {
            format!("\"{guarded}\"")
        } else {
            guarded
        };
        assert_eq!(out, want, "start {start:?}");
    }
    // Only a start matters, and a number the library made is left alone.
    assert_eq!(render(&Cell::text("a=b")), "a=b");
    assert_eq!(render(&Cell::Number("-12.34".to_string())), "-12.34");
}

#[test]
fn bk_21_a_name_that_is_a_formula_is_guarded_in_the_ledger() {
    let kit = rich_kit();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    assert!(ledger.contains(&format!(",'{NASTY_NAME}\r\n")), "{ledger}");
    assert!(!ledger.contains(&format!(",{NASTY_NAME}\r\n")));
}

#[test]
fn bk_21_a_negative_amount_has_a_leading_minus_and_no_guard() {
    let kit = rich_kit();
    let books = kit
        .books()
        .with_disposals(vec![fee(FEB_2_NOON, 5, Some(7))])
        .expect("disposals");
    let feb = books.accounting_export().expect("export").remove(1).1;
    assert!(feb.contains(",-0.07\r\n") && !feb.contains("'-"));
}

#[test]
fn bk_22_amounts_have_the_minor_digits_of_the_currency() {
    let c = |code: &[u8; 3]| Currency::new(*code).expect("currency");
    assert_eq!(format_minor(1850, c(b"USD")), "18.50");
    assert_eq!(format_minor(5, c(b"USD")), "0.05");
    assert_eq!(format_minor(0, c(b"USD")), "0.00");
    assert_eq!(format_minor(123_456_789, c(b"USD")), "1234567.89");
    assert_eq!(format_minor(1850, c(b"JPY")), "1850");
    assert_eq!(format_minor(1234, c(b"KWD")), "1.234");
    assert_eq!(format_minor(5, c(b"KWD")), "0.005");
    assert_eq!(format_minor(u64::MAX, c(b"USD")), "184467440737095516.15");
    assert_eq!(crate::minor_digits(c(b"CLF")), 4);
}

#[test]
fn bk_22_the_ledger_has_full_times_and_the_accounting_export_has_dates() {
    let kit = rich_kit();
    let ledger = kit
        .books()
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    assert!(ledger.contains("2025-01-31T23:59:30Z"));
    let jan = kit.books().accounting_export().expect("export").remove(0).1;
    assert!(jan.contains("\r\n2025-01-31,Sale "));
}
