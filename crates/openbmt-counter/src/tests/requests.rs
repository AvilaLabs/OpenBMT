//! Section 21: requests to send and a note on a sale (CR-205 to CR-211).

use openbmt_core::records::RecordKind;
use openbmt_core::say::{check, Who};
use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::{
    Amount, End, Event, NoteRefused, OpenRefused, Opened, RequestDays, RequoteRefused, Sale,
    SaleExtras, State, EXPIRY_SECS, QUOTE_SECS,
};

const DAY: u64 = 86_400;

fn open_with(
    w: &mut World,
    cents: u64,
    extras: SaleExtras<'_>,
) -> Result<Opened, OpenRefused<&'static str>> {
    let facts = w.open_facts();
    w.c.open_with_extras(
        Amount(cents),
        Amount(0),
        usd(),
        rate(),
        &fresh(facts.now),
        facts,
        None,
        extras,
        &mut w.m,
    )
}

fn request(w: &mut World, cents: u64, days: RequestDays) -> Opened {
    open_with(
        w,
        cents,
        SaleExtras {
            note: None,
            request: Some(days),
        },
    )
    .unwrap()
}

fn noted(w: &mut World, cents: u64, note: &str) -> Opened {
    open_with(
        w,
        cents,
        SaleExtras {
            note: Some(note),
            request: None,
        },
    )
    .unwrap()
}

fn all_days() -> [(RequestDays, u64); 3] {
    [
        (RequestDays::One, 1),
        (RequestDays::Three, 3),
        (RequestDays::Seven, 7),
    ]
}

// CR-205.

#[test]
fn cr_205_a_request_lasts_one_three_or_seven_days() {
    for (days, n) in all_days() {
        assert_eq!(days.days(), n as u8);
        assert_eq!(days.secs(), n * DAY);
        assert_eq!(RequestDays::from_days(n as u8), Some(days));
    }
    for other in [0u8, 2, 4, 5, 6, 8, 14, 255] {
        assert_eq!(RequestDays::from_days(other), None, "{other}");
    }
}

#[test]
fn cr_205_the_one_quote_of_a_request_holds_for_its_whole_lifetime() {
    for (days, n) in all_days() {
        let mut w = World::new();
        let o = request(&mut w, 500, days);
        assert_eq!(o.needs_sats, 5_000);
        assert_eq!(o.quote.from, T0);
        assert_eq!(o.quote.until, T0 + n * DAY);
        assert!(o.quote.holds(T0 + n * DAY - 1));
        assert!(!o.quote.holds(T0 + n * DAY));
        let sale = w.c.sale(o.index).unwrap();
        assert_eq!(sale.request(), Some(days));
        assert_eq!(sale.quotes().len(), 1);
        assert_eq!(sale.expires_at(), T0 + n * DAY);
    }
    // A sale shown at the till keeps the five-minute quote.
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(o.quote.until, T0 + QUOTE_SECS);
    assert_eq!(w.c.sale(o.index).unwrap().request(), None);
}

#[test]
fn cr_205_the_satoshis_are_fixed_when_the_request_is_made() {
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::Seven);
    w.tick(5 * DAY);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::NotPaidYet);
    assert_eq!(screen.needs_sats, 5_000);
    assert_eq!(w.c.sale(o.index).unwrap().needs_sats(), 5_000);
    // A payment of what was asked is enough, whatever the price did.
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Confirmed);
    assert_eq!(screen.needs_sats, 5_000);
}

// CR-206.

#[test]
fn cr_206_a_sale_takes_a_note_when_it_opens() {
    let mut w = World::new();
    let o = noted(&mut w, 500, "Tour 12 May");
    assert_eq!(w.c.sale(o.index).unwrap().note(), Some("Tour 12 May"));
    let events = w.m.events();
    assert!(matches!(events[0], Event::Opened { .. }));
    assert_eq!(
        events[1],
        Event::Noted {
            index: o.index,
            at: T0,
            note: "Tour 12 May".to_string()
        }
    );
    assert_eq!(events[1].kind(), RecordKind::Sale);
    // A restart finds the note.
    let reloaded = w.m.reload();
    assert_eq!(reloaded.sale(o.index).unwrap().note(), Some("Tour 12 May"));
}

#[test]
fn cr_206_a_note_is_added_once_later() {
    let mut w = World::new();
    let o = w.open_five();
    assert_eq!(w.c.sale(o.index).unwrap().note(), None);
    w.tick(60);
    w.c.add_note(o.index, "Deposit for Ana", w.now, &mut w.m)
        .unwrap();
    assert_eq!(w.c.sale(o.index).unwrap().note(), Some("Deposit for Ana"));
    let last = w.m.events().pop().unwrap();
    assert_eq!(
        last,
        Event::Noted {
            index: o.index,
            at: T0 + 60,
            note: "Deposit for Ana".to_string()
        }
    );
    let err =
        w.c.add_note(o.index, "Another", w.now, &mut w.m)
            .unwrap_err();
    assert_eq!(err, NoteRefused::AlreadyNoted);
    assert_eq!(err.rule(), "CR-206");
    assert_eq!(w.c.sale(o.index).unwrap().note(), Some("Deposit for Ana"));
    let err = w.c.add_note(99, "x", w.now, &mut w.m).unwrap_err();
    assert_eq!(err, NoteRefused::UnknownSale(99));
}

#[test]
fn cr_206_a_note_is_plain_text_of_one_to_sixty_four_characters() {
    let sixty_four = "n".repeat(64);
    let accents = "\u{e9}".repeat(64);
    for good in [
        "a",
        sixty_four.as_str(),
        accents.as_str(),
        "Tour, 12/5 - 4 pax",
    ] {
        let mut w = World::new();
        let o = noted(&mut w, 500, good);
        assert_eq!(w.c.sale(o.index).unwrap().note(), Some(good));
    }
    let sixty_five = "n".repeat(65);
    for bad in ["", sixty_five.as_str(), "two\nlines", "tab\there", "nul\0"] {
        let mut w = World::new();
        let before = w.m.writes;
        let err = open_with(
            &mut w,
            500,
            SaleExtras {
                note: Some(bad),
                request: None,
            },
        )
        .unwrap_err();
        assert_eq!(err, OpenRefused::BadNote, "{bad:?}");
        assert_eq!(err.rule(), "CR-206");
        // Nothing is written for a note that is refused.
        assert_eq!(w.m.writes, before);
        assert!(w.m.events().is_empty());
        let o = w.open_five();
        let err = w.c.add_note(o.index, bad, w.now, &mut w.m).unwrap_err();
        assert_eq!(err, NoteRefused::BadNote, "{bad:?}");
        assert_eq!(w.c.sale(o.index).unwrap().note(), None);
    }
}

#[test]
fn cr_206_the_note_words_are_the_given_ones() {
    let words = NoteRefused::<&str>::BadNote.words();
    assert_eq!(check(&words), Ok(()));
    assert_eq!(words.heading, "Note not accepted");
    assert_eq!(
        words.what,
        "A note must be plain text of 1 to 64 characters, so it was not saved."
    );
    assert_eq!(words.next, "Type it again, shorter.");
    assert_eq!(words.who, Who::Cashier);
    assert_eq!(OpenRefused::<&str>::BadNote.words(), words);
}

#[test]
fn cr_206_the_record_of_a_note_is_tag_24_and_strict() {
    let e = Event::Noted {
        index: 4,
        at: T0,
        note: "Tour 12 May".to_string(),
    };
    let bytes = e.encode();
    assert_eq!(bytes[0], 24);
    assert_eq!(Event::decode(&bytes).unwrap(), e);
    for n in 0..bytes.len() {
        assert!(Event::decode(&bytes[..n]).is_err(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Event::decode(&longer).is_err());
    // An empty note, a note of control characters and one past 64
    // characters are refused.
    let head = &bytes[..1 + 4 + 8];
    for bad in [
        vec![0u8],
        vec![2, b'a', 0x0a],
        [vec![65u8], vec![b'n'; 65]].concat(),
    ] {
        let mut m = head.to_vec();
        m.extend_from_slice(&bad);
        assert!(
            matches!(Event::decode(&m), Err(DecodeError::NonCanonical(_))),
            "{bad:?}"
        );
    }
    // Every bit flip that still reads back is the same bytes again.
    let mut m = bytes.clone();
    for i in 0..m.len() {
        for bit in 0..8 {
            m[i] ^= 1 << bit;
            if let Ok(v) = Event::decode(&m) {
                assert_eq!(v.encode(), m, "byte {i} bit {bit}");
            }
            m[i] ^= 1 << bit;
        }
    }
}

#[test]
fn cr_206_a_note_changes_nothing_about_the_payment() {
    let mut w = World::new();
    let o = noted(&mut w, 500, "Deposit");
    let t = w.pay(&o, 5_000);
    w.confirm(&[t], 100);
    assert_eq!(w.judge(o.index).state, State::Confirmed);
    // A note is added to a sale that was paid, too.
    let p = w.open_five();
    let t = w.pay(&p, 5_000);
    w.confirm(&[t], 100);
    w.judge(p.index);
    w.c.add_note(p.index, "Balance", w.now, &mut w.m).unwrap();
    assert_eq!(w.judge(p.index).state, State::Confirmed);
}

// CR-207.

#[test]
fn cr_207_a_request_takes_no_new_quote() {
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::One);
    w.tick(600);
    let err = w.requote(o.index, 5_000_000).unwrap_err();
    assert_eq!(err, RequoteRefused::IsRequest);
    assert_eq!(err.rule(), "CR-207");
    let sale = w.c.sale(o.index).unwrap();
    assert_eq!(sale.quotes().len(), 1);
    assert_eq!(sale.needs_sats(), 5_000);
    // A sale at the till still takes one.
    let p = w.open_five();
    w.tick(400);
    assert!(w.requote(p.index, 5_000_000).is_ok());
}

// CR-208.

#[test]
fn cr_208_a_request_is_never_accepted_at_once() {
    // The same payment, seen by three groups with the conflict watch run
    // out, is paid at once for a sale at the till.
    let mut w = World::new();
    let p = w.open_five();
    let t = w.pay(&p, 5_000);
    assert_eq!(w.accept_at_once(&p, &t).state, State::PaidInstant);

    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::Three);
    let t = w.pay(&o, 5_000);
    w.tick(10);
    w.see(&t, 3);
    w.judge(o.index);
    w.tick(60);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(w.c.exposure_outstanding(), 0);
    assert!(w.c.sale(o.index).unwrap().accepted().is_none());
    assert!(!w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, Event::PaidAtOnce { .. })));
    // It is paid when what arrived is at the confirmation depth.
    w.confirm(&[t], 100);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Confirmed);
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, Event::Confirmed { .. })));
}

#[test]
fn cr_208_a_request_part_paid_or_overpaid_is_judged_as_any_sale() {
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::Seven);
    let t = w.pay(&o, 2_000);
    w.tick(10);
    w.see(&t, 3);
    assert_eq!(w.judge(o.index).state, State::PartPaid);
    // A request that is part paid does not expire.
    w.tick(8 * DAY);
    assert_eq!(w.judge(o.index).state, State::PartPaid);

    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::One);
    let t = w.pay(&o, 7_000);
    w.confirm(&[t], 100);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Confirmed);
    assert!(w
        .m
        .events()
        .iter()
        .any(|e| matches!(e, Event::SurplusOwed { sats: 2_000, .. })));
}

// CR-209.

#[test]
fn cr_209_a_request_expires_when_its_lifetime_runs_out_with_nothing_received() {
    for (days, n) in all_days() {
        let mut w = World::new();
        let o = request(&mut w, 500, days);
        // The quarter hour of a sale at the till does not end it.
        w.tick(EXPIRY_SECS * 4);
        assert_eq!(w.judge(o.index).state, State::NotPaidYet, "{n}");
        w.now = T0 + n * DAY - 1;
        assert_eq!(w.judge(o.index).state, State::NotPaidYet, "{n}");
        w.now = T0 + n * DAY;
        let screen = w.judge(o.index);
        assert_eq!(screen.state, State::Expired, "{n}");
        assert_eq!(
            w.c.sale(o.index).unwrap().ended(),
            Some((End::Expired, w.now))
        );
        assert!(w
            .m
            .events()
            .iter()
            .any(|e| matches!(e, Event::Expired { .. })));
    }
}

#[test]
fn cr_209_a_request_can_be_cancelled_like_a_sale() {
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::Three);
    w.tick(DAY);
    w.c.cancel(o.index, w.now, &mut w.m).unwrap();
    assert_eq!(w.judge(o.index).state, State::Cancelled);
    assert!(w.c.watched_scripts().contains(&o.script));

    // With money on it, it is not cancelled.
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::Three);
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    assert!(w.c.cancel(o.index, w.now, &mut w.m).is_err());
}

#[test]
fn cr_209_a_late_payment_to_an_expired_request_is_a_late_payment() {
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::One);
    w.tick(DAY);
    assert_eq!(w.judge(o.index).state, State::Expired);
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Expired);
    assert!(screen.late);
}

// CR-210.

#[test]
fn cr_210_a_request_paid_while_the_counter_was_closed_shows_paid_on_the_next_start() {
    let mut w = World::new();
    let o = request(&mut w, 500, RequestDays::Seven);
    assert_eq!(w.judge(o.index).state, State::NotPaidYet);
    // The counter is closed: what is stored is all that is left of it.
    let t = w.pay(&o, 5_000);
    let block = w.mine_at(&[t], 100);
    w.tick(2 * DAY);
    w.restart();
    assert_eq!(
        w.c.sale(o.index).unwrap().request(),
        Some(RequestDays::Seven)
    );
    assert!(w.c.watched_scripts().contains(&o.script));
    // On the next start the chain is caught up with and the block is offered.
    w.offer(&block);
    let screen = w.judge(o.index);
    assert_eq!(screen.state, State::Confirmed);
    assert_eq!(screen.received_sats, 5_000);
    assert_eq!(screen.needs_sats, 5_000);
}

// CR-211.

fn a_request() -> Sale {
    let mut w = World::new();
    let o = open_with(
        &mut w,
        500,
        SaleExtras {
            note: Some("Tour 12 May"),
            request: Some(RequestDays::Three),
        },
    )
    .unwrap();
    w.c.sale(o.index).unwrap().clone()
}

fn assert_strict(bytes: &[u8]) {
    let sale = Sale::from_bytes(bytes).expect("valid sale decodes");
    assert_eq!(sale.to_bytes(), bytes, "canonical re-encoding");
    for n in 0..bytes.len() {
        assert!(Sale::from_bytes(&bytes[..n]).is_err(), "prefix of {n}");
    }
    let mut longer = bytes.to_vec();
    longer.push(0);
    assert!(Sale::from_bytes(&longer).is_err());
    let mut m = bytes.to_vec();
    for i in 0..m.len() {
        for bit in 0..8 {
            m[i] ^= 1 << bit;
            if let Ok(v) = Sale::from_bytes(&m) {
                assert_eq!(v.to_bytes(), m, "byte {i} bit {bit}");
            }
            m[i] ^= 1 << bit;
        }
    }
}

#[test]
fn cr_211_a_request_or_a_sale_with_a_note_is_version_five_and_strict() {
    let request = a_request();
    let bytes = request.to_bytes();
    assert_eq!(&bytes[..5], b"OBML\x05");
    assert_strict(&bytes);
    let back = Sale::from_bytes(&bytes).unwrap();
    assert_eq!(back.request(), Some(RequestDays::Three));
    assert_eq!(back.note(), Some("Tour 12 May"));
    // A request without a note, and a note without a request.
    let mut w = World::new();
    let r = request_of(&mut w);
    assert_strict(&r.to_bytes());
    let n = {
        let o = noted(&mut w, 500, "Balance due");
        w.c.sale(o.index).unwrap().clone()
    };
    assert_eq!(n.to_bytes()[4], 5);
    assert_strict(&n.to_bytes());
    // A sale with neither is written as before.
    let p = w.open_five();
    assert_eq!(w.c.sale(p.index).unwrap().to_bytes()[4], 1);
}

fn request_of(w: &mut World) -> Sale {
    let o = request(w, 500, RequestDays::One);
    w.c.sale(o.index).unwrap().clone()
}

#[test]
fn cr_211_a_version_five_sale_that_is_not_what_the_counter_writes_is_refused() {
    let mut w = World::new();
    let o = noted(&mut w, 500, "Deposit");
    let stored = w.c.sale(o.index).unwrap().to_bytes();
    // After the watch: tip (8), lightning flag (1), refund count (4), request
    // (1), note flag (1), length (1) and the text.
    let tail = 8 + 1 + 4 + 1 + 1 + 1 + "Deposit".len();
    let request_at = stored.len() - "Deposit".len() - 3;
    assert_eq!(stored[request_at], 0);
    assert_eq!(stored[stored.len() - tail..][8], 0);

    // Neither a request nor a note.
    let mut bare = stored[..request_at].to_vec();
    bare.extend_from_slice(&[0, 0]);
    assert!(matches!(
        Sale::from_bytes(&bare),
        Err(DecodeError::NonCanonical(_))
    ));
    // A request of two days.
    let mut two = stored.clone();
    two[request_at] = 2;
    assert!(Sale::from_bytes(&two).is_err());
    // A request whose quote is the five minutes of a sale at the till.
    let mut short = stored.clone();
    short[request_at] = 1;
    assert!(Sale::from_bytes(&short).is_err());
    // A note that is not text of 1 to 64 characters.
    let mut empty = stored[..request_at + 1].to_vec();
    empty.extend_from_slice(&[1, 0]);
    assert!(Sale::from_bytes(&empty).is_err());
    let mut long = stored[..request_at + 1].to_vec();
    long.extend_from_slice(&[1, 65]);
    long.extend(std::iter::repeat_n(b'n', 65));
    assert!(Sale::from_bytes(&long).is_err());
    let mut control = stored[..request_at + 1].to_vec();
    control.extend_from_slice(&[1, 2, b'a', 0x07]);
    assert!(Sale::from_bytes(&control).is_err());

    // A request with a second quote, or paid over Lightning.
    let mut r = a_request();
    r.quotes
        .push(crate::Quote::lasting(rate(), T0 + 1, 3 * DAY));
    assert!(Sale::from_bytes(&r.to_bytes()).is_err());
    let mut r = a_request();
    r.lightning = Some(crate::sale::LightningPaid {
        payment_hash: [1; 32],
        amount_msat: 5_000_000,
        settled_at: None,
    });
    r.books_final = true;
    assert!(Sale::from_bytes(&r.to_bytes()).is_err());
}

#[test]
fn cr_211_a_sale_of_an_earlier_version_reads_as_no_note_and_no_request() {
    let mut w = World::new();
    let o = w.open_five();
    let stored = w.c.sale(o.index).unwrap().to_bytes();
    assert_eq!(stored[4], 1);
    let sale = Sale::from_bytes(&stored).unwrap();
    assert_eq!(sale.note(), None);
    assert_eq!(sale.request(), None);
    assert_eq!(sale.expires_at(), T0 + EXPIRY_SECS);
}
