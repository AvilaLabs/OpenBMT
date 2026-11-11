//! BK-01 to BK-04: what is read, and what refuses the build.

use bitcoin::hashes::Hash;
use openbmt_core::records::{Chain, RecordKind};
use openbmt_counter::Event;

use super::fixture::*;
use crate::date::MAX_UNIX;
use crate::{Books, BooksError};

#[test]
fn bk_01_a_bad_chain_refuses_the_whole_build_naming_counter_and_record() {
    let good = kit_with(&[opened(0, JAN_31_235930, 100)]);
    // Two counters on one list: the second is the owner key standing in for
    // another counter, with the bad chain.
    let other = identity(3);
    let mut chain_other = Chain::new();
    write(&mut chain_other, &other, &opened(0, JAN_31_235930, 100));
    let mut records = chain_other.records().to_vec();
    if let Some(r) = records.first_mut() {
        r.body.push(7);
    }
    let broken = Chain::from_records(records);
    let devices = openbmt_core::records::DeviceList::new([
        good.counter_id(),
        other.device_id(),
        good.owner.device_id(),
    ]);
    let err = Books::from_chains(
        &devices,
        &[
            (good.counter_id(), &good.chain),
            (other.device_id(), &broken),
        ],
    )
    .expect_err("refused");
    match &err {
        BooksError::BadChain { counter, .. } => assert_eq!(*counter, other.device_id()),
        e => panic!("wrong error {e:?}"),
    }
    let text = err.to_string();
    assert!(
        text.contains("BK-01") && text.contains("record 0"),
        "{text}"
    );
    assert!(text.contains(&crate::error::hex_id(&other.device_id())));
}

#[test]
fn bk_01_a_counter_not_on_the_list_is_refused() {
    let kit = rich_kit();
    let stranger = identity(9);
    let err = Books::from_chains(&kit.devices, &[(stranger.device_id(), &kit.chain)])
        .expect_err("refused");
    assert_eq!(
        err,
        BooksError::CounterNotOnList {
            counter: stranger.device_id()
        }
    );
}

#[test]
fn bk_01_a_counter_given_twice_is_refused() {
    let kit = rich_kit();
    let err = Books::from_chains(
        &kit.devices,
        &[
            (kit.counter_id(), &kit.chain),
            (kit.counter_id(), &kit.chain),
        ],
    )
    .expect_err("refused");
    assert!(matches!(err, BooksError::DuplicateCounter { .. }));
}

#[test]
fn bk_02_only_the_counters_own_event_records_are_read() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    // Records that hold no money are passed over, even with a body that is
    // no event.
    kit.chain
        .append(&kit.counter, RecordKind::Setting, vec![0xff; 5])
        .expect("appended");
    kit.chain.append_check_in(&kit.counter);
    // A sale-kind record of another device is not the counter's.
    kit.chain
        .append(&kit.owner, RecordKind::Sale, opened(1, 5, 5).encode())
        .expect("appended");
    let books = kit.books();
    assert_eq!(books.sales().len(), 1);
}

/// A request for a refund of sale `index`, as the counter writes it.
fn requested(index: u32, at: u64, number: u64, sats: u64) -> Event {
    Event::RefundRequested {
        index,
        at,
        number,
        role: openbmt_counter::Role::Cashier,
        name: "ann".to_string(),
        reason: "wrong size".to_string(),
        destination: bitcoin::ScriptBuf::new_p2wpkh(&bitcoin::WPubkeyHash::from_byte_array(
            [7; 20],
        )),
        received_sats: 4_321_987,
        waiting_for_depth: false,
        sats,
        ask: openbmt_counter::Ask::Sats,
    }
}

#[test]
fn bk_02_a_refund_kind_record_that_is_no_event_refuses_the_build() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    kit.chain
        .append(&kit.counter, RecordKind::Refund, vec![1, 2, 3])
        .expect("appended");
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert!(
        matches!(err, BooksError::BadEvent { seq: 1, .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("BK-03"));
}

#[test]
fn bk_02_a_request_and_a_withdrawal_are_read_and_are_not_a_disposal() {
    let kit = kit_with(&[
        opened(0, JAN_31_235930, 100),
        final_event(0, FEB_1_0100, SATS_PAID),
        requested(0, FEB_1_0100 + 5, 1, 1_000),
        Event::RefundWithdrawn {
            index: 0,
            at: FEB_1_0100 + 9,
            number: 1,
            role: openbmt_counter::Role::Manager,
            name: "max".to_string(),
        },
    ]);
    let books = kit.books();
    assert!(books.disposals().is_empty());
    assert!(books.cash_returns().is_empty());
    assert_eq!(books.sales().len(), 1);
    assert_eq!(books.high_water().get(&kit.counter_id()), Some(&Some(0)));
}

#[test]
fn bk_13_a_cash_refund_record_is_a_return_line() {
    let kit = kit_with(&[
        opened(0, JAN_31_235930, 100),
        final_event(0, FEB_1_0100, SATS_PAID),
        Event::RefundPaidInCash {
            index: 0,
            at: FEB_1_0100 + 50,
            role: openbmt_counter::Role::Cashier,
            name: "ann".to_string(),
            reason: "wrong size".to_string(),
            minor: 25,
            sats: 1_000,
        },
    ]);
    let books = kit.books();
    assert_eq!(
        books.cash_returns(),
        [crate::CashReturn {
            at: FEB_1_0100 + 50,
            sale: kit.key(0),
            minor: 25
        }]
    );
    // A caller's own returns are added to it, in time order.
    let more = books
        .with_cash_returns(vec![crate::CashReturn {
            at: FEB_1_0100 + 10,
            sale: kit.key(0),
            minor: 5,
        }])
        .expect("returns");
    let minors: Vec<u64> = more.cash_returns().iter().map(|r| r.minor).collect();
    assert_eq!(minors, [5, 25]);
}

#[test]
fn bk_04_a_refund_event_for_a_sale_never_opened_refuses_the_build() {
    let kit = kit_with(&[requested(3, FEB_1_0100, 1, 1_000)]);
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert!(
        matches!(err, BooksError::UnknownSale { index: 3, .. }),
        "{err:?}"
    );
}

#[test]
fn bk_03_a_refund_event_written_as_another_kind_refuses_the_build() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    kit.chain
        .append(
            &kit.counter,
            RecordKind::Sale,
            requested(0, FEB_1_0100, 1, 1_000).encode(),
        )
        .expect("appended");
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert!(
        matches!(err, BooksError::KindMismatch { seq: 1, .. }),
        "{err:?}"
    );
}

#[test]
fn bk_03_an_unknown_event_tag_is_an_error_not_skipped() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    let mut body = opened(0, 1, 1).encode();
    body[0] = 99;
    kit.chain
        .append(&kit.counter, RecordKind::Sale, body)
        .expect("appended");
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    match &err {
        BooksError::BadEvent { seq, .. } => assert_eq!(*seq, 1),
        e => panic!("wrong error {e:?}"),
    }
    assert!(err.to_string().contains("BK-03"));
}

#[test]
fn bk_03_a_body_with_bytes_left_over_is_an_error() {
    let mut kit = kit_with(&[]);
    let mut body = opened(0, 1, 1).encode();
    body.push(0);
    kit.chain
        .append(&kit.counter, RecordKind::Sale, body)
        .expect("appended");
    assert!(matches!(
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]),
        Err(BooksError::BadEvent { .. })
    ));
}

#[test]
fn bk_03_an_event_in_a_record_of_another_kind_is_an_error() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    let expired = Event::Expired { index: 0, at: 5 };
    kit.chain
        .append(&kit.counter, RecordKind::Payment, expired.encode())
        .expect("appended");
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert_eq!(
        err,
        BooksError::KindMismatch {
            counter: kit.counter_id(),
            seq: 1
        }
    );
}

#[test]
fn bk_04_an_event_for_a_sale_never_opened_is_refused() {
    let kit = kit_with(&[Event::Expired { index: 4, at: 5 }]);
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert_eq!(
        err,
        BooksError::UnknownSale {
            counter: kit.counter_id(),
            seq: 0,
            index: 4
        }
    );
}

#[test]
fn bk_04_a_sale_opened_twice_is_refused() {
    let kit = kit_with(&[opened(2, 5, 100), opened(2, 6, 100)]);
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert!(matches!(
        err,
        BooksError::OpenedTwice {
            index: 2,
            seq: 1,
            ..
        }
    ));
}

#[test]
fn bk_04_a_time_that_is_not_a_date_is_refused() {
    let kit = kit_with(&[opened(0, MAX_UNIX + 1, 100)]);
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert_eq!(err, BooksError::TimeOutOfRange { at: MAX_UNIX + 1 });
    let kit = kit_with(&[opened(0, MAX_UNIX, 100)]);
    assert!(Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).is_ok());
}
