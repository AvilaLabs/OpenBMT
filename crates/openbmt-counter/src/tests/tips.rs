//! Section 18: tips at the counter (CR-187 to CR-192).

use openbmt_core::records::RecordKind;
use openbmt_core::say::check;
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::refund::{RefundAsk, RefundRequest};
use crate::sale::LightningPaid;
use crate::say::{
    tip_button_label, tip_choose_words, tips_person_label, TIPS_LINE_LABEL, TIP_NONE_LABEL,
    TIP_OTHER_LABEL,
};
use crate::{
    tip_for_percent, Actor, Amount, Event, OpenRefused, Quote, Report, Role, Sale, State, TipChoice,
};

fn ann() -> Actor {
    Actor {
        role: Role::Cashier,
        name: "ann".to_string(),
    }
}

fn bob() -> Actor {
    Actor {
        role: Role::Cashier,
        name: "bob".to_string(),
    }
}

fn open_tipped(
    w: &mut World,
    cents: u64,
    tip: u64,
    by: Option<&Actor>,
) -> Result<crate::Opened, OpenRefused<&'static str>> {
    let facts = w.open_facts();
    w.c.open_with_tip(
        Amount(cents),
        Amount(tip),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        by,
        &mut w.m,
    )
}

fn tips_on(w: &mut World, percents: &[u8]) {
    let mut s = settings();
    s.tips_on = true;
    s.tip_percents = percents.to_vec();
    w.c.set_settings(s);
}

fn choice(percent: u8, tip: u64) -> TipChoice {
    TipChoice {
        percent,
        tip: Amount(tip),
    }
}

// CR-187.

#[test]
fn cr_187_the_counter_offers_the_percents_of_the_settings_only_when_tips_are_on() {
    let mut w = World::new();
    assert!(w.c.tip_choices(Amount(1_000)).is_empty());
    tips_on(&mut w, &[10, 15, 20]);
    assert_eq!(
        w.c.tip_choices(Amount(1_000)),
        vec![choice(10, 100), choice(15, 150), choice(20, 200)]
    );
    // The order of the settings is kept, and fewer than three are offered.
    tips_on(&mut w, &[25, 5]);
    assert_eq!(
        w.c.tip_choices(Amount(1_000)),
        vec![choice(25, 250), choice(5, 50)]
    );
    tips_on(&mut w, &[]);
    assert!(w.c.tip_choices(Amount(1_000)).is_empty());
    // Off, the percents are still in the settings and none is offered.
    let mut s = settings();
    s.tip_percents = vec![10];
    w.c.set_settings(s);
    assert!(w.c.tip_choices(Amount(1_000)).is_empty());
}

// CR-188.

#[test]
fn cr_188_a_percent_is_rounded_half_up_to_a_whole_minor_unit() {
    let tip = |amount, percent| tip_for_percent(Amount(amount), percent).unwrap().0;
    assert_eq!(tip(1_000, 15), 150);
    // 12.50 at 15% is 1.875: up to 1.88.
    assert_eq!(tip(1_250, 15), 188);
    // Exactly half goes up, a hair under stays down.
    assert_eq!(tip(5, 10), 1);
    assert_eq!(tip(4, 10), 0);
    assert_eq!(tip(1_050, 5), 53);
    assert_eq!(tip(1_049, 5), 52);
    assert_eq!(tip(1, 50), 1);
    assert_eq!(tip(0, 20), 0);
    // The largest amount does not overflow.
    assert!(tip_for_percent(Amount(u64::MAX), 50).is_some());
}

#[test]
fn cr_188_the_screen_and_the_buttons_use_the_given_words() {
    let words = tip_choose_words("$12.50");
    assert_eq!(check(&words), Ok(()));
    assert_eq!(words.heading, "Add a tip?");
    assert_eq!(
        words.what,
        "The sale is $12.50. A tip goes to the shop with it."
    );
    assert_eq!(words.next, "Choose a tip, or No tip.");
    assert_eq!(words.who, openbmt_core::say::Who::Customer);
    assert_eq!(tip_button_label(15, "$1.88"), "15% \u{b7} $1.88");
    assert_eq!(TIP_OTHER_LABEL, "Other amount");
    assert_eq!(TIP_NONE_LABEL, "No tip");
}

// CR-189.

#[test]
fn cr_189_the_code_is_for_the_amount_and_the_tip_together() {
    let mut w = World::new();
    tips_on(&mut w, &[15]);
    let o = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    // $5.75 at 1,000 sats a dollar.
    assert_eq!(o.needs_sats, 5_750);
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.amount(), Amount(575));
    assert_eq!(sale.tip(), Amount(75));
    assert_eq!(sale.own_amount(), Amount(500));
    assert_eq!(sale.needs_sats(), 5_750);
    // Short of the total by more than the tolerance is still not paid.
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    assert_eq!(w.judge(o.index).state, State::PartPaid);
    // A requote prices the total too: the $5.00 that arrived is valued at
    // the old rate and the $0.75 still missing is priced at half the price.
    let quote = w.requote(o.index, 5_000_000).unwrap();
    assert_eq!(quote.rate.minor_per_btc(), 5_000_000);
    assert_eq!(w.c.sale(o.index).unwrap().needs_sats(), 5_000 + 1_500);
}

#[test]
fn cr_189_the_instant_limit_is_judged_on_the_total() {
    // The light check takes $10 at most as paid at once. $9.00 does; $9.00
    // with $2.00 of tip is $11.00 and waits.
    let mut w = World::new();
    let plain = open_tipped(&mut w, 900, 0, None).unwrap();
    let t = w.pay(&plain, 9_000);
    assert_eq!(w.accept_at_once(&plain, &t).state, State::PaidInstant);

    let mut w = World::new();
    let tipped = open_tipped(&mut w, 900, 200, None).unwrap();
    assert_eq!(tipped.needs_sats, 11_000);
    let t = w.pay(&tipped, 11_000);
    w.tick(10);
    w.see(&t, 3);
    w.judge(tipped.index);
    w.tick(8);
    let screen = w.judge(tipped.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(w.c.exposure_outstanding(), 0);
}

#[test]
fn cr_189_the_smallest_sale_and_the_dust_line_are_judged_on_the_total() {
    let mut w = World::new();
    let mut s = settings();
    s.min_sale_sats = 26_000;
    w.c.set_settings(s);
    // $20.00 alone is 20,000 sats, under the floor; with $6.00 it is 26,000.
    assert!(matches!(
        open_tipped(&mut w, 2_000, 0, None),
        Err(OpenRefused::BelowMinimum { .. })
    ));
    assert!(open_tipped(&mut w, 2_000, 600, None).is_ok());
}

#[test]
fn cr_189_the_record_holds_the_amount_and_the_tip_as_two_fields() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let events = w.m.events();
    let Event::Opened {
        amount,
        tip,
        needs_sats,
        by,
        ..
    } = &events[0]
    else {
        panic!("{:?}", events[0]);
    };
    assert_eq!(
        (*amount, *tip, *needs_sats),
        (Amount(500), Amount(75), 5_750)
    );
    assert_eq!(by, &Some((Role::Cashier, "ann".to_string())));
    assert_eq!(events[0].kind(), RecordKind::Sale);
    assert_eq!(events[0].index(), o.index);
    // Tag 20, and it reads back to the same event.
    let bytes = events[0].encode();
    assert_eq!(bytes[0], 20);
    assert_eq!(Event::decode(&bytes).unwrap(), events[0]);
}

#[test]
fn cr_189_a_sale_without_a_tip_is_written_as_before_and_old_records_read_as_a_tip_of_nothing() {
    let mut w = World::new();
    open_tipped(&mut w, 500, 0, Some(&ann())).unwrap();
    let event = w.m.events().remove(0);
    let bytes = event.encode();
    assert_eq!(bytes[0], 1);
    let Event::Opened { tip, .. } = Event::decode(&bytes).unwrap() else {
        panic!();
    };
    assert_eq!(tip, Amount(0));
    // The same bytes with a tip, put in as a version of the record would.
    let tagged = open_tipped(&mut w, 500, 75, None).unwrap();
    let with = w.m.events().remove(1);
    assert_eq!(with.index(), tagged.index);
    let mut zero = with.encode();
    // Cut the tip out and the tag back to 1: the record of an older build.
    let tip_at = zero.len() - 8;
    zero.drain(tip_at..);
    zero[0] = 1;
    let Event::Opened { tip, amount, .. } = Event::decode(&zero).unwrap() else {
        panic!();
    };
    assert_eq!((tip, amount), (Amount(0), Amount(500)));
}

#[test]
fn cr_189_a_record_of_a_sale_with_a_tip_of_nothing_or_cut_short_is_refused() {
    let mut w = World::new();
    open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let event = w.m.events().remove(0);
    let bytes = event.encode();
    // The tip sits before the person: role (1), name length (1), "ann" (3).
    let tip_at = bytes.len() - (1 + 1 + 3) - 8;
    let mut zero = bytes.clone();
    zero[tip_at..tip_at + 8].fill(0);
    assert!(matches!(
        Event::decode(&zero),
        Err(DecodeError::NonCanonical(_))
    ));
    // Cut anywhere inside the tip and it is a short record, or one whose
    // person is not a person.
    for cut in 1..=8 {
        let mut short = bytes[..tip_at + 8 - cut].to_vec();
        short.truncate(tip_at + 8 - cut);
        assert!(Event::decode(&short).is_err(), "{cut}");
    }
    // A byte left over is refused.
    let mut long = bytes.clone();
    long.push(0);
    assert!(Event::decode(&long).is_err());
}

#[test]
fn cr_189_the_stored_sale_reads_back_with_its_tip() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let stored = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(stored[4], 4);
    let back = Sale::from_bytes(&stored).unwrap();
    assert_eq!(back.tip(), Amount(75));
    assert_eq!(back.amount(), Amount(575));
    assert_eq!(back.to_bytes(), stored);
    // A restart finds the same sale.
    let reloaded = w.m.reload();
    assert_eq!(reloaded.sale(o.index).unwrap().tip(), Amount(75));
    assert_eq!(reloaded.sale(o.index).unwrap().needs_sats(), 5_750);
    // A sale without a tip is version 1, as every earlier build wrote it.
    let plain = open_tipped(&mut w, 500, 0, None).unwrap();
    assert_eq!(w.c.sale(plain.index).unwrap().to_bytes()[4], 1);
}

#[test]
fn cr_189_a_stored_sale_with_a_tip_of_nothing_or_above_its_own_amount_is_refused() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 75, None).unwrap();
    let stored = w.c.sale(o.index).unwrap().to_bytes();
    // The tip is the eight bytes after the watch; find them from the end:
    // lightning flag (1) and refund count (4) follow it.
    let tip_at = stored.len() - 8 - 1 - 4;
    assert_eq!(&stored[tip_at..tip_at + 8], &75u64.to_le_bytes());
    for bad in [0u64, 300] {
        let mut edit = stored.clone();
        edit[tip_at..tip_at + 8].copy_from_slice(&bad.to_le_bytes());
        assert!(Sale::from_bytes(&edit).is_err(), "{bad}");
    }
    // The largest tip there may be is the sale's own amount.
    let mut half = stored.clone();
    half[tip_at..tip_at + 8].copy_from_slice(&287u64.to_le_bytes());
    assert!(Sale::from_bytes(&half).is_ok());
    // Version 4 with a lightning payment and refunds keeps both.
    let mut sale = Sale::new(
        7,
        p2wsh(9),
        T0,
        Amount(575),
        Amount(75),
        usd(),
        Quote::new(rate(), T0),
        5_750,
        50,
    );
    sale.lightning = Some(LightningPaid {
        payment_hash: [1; 32],
        amount_msat: 5_750_000,
        settled_at: None,
    });
    sale.books_final = true;
    let bytes = sale.to_bytes();
    assert_eq!(bytes[4], 4);
    let back = Sale::from_bytes(&bytes).unwrap();
    assert_eq!(back.lightning, sale.lightning);
    assert_eq!(back.tip(), Amount(75));
    assert_eq!(back.to_bytes(), bytes);
}

#[test]
fn cr_189_the_books_report_gives_the_sales_own_amount() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let t = w.pay(&o, 5_750);
    w.confirm(&[t], 95);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Final);
    assert!(screen.reports.iter().any(|r| matches!(
        r,
        Report::BooksFinal { amount, received_sats: 5_750, .. } if *amount == Amount(500)
    )));
}

// CR-190.

#[test]
fn cr_190_a_tip_above_the_sale_is_refused_and_nothing_is_written() {
    let mut w = World::new();
    w.m.reset();
    let err = open_tipped(&mut w, 500, 501, Some(&ann())).unwrap_err();
    assert_eq!(err, OpenRefused::TipTooLarge);
    assert_eq!(err.rule(), "CR-190");
    assert!(err.to_string().starts_with("CR-190: "));
    assert!(w.m.log.is_empty(), "{:?}", w.m.log);
    assert!(w.m.records.is_empty());
    // A tip as large as the sale is taken, and so is no tip.
    assert!(open_tipped(&mut w, 500, 500, None).is_ok());
    assert!(open_tipped(&mut w, 500, 0, None).is_ok());
    let words = err.words();
    assert_eq!(check(&words), Ok(()));
    assert_eq!(words.heading, "Tip not accepted");
    assert_eq!(
        words.what,
        "The tip is more than the sale itself, so it was not added."
    );
    assert_eq!(words.next, "Type a smaller tip, or choose No tip.");
    assert_eq!(words.who, openbmt_core::say::Who::Cashier);
}

// CR-191.

#[test]
fn cr_191_today_has_the_tips_of_paid_sales_in_all_and_for_each_person() {
    let mut w = World::new();
    let a = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let b = open_tipped(&mut w, 1_000, 200, Some(&bob())).unwrap();
    let c = open_tipped(&mut w, 500, 50, Some(&ann())).unwrap();
    let d = open_tipped(&mut w, 500, 0, Some(&ann())).unwrap();
    // a and b are paid in full; c waits; d is paid and has no tip.
    for (o, sats) in [(&a, 5_750), (&b, 12_000), (&d, 5_000)] {
        let t = w.pay(o, sats);
        w.confirm(&[t], 95);
        assert_eq!(w.judge(o.index).state, State::Final);
    }
    let day = w.c.day_totals(T0 - 10, T0 + 1_000, true, &w.m).unwrap();
    assert_eq!(day.paid.count, 3);
    assert_eq!(day.tips.count, 2);
    assert_eq!(day.tips.minor, 275);
    // The tallies hold the sales' own amounts, without the tips.
    assert_eq!(day.paid.minor, 500 + 1_000 + 500);
    assert_eq!(day.waiting.count, 1);
    let tips_of = |name: &str| {
        day.people
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.tips_minor)
    };
    assert_eq!(tips_of("ann"), Some(75));
    assert_eq!(tips_of("bob"), Some(200));
    // The tip of the waiting sale is in no line.
    let _ = c;
    // Without the lines per person the people are left out, the total stays.
    let all = w.c.day_totals(T0 - 10, T0 + 1_000, false, &w.m).unwrap();
    assert!(all.people.is_empty());
    assert_eq!(all.tips.minor, 275);
    assert_eq!(TIPS_LINE_LABEL, "Tips");
    assert_eq!(tips_person_label("ann"), "Tips taken by ann");
}

#[test]
fn cr_191_a_sale_whose_payment_was_replaced_adds_no_tip() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let t = w.pay(&o, 5_750);
    let block = w.confirm(std::slice::from_ref(&t), 95);
    assert_eq!(w.judge(o.index).state, State::Final);
    let day = w.c.day_totals(T0 - 10, T0 + 1_000, true, &w.m).unwrap();
    assert_eq!(day.tips.minor, 75);
    w.orphan(&block);
    w.tick(5);
    let after = w.judge(o.index);
    assert_ne!(after.state, State::Final);
    let day = w.c.day_totals(T0 - 10, T0 + 1_000, true, &w.m).unwrap();
    assert_eq!(day.tips.minor, 0);
    assert_eq!(day.people[0].tips_minor, 0);
}

// CR-192.

fn request(sats: u64) -> RefundRequest {
    RefundRequest {
        ask: RefundAsk::Sats(sats),
        destination: p2wsh(0xa1),
        actor: ann(),
        reason: "wrong size".to_string(),
    }
}

#[test]
fn cr_192_a_refund_says_how_much_of_it_is_tip() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 75, Some(&ann())).unwrap();
    let t = w.pay(&o, 5_750);
    w.confirm(&[t], 95);
    assert_eq!(w.judge(o.index).state, State::Final);
    // The sale and its tip may be refunded: more than the sale's own $5.00
    // is allowed, up to the $5.75 received.
    let ask = |w: &mut World, sats| {
        w.c.request_refund(o.index, request(sats), w.now, &w.chain, &mut w.m)
    };
    ask(&mut w, 4_000).unwrap();
    ask(&mut w, 1_420).unwrap();
    let views = w.c.refund_views(o.index);
    // $4.00 is all sale; the next $1.42 is $1.00 of sale and 42 cents of tip.
    assert_eq!(views[0].tip_minor, 0);
    assert_eq!(views[1].tip_minor, 42);
    // Past what was received is refused as before.
    assert!(ask(&mut w, 331).is_err());
    ask(&mut w, 330).unwrap();
    let views = w.c.refund_views(o.index);
    assert_eq!(views[2].tip_minor, 33);
    // A withdrawn request asks for nothing and says 0.
    w.c.withdraw_refund(o.index, 3, ann(), w.now, &mut w.m)
        .unwrap();
    assert_eq!(w.c.refund_views(o.index)[2].tip_minor, 0);
}

#[test]
fn cr_192_a_sale_without_a_tip_says_no_tip_in_its_refunds() {
    let mut w = World::new();
    let o = open_tipped(&mut w, 500, 0, Some(&ann())).unwrap();
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 95);
    w.judge(o.index);
    w.c.request_refund(o.index, request(5_000), w.now, &w.chain, &mut w.m)
        .unwrap();
    assert!(w.c.refund_views(o.index).iter().all(|v| v.tip_minor == 0));
}
