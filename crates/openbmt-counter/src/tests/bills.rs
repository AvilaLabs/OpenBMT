//! Section 25: bills, tabs and the till's reference (CR-245 to CR-255).

use openbmt_core::records::RecordKind;
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::bill_say::is_placeholder;
use crate::bills::{reference_ok, split_shares, MAGIC_BILLS};
use crate::say::all_words;
use crate::{
    Actor, Amount, BillEnd, BillKind, BillPart, BillRefused, Bills, Event, Facts, LightningPaid,
    OpenRefused, PartRefused, PartWay, Role, SaleExtras, SplitError, State, MAX_BILL_PARTS,
    MAX_OPEN_BILLS,
};

const DAY: u64 = 86_400;

fn manager() -> Actor {
    Actor {
        role: Role::Manager,
        name: "May".to_string(),
    }
}

impl World {
    fn bill(&mut self, kind: BillKind, cents: u64) -> u32 {
        let now = self.now;
        self.c
            .open_bill(kind, Amount(cents), None, now, &mut self.m)
            .unwrap()
    }

    fn part(
        &mut self,
        number: u32,
        cents: u64,
    ) -> Result<crate::Opened, PartRefused<&'static str>> {
        let facts = self.open_facts();
        self.c.open_bill_part(
            number,
            Amount(cents),
            Amount(0),
            rate(),
            &fresh(facts.now),
            facts,
            None,
            SaleExtras::default(),
            &mut self.m,
        )
    }

    /// Pay a sale on chain and confirm it, then judge it.
    fn pay_and_confirm(&mut self, o: &crate::Opened, height: u32) -> State {
        let t = self.pay(o, o.needs_sats);
        self.confirm(&[t], height);
        self.judge(o.index).state
    }

    fn ln(&mut self, index: u32, sats: u64) {
        let facts = Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        let paid = LightningPaid {
            payment_hash: [index as u8 + 1; 32],
            amount_msat: sats * 1_000,
            settled_at: Some(self.now),
        };
        self.c
            .lightning_paid(index, paid, &facts, &mut self.m)
            .unwrap();
    }

    fn view(&self, number: u32) -> crate::BillView {
        self.c.bill_view(number).unwrap()
    }
}

fn ended_how(w: &World, number: u32) -> Option<BillEnd> {
    w.c.bill_view(number)?.bill.ended().map(|e| e.how)
}

// CR-245.

#[test]
fn cr_245_a_reference_is_one_to_thirty_two_of_the_allowed_characters() {
    for ok in ["A", "INV-2026/0042", "a_b#c.d-9", &"x".repeat(32)] {
        assert!(reference_ok(ok), "{ok}");
    }
    for bad in [
        "",
        &"x".repeat(33),
        "a b",
        "a+b",
        "caf\u{e9}",
        "a\n",
        "a,b",
        "\u{ff21}",
    ] {
        assert!(!reference_ok(bad), "{bad:?}");
    }
}

#[test]
fn cr_245_a_sale_takes_a_reference_when_it_opens_and_it_is_a_tag_25_record() {
    let mut w = World::new();
    let facts = w.open_facts();
    let o =
        w.c.open_with_extras(
            Amount(500),
            Amount(0),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            None,
            SaleExtras {
                reference: Some("CHK-17"),
                ..SaleExtras::default()
            },
            &mut w.m,
        )
        .unwrap();
    assert_eq!(w.c.sale_reference(o.index), Some("CHK-17"));
    let events = w.m.events();
    assert!(matches!(events[0], Event::Opened { .. }));
    let e = Event::Referenced {
        index: o.index,
        at: T0,
        reference: "CHK-17".to_string(),
    };
    assert_eq!(events[1], e);
    let bytes = e.encode();
    assert_eq!(bytes[0], 25);
    assert_eq!(&bytes[1..5], &o.index.to_le_bytes());
    assert_eq!(&bytes[5..13], &T0.to_le_bytes());
    assert_eq!(&bytes[13..], b"\x06CHK-17");
    assert_eq!(e.kind(), RecordKind::Sale);
    assert_eq!(Event::decode(&bytes), Ok(e));
}

#[test]
fn cr_245_a_reference_that_does_not_fit_is_refused_with_nothing_written() {
    let mut w = World::new();
    let facts = w.open_facts();
    let err =
        w.c.open_with_extras(
            Amount(500),
            Amount(0),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            None,
            SaleExtras {
                reference: Some("has space"),
                ..SaleExtras::default()
            },
            &mut w.m,
        )
        .unwrap_err();
    assert_eq!(err, OpenRefused::BadReference);
    assert_eq!(err.rule(), "CR-245");
    assert_eq!(w.m.writes, 0);
    let o = w.open_five();
    w.m.reset();
    for bad in ["", "no good", &"y".repeat(33)] {
        let err = w.c.add_reference(o.index, bad, T0, &mut w.m).unwrap_err();
        assert_eq!(err, BillRefused::BadReference);
        let now = w.now;
        let n = w.bill(BillKind::Bill, 100);
        w.m.reset();
        let err = w.c.add_bill_reference(n, bad, now, &mut w.m).unwrap_err();
        assert_eq!(err, BillRefused::BadReference);
        assert_eq!(w.m.writes, 0);
    }
    let err =
        w.c.open_bill(BillKind::Bill, Amount(100), Some("a b"), T0, &mut w.m)
            .unwrap_err();
    assert_eq!(err, BillRefused::BadReference);
    assert_eq!(w.m.writes, 0);
}

#[test]
fn cr_245_a_reference_is_added_once_and_survives_a_restart() {
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(w.c.sale_reference(o.index), None);
    w.c.add_reference(o.index, "R-1", T0 + 1, &mut w.m).unwrap();
    let err =
        w.c.add_reference(o.index, "R-2", T0 + 2, &mut w.m)
            .unwrap_err();
    assert_eq!(err, BillRefused::AlreadyReferenced);
    assert_eq!(w.c.sale_reference(o.index), Some("R-1"));
    assert_eq!(
        w.c.add_reference(77, "R-2", T0, &mut w.m).unwrap_err(),
        BillRefused::UnknownSale(77)
    );
    let c = w.m.reload();
    assert_eq!(c.sale_reference(o.index), Some("R-1"));
    // A failed write leaves the sale with none.
    let p = w.open_five();
    w.m.reset();
    w.m.fail_on = Some(1);
    assert!(matches!(
        w.c.add_reference(p.index, "R-3", T0, &mut w.m),
        Err(BillRefused::Store(_))
    ));
    assert_eq!(w.c.sale_reference(p.index), None);
}

#[test]
fn cr_245_a_bill_takes_a_reference_with_a_byte_that_says_it_is_a_bills() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 500);
    w.c.add_bill_reference(n, "TAB/9", T0, &mut w.m).unwrap();
    assert_eq!(w.view(n).bill.reference(), Some("TAB/9"));
    assert_eq!(
        w.c.add_bill_reference(n, "TAB/10", T0, &mut w.m)
            .unwrap_err(),
        BillRefused::AlreadyReferenced
    );
    let e = Event::BillReferenced {
        number: n,
        at: T0,
        reference: "TAB/9".to_string(),
    };
    let bytes = e.encode();
    assert_eq!(bytes[0], 25);
    assert_eq!(&bytes[13..], b"\x05TAB/9\x01");
    assert_eq!(Event::decode(&bytes), Ok(e.clone()));
    assert_eq!(e.bill_number(), Some(n));
    assert_eq!(e.sale_index(), None);
    let mut other = bytes.clone();
    *other.last_mut().unwrap() = 2;
    assert!(Event::decode(&other).is_err());
    other.push(0);
    assert!(Event::decode(&other).is_err());
    // A bill opened with one writes both records.
    let m =
        w.c.open_bill(BillKind::Tab, Amount(900), Some("T-1"), T0, &mut w.m);
    let m = m.unwrap();
    assert_eq!(w.view(m).bill.reference(), Some("T-1"));
    // The records of a bill do not move a sale's last record.
    assert_eq!(
        w.m.reload().bill_view(n).unwrap().bill.reference(),
        Some("TAB/9")
    );
}

// CR-246.

#[test]
fn cr_246_a_bill_has_a_number_from_one_and_the_amount_is_above_nothing() {
    let mut w = World::new();
    assert_eq!(w.c.next_bill_number(), 1);
    let a = w.bill(BillKind::Bill, 500);
    let b = w.bill(BillKind::Tab, 700);
    assert_eq!((a, b), (1, 2));
    let v = w.view(a);
    assert_eq!(v.bill.amount(), Amount(500));
    assert_eq!(v.bill.kind(), BillKind::Bill);
    assert_eq!(v.bill.currency(), usd());
    assert!(v.bill.is_open());
    w.m.reset();
    for bad in [0, crate::MAX_BILL_AMOUNT + 1] {
        let err =
            w.c.open_bill(BillKind::Bill, Amount(bad), None, T0, &mut w.m)
                .unwrap_err();
        assert_eq!(err, BillRefused::BadAmount);
        assert_eq!(err.rule(), "CR-246");
    }
    assert_eq!(w.m.writes, 0);
    assert!(w.c.bill_view(99).is_none());
}

#[test]
fn cr_246_the_number_is_stored_before_it_is_used_and_never_given_twice() {
    let mut w = World::new();
    w.m.reset();
    // Write 1 is the file with the next number; stopping there wastes one.
    w.m.fail_on = Some(2);
    let err =
        w.c.open_bill(BillKind::Bill, Amount(500), None, T0, &mut w.m)
            .unwrap_err();
    assert!(matches!(err, BillRefused::Store(_)));
    assert_eq!(w.m.log[0], "Bills");
    assert_eq!(w.m.log[1], "FAILED record");
    let mut c = w.m.reload();
    assert_eq!(c.next_bill_number(), 2, "number 1 is wasted, not reused");
    assert!(c.bill_views().is_empty());
    w.m.reset();
    let n = c
        .open_bill(BillKind::Bill, Amount(500), None, T0, &mut w.m)
        .unwrap();
    assert_eq!(n, 2);
    // A failure of the first write gives no number at all.
    let mut w = World::new();
    w.m.reset();
    w.m.fail_on = Some(1);
    assert!(w
        .c
        .open_bill(BillKind::Bill, Amount(500), None, T0, &mut w.m)
        .is_err());
    assert_eq!(w.c.next_bill_number(), 1);
    assert_eq!(w.m.reload().next_bill_number(), 1);
}

// CR-247.

#[test]
fn cr_247_the_records_of_a_bill_have_tags_26_to_30_and_are_strict() {
    let cases: Vec<(Event, u8, Vec<u8>)> = vec![
        (
            Event::BillOpened {
                number: 7,
                at: 9,
                kind: BillKind::Tab,
                amount: Amount(1_234),
                currency: usd(),
            },
            26,
            [&[2u8][..], &1_234u64.to_le_bytes(), b"USD"].concat(),
        ),
        (
            Event::BillAmount {
                number: 7,
                at: 9,
                amount: Amount(2_000),
            },
            27,
            2_000u64.to_le_bytes().to_vec(),
        ),
        (
            Event::BillPartPaid {
                number: 7,
                at: 9,
                amount: Amount(300),
                way: PartWay::Card,
            },
            28,
            [&300u64.to_le_bytes()[..], &[2u8]].concat(),
        ),
        (
            Event::BillEnded {
                number: 7,
                at: 9,
                how: BillEnd::ClosedShort,
                unpaid: Amount(450),
            },
            29,
            [&[2u8][..], &450u64.to_le_bytes()].concat(),
        ),
        (
            Event::BillPartSale {
                number: 7,
                at: 9,
                sale: 3,
            },
            30,
            3u32.to_le_bytes().to_vec(),
        ),
    ];
    for (e, tag, tail) in cases {
        let bytes = e.encode();
        assert_eq!(bytes[0], tag);
        assert_eq!(&bytes[1..5], &7u32.to_le_bytes());
        assert_eq!(&bytes[5..13], &9u64.to_le_bytes());
        assert_eq!(&bytes[13..], &tail[..]);
        assert_eq!(e.kind(), RecordKind::Sale);
        assert_eq!(Event::decode(&bytes), Ok(e.clone()));
        let mut long = bytes.clone();
        long.push(0);
        assert_eq!(Event::decode(&long), Err(DecodeError::TrailingBytes));
        assert!(Event::decode(&bytes[..bytes.len() - 1]).is_err());
    }
    // Things the counter never writes.
    let zero = |mut b: Vec<u8>, from: usize, to: usize| {
        b[from..to].fill(0);
        b
    };
    let open = Event::BillOpened {
        number: 1,
        at: 1,
        kind: BillKind::Bill,
        amount: Amount(5),
        currency: usd(),
    }
    .encode();
    assert!(
        Event::decode(&zero(open.clone(), 14, 22)).is_err(),
        "no amount"
    );
    assert!(
        Event::decode(&zero(open.clone(), 13, 14)).is_err(),
        "no kind"
    );
    let ended = Event::BillEnded {
        number: 1,
        at: 1,
        how: BillEnd::Settled,
        unpaid: Amount(0),
    }
    .encode();
    let mut bad = ended.clone();
    bad[14] = 9;
    assert!(Event::decode(&bad).is_err(), "unknown end");
    let mut bad = ended.clone();
    bad[14] = 2;
    assert!(
        Event::decode(&bad).is_err(),
        "closed short with nothing unpaid"
    );
    let mut bad = ended;
    bad[15] = 1;
    assert!(
        Event::decode(&bad).is_err(),
        "settled with something unpaid"
    );
}

#[test]
fn cr_247_each_record_is_stored_before_the_state_it_names() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 500);
    w.m.reset();
    w.c.add_other_part(n, Amount(100), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    assert_eq!(w.m.log, ["record", "Bills"]);
    // A stop after the record: the record names a part the file lacks.
    w.m.reset();
    w.m.fail_on = Some(2);
    assert!(w
        .c
        .add_other_part(n, Amount(100), PartWay::Card, T0, &mut w.m)
        .is_err());
    assert_eq!(w.view(n).received(), 100);
    assert_eq!(w.m.reload().bill_view(n).unwrap().received(), 100);
    assert!(matches!(
        w.m.events().last(),
        Some(Event::BillPartPaid {
            way: PartWay::Card,
            ..
        })
    ));
    w.m.reset();
    w.c.set_tab_amount(n, Amount(900), T0, &mut w.m)
        .unwrap_err();
    assert_eq!(w.m.writes, 0, "a bill's amount does not change");
    let t = w.bill(BillKind::Tab, 500);
    w.m.reset();
    w.c.set_tab_amount(t, Amount(900), T0, &mut w.m).unwrap();
    assert_eq!(w.m.log, ["record", "Bills"]);
    w.m.reset();
    w.c.cancel_bill(t, T0, &mut w.m).unwrap();
    assert_eq!(w.m.log, ["record", "Bills"]);
}

// CR-248.

#[test]
fn cr_248_a_bitcoin_part_is_a_sale_opened_for_the_bill_with_every_rule_of_a_sale() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 1_000);
    let o = w.part(n, 400).unwrap();
    assert_eq!(o.needs_sats, 4_000);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.amount(), Amount(400));
    assert_eq!(w.c.bill_of_sale(o.index), Some(n));
    assert_eq!(w.view(n).bill.parts(), [BillPart::Sale(o.index)]);
    assert!(matches!(
        w.m.events().last(),
        Some(Event::BillPartSale { sale, .. }) if *sale == o.index
    ));
    // A part has the quote of any sale: five minutes, and then a new one.
    assert_eq!(o.quote.until, T0 + 300);
    // A sale that does not open (a stale rate) is not a part.
    let facts = w.open_facts();
    let err =
        w.c.open_bill_part(
            n,
            Amount(100),
            Amount(0),
            rate(),
            &fresh(facts.now - 10_000),
            facts,
            None,
            SaleExtras::default(),
            &mut w.m,
        )
        .unwrap_err();
    assert!(matches!(
        err,
        PartRefused::Open(OpenRefused::StaleRate { .. })
    ));
    assert_eq!(w.view(n).bill.parts().len(), 1);
    // A part for a bill that ended or does not exist opens no sale.
    let sales = w.c.held_sales();
    assert_eq!(
        w.part(99, 100).unwrap_err(),
        PartRefused::Bill(BillRefused::UnknownBill(99))
    );
    let c = w.bill(BillKind::Bill, 100);
    w.c.cancel_bill(c, T0, &mut w.m).unwrap();
    assert_eq!(
        w.part(c, 100).unwrap_err(),
        PartRefused::Bill(BillRefused::NotOpen)
    );
    assert_eq!(w.c.held_sales(), sales);
}

#[test]
fn cr_248_a_paid_deposit_request_is_joined_to_a_bill_and_joining_is_final() {
    let mut w = World::new();
    // A request to send (CR-205) of $20.00, paid and confirmed.
    let facts = w.open_facts();
    let dep =
        w.c.open_with_extras(
            Amount(2_000),
            Amount(0),
            usd(),
            rate(),
            &fresh(facts.now),
            facts,
            None,
            SaleExtras {
                request: Some(crate::RequestDays::Seven),
                ..SaleExtras::default()
            },
            &mut w.m,
        )
        .unwrap();
    assert_eq!(w.pay_and_confirm(&dep, 100), State::Confirmed);
    let n = w.bill(BillKind::Bill, 10_000);
    w.c.join_sale(n, dep.index, T0, &mut w.m).unwrap();
    let v = w.view(n);
    assert_eq!(v.received_bitcoin, 2_000);
    assert_eq!(v.left, 8_000);
    // Final: the sale joins no second bill, nor the same one twice.
    let m = w.bill(BillKind::Bill, 5_000);
    assert_eq!(
        w.c.join_sale(m, dep.index, T0, &mut w.m).unwrap_err(),
        BillRefused::SaleInABill(n)
    );
    assert_eq!(
        w.c.join_sale(n, dep.index, T0, &mut w.m).unwrap_err(),
        BillRefused::SaleInABill(n)
    );
    assert_eq!(
        w.c.join_sale(n, 55, T0, &mut w.m).unwrap_err(),
        BillRefused::UnknownSale(55)
    );
    // A sale that ended with nothing received cannot join.
    let gone = w.open_five();
    w.c.cancel(gone.index, T0, &mut w.m).unwrap();
    assert_eq!(
        w.c.join_sale(n, gone.index, T0, &mut w.m).unwrap_err(),
        BillRefused::SaleEnded
    );
    // An open sale joins and counts once it is paid.
    let open = w.open_five();
    w.c.join_sale(n, open.index, T0, &mut w.m).unwrap();
    assert_eq!(w.view(n).pending, 1);
    assert_eq!(w.view(n).received_bitcoin, 2_000);
}

#[test]
fn cr_248_the_tip_is_not_part_of_the_bill() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 1_000);
    let facts = w.open_facts();
    let o =
        w.c.open_bill_part(
            n,
            Amount(600),
            Amount(100),
            rate(),
            &fresh(facts.now),
            facts,
            None,
            SaleExtras::default(),
            &mut w.m,
        )
        .unwrap();
    assert_eq!(
        o.needs_sats, 7_000,
        "the code is for the amount and the tip"
    );
    assert_eq!(w.pay_and_confirm(&o, 100), State::Confirmed);
    let v = w.view(n);
    assert_eq!(v.received_bitcoin, 600);
    assert_eq!(v.left, 400);
    assert!(v.bill.is_open());
}

// CR-249.

#[test]
fn cr_249_a_pending_part_counts_for_nothing_and_a_paid_part_counts_its_amount() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 1_000);
    let o = w.part(n, 400).unwrap();
    let v = w.view(n);
    assert_eq!((v.received(), v.pending, v.left), (0, 1, 1_000));
    // Part paid is still pending.
    let first = w.pay(&o, 2_000);
    w.see(&first, 1);
    assert_eq!(w.judge(o.index).state, State::PartPaid);
    assert_eq!(w.view(n).received(), 0);
    // Paid by its own rules: now it counts.
    let second = w.pay(&o, 2_000);
    w.confirm(&[first, second], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    let v = w.view(n);
    assert_eq!((v.received_bitcoin, v.pending, v.left), (400, 0, 600));
    assert!(v.bill.is_open());
    w.c.add_other_part(n, Amount(250), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    let v = w.view(n);
    assert_eq!((v.received_other, v.received(), v.left), (250, 650, 350));
}

#[test]
fn cr_249_a_bill_split_across_three_customers_by_lightning_chain_and_cash() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 9_000);
    assert_eq!(w.c.bill_split::<&str>(n, 3).unwrap(), [3_000, 3_000, 3_000]);
    // The first pays over Lightning.
    let a = w.part(n, 3_000).unwrap();
    w.ln(a.index, a.needs_sats);
    let v = w.view(n);
    assert_eq!((v.received_bitcoin, v.left), (3_000, 6_000));
    assert_eq!(ended_how(&w, n), None);
    // The second pays in cash.
    w.c.add_other_part(n, Amount(3_000), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    assert_eq!(w.view(n).left, 3_000);
    assert_eq!(ended_how(&w, n), None);
    // The third pays on chain; the bill settles when the sale is judged paid.
    let c = w.part(n, 3_000).unwrap();
    assert_eq!(w.pay_and_confirm(&c, 100), State::Confirmed);
    let v = w.view(n);
    assert_eq!(
        (v.received_bitcoin, v.received_other, v.left),
        (6_000, 3_000, 0)
    );
    assert_eq!(ended_how(&w, n), Some(BillEnd::Settled));
    assert!(matches!(
        w.m.events().last(),
        Some(Event::BillEnded {
            how: BillEnd::Settled,
            ..
        })
    ));
    // The state is on disk.
    assert_eq!(
        w.m.reload()
            .bill_view(n)
            .unwrap()
            .bill
            .ended()
            .map(|e| e.how),
        Some(BillEnd::Settled)
    );
    // A bill that is settled takes no more.
    assert_eq!(
        w.c.add_other_part(n, Amount(1), PartWay::Cash, T0, &mut w.m)
            .unwrap_err(),
        BillRefused::NotOpen
    );
}

#[test]
fn cr_249_the_part_that_passes_the_amount_is_not_cut_short() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 1_000);
    let o = w.part(n, 1_500).unwrap();
    assert_eq!(o.needs_sats, 15_000, "the sale keeps its own amount");
    assert_eq!(w.pay_and_confirm(&o, 100), State::Confirmed);
    assert_eq!(ended_how(&w, n), Some(BillEnd::Settled));
    assert_eq!(w.view(n).received_bitcoin, 1_500);
    // An other-way part that passes it settles it too.
    let m = w.bill(BillKind::Bill, 1_000);
    w.c.add_other_part(m, Amount(1_200), PartWay::Card, T0, &mut w.m)
        .unwrap();
    assert_eq!(ended_how(&w, m), Some(BillEnd::Settled));
}

#[test]
fn cr_249_a_part_joined_after_it_is_paid_settles_the_bill_at_once() {
    let mut w = World::new();
    let o = w.open(1_000);
    assert_eq!(w.pay_and_confirm(&o, 100), State::Confirmed);
    let n = w.bill(BillKind::Bill, 1_000);
    w.c.join_sale(n, o.index, T0, &mut w.m).unwrap();
    assert_eq!(ended_how(&w, n), Some(BillEnd::Settled));
}

// CR-250.

#[test]
fn cr_250_the_split_rounds_up_and_the_last_share_takes_what_is_left_over() {
    assert_eq!(split_shares(1_000, 4), Ok(vec![250, 250, 250, 250]));
    assert_eq!(split_shares(1_000, 3), Ok(vec![334, 334, 332]));
    assert_eq!(split_shares(10, 4), Ok(vec![3, 3, 3, 1]));
    assert_eq!(split_shares(7, 2), Ok(vec![4, 3]));
    for people in 2..=20u8 {
        let shares = split_shares(12_345, people).unwrap();
        assert_eq!(shares.len(), usize::from(people));
        assert_eq!(shares.iter().sum::<u64>(), 12_345);
        assert!(shares.windows(2).all(|p| p[0] >= p[1]));
    }
    for people in [0u8, 1, 21, 255] {
        assert_eq!(split_shares(1_000, people), Err(SplitError::People));
    }
    // Nothing for the last person is refused.
    assert_eq!(split_shares(10, 6), Err(SplitError::TooLittle));
    assert_eq!(split_shares(0, 2), Err(SplitError::TooLittle));
    // It splits what is left, and opens nothing.
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 1_000);
    w.c.add_other_part(n, Amount(100), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    w.m.reset();
    assert_eq!(w.c.bill_split::<&str>(n, 3).unwrap(), [300, 300, 300]);
    assert_eq!(w.m.writes, 0);
    assert_eq!(
        w.c.bill_split::<&str>(n, 1).unwrap_err(),
        BillRefused::BadSplit(SplitError::People)
    );
    assert_eq!(
        w.c.bill_split::<&str>(9, 2).unwrap_err(),
        BillRefused::UnknownBill(9)
    );
}

// CR-251.

#[test]
fn cr_251_a_tab_raised_twice_stays_open_until_it_is_finished_and_paid() {
    let mut w = World::new();
    let t = w.bill(BillKind::Tab, 2_000);
    w.c.set_tab_amount(t, Amount(3_500), T0 + 10, &mut w.m)
        .unwrap();
    w.c.set_tab_amount(t, Amount(5_000), T0 + 20, &mut w.m)
        .unwrap();
    assert_eq!(w.view(t).bill.amount(), Amount(5_000));
    let amounts: Vec<u64> =
        w.m.events()
            .iter()
            .filter_map(|e| match e {
                Event::BillAmount { amount, .. } => Some(amount.0),
                _ => None,
            })
            .collect();
    assert_eq!(amounts, [3_500, 5_000]);
    // Receiving the whole amount does not settle a tab.
    let o = w.part(t, 5_000).unwrap();
    assert_eq!(w.pay_and_confirm(&o, 100), State::Confirmed);
    let v = w.view(t);
    assert_eq!((v.received(), v.left), (5_000, 0));
    assert!(v.bill.is_open(), "a tab is finished by the cashier");
    // It is lowered to no less than what it has received.
    assert_eq!(
        w.c.set_tab_amount(t, Amount(4_999), T0, &mut w.m)
            .unwrap_err(),
        BillRefused::BelowReceived { received: 5_000 }
    );
    w.c.set_tab_amount(t, Amount(5_000), T0, &mut w.m).unwrap();
    w.c.finish_tab(t, T0 + 30, &mut w.m).unwrap();
    assert_eq!(ended_how(&w, t), Some(BillEnd::Settled));
    assert_eq!(
        w.c.set_tab_amount(t, Amount(6_000), T0, &mut w.m)
            .unwrap_err(),
        BillRefused::NotOpen
    );
}

#[test]
fn cr_251_a_tab_is_finished_only_when_it_has_received_its_amount() {
    let mut w = World::new();
    let t = w.bill(BillKind::Tab, 2_000);
    w.c.add_other_part(t, Amount(500), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    let err = w.c.finish_tab(t, T0, &mut w.m).unwrap_err();
    assert_eq!(err, BillRefused::NotReached { left: 1_500 });
    assert_eq!(err.rule(), "CR-251");
    assert!(w.view(t).bill.is_open());
    // A bill's amount does not change, and only a tab is finished by hand.
    let b = w.bill(BillKind::Bill, 2_000);
    assert_eq!(
        w.c.set_tab_amount(b, Amount(3_000), T0, &mut w.m)
            .unwrap_err(),
        BillRefused::NotATab
    );
    assert_eq!(
        w.c.finish_tab(b, T0, &mut w.m).unwrap_err(),
        BillRefused::NotAFinishableBill
    );
    // A tab can be raised again after parts arrived.
    w.c.set_tab_amount(t, Amount(2_500), T0, &mut w.m).unwrap();
    assert_eq!(w.view(t).left, 2_000);
}

// CR-252.

#[test]
fn cr_252_close_short_needs_the_manager_and_writes_what_was_not_paid() {
    let mut w = World::new();
    let n = w.bill(BillKind::Bill, 1_000);
    w.c.add_other_part(n, Amount(300), PartWay::Card, T0, &mut w.m)
        .unwrap();
    let cashier = Actor {
        role: Role::Cashier,
        name: "Ann".to_string(),
    };
    w.m.reset();
    let err = w.c.close_short(n, &cashier, T0, &mut w.m).unwrap_err();
    assert_eq!(err, BillRefused::NeedsManager);
    assert_eq!(err.rule(), "CR-252");
    assert_eq!(w.m.writes, 0);
    assert!(w.view(n).bill.is_open());
    assert_eq!(
        w.c.close_short(n, &manager(), T0 + 5, &mut w.m).unwrap(),
        Amount(700)
    );
    let e = w.view(n).bill.ended().unwrap();
    assert_eq!(e.how, BillEnd::ClosedShort);
    assert_eq!(e.unpaid, Amount(700));
    assert!(matches!(
        w.m.events().last(),
        Some(Event::BillEnded {
            how: BillEnd::ClosedShort,
            unpaid: Amount(700),
            ..
        })
    ));
    assert_eq!(w.m.reload().bill_view(n).unwrap().bill.ended(), Some(e));
    // A bill that has received its amount is not closed short.
    let t = w.bill(BillKind::Tab, 100);
    w.c.add_other_part(t, Amount(100), PartWay::Other, T0, &mut w.m)
        .unwrap();
    assert_eq!(
        w.c.close_short(t, &manager(), T0, &mut w.m).unwrap_err(),
        BillRefused::NothingShort
    );
}

#[test]
fn cr_252_cancel_is_refused_once_a_part_counts_or_is_pending() {
    let mut w = World::new();
    // With nothing at all, it cancels.
    let a = w.bill(BillKind::Bill, 1_000);
    w.c.cancel_bill(a, T0, &mut w.m).unwrap();
    assert_eq!(ended_how(&w, a), Some(BillEnd::Cancelled));
    // A part paid another way counts.
    let b = w.bill(BillKind::Bill, 1_000);
    w.c.add_other_part(b, Amount(100), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    let err = w.c.cancel_bill(b, T0, &mut w.m).unwrap_err();
    assert_eq!(err, BillRefused::HasPart);
    assert_eq!(err.rule(), "CR-252");
    // A part that is pending blocks it.
    let c = w.bill(BillKind::Bill, 1_000);
    let o = w.part(c, 500).unwrap();
    assert_eq!(
        w.c.cancel_bill(c, T0, &mut w.m).unwrap_err(),
        BillRefused::HasPart
    );
    // A bitcoin part that counts blocks it.
    assert_eq!(w.pay_and_confirm(&o, 100), State::Confirmed);
    assert_eq!(
        w.c.cancel_bill(c, T0, &mut w.m).unwrap_err(),
        BillRefused::HasPart
    );
    // A part whose sale ended with nothing received does not.
    let d = w.bill(BillKind::Bill, 1_000);
    let p = w.part(d, 500).unwrap();
    w.c.cancel(p.index, T0, &mut w.m).unwrap();
    w.c.cancel_bill(d, T0, &mut w.m).unwrap();
    assert_eq!(ended_how(&w, d), Some(BillEnd::Cancelled));
}

// CR-253.

#[test]
fn cr_253_the_bills_file_has_magic_obm_bracket_and_reads_back_whole() {
    let mut w = World::new();
    let a = w.bill(BillKind::Bill, 1_000);
    let t = w.bill(BillKind::Tab, 500);
    w.c.add_bill_reference(a, "INV-1", T0, &mut w.m).unwrap();
    w.c.add_other_part(a, Amount(100), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    let o = w.part(a, 200).unwrap();
    w.c.cancel_bill(t, T0, &mut w.m).unwrap();
    let bytes = w.m.slots.get(&crate::Slot::Bills).unwrap().clone();
    assert_eq!(&bytes[..4], b"OBM[");
    assert_eq!(&bytes[..4], &MAGIC_BILLS);
    assert_eq!(bytes[4], 1);
    assert_eq!(&bytes[5..9], &3u32.to_le_bytes(), "the next number");
    let bills = Bills::from_bytes(&bytes).unwrap();
    assert_eq!(bills.to_bytes(), bytes);
    let got = bills.get(a).unwrap();
    assert_eq!(got.reference(), Some("INV-1"));
    assert_eq!(
        got.parts(),
        [
            BillPart::Other {
                amount: Amount(100),
                way: PartWay::Cash
            },
            BillPart::Sale(o.index)
        ]
    );
    assert!(!bills.get(t).unwrap().is_open());
    assert_eq!(w.m.reload().bill_views().len(), 2);
}

#[test]
fn cr_253_the_read_is_strict_and_bounded() {
    let mut w = World::new();
    let a = w.bill(BillKind::Bill, 1_000);
    w.c.add_other_part(a, Amount(100), PartWay::Cash, T0, &mut w.m)
        .unwrap();
    let ok = w.m.slots.get(&crate::Slot::Bills).unwrap().clone();
    assert!(Bills::from_bytes(&ok).is_ok());
    let mut long = ok.clone();
    long.push(0);
    assert_eq!(Bills::from_bytes(&long), Err(DecodeError::TrailingBytes));
    for cut in 0..ok.len() {
        assert!(Bills::from_bytes(&ok[..cut]).is_err(), "cut at {cut}");
    }
    let mut bad = ok.clone();
    bad[0] = b'X';
    assert_eq!(Bills::from_bytes(&bad), Err(DecodeError::BadMagic));
    let mut bad = ok.clone();
    bad[4] = 2;
    assert!(Bills::from_bytes(&bad).is_err());
    // The next number is above every number and not 0.
    let mut bad = ok.clone();
    bad[5..9].copy_from_slice(&1u32.to_le_bytes());
    assert!(Bills::from_bytes(&bad).is_err());
    bad[5..9].copy_from_slice(&0u32.to_le_bytes());
    assert!(Bills::from_bytes(&bad).is_err());
    // A count past the bytes, and past the limits.
    let mut bad = ok.clone();
    bad[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Bills::from_bytes(&bad).is_err());
    // An amount of nothing, a kind that is not known, a part of nothing.
    let mut bad = ok.clone();
    bad[13 + 5..13 + 13].fill(0);
    assert!(Bills::from_bytes(&bad).is_err());
    let mut bad = ok.clone();
    bad[13 + 4] = 0;
    assert!(Bills::from_bytes(&bad).is_err());
    // A bill that does not read back is a counter that does not start.
    let mut stored = w.m.stored();
    stored.bills = Some(&bad);
    let err = crate::Counter::load(id(3), pairing(), settings(), stored, &w.m).unwrap_err();
    assert!(matches!(err, crate::LoadError::Bills(_)));
}

#[test]
fn cr_253_the_read_refuses_a_sale_in_two_parts_and_a_misplaced_unpaid() {
    let mut w = World::new();
    let a = w.bill(BillKind::Bill, 1_000);
    let o = w.part(a, 100).unwrap();
    let b = w.bill(BillKind::Bill, 1_000);
    let mut bills = w.m.reload();
    let _ = &mut bills;
    let mut file = Bills::from_bytes(w.m.slots.get(&crate::Slot::Bills).unwrap()).unwrap();
    file.bills
        .get_mut(&b)
        .unwrap()
        .parts
        .push(BillPart::Sale(o.index));
    assert!(Bills::from_bytes(&file.to_bytes()).is_err());
    let mut file = Bills::from_bytes(w.m.slots.get(&crate::Slot::Bills).unwrap()).unwrap();
    file.bills.get_mut(&b).unwrap().ended = Some(crate::Ended {
        how: BillEnd::Settled,
        at: 1,
        unpaid: Amount(5),
    });
    assert!(Bills::from_bytes(&file.to_bytes()).is_err());
}

#[test]
fn cr_253_at_most_256_bills_are_open_and_a_bill_has_at_most_32_parts() {
    let mut w = World::new();
    assert_eq!((MAX_OPEN_BILLS, MAX_BILL_PARTS), (256, 32));
    for _ in 0..MAX_OPEN_BILLS {
        w.bill(BillKind::Tab, 1_000);
    }
    w.m.reset();
    let err =
        w.c.open_bill(BillKind::Bill, Amount(1), None, T0, &mut w.m)
            .unwrap_err();
    assert_eq!(err, BillRefused::TooManyOpen);
    assert_eq!(err.rule(), "CR-253");
    assert_eq!(w.m.writes, 0, "no number is used");
    assert_eq!(w.c.next_bill_number(), 257);
    // The file of 256 reads back; one that ended makes room.
    assert_eq!(w.m.reload().bill_views().len(), 256);
    w.c.cancel_bill(1, T0, &mut w.m).unwrap();
    w.c.cancel_bill(2, T0, &mut w.m).unwrap();
    assert_eq!(w.bill(BillKind::Bill, 5), 257);
    // 32 parts.
    let n = w.bill(BillKind::Tab, 1_000_000);
    for i in 0..MAX_BILL_PARTS {
        w.c.add_other_part(n, Amount(1), PartWay::Cash, T0, &mut w.m)
            .unwrap_or_else(|e| panic!("part {i}: {e}"));
    }
    w.m.reset();
    let err =
        w.c.add_other_part(n, Amount(1), PartWay::Cash, T0, &mut w.m)
            .unwrap_err();
    assert_eq!(err, BillRefused::TooManyParts);
    let sales = w.c.held_sales();
    assert_eq!(
        w.part(n, 100).unwrap_err(),
        PartRefused::Bill(BillRefused::TooManyParts)
    );
    assert_eq!(w.c.held_sales(), sales, "no sale is opened for a full bill");
    let o = w.open_five();
    w.m.reset();
    assert_eq!(
        w.c.join_sale(n, o.index, T0, &mut w.m).unwrap_err(),
        BillRefused::TooManyParts
    );
    assert_eq!(w.m.writes, 0);
    // A file with 33 parts is not read.
    let mut file = Bills::from_bytes(w.m.slots.get(&crate::Slot::Bills).unwrap()).unwrap();
    file.bills.get_mut(&n).unwrap().parts.push(BillPart::Other {
        amount: Amount(1),
        way: PartWay::Cash,
    });
    assert!(Bills::from_bytes(&file.to_bytes()).is_err());
}

#[test]
fn cr_253_a_bill_that_ended_stays_seven_days() {
    let mut w = World::new();
    let a = w.bill(BillKind::Bill, 1_000);
    w.c.cancel_bill(a, T0, &mut w.m).unwrap();
    let b = w.bill(BillKind::Bill, 1_000);
    // Six days later it is still there, and a write after seven drops it.
    w.c.add_other_part(b, Amount(1), PartWay::Cash, T0 + 6 * DAY, &mut w.m)
        .unwrap();
    assert!(w.c.bill_view(a).is_some());
    w.c.add_other_part(b, Amount(1), PartWay::Cash, T0 + 7 * DAY, &mut w.m)
        .unwrap();
    assert!(w.c.bill_view(a).is_none());
    assert!(w.c.bill_view(b).is_some());
    assert_eq!(w.c.next_bill_number(), 3, "a number is not reused");
}

#[test]
fn cr_253_a_sale_that_is_a_part_of_an_open_bill_is_not_settled_away() {
    let mut w = World::new();
    let n = w.bill(BillKind::Tab, 1_000);
    let o = w.part(n, 100).unwrap();
    assert_eq!(w.pay_and_confirm(&o, 100), State::Confirmed);
    w.c.set_owner_mark(w.c.records_len());
    w.tick(31 * DAY);
    let done = w.c.settle(&mut w.m, w.now).unwrap();
    assert_eq!(done.count, 0);
    assert_eq!(w.view(n).received(), 100);
}

// CR-255.

#[test]
fn cr_255_every_state_and_refusal_has_a_placeholder_until_the_words_are_written() {
    let all = all_words();
    let placeholders: Vec<_> = all
        .iter()
        .filter(|(name, _)| name.starts_with("BillRefused::") || name.starts_with("BillView::"))
        .collect();
    assert_eq!(placeholders.len(), 26);
    for (name, w) in placeholders {
        assert!(is_placeholder(w), "{name}");
        assert!(w.heading.starts_with("WORDS_NEEDED_CR_2"), "{name}");
        assert!(w.heading.chars().count() < 28, "{name}");
        assert!(w.what.ends_with('.') && w.next.ends_with('.'), "{name}");
    }
    let (_, w) = all
        .iter()
        .find(|(n, _)| n == "OpenRefused::BadReference")
        .unwrap();
    assert_eq!(w.heading, "WORDS_NEEDED_CR_245_BAD");
    let (_, w) = all.iter().find(|(n, _)| n == "LoadError::Bills").unwrap();
    assert_eq!(w.heading, "WORDS_NEEDED_CR_253_UNREAD");
}
