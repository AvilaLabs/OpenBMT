//! CR-205 to CR-216 through `dispatch`: `open_request` makes a request that
//! lasts its days with one fixed quote, a note is set when a sale opens or
//! added once, `sale_info`, `requests` and `request_link` give the app what
//! its screens show.

use super::kit::*;
use super::wire::{Opened, Rd, W};

const DAY: u64 = 86_400;

fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    r.pump();
    r.done();
    (rig, handle)
}

/// The fields of `open_request`, with no person at the till.
fn request_fields(now: u64, amount: u64, note: Option<&str>, days: u8) -> Vec<u8> {
    W::new()
        .u64(now)
        .u64(amount)
        .string("USD")
        .u64(RATE)
        .string("test-fixed")
        .u64(now)
        .u8(0)
        .opt_string(note)
        .u8(days)
        .0
}

fn open_request(rig: &Rig, handle: u64, fields: &[u8]) -> Opened {
    let response = rig.call(61, handle, fields);
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    opened
}

fn sale_info(rig: &Rig, handle: u64, index: u32) -> (Option<String>, u8, u64) {
    let response = rig.call(63, handle, &W::new().u32(index).0);
    let mut r = Rd::ok(&response);
    let info = (r.opt(Rd::string), r.u8(), r.u64());
    r.done();
    info
}

fn rows(rig: &Rig, handle: u64, now: u64) -> Vec<(u32, Option<String>, u64, u64, u64, u64)> {
    let response = rig.call(64, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| {
        (
            r.u32(),
            r.opt(Rd::string),
            r.u64(),
            r.u64(),
            r.u64(),
            r.u64(),
        )
    });
    r.done();
    rows
}

#[test]
fn cr_205_a_request_lasts_one_three_or_seven_days_with_one_fixed_quote() {
    for (days, n) in [(1u8, 1u64), (3, 3), (7, 7)] {
        let (rig, handle) = ready();
        let opened = open_request(&rig, handle, &request_fields(T0, 500, None, days));
        assert_eq!(opened.needs_sats, 5_000);
        assert_eq!(opened.quote_from, T0);
        assert_eq!(opened.quote_until, T0 + n * DAY);
        assert_eq!(
            sale_info(&rig, handle, opened.index),
            (None, days, T0 + n * DAY)
        );
    }
    // A sale opened as before has the five minutes and no request.
    let (rig, handle) = ready();
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "USD", RATE, "test-fixed").0,
    );
    let mut r = Rd::ok(&response);
    let plain = r.opened();
    r.done();
    assert_eq!(plain.quote_until, plain.quote_from + 300);
    assert_eq!(sale_info(&rig, handle, plain.index), (None, 0, T0 + 900));
}

#[test]
fn cr_205_any_other_number_of_days_is_refused_and_opens_nothing() {
    let (rig, handle) = ready();
    for days in [0u8, 2, 4, 14, 255] {
        let f = Rd::error(&rig.call(61, handle, &request_fields(T0, 500, None, days)))
            .expect("refused");
        assert_eq!((f.status, f.rule.as_str()), (1, "CR-205"), "{days}");
    }
    assert_eq!(
        open_request(&rig, handle, &request_fields(T0, 500, None, 1)).index,
        0
    );
}

#[test]
fn cr_206_a_note_is_set_when_the_request_opens_and_added_once_to_a_sale() {
    let (rig, handle) = ready();
    let opened = open_request(
        &rig,
        handle,
        &request_fields(T0, 500, Some("Tour 12 May"), 3),
    );
    assert_eq!(
        sale_info(&rig, handle, opened.index).0.as_deref(),
        Some("Tour 12 May")
    );

    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "USD", RATE, "test-fixed").0,
    );
    let mut r = Rd::ok(&response);
    let plain = r.opened();
    r.done();
    assert_eq!(sale_info(&rig, handle, plain.index).0, None);
    let add = |index: u32, note: &str| {
        rig.call(62, handle, &W::new().u64(T0 + 5).u32(index).string(note).0)
    };
    Rd::ok(&add(plain.index, "Balance for Ana")).done();
    assert_eq!(
        sale_info(&rig, handle, plain.index).0.as_deref(),
        Some("Balance for Ana")
    );
    // Once only.
    let f = Rd::error(&add(plain.index, "Another")).expect("refused");
    assert_eq!((f.status, f.rule.as_str()), (1, "CR-206"));
    assert_eq!(
        sale_info(&rig, handle, plain.index).0.as_deref(),
        Some("Balance for Ana")
    );
    // No such sale.
    let f = Rd::error(&add(77, "x")).expect("refused");
    assert_eq!(f.rule, "CR-11");
}

#[test]
fn cr_206_a_note_that_is_not_plain_text_is_refused_with_its_words_and_opens_nothing() {
    let (rig, handle) = ready();
    let sixty_five = "n".repeat(65);
    for bad in ["", sixty_five.as_str(), "two\nlines"] {
        let f = Rd::error(&rig.call(61, handle, &request_fields(T0, 500, Some(bad), 1)))
            .expect("refused");
        assert_eq!((f.status, f.rule.as_str()), (1, "CR-206"), "{bad:?}");
        assert_eq!(f.words.heading, "Note not accepted");
        assert_eq!(
            f.words.what,
            "A note must be plain text of 1 to 64 characters, so it was not saved."
        );
        assert_eq!(f.words.next, "Type it again, shorter.");
    }
    let opened = open_request(&rig, handle, &request_fields(T0, 500, None, 1));
    assert_eq!(opened.index, 0);
    let f = Rd::error(&rig.call(62, handle, &W::new().u64(T0).u32(opened.index).string("").0))
        .expect("refused");
    assert_eq!(f.words.heading, "Note not accepted");
}

#[test]
fn cr_214_the_link_has_the_address_the_amount_and_a_label() {
    let (rig, handle) = ready();
    let opened = open_request(
        &rig,
        handle,
        &request_fields(T0, 500, Some("Tour 12 May"), 3),
    );
    let link = |label: &str| {
        let response = rig.call(65, handle, &W::new().u32(opened.index).string(label).0);
        let mut r = Rd::ok(&response);
        let text = r.string();
        r.done();
        text
    };
    // 5,000 satoshis are 0.00005 bitcoin.
    assert_eq!(
        link("Tour 12 May"),
        format!(
            "bitcoin:{}?amount=0.00005&label=Tour%2012%20May",
            opened.payment.address
        )
    );
    assert_eq!(
        link("$5.00 \u{e9}&=?"),
        format!(
            "bitcoin:{}?amount=0.00005&label=%245.00%20%C3%A9%26%3D%3F",
            opened.payment.address
        )
    );
    assert!(Rd::error(&rig.call(65, handle, &W::new().u32(9).string("x").0)).is_some());
}

#[test]
fn cr_215_the_open_requests_are_listed_newest_first_until_paid_or_expired() {
    let (rig, handle) = ready();
    let first = open_request(&rig, handle, &request_fields(T0, 500, Some("Tour"), 1));
    let second = open_request(&rig, handle, &request_fields(T0 + 10, 700, None, 7));
    // A sale shown at the till is not a request.
    Rd::ok(&rig.call(
        11,
        handle,
        &W::new().open(T0 + 20, 900, "USD", RATE, "test-fixed").0,
    ))
    .opened();
    assert_eq!(
        rows(&rig, handle, T0 + 30),
        vec![
            (second.index, None, 700, T0 + 10, T0 + 10 + 7 * DAY, 7_000),
            (
                first.index,
                Some("Tour".to_string()),
                500,
                T0,
                T0 + DAY,
                5_000
            ),
        ]
    );
    // The day runs out for the first: it is not listed.
    let later: Vec<u32> = rows(&rig, handle, T0 + DAY).iter().map(|r| r.0).collect();
    assert_eq!(later, vec![second.index]);

    // The second is paid and confirmed: it leaves the list.
    let pay = spend(1, &[(&script_of(second.index), 7_000)]);
    rig.send(vec![sighting(&pay, vec![peer(20)])]);
    let block = mine(genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![joined(1, &block), block_event(&block)]);
    let response = rig.call(10, handle, &W::new().u64(T0 + 40).u32(50).0);
    let mut r = Rd::ok(&response);
    let (reports, _, _) = r.pump();
    r.done();
    assert!(
        reports.contains(&super::wire::Report::PosPaid(second.index)),
        "{reports:?}"
    );
    let left: Vec<u32> = rows(&rig, handle, T0 + 50).iter().map(|r| r.0).collect();
    assert_eq!(left, vec![first.index]);
}

#[test]
fn cr_215_the_requests_survive_a_restart_of_the_list_call() {
    let (rig, handle) = ready();
    open_request(&rig, handle, &request_fields(T0, 500, None, 1));
    assert_eq!(rows(&rig, handle, T0).len(), 1);
    assert_eq!(rows(&rig, handle, T0).len(), 1);
}

#[test]
fn cr_205_the_request_ops_refuse_every_cut_of_a_call() {
    let (rig, handle) = ready();
    let opened = open_request(&rig, handle, &request_fields(T0, 500, Some("a"), 1));
    for (op, body) in [
        (61u16, request_fields(T0, 500, Some("a"), 1)),
        (62, W::new().u64(T0).u32(opened.index).string("b").0),
        (63, W::new().u32(opened.index).0),
        (64, W::new().u64(T0).0),
        (65, W::new().u32(opened.index).string("l").0),
    ] {
        for n in 0..body.len() {
            let response = rig.call(op, handle, &body[..n]);
            assert!(Rd::error(&response).is_some(), "op {op} cut at {n}");
        }
        let mut longer = body.clone();
        longer.push(0);
        assert!(
            Rd::error(&rig.call(op, handle, &longer)).is_some(),
            "op {op}"
        );
        assert!(Rd::error(&rig.call(op, 0, &body)).is_some(), "op {op}");
    }
}

#[test]
fn cr_205_a_request_screen_has_its_code_and_a_requote_is_refused_with_words() {
    let (rig, handle) = ready();
    let opened = open_request(&rig, handle, &request_fields(T0, 500, None, 1));
    let response = rig.call(12, handle, &W::new().u64(T0 + 1_000).u32(opened.index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    assert_eq!(screen.needs, 5_000);
    assert!(screen.payment.is_some());
    assert!(screen.lightning.is_none());
    // Its quote is the one it was made with: asking for another is refused.
    let f = Rd::error(
        &rig.call(
            13,
            handle,
            &W::new()
                .u64(T0 + 1_000)
                .u32(opened.index)
                .u64(RATE)
                .string("test-fixed")
                .u64(T0 + 1_000)
                .0,
        ),
    )
    .expect("refused");
    assert_eq!((f.status, f.rule.as_str()), (1, "CR-207"));
    assert_eq!(f.words.heading, "Request keeps its price");
}
