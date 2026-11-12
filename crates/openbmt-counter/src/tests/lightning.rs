//! Section 12: a sale paid over Lightning through the shop's own node
//! (CR-109 to CR-111, CR-117). The rules about asking the node for an invoice, the
//! sale code, the transport, the credential and the health line (CR-112 to
//! CR-116, CR-118) belong to the node, the library for the app and the app, and are
//! tested there.

use openbmt_core::records::RecordKind;
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::refund::{CashRefund, RefundAsk, RefundRequest};
use crate::say::{check_line_name, CheckLine};
use crate::{
    Actor, Amount, Event, LightningPaid, LightningRefused, RefundRefused, Report, Role, Sale,
    Screen, State, Why,
};

fn paid(msat: u64) -> LightningPaid {
    LightningPaid {
        payment_hash: [7; 32],
        amount_msat: msat,
        settled_at: Some(1_700_000_123),
    }
}

impl World {
    fn lightning(
        &mut self,
        index: u32,
        paid: LightningPaid,
    ) -> Result<Screen, LightningRefused<&'static str>> {
        let facts = crate::Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c.lightning_paid(index, paid, &facts, &mut self.m)
    }
}

fn ann() -> Actor {
    Actor {
        role: Role::Cashier,
        name: "ann".to_string(),
    }
}

#[test]
fn cr_109_a_lightning_payment_makes_the_sale_paid_and_final_at_once() {
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(o.needs_sats, 5_000);
    w.tick(30);
    let screen = w.lightning(o.index, paid(5_000_000)).unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert_eq!(screen.why, Why::PaidByLightning);
    assert_eq!(screen.state.name(), "Paid");
    assert!(screen.state.is_paid());
    assert_eq!(screen.received_sats, 5_000);
    // The POS and the books are told, once, and no exposure is counted.
    assert!(screen
        .reports
        .iter()
        .any(|r| matches!(r, Report::PosPaid { index } if *index == o.index)));
    assert!(screen.reports.iter().any(|r| matches!(
        r,
        Report::BooksFinal { index, received_sats: 5_000, .. } if *index == o.index
    )));
    assert_eq!(w.c.exposure_outstanding(), 0);
    let sale = w.c.sale(o.index).unwrap();
    assert!(sale.books_final() && !sale.is_live());
    assert_eq!(sale.lightning(), Some(&paid(5_000_000)));
    assert!(sale.accepted().is_none());
    // The records: the payment and then the POS.
    let events = w.m.events();
    assert!(matches!(
        &events[..],
        [
            Event::Opened { .. },
            Event::PaidByLightning {
                payment_hash,
                amount_msat: 5_000_000,
                settled_at: Some(1_700_000_123),
                valued: Amount(500),
                ..
            },
            Event::PosPaid { .. }
        ] if *payment_hash == [7; 32]
    ));
    assert_eq!(events[1].kind(), RecordKind::Payment);
    // Judging again says the same and reports nothing more.
    w.tick(1_000);
    let again = w.judge(o.index);
    assert_eq!(again.state, State::PaidLightning);
    assert!(again.reports.is_empty());
    // Asking again for the same payment changes nothing.
    let records = w.m.records.len();
    let twice = w.lightning(o.index, paid(5_000_000)).unwrap();
    assert_eq!(twice.state, State::PaidLightning);
    assert!(twice.reports.is_empty());
    assert_eq!(w.m.records.len(), records);
}

#[test]
fn cr_109_the_sale_does_not_expire_and_cannot_be_cancelled_or_requoted() {
    let mut w = World::new();
    let o = w.open_five();
    w.lightning(o.index, paid(5_000_000)).unwrap();
    w.tick(10_000);
    assert_eq!(w.judge(o.index).state, State::PaidLightning);
    assert!(w.c.cancel(o.index, w.now, &mut w.m).is_err());
    assert!(w.requote(o.index, 11_000_000).is_err());
}

#[test]
fn cr_109_a_payment_is_not_added_to_a_sale_that_is_closed_or_paid_another_way() {
    let mut w = World::new();
    // Unknown.
    assert_eq!(
        w.lightning(99, paid(5_000_000)).unwrap_err(),
        LightningRefused::UnknownSale(99)
    );
    // Cancelled.
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    assert_eq!(
        w.lightning(o.index, paid(5_000_000)).unwrap_err(),
        LightningRefused::SaleClosed
    );
    // Accepted at once on the chain.
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    assert_eq!(
        w.lightning(o.index, paid(5_000_000)).unwrap_err(),
        LightningRefused::PaidAnotherWay
    );
    // Enough on the address, waiting for a block.
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    assert_eq!(w.judge(o.index).state, State::Waiting);
    assert_eq!(
        w.lightning(o.index, paid(5_000_000)).unwrap_err(),
        LightningRefused::PaidAnotherWay
    );
    // Nothing was written for any of them.
    assert!(w
        .m
        .events()
        .iter()
        .all(|e| !matches!(e, Event::PaidByLightning { .. })));
    // Each refusal names its rule.
    assert_eq!(LightningRefused::<&str>::SaleClosed.rule(), "CR-109");
    assert_eq!(LightningRefused::<&str>::PaidAnotherWay.rule(), "CR-109");
    assert_eq!(LightningRefused::<&str>::UnknownSale(1).rule(), "CR-11");
}

#[test]
fn cr_109_a_payment_for_less_than_any_quote_asks_is_not_taken() {
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(
        w.lightning(o.index, paid(4_999_999)).unwrap_err(),
        LightningRefused::TooLittle {
            sats: 4_999,
            least: 5_000
        }
    );
    assert!(w.lightning(o.index, paid(999)).is_err());
    // The invoice of an earlier quote, at a price below the newer quote, is
    // still taken: the least any quote asks is what counts.
    w.tick(10);
    w.requote(o.index, 9_000_000).unwrap();
    assert_eq!(w.c.sale(o.index).unwrap().needs_sats(), 5_556);
    let screen = w.lightning(o.index, paid(5_555_000)).unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    // A little more than asked is taken whole.
    let o = w.open_five();
    w.lightning(o.index, paid(5_000_900)).unwrap();
    assert_eq!(
        w.c.sale(o.index).unwrap().lightning().unwrap().sats(),
        5_000
    );
}

#[test]
fn cr_109_the_screen_says_paid_over_lightning_and_the_check_line_says_final() {
    let mut w = World::new();
    let o = w.open_five();
    let screen = w.lightning(o.index, paid(5_000_000)).unwrap();
    let words = screen.words();
    assert_eq!(words.tone, openbmt_core::say::Tone::Good);
    assert_eq!(words.heading, "Paid over Lightning");
    assert!(words.next.contains("cash") && !words.next.contains("on-chain"));
    let line = screen.check_line().unwrap();
    assert_eq!(line, CheckLine::LightningFinal);
    assert_eq!(check_line_name(line), "LightningFinal");
    assert_eq!(line.words().heading, "Lightning, final");
    // A sale that is not paid over Lightning has no such line.
    let other = w.open_five();
    assert_eq!(w.judge(other.index).check_line(), None);
}

#[test]
fn cr_110_the_stored_form_keeps_the_payment_and_reads_back() {
    let mut w = World::new();
    let o = w.open_five();
    w.lightning(o.index, paid(5_000_000)).unwrap();
    let bytes = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(bytes.get(4), Some(&3));
    let back = Sale::from_bytes(&bytes).unwrap();
    assert_eq!(back.lightning(), Some(&paid(5_000_000)));
    assert_eq!(back.to_bytes(), bytes);
    // Every cut is refused, and so is a byte more.
    for n in 0..bytes.len() {
        assert!(Sale::from_bytes(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Sale::from_bytes(&longer).is_err());
    // A counter started from what is stored shows the same screen.
    w.restart();
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidLightning);
    assert!(screen.reports.is_empty());
    // A sale that was not paid over Lightning is written as before.
    let other = w.open_five();
    assert_eq!(w.c.sale(other.index).unwrap().to_bytes().get(4), Some(&1));
    // A Lightning form that is not final, or is for nothing, does not read.
    let mut bad = bytes.clone();
    let hash_at = bytes.windows(32).position(|w| w == [7u8; 32]).unwrap();
    for b in bad.iter_mut().skip(hash_at + 32).take(8) {
        *b = 0;
    }
    assert!(Sale::from_bytes(&bad).is_err());
}

#[test]
fn cr_110_the_record_is_strict() {
    let event = Event::PaidByLightning {
        index: 4,
        at: 1_700_000_000,
        payment_hash: [9; 32],
        amount_msat: 25_000_000,
        settled_at: None,
        valued: Amount(2_000),
    };
    let bytes = event.encode();
    assert_eq!(Event::decode(&bytes).unwrap(), event);
    let with_time = Event::PaidByLightning {
        index: 4,
        at: 1_700_000_000,
        payment_hash: [9; 32],
        amount_msat: 25_000_001,
        settled_at: Some(1_699_999_999),
        valued: Amount(2_000),
    };
    assert_eq!(Event::decode(&with_time.encode()).unwrap(), with_time);
    assert_eq!(event.index(), 4);
    assert_eq!(event.at(), 1_700_000_000);
    assert_eq!(event.kind(), RecordKind::Payment);
    for n in 0..bytes.len() {
        assert!(Event::decode(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Event::decode(&longer).is_err());
    // A payment for nothing is not a payment.
    let zero = Event::PaidByLightning {
        index: 4,
        at: 1,
        payment_hash: [9; 32],
        amount_msat: 0,
        settled_at: None,
        valued: Amount(0),
    };
    assert!(matches!(
        Event::decode(&zero.encode()),
        Err(DecodeError::NonCanonical(_))
    ));
}

#[test]
fn cr_111_what_arrived_before_is_owed_back_and_what_arrives_after_is_late() {
    let mut w = World::new();
    let o = w.open_five();
    // 3,000 sats arrive on the address, then the customer pays the invoice.
    let part = w.pay(&o, 3_000);
    w.see(&part, 1);
    assert_eq!(w.judge(o.index).state, State::PartPaid);
    let screen = w.lightning(o.index, paid(5_000_000)).unwrap();
    assert_eq!(screen.state, State::PaidLightning);
    assert!(screen
        .reports
        .iter()
        .any(|r| matches!(r, Report::SurplusOwed { sats: 3_000, .. })));
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, Event::SurplusOwed { sats: 3_000, .. })));
    // The same transaction seen again is not a late payment.
    w.see(&part, 1);
    assert!(!w.judge(o.index).late);
    // Coins that arrive later are a late payment, with the sale's words.
    let again = w.pay(&o, 5_000);
    let seen = w.see(&again, 1);
    assert!(seen
        .reports
        .iter()
        .any(|r| matches!(r, Report::LatePayment { sats: 5_000, .. })));
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidLightning);
    assert!(screen.late);
    assert!(screen
        .flag_words()
        .iter()
        .any(|f| f.heading == "Late payment"));
    // A block that holds it adds nothing the second time.
    w.confirm(&[again], 99);
    assert_eq!(w.c.sale(o.index).unwrap().late_payments().len(), 1);
}

fn request(sats: u64, to: u8) -> RefundRequest {
    RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: p2wsh(to),
        actor: Actor {
            role: Role::Manager,
            name: "max".to_string(),
        },
        reason: "wrong size".to_string(),
    }
}

fn cash(minor: u64) -> CashRefund {
    CashRefund {
        actor: ann(),
        minor: Amount(minor),
        reason: "till refund".to_string(),
    }
}

#[test]
fn cr_111_what_arrived_over_lightning_counts_as_received_for_a_cash_refund() {
    let mut w = World::new();
    let o = w.open_five();
    w.lightning(o.index, paid(5_000_000)).unwrap();
    // A cash refund of $2.00 (2,000 sats) is taken, and so is the rest.
    w.c.refund_in_cash(o.index, cash(200), w.now, &w.chain, &mut w.m)
        .unwrap();
    w.c.refund_in_cash(o.index, cash(300), w.now, &w.chain, &mut w.m)
        .unwrap();
    assert!(w.c.sale(o.index).unwrap().at_refund_depth());
    // More than was received is still refused.
    let err =
        w.c.refund_in_cash(o.index, cash(1), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert!(matches!(err, RefundRefused::MoreThanReceived { .. }));
}

#[test]
fn cr_217_a_sale_paid_only_over_lightning_is_refunded_in_cash_only() {
    let mut w = World::new();
    let o = w.open_five();
    w.lightning(o.index, paid(5_000_000)).unwrap();
    assert_eq!(w.c.refund_onchain_room(o.index, w.now, &w.chain), Some(0));
    let err =
        w.c.request_refund(o.index, request(3_000, 0xa1), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert_eq!(
        err,
        RefundRefused::LightningCashOnly {
            room: 0,
            sats: 3_000
        }
    );
    assert_eq!(err.rule(), "CR-217");
    assert!(err.to_string().starts_with("CR-217: "));
    assert_eq!(err.words().heading, "Refund in cash");
    // Nothing was written, and the cash refund is open.
    assert!(w.c.refund_views(o.index).is_empty());
    w.c.refund_in_cash(o.index, cash(300), w.now, &w.chain, &mut w.m)
        .unwrap();
}

#[test]
fn cr_217_a_part_lightning_sale_is_refunded_on_chain_up_to_its_on_chain_part() {
    let mut w = World::new();
    let o = w.open_five();
    let part = w.pay(&o, 3_000);
    w.see(&part, 1);
    w.lightning(o.index, paid(5_000_000)).unwrap();
    assert_eq!(
        w.c.refund_onchain_room(o.index, w.now, &w.chain),
        Some(3_000)
    );
    // Past the on-chain part: refused, though less than was received in all.
    let err =
        w.c.request_refund(o.index, request(3_001, 0xa1), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert_eq!(
        err,
        RefundRefused::LightningCashOnly {
            room: 3_000,
            sats: 3_001
        }
    );
    // The on-chain part is open, and takes the room.
    w.c.request_refund(o.index, request(2_000, 0xa1), w.now, &w.chain, &mut w.m)
        .unwrap();
    assert_eq!(
        w.c.refund_onchain_room(o.index, w.now, &w.chain),
        Some(1_000)
    );
    let err =
        w.c.request_refund(o.index, request(1_001, 0xa2), w.now, &w.chain, &mut w.m)
            .unwrap_err();
    assert_eq!(
        err,
        RefundRefused::LightningCashOnly {
            room: 1_000,
            sats: 1_001
        }
    );
    // Cash is open for the rest: $3.00 is 3,000 sats, within what was
    // received.
    w.c.refund_in_cash(o.index, cash(300), w.now, &w.chain, &mut w.m)
        .unwrap();
}

#[test]
fn cr_217_a_sale_not_paid_over_lightning_has_no_such_limit() {
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(w.c.refund_onchain_room(o.index, w.now, &w.chain), None);
}

fn late_paid(hash: u8, msat: u64) -> LightningPaid {
    LightningPaid {
        payment_hash: [hash; 32],
        amount_msat: msat,
        settled_at: None,
    }
}

#[test]
fn cr_117_an_invoice_settled_after_a_cancel_is_a_late_payment() {
    let mut w = World::new();
    let o = w.open_five();
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    // The counter does not take it as a payment of the sale.
    assert!(matches!(
        w.lightning(o.index, paid(5_000_000)),
        Err(LightningRefused::SaleClosed)
    ));
    let reports =
        w.c.lightning_late(o.index, paid(5_000_000), w.now, &mut w.m)
            .unwrap();
    assert_eq!(
        reports,
        vec![Report::LatePayment {
            index: o.index,
            sats: 5_000
        }]
    );
    assert!(w.m.events().iter().any(|e| matches!(
        e,
        Event::LateLightning { index, payment_hash, amount_msat: 5_000_000, .. }
            if *index == o.index && *payment_hash == [7; 32]
    )));
    let screen = w.judge(o.index);
    assert!(screen.late);
    assert!(screen
        .flag_words()
        .iter()
        .any(|f| f.heading == "Late payment"));
    // Asked again, nothing more is written or reported.
    let before = w.m.events().len();
    let again =
        w.c.lightning_late(o.index, paid(5_000_000), w.now, &mut w.m)
            .unwrap();
    assert!(again.is_empty());
    assert_eq!(w.m.events().len(), before);
    // The stored sale reads back with it, even for less than a satoshi.
    let dust =
        w.c.lightning_late(o.index, late_paid(8, 400), w.now, &mut w.m)
            .unwrap();
    assert_eq!(
        dust,
        vec![Report::LatePayment {
            index: o.index,
            sats: 1
        }]
    );
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.late_payments().len(), 2);
    assert_eq!(
        Sale::from_bytes(&sale.to_bytes())
            .unwrap()
            .late_payments()
            .len(),
        2
    );
    // A sale the counter does not hold is refused.
    assert!(matches!(
        w.c.lightning_late(99, paid(5_000_000), w.now, &mut w.m),
        Err(LightningRefused::UnknownSale(99))
    ));
}

#[test]
fn cr_117_a_second_invoice_of_a_sale_paid_over_lightning_is_late() {
    let mut w = World::new();
    let o = w.open_five();
    w.lightning(o.index, paid(5_000_000)).unwrap();
    // The invoice that paid the sale is not late.
    assert!(w
        .c
        .lightning_late(o.index, paid(5_000_000), w.now, &mut w.m)
        .unwrap()
        .is_empty());
    let reports =
        w.c.lightning_late(o.index, late_paid(8, 3_000_000), w.now, &mut w.m)
            .unwrap();
    assert_eq!(reports.len(), 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::PaidLightning);
    assert!(screen.late);
}

#[test]
fn cr_117_the_record_of_a_late_lightning_payment_is_strict() {
    let event = Event::LateLightning {
        index: 4,
        at: 1_700_000_000,
        payment_hash: [9; 32],
        amount_msat: 25_000_001,
    };
    let bytes = event.encode();
    assert_eq!(Event::decode(&bytes).unwrap(), event);
    assert_eq!(event.index(), 4);
    assert_eq!(event.at(), 1_700_000_000);
    assert_eq!(event.kind(), RecordKind::Payment);
    for n in 0..bytes.len() {
        assert!(Event::decode(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Event::decode(&longer).is_err());
    let zero = Event::LateLightning {
        index: 4,
        at: 1,
        payment_hash: [9; 32],
        amount_msat: 0,
    };
    assert!(matches!(
        Event::decode(&zero.encode()),
        Err(DecodeError::NonCanonical(_))
    ));
}
