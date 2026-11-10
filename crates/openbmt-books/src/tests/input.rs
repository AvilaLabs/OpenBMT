//! BK-01 to BK-04: what is read, and what refuses the build.

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
    kit.chain
        .append(&kit.counter, RecordKind::Pairing, vec![0xff; 5])
        .expect("appended");
    kit.chain.append_check_in(&kit.counter);
    // A sale-kind record of another device is not the counter's.
    kit.chain
        .append(&kit.owner, RecordKind::Sale, opened(1, 5, 5).encode())
        .expect("appended");
    let books = kit.books();
    assert_eq!(books.sales().len(), 1);
}

#[test]
fn bk_02_a_refund_kind_record_refuses_the_build() {
    let mut kit = kit_with(&[opened(0, JAN_31_235930, 100)]);
    kit.chain
        .append(&kit.counter, RecordKind::Refund, vec![1, 2, 3])
        .expect("appended");
    let err =
        Books::from_chains(&kit.devices, &[(kit.counter_id(), &kit.chain)]).expect_err("refused");
    assert_eq!(
        err,
        BooksError::RefundRecord {
            counter: kit.counter_id(),
            seq: 1
        }
    );
    assert!(err.to_string().contains("BK-02"));
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
