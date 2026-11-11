//! BX-40 and BX-42: the paid notice, the POS notice, and the sale facts
//! built from notices.

use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::{Chain, DeviceList};
use openbmt_core::wire::DecodeError;
use openbmt_counter::{Amount, Event, RateSource, Role};

use super::kit::*;
use crate::*;

fn draft(index: u32, state: NoticeState, at: u64) -> NoticeDraft {
    NoticeDraft {
        index,
        amount: Amount(500),
        currency: usd(),
        opened_at: T0,
        state,
        state_at: at,
    }
}

fn notice(number: u64, index: u32, state: NoticeState, at: u64) -> PaidNotice {
    PaidNotice::sign(&identity(3), number, &draft(index, state, at)).unwrap()
}

#[test]
fn bx_40_a_paid_notice_has_a_fixed_length_and_no_satoshi_rate_address_or_txid() {
    let n = notice(1, 7, NoticeState::Paid, T0 + 5);
    let bytes = n.to_bytes();
    // magic and version 5, counter 32, number 8, index 4, amount 8,
    // currency 3, opened 8, state 1, state time 8, signature 64.
    assert_eq!(PAID_NOTICE_LEN, 5 + 32 + 8 + 4 + 8 + 3 + 8 + 1 + 8 + 64);
    assert_eq!(bytes.len(), PAID_NOTICE_LEN);
    assert_eq!(&bytes[..4], b"OBMc");
    // Every notice is that long, whatever it says: there is nowhere for a
    // satoshi amount, a rate, an address or a transaction ID to go.
    for state in [
        NoticeState::Opened,
        NoticeState::Paid,
        NoticeState::Unpaid,
        NoticeState::Final,
    ] {
        let n = PaidNotice::sign(
            &identity(3),
            u64::MAX,
            &NoticeDraft {
                amount: Amount(u64::MAX),
                opened_at: u64::MAX,
                state_at: u64::MAX,
                index: u32::MAX,
                ..draft(0, state, 0)
            },
        )
        .unwrap();
        assert_eq!(n.to_bytes().len(), PAID_NOTICE_LEN);
    }
    // And one more byte is refused.
    let mut long = bytes;
    long.push(0);
    assert!(PaidNotice::from_bytes(&long).is_err());
}

#[test]
fn bx_40_a_paid_notice_reads_back_strictly() {
    let n = notice(3, 7, NoticeState::Final, T0 + 99);
    let bytes = n.to_bytes();
    let back = PaidNotice::from_bytes(&bytes).unwrap();
    assert_eq!(back, n);
    assert!(back.verify());
    assert_eq!(back.to_bytes(), bytes);
    // Every cut is refused.
    for cut in 0..bytes.len() {
        assert!(PaidNotice::from_bytes(&bytes[..cut]).is_err(), "cut {cut}");
    }
    // A state that is not known, a number of zero and a currency in lower
    // case are refused.
    let at = |i: usize, v: u8| {
        let mut b = bytes.clone();
        b[i] = v;
        PaidNotice::from_bytes(&b)
    };
    assert!(matches!(
        at(5 + 32 + 8 + 4 + 8 + 3 + 8, 0),
        Err(DecodeError::NonCanonical(_))
    ));
    assert!(matches!(
        at(5 + 32 + 8 + 4 + 8 + 3 + 8, 5),
        Err(DecodeError::NonCanonical(_))
    ));
    assert!(matches!(
        at(5 + 32 + 8 + 4 + 8, b'u'),
        Err(DecodeError::NonCanonical(_))
    ));
    let zero = {
        let mut b = bytes.clone();
        for x in &mut b[5 + 32..5 + 32 + 8] {
            *x = 0;
        }
        PaidNotice::from_bytes(&b)
    };
    assert!(matches!(zero, Err(DecodeError::NonCanonical(_))));
    assert!(PaidNotice::sign(&identity(3), 0, &draft(1, NoticeState::Opened, T0)).is_none());
}

#[test]
fn bx_40_any_changed_byte_of_a_paid_notice_breaks_it() {
    let bytes = notice(2, 1, NoticeState::Paid, T0 + 1).to_bytes();
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        b[i] ^= 0x01;
        match PaidNotice::from_bytes(&b) {
            Err(_) => {}
            Ok(n) => assert!(
                !n.verify(),
                "byte {i} changed and the notice still verifies"
            ),
        }
    }
}

#[test]
fn bx_40_facts_from_notices_follow_the_counters_states() {
    let notices = [
        notice(1, 1, NoticeState::Opened, T0),
        notice(2, 1, NoticeState::Paid, T0 + 30),
        notice(3, 1, NoticeState::Unpaid, T0 + 60),
        notice(4, 1, NoticeState::Paid, T0 + 30),
        notice(5, 1, NoticeState::Final, T0 + 90),
        notice(6, 2, NoticeState::Opened, T0 + 100),
    ];
    let facts = facts_from_notices(&notices);
    assert_eq!(facts.len(), 2);
    let f = &facts[0];
    assert_eq!((f.index, f.paid_at), (1, Some(T0 + 30)));
    assert!(f.is_final && !f.unpaid_after);
    assert_eq!(f.reference, Some(RefCode::for_sale(&counter(3), 1)));
    assert_eq!(facts[1].paid_at, None);
    // The order the notices are given in does not matter, the numbers do.
    let mut shuffled = notices.to_vec();
    shuffled.reverse();
    assert_eq!(facts_from_notices(&shuffled), facts);
    // Unpaid again clears final.
    let unpaid = facts_from_notices(&notices[..3]);
    assert!(unpaid[0].unpaid_after && !unpaid[0].is_paid_now());
}

#[test]
fn bx_40_a_notice_that_changes_the_sale_is_left_out_of_the_facts() {
    let mut other = draft(1, NoticeState::Final, T0 + 40);
    other.amount = Amount(900);
    let changed = PaidNotice::sign(&identity(3), 3, &other).unwrap();
    let notices = [
        notice(1, 1, NoticeState::Opened, T0),
        notice(2, 1, NoticeState::Paid, T0 + 30),
        changed.clone(),
    ];
    let facts = facts_from_notices(&notices);
    assert_eq!(facts[0].amount, Amount(500));
    assert!(!facts[0].is_final);
    assert_eq!(facts[0].check_notice(&changed), Err(SaleMismatch::Amount));
}

// The property: facts from the counter's records equal facts from the
// notices the counter would send.

fn opened(index: u32, at: u64, cents: u64) -> Event {
    Event::Opened {
        index,
        at,
        amount: Amount(cents),
        currency: usd(),
        needs_sats: 1,
        minor_per_btc: 1,
        source: RateSource::new("test-fixed", at).unwrap(),
        by: None,
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
        first_seen_at: at,
        valued: Amount(0),
        payments: vec![([1; 32], 1)],
        more: false,
    }
}

/// What a sale has been through, so that the events a generator writes for
/// it are the ones the counter writes: a `PosPaid` when it reports the sale
/// paid, a `PosUnpaid` when it reports it unpaid again.
#[derive(Default, Clone, Copy)]
struct SaleState {
    opened: bool,
    reported: bool,
    confirmed: bool,
    ever_paid: bool,
}

/// A random run of events for a few sales, as the counter writes them. With
/// `legacy` the counter writes no `PosPaid` and no `PosUnpaid`, as in a
/// chain from before they existed (PS-06).
fn events(rng: &mut Rng, legacy: bool) -> Vec<Event> {
    let mut out = Vec::new();
    let mut states = [SaleState::default(); 4];
    let mut at = T0;
    for _ in 0..rng.below(40) {
        at += rng.below(90);
        let i = rng.below(4) as usize;
        let index = i as u32 + 1;
        let s = &mut states[i];
        if !s.opened {
            s.opened = true;
            out.push(opened(index, at, 100 * (1 + rng.below(4))));
            continue;
        }
        let pos_paid = |s: &mut SaleState, out: &mut Vec<Event>| {
            if !s.reported && !legacy {
                out.push(Event::PosPaid { index, at });
            }
            s.reported = true;
            s.ever_paid = true;
        };
        match rng.below(8) {
            0 => {
                out.push(at_once(index, at));
                pos_paid(s, &mut out);
            }
            1 => {
                out.push(confirmed(index, at));
                s.confirmed = true;
                pos_paid(s, &mut out);
            }
            2 => {
                pos_paid(s, &mut out);
                out.push(final_(index, at));
                s.confirmed = true;
            }
            3 if s.reported && !s.confirmed => {
                out.push(Event::Replaced { index, at });
                if !legacy {
                    out.push(Event::PosUnpaid { index, at });
                }
                s.reported = false;
            }
            4 if s.ever_paid => {
                out.push(Event::Reopened { index, at });
                s.confirmed = false;
                if s.reported && !legacy {
                    out.push(Event::PosUnpaid { index, at });
                }
                s.reported = false;
            }
            5 => {
                out.push(Event::Override {
                    index,
                    at,
                    role: Role::Manager,
                    name: "Sam".to_string(),
                    sats: 1,
                });
                pos_paid(s, &mut out);
            }
            6 => out.push(Event::Dust { index, at, sats: 1 }),
            _ => {}
        }
    }
    out
}

fn chain_facts(events: &[Event]) -> Vec<SaleFact> {
    let me = identity(3);
    let mut chain = Chain::new();
    for e in events {
        chain.append(&me, e.kind(), e.encode()).unwrap();
    }
    facts_from_chain(&chain, &DeviceList::new([me.device_id()]), &me.device_id()).unwrap()
}

fn notices_for(events: &[Event]) -> Vec<PaidNotice> {
    let me = identity(3);
    let mut maker = NoticeMaker::new(me.device_id());
    let mut out = Vec::new();
    for e in events {
        if let Some(d) = maker.apply(e).unwrap() {
            let number = out.len() as u64 + 1;
            out.push(PaidNotice::sign(&me, number, &d).unwrap());
        }
    }
    out
}

#[test]
fn bx_40_facts_from_the_counters_records_equal_facts_from_its_notices() {
    let mut rng = Rng(0x5eed_0b7c_0b7c_0b7c);
    let mut with_notices = 0;
    for run in 0..1_000 {
        let legacy = run % 4 == 0;
        let events = events(&mut rng, legacy);
        let from_chain = chain_facts(&events);
        let notices = notices_for(&events);
        with_notices += usize::from(!notices.is_empty());
        let from_notices = facts_from_notices(&notices);
        assert_eq!(from_notices, from_chain, "run {run}: {events:?}");
        // Sent in any order and through the bytes, the same.
        let mut again: Vec<PaidNotice> = notices
            .iter()
            .map(|n| PaidNotice::from_bytes(&n.to_bytes()).unwrap())
            .collect();
        rng.shuffle(&mut again);
        assert_eq!(facts_from_notices(&again), from_chain, "run {run}");
    }
    assert!(with_notices > 600);
}

#[test]
fn bx_40_a_sale_with_no_change_of_state_makes_no_notice() {
    let me = identity(3);
    let mut maker = NoticeMaker::new(me.device_id());
    assert!(maker.apply(&opened(1, T0, 500)).unwrap().is_some());
    // A requote, a dust payment: nothing for the Box.
    assert!(maker
        .apply(&Event::Dust {
            index: 1,
            at: T0 + 1,
            sats: 1
        })
        .unwrap()
        .is_none());
    let paid = maker.apply(&at_once(1, T0 + 2)).unwrap().unwrap();
    assert_eq!((paid.state, paid.state_at), (NoticeState::Paid, T0 + 2));
    // The POS record repeats the paid state: no second notice.
    assert!(maker
        .apply(&Event::PosPaid {
            index: 1,
            at: T0 + 2
        })
        .unwrap()
        .is_none());
    // An event for a sale that was never opened is refused.
    assert!(maker.apply(&at_once(9, T0)).is_err());
}

fn a_match() -> PosItem {
    PosItem::Match {
        tender: tkey("o1", "t1"),
        matched: Match {
            sale: SaleKey {
                counter: counter(3),
                index: 4,
            },
            how: MatchHow::Reference,
            by_order: true,
            matched_at: T0 + 5,
        },
    }
}

fn an_alert() -> PosItem {
    let mut ledger = Ledger::new();
    let s = sale(3, 1, 500, T0);
    let o = simple("o1", "Main", 500, T0 + 5);
    step(&mut ledger, &[o], &[paid(s, T0 + 5)], T0 + 5);
    // A tender with no payment: raised after ten minutes.
    let o2 = simple("o2", "Main", 700, T0 + 6);
    let delta = ledger
        .update_stored(
            &[o2],
            ledger.cursor().clone(),
            &[],
            &settings(),
            T0 + 6 + GRACE_SECS,
            |_| Ok::<(), std::convert::Infallible>(()),
        )
        .unwrap();
    PosItem::Alert(delta.new_alerts[0].clone())
}

#[test]
fn bx_42_a_pos_notice_reads_back_strictly_for_every_item() {
    let sale = SaleKey {
        counter: counter(3),
        index: 4,
    };
    let items = [
        a_match(),
        an_alert(),
        PosItem::Closed {
            sale,
            order: oid("o9"),
        },
        PosItem::UnpaidAfterClose {
            sale,
            order: oid("o9"),
        },
    ];
    for (n, item) in items.into_iter().enumerate() {
        let notice = PosNotice::sign(&identity(7), n as u64 + 1, T0, item).unwrap();
        let bytes = notice.to_bytes();
        assert_eq!(&bytes[..4], b"OBMd");
        let back = PosNotice::from_bytes(&bytes).unwrap();
        assert_eq!(back, notice);
        assert!(back.verify());
        assert_eq!(back.to_bytes(), bytes);
        assert!(!back.text().is_empty());
        for cut in 0..bytes.len() {
            assert!(PosNotice::from_bytes(&bytes[..cut]).is_err(), "cut {cut}");
        }
        let mut long = bytes.clone();
        long.push(0);
        assert!(PosNotice::from_bytes(&long).is_err());
        for i in 0..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0x80;
            if let Ok(n) = PosNotice::from_bytes(&b) {
                assert!(!n.verify(), "byte {i}");
            }
        }
    }
    assert!(PosNotice::sign(&identity(7), 0, T0, a_match()).is_none());
}

#[test]
fn bx_42_a_pos_notice_holds_no_satoshi_amount_address_or_txid() {
    // The texts the alerts and the items give are in the shop's money.
    let notice = PosNotice::sign(&identity(7), 1, T0, an_alert()).unwrap();
    let text = notice.text().to_lowercase();
    for word in ["satoshi", "sats", "txid", "bc1", "address"] {
        assert!(!text.contains(word), "{text}");
    }
}
