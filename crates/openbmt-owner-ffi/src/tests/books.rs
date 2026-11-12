//! The books through `dispatch` (op 144, `docs/OWNER-APP-RULES.md` OA-103)
//! and the price a cash-out is recorded at (OA-102).

use openbmt_core::identity::DeviceId;
use openbmt_counter::Currency;
use openbmt_owner_node::{PaidEntry, PaidValue, PaidWhat};

use super::kit::*;
use super::wire::{Rd, W};
use crate::host::cash_price;
use crate::views::CashPriceView;

fn books_request(offset: i32) -> Vec<u8> {
    W::new().u32(offset as u32).0
}

#[test]
fn oa_103_a_node_that_holds_no_settings_has_empty_books() {
    let rig = Rig::new();
    let handle = rig.start();
    let response = rig.call(144, handle, &books_request(-300));
    let mut r = Rd::ok(&response);
    assert_eq!(r.string(), "", "no settings, no currency");
    assert!(r
        .list(|r| (r.string(), r.u64(), r.u64(), r.string()))
        .is_empty());
    assert!(r.list(|r| (r.string(), r.bytes())).is_empty());
    assert!(r.opt(|r| r.words()).is_none());
    r.done();
}

#[test]
fn bk_19_the_full_ledger_is_made_only_when_the_answer_is_yes() {
    let rig = Rig::new();
    let handle = rig.start();
    let refused = Rd::error(&rig.call(146, handle, &[0])).expect("refused");
    assert_eq!(refused.status, 1);
    assert_eq!(refused.rule, "BK-19");
    assert!(refused.text.contains("BK-19"), "{}", refused.text);
    // Anything but 0 or 1 is not an answer.
    assert!(Rd::error(&rig.call(146, handle, &[2])).is_some());
    assert!(Rd::error(&rig.call(146, handle, &[])).is_some());
    assert!(Rd::error(&rig.call(146, handle, &[1, 1])).is_some());
    // A phone with no books gives no bytes, and still only on a yes.
    let response = rig.call(146, handle, &[1]);
    let mut r = Rd::ok(&response);
    assert!(r.bytes().is_empty());
    r.done();
    assert_eq!(
        Rd::error(&rig.call(146, 0, &[1])).expect("refused").rule,
        "handle-unknown"
    );
}

#[test]
fn bk_19_the_ledger_call_gives_the_bytes_the_library_gives() {
    let books = openbmt_books::Books::default();
    let made = crate::books::full_ledger_bytes(&books).expect("ledger");
    let direct = books
        .full_ledger(openbmt_books::RevealsPayments::Acknowledged)
        .expect("ledger");
    assert_eq!(made, direct.into_bytes());
    assert!(!made.is_empty());
}

#[test]
fn oa_103_the_request_is_read_completely_and_the_op_needs_a_node() {
    let rig = Rig::new();
    let handle = rig.start();
    let mut extra = books_request(0);
    extra.push(0);
    assert_eq!(
        Rd::error(&rig.call(144, handle, &extra))
            .expect("refused")
            .status,
        1
    );
    assert!(Rd::error(&rig.call(144, handle, &[1, 2])).is_some());
    assert_eq!(
        Rd::error(&rig.call(144, 0, &books_request(0)))
            .expect("refused")
            .rule,
        "handle-unknown"
    );
}

#[test]
fn oa_102_the_price_of_a_cash_out_is_in_minor_units_of_the_currency() {
    let usd = cash_price(&CashPriceView {
        currency: "USD".to_string(),
        whole_per_btc: 98_500,
    })
    .unwrap();
    assert_eq!((usd.currency, usd.minor_per_btc), (*b"USD", 9_850_000));
    let yen = cash_price(&CashPriceView {
        currency: "JPY".to_string(),
        whole_per_btc: 15_000_000,
    })
    .unwrap();
    assert_eq!(yen.minor_per_btc, 15_000_000);
    // Bitcoin Standard: a satoshi is the minor unit and a bitcoin is 100,000,000 of them.
    let sats = cash_price(&CashPriceView {
        currency: "SAT".to_string(),
        whole_per_btc: 100_000_000,
    })
    .unwrap();
    assert_eq!((sats.currency, sats.minor_per_btc), (*b"SAT", 100_000_000));
    for (code, whole) in [("USD", 0), ("XYZ", 5), ("usd", 5), ("USD", u64::MAX)] {
        let refused = cash_price(&CashPriceView {
            currency: code.to_string(),
            whole_per_btc: whole,
        });
        assert!(refused.is_err(), "{code} {whole}");
    }
}

#[test]
fn oa_103_what_was_paid_out_becomes_disposals_with_their_values_or_none() {
    let counter: DeviceId = counter();
    let usd = Currency::new(*b"USD").unwrap();
    let entry = |what, value| PaidEntry {
        what,
        number: 1,
        txid: bitcoin::Txid::from_raw_hash(bitcoin::hashes::Hash::from_byte_array([1; 32])),
        sats: 10_000,
        at: 1_700_000_000,
        value,
    };
    let paid = [
        entry(
            PaidWhat::Refund {
                sale: openbmt_owner::SaleKey { counter, index: 3 },
                request: Some(2),
            },
            Some(PaidValue {
                currency: *b"USD",
                minor: 912,
            }),
        ),
        entry(PaidWhat::CashOut, None),
    ];
    let made = crate::books::disposals(&paid, usd);
    assert_eq!(made.len(), 2);
    assert_eq!(made[0].minor_value, Some(912));
    assert_eq!(made[0].sats, 10_000);
    assert_eq!(made[0].at, 1_700_000_000);
    assert!(matches!(
        made[0].kind,
        openbmt_books::DisposalKind::Refund { sale } if sale.counter == counter && sale.index == 3
    ));
    assert_eq!((made[1].minor_value, made[1].currency), (None, usd));
    assert!(matches!(made[1].kind, openbmt_books::DisposalKind::CashOut));
}
