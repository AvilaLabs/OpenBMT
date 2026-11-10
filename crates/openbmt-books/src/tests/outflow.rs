//! BK-12 to BK-14: what goes out, and checked sums.

use openbmt_counter::Event;

use super::fixture::*;
use crate::books::MAX_PER_SALE;
use crate::{Basis, BooksError, CashReturn, Disposal, DisposalKind, ExportError, RevealsPayments};

pub fn fee(at: u64, sats: u64, minor: Option<u64>) -> Disposal {
    Disposal {
        at,
        kind: DisposalKind::Fee,
        currency: usd(),
        sats,
        minor_value: minor,
        basis: None,
    }
}

#[test]
fn bk_12_disposals_are_kept_in_time_order_as_given() {
    let kit = rich_kit();
    let refund = Disposal {
        at: FEB_2_NOON + 100,
        kind: DisposalKind::Refund { sale: kit.key(0) },
        currency: usd(),
        sats: SATS_REFUND,
        minor_value: Some(500),
        basis: Some(Basis { minor: 450 }),
    };
    let cash_out = Disposal {
        at: FEB_2_NOON + 50,
        kind: DisposalKind::CashOut,
        currency: usd(),
        sats: 10,
        minor_value: None,
        basis: None,
    };
    let books = kit
        .books()
        .with_disposals(vec![refund, cash_out])
        .expect("disposals");
    assert_eq!(books.disposals(), &[cash_out, refund]);
}

#[test]
fn bk_12_a_value_or_basis_not_given_is_unknown_in_the_ledger_and_says_why() {
    let kit = rich_kit();
    let books = kit
        .books()
        .with_disposals(vec![fee(FEB_2_NOON, 99, None)])
        .expect("disposals");
    let ledger = books
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let row = ledger
        .lines()
        .find(|l| l.starts_with("fee,"))
        .expect("a fee row");
    assert!(row.contains("unknown"));
    assert!(row.contains("dollar value: ") && row.contains("basis: "));
    assert!(row.contains("Next: "));
}

#[test]
fn bk_12_a_refund_of_a_sale_not_in_the_books_is_refused() {
    let kit = rich_kit();
    let d = Disposal {
        at: 1,
        kind: DisposalKind::Refund { sale: kit.key(42) },
        currency: usd(),
        sats: 1,
        minor_value: Some(1),
        basis: None,
    };
    assert_eq!(
        kit.books().with_disposals(vec![d]).expect_err("refused"),
        BooksError::SaleNotInBooks { sale: kit.key(42) }
    );
}

#[test]
fn bk_13_a_cash_return_names_a_sale_in_the_books() {
    let kit = rich_kit();
    let ok = CashReturn {
        at: FEB_2_NOON + 1,
        sale: kit.key(0),
        minor: 500,
    };
    let books = kit.books().with_cash_returns(vec![ok]).expect("returns");
    assert_eq!(books.cash_returns(), &[ok]);
    let bad = CashReturn {
        sale: kit.key(77),
        ..ok
    };
    assert_eq!(
        kit.books()
            .with_cash_returns(vec![bad])
            .expect_err("refused"),
        BooksError::SaleNotInBooks { sale: kit.key(77) }
    );
}

#[test]
fn bk_13_a_cash_return_is_a_negative_line_and_moves_no_sats() {
    let kit = rich_kit();
    let books = kit
        .books()
        .with_cash_returns(vec![CashReturn {
            at: FEB_2_NOON + 1,
            sale: kit.key(5),
            minor: 1234,
        }])
        .expect("returns");
    let files = books.accounting_export().expect("export");
    let feb = &files.last().expect("a month").1;
    assert!(feb.contains(",-12.34\r\n"), "{feb}");
    let ledger = books
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let row = ledger
        .lines()
        .find(|l| l.starts_with("cash-return,"))
        .expect("a return row");
    let cells: Vec<&str> = row.split(',').collect();
    assert_eq!(cells.get(9), Some(&"-12.34"));
    assert_eq!(cells.get(11), Some(&""), "no sats move: {row}");
}

#[test]
fn bk_14_a_sum_that_does_not_fit_is_an_error() {
    let kit = rich_kit();
    let err = kit
        .books()
        .with_disposals(vec![fee(1, u64::MAX, Some(1)), fee(2, 1, Some(1))])
        .expect_err("overflow");
    assert_eq!(err, BooksError::Overflow);
    let err = kit
        .books()
        .with_disposals(vec![fee(1, 1, Some(u64::MAX)), fee(2, 1, Some(1))])
        .expect_err("overflow");
    assert_eq!(err, BooksError::Overflow);
    let err = kit
        .books()
        .with_cash_returns(vec![
            CashReturn {
                at: 1,
                sale: kit.key(0),
                minor: u64::MAX,
            },
            CashReturn {
                at: 2,
                sale: kit.key(0),
                minor: 1,
            },
        ])
        .expect_err("overflow");
    assert_eq!(err, BooksError::Overflow);
}

#[test]
fn bk_14_totals_that_do_not_fit_are_an_error_not_a_wrapped_number() {
    let big = |index| final_event(index, FEB_2_NOON, 1);
    let kit = kit_with(&[
        opened(0, 1, u64::MAX),
        big(0),
        opened(1, 1, u64::MAX),
        big(1),
    ]);
    assert_eq!(kit.books().utc_day_totals(), Err(BooksError::Overflow));
    let sats = |index| final_event(index, FEB_2_NOON, u64::MAX);
    let kit = kit_with(&[opened(0, 1, 1), sats(0), opened(1, 1, 1), sats(1)]);
    assert_eq!(kit.books().utc_day_totals(), Err(BooksError::Overflow));
}

#[test]
fn bk_14_a_sale_with_too_many_late_payments_is_refused() {
    let mut events = vec![opened(0, 1, 1)];
    for n in 0..=MAX_PER_SALE {
        events.push(Event::LatePayment {
            index: 0,
            at: 2,
            txid: [n as u8; 32],
            sats: 1,
        });
    }
    let kit = kit_with(&events);
    assert_eq!(
        crate::Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]),
        Err(BooksError::TooMany)
    );
}

#[test]
fn bk_14_an_unknown_value_stops_the_accounting_export_and_says_which() {
    let kit = rich_kit();
    let books = kit
        .books()
        .with_disposals(vec![
            fee(FEB_2_NOON, 5, Some(1)),
            fee(FEB_2_NOON + 1, 5, None),
        ])
        .expect("disposals");
    assert_eq!(
        books.accounting_export(),
        Err(ExportError::DisposalValueUnknown { position: 1 })
    );
}
