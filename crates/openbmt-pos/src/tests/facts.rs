//! PS-05 to PS-07: the counter's sales as facts, from its signed events.

use openbmt_core::identity::Identity;
use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::{Chain, ChainError, DeviceList, RecordKind};
use openbmt_counter::{Amount, Event, Role};

use super::kit::*;
use crate::*;

fn opened(index: u32, at: u64, cents: u64) -> Event {
    Event::Opened {
        index,
        at,
        amount: Amount(cents),
        currency: usd(),
        needs_sats: 1,
        minor_per_btc: 1,
    }
}

fn at_once(index: u32, at: u64) -> Event {
    Event::PaidAtOnce {
        index,
        at,
        at_risk_sats: 1,
        label: InstantLabel::LightCheck,
        mode: CheckMode::Light,
    }
}

fn confirmed(index: u32, at: u64) -> Event {
    Event::Confirmed {
        index,
        at,
        received_sats: 1,
        depth: 1,
    }
}

fn final_(index: u32, at: u64) -> Event {
    Event::Final {
        index,
        at,
        received_sats: 1,
        depth: 6,
    }
}

fn override_(index: u32, at: u64) -> Event {
    Event::Override {
        index,
        at,
        role: Role::Manager,
        name: "Sam".to_string(),
        sats: 1,
    }
}

fn chain_of(author: &Identity, events: &[Event]) -> Chain {
    let mut c = Chain::new();
    for e in events {
        c.append(author, e.kind(), e.encode()).unwrap();
    }
    c
}

fn facts_of(events: &[Event]) -> Result<Vec<SaleFact>, FactsError> {
    let me = identity(3);
    let chain = chain_of(&me, events);
    facts_from_chain(&chain, &DeviceList::new([me.device_id()]), &me.device_id())
}

fn one(events: &[Event]) -> SaleFact {
    let mut f = facts_of(events).unwrap();
    assert_eq!(f.len(), 1);
    f.remove(0)
}

#[test]
fn ps_05_facts_come_from_a_verified_chain_of_signed_events() {
    let events = [
        opened(1, T0, 500),
        at_once(1, T0 + 30),
        confirmed(1, T0 + 600),
    ];
    let me = identity(3);
    let chain = chain_of(&me, &events);
    let devices = DeviceList::new([me.device_id()]);
    let facts = facts_from_chain(&chain, &devices, &me.device_id()).unwrap();
    assert_eq!(facts.len(), 1);
    let f = &facts[0];
    assert_eq!(
        (f.counter, f.index, f.amount, f.currency),
        (me.device_id(), 1, Amount(500), usd())
    );
    assert_eq!(f.opened_at, T0);
    assert_eq!(f.reference, Some(RefCode::for_sale(&me.device_id(), 1)));

    // A chain with a record cut out of the middle does not verify.
    let mut records = chain.records().to_vec();
    records.remove(1);
    let broken = Chain::from_records(records);
    assert!(matches!(
        facts_from_chain(&broken, &devices, &me.device_id()),
        Err(FactsError::Chain(ChainError::SeqMismatch { .. }))
    ));
    // Nor does one signed by a device that is not listed.
    let other = DeviceList::new([identity(8).device_id()]);
    assert!(matches!(
        facts_from_chain(&chain, &other, &me.device_id()),
        Err(FactsError::Chain(ChainError::UnknownAuthor { .. }))
    ));
}

#[test]
fn ps_05_only_the_counters_own_sale_records_are_read() {
    let me = identity(3);
    let owner = identity(1);
    let mut c = Chain::new();
    c.append(&me, RecordKind::Sale, opened(1, T0, 500).encode())
        .unwrap();
    // The owner's records, even if they hold a body that looks like an event,
    // are not the counter's.
    c.append(&owner, RecordKind::Sale, opened(2, T0, 900).encode())
        .unwrap();
    c.append(&me, RecordKind::Setting, vec![0xff, 0xff])
        .unwrap();
    c.append_check_in(&me);
    let devices = DeviceList::new([me.device_id(), owner.device_id()]);
    let facts = facts_from_chain(&c, &devices, &me.device_id()).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].index, 1);
}

#[test]
fn ps_06_paid_is_the_first_of_the_four_paid_events() {
    assert_eq!(one(&[opened(1, T0, 500)]).paid_at, None);
    assert_eq!(
        one(&[opened(1, T0, 500), at_once(1, T0 + 5)]).paid_at,
        Some(T0 + 5)
    );
    assert_eq!(
        one(&[opened(1, T0, 500), confirmed(1, T0 + 6)]).paid_at,
        Some(T0 + 6)
    );
    assert_eq!(
        one(&[opened(1, T0, 500), override_(1, T0 + 7)]).paid_at,
        Some(T0 + 7)
    );
    assert_eq!(
        one(&[opened(1, T0, 500), final_(1, T0 + 8)]).paid_at,
        Some(T0 + 8)
    );
    // The first one counts, not a later one.
    let f = one(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        confirmed(1, T0 + 99),
    ]);
    assert_eq!(f.paid_at, Some(T0 + 5));
    assert!(f.is_paid_now() && !f.is_final);
}

#[test]
fn ps_06_reopened_and_replaced_make_a_paid_sale_unpaid_until_paid_again() {
    let f = one(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        Event::Replaced {
            index: 1,
            at: T0 + 9,
        },
    ]);
    assert!(f.unpaid_after && !f.is_paid_now());
    assert_eq!(f.paid_at, Some(T0 + 5));

    let f = one(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        Event::Reopened {
            index: 1,
            at: T0 + 9,
        },
    ]);
    assert!(f.unpaid_after);

    // Paid again by a person: not unpaid any more, first time kept.
    let f = one(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        Event::Replaced {
            index: 1,
            at: T0 + 9,
        },
        override_(1, T0 + 20),
    ]);
    assert!(!f.unpaid_after);
    assert_eq!(f.paid_at, Some(T0 + 5));

    // A conflict on a sale never reported paid says nothing about the POS.
    let f = one(&[
        opened(1, T0, 500),
        Event::Replaced {
            index: 1,
            at: T0 + 9,
        },
    ]);
    assert!(!f.unpaid_after && f.paid_at.is_none());
}

#[test]
fn ps_06_a_conflict_on_a_confirmed_sale_is_not_an_unpaid_report() {
    let f = one(&[
        opened(1, T0, 500),
        confirmed(1, T0 + 5),
        Event::Replaced {
            index: 1,
            at: T0 + 9,
        },
    ]);
    assert!(f.is_paid_now());
    // A reorg of the confirming block is.
    let f = one(&[
        opened(1, T0, 500),
        confirmed(1, T0 + 5),
        Event::Reopened {
            index: 1,
            at: T0 + 9,
        },
    ]);
    assert!(f.unpaid_after);
    // Confirmed again, a conflict is once more not an unpaid report.
    let f = one(&[
        opened(1, T0, 500),
        confirmed(1, T0 + 5),
        Event::Reopened {
            index: 1,
            at: T0 + 9,
        },
        confirmed(1, T0 + 12),
        Event::Replaced {
            index: 1,
            at: T0 + 15,
        },
    ]);
    assert!(f.is_paid_now());
}

#[test]
fn ps_06_final_holds_until_a_reorg() {
    let f = one(&[opened(1, T0, 500), at_once(1, T0 + 5), final_(1, T0 + 9)]);
    assert!(f.is_final);
    let f = one(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        final_(1, T0 + 9),
        Event::Reopened {
            index: 1,
            at: T0 + 12,
        },
    ]);
    assert!(!f.is_final && f.unpaid_after);
}

#[test]
fn ps_06_events_for_unknown_or_doubled_sales_and_bad_bodies_are_refused() {
    assert_eq!(
        facts_of(&[at_once(4, T0)]),
        Err(FactsError::UnknownSale { index: 4 })
    );
    assert_eq!(
        facts_of(&[opened(4, T0, 1), opened(4, T0 + 1, 1)]),
        Err(FactsError::OpenedTwice { index: 4 })
    );
    let me = identity(3);
    let mut c = Chain::new();
    c.append(&me, RecordKind::Sale, vec![99, 0, 0]).unwrap();
    assert!(matches!(
        facts_from_chain(&c, &DeviceList::new([me.device_id()]), &me.device_id()),
        Err(FactsError::BadEvent { seq: 0, .. })
    ));
}

#[test]
fn ps_06_facts_are_ordered_by_sale_index() {
    let f = facts_of(&[opened(5, T0, 1), opened(2, T0, 1), opened(9, T0, 1)]).unwrap();
    assert_eq!(f.iter().map(|f| f.index).collect::<Vec<_>>(), vec![2, 5, 9]);
}

#[test]
fn ps_07_what_the_events_do_not_carry_stays_empty() {
    // Ended sales, late payments, dust and surplus carry nothing the facts
    // need: the fact is the open sale as it was.
    let f = one(&[
        opened(1, T0, 500),
        Event::Dust {
            index: 1,
            at: T0 + 1,
            sats: 5,
        },
        Event::SurplusOwed {
            index: 1,
            at: T0 + 2,
            sats: 5,
        },
        Event::LatePayment {
            index: 1,
            at: T0 + 3,
            txid: [1; 32],
            sats: 5,
        },
        Event::Expired {
            index: 1,
            at: T0 + 900,
        },
    ]);
    assert_eq!(f.paid_at, None);
    assert!(!f.unpaid_after && !f.is_final);
    // The times are the counter's, taken from the events.
    assert_eq!(f.opened_at, T0);
}

fn pos_paid(index: u32, at: u64) -> Event {
    Event::PosPaid { index, at }
}

fn pos_unpaid(index: u32, at: u64) -> Event {
    Event::PosUnpaid { index, at }
}

#[test]
fn ps_09_the_explicit_records_are_used_when_a_sale_has_them() {
    // The times come from the explicit record, not from the paid event.
    let f = one(&[opened(1, T0, 500), at_once(1, T0 + 5), pos_paid(1, T0 + 7)]);
    assert_eq!(f.paid_at, Some(T0 + 7));
    assert!(f.is_paid_now());
    // Unpaid and paid again.
    let f = one(&[
        opened(1, T0, 500),
        pos_paid(1, T0 + 7),
        pos_unpaid(1, T0 + 9),
    ]);
    assert!(f.unpaid_after && f.paid_at == Some(T0 + 7));
    let f = one(&[
        opened(1, T0, 500),
        pos_paid(1, T0 + 7),
        pos_unpaid(1, T0 + 9),
        pos_paid(1, T0 + 20),
    ]);
    assert!(!f.unpaid_after && f.paid_at == Some(T0 + 7));
}

#[test]
fn ps_09_a_conflict_on_a_confirmed_sale_is_not_unpaid_unless_the_counter_said_so() {
    // The inference would say unpaid; the counter wrote no PosUnpaid.
    let f = one(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        pos_paid(1, T0 + 5),
        Event::Replaced {
            index: 1,
            at: T0 + 9,
        },
    ]);
    assert!(f.is_paid_now());
}

#[test]
fn ps_09_without_the_records_the_inference_applies_per_sale() {
    // Sale 1 comes from before the records existed, sale 2 after.
    let f = facts_of(&[
        opened(1, T0, 500),
        at_once(1, T0 + 5),
        Event::Reopened {
            index: 1,
            at: T0 + 9,
        },
        opened(2, T0 + 100, 700),
        at_once(2, T0 + 105),
        pos_paid(2, T0 + 105),
    ])
    .unwrap();
    assert!(f[0].unpaid_after && f[0].paid_at == Some(T0 + 5));
    assert!(f[1].is_paid_now());
}
