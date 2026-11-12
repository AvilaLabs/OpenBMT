//! CR-245 to CR-254 through `dispatch`: the till's reference, bills and tabs,
//! parts, the split helper, close short with a manager, and what the Paid
//! screen reads (`sale_info`). The words are placeholders (CR-255).

use super::kit::*;
use super::wire::{Rd, W};

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

/// A bill as `bills` lists it: the fields the tests look at.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    number: u32,
    kind: u8,
    amount: u64,
    reference: Option<String>,
    state: u8,
    unpaid: u64,
    other: u64,
    pending: u32,
    left: u64,
    parts: Vec<(Option<u32>, u64, Option<u8>, bool)>,
    heading: String,
}

fn bills(rig: &Rig, handle: u64) -> Vec<Row> {
    let response = rig.call(84, handle, &W::new().u64(T0).0);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| {
        let number = r.u32();
        let kind = r.u8();
        let amount = r.u64();
        let reference = r.opt(Rd::string);
        let _opened_at = r.u64();
        let state = r.u8();
        let _ended_at = r.u64();
        let unpaid = r.u64();
        let _bitcoin = r.u64();
        let other = r.u64();
        let pending = r.u32();
        let left = r.u64();
        let parts = r.list(|r| (r.opt(Rd::u32), r.u64(), r.opt(Rd::u8), r.bool()));
        let heading = r.words().heading;
        Row {
            number,
            kind,
            amount,
            reference,
            state,
            unpaid,
            other,
            pending,
            left,
            parts,
            heading,
        }
    });
    r.done();
    rows
}

fn open_bill(rig: &Rig, handle: u64, kind: u8, amount: u64, reference: Option<&str>) -> u32 {
    let response = rig.call(
        74,
        handle,
        &W::new()
            .u64(T0)
            .u8(kind)
            .u64(amount)
            .opt_string(reference)
            .0,
    );
    let mut r = Rd::ok(&response);
    let number = r.u32();
    r.done();
    number
}

fn part_fields(number: u32, amount: u64, reference: Option<&str>) -> Vec<u8> {
    W::new()
        .u64(T0)
        .u32(number)
        .u64(amount)
        .u64(0)
        .u64(RATE)
        .string("test-fixed")
        .u64(T0)
        .u8(0)
        .opt_string(reference)
        .opt_string(None)
        .0
}

fn other_part(rig: &Rig, handle: u64, number: u32, amount: u64, way: u8) -> Vec<u8> {
    rig.call(
        78,
        handle,
        &W::new().u64(T0).u32(number).u64(amount).u8(way).0,
    )
}

fn refused(response: &[u8]) -> (u8, String) {
    let f = Rd::error(response).expect("an error");
    (f.status, f.rule)
}

#[test]
fn cr_246_a_bill_opens_with_a_number_and_is_listed_with_placeholder_words() {
    let (rig, handle) = ready();
    assert!(bills(&rig, handle).is_empty());
    let first = open_bill(&rig, handle, 1, 1_200, Some("T-14"));
    let second = open_bill(&rig, handle, 2, 5_000, None);
    assert_eq!((first, second), (1, 2));
    let rows = bills(&rig, handle);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].reference.as_deref(), Some("T-14"));
    assert_eq!((rows[0].kind, rows[0].state, rows[0].left), (1, 0, 1_200));
    assert_eq!((rows[1].kind, rows[1].amount), (2, 5_000));
    assert!(rows[0].heading.starts_with("WORDS_NEEDED_"));
    // A bill of nothing is refused, with the rule.
    let response = rig.call(74, handle, &W::new().u64(T0).u8(1).u64(0).opt_string(None).0);
    assert_eq!(refused(&response), (1, "CR-246".to_string()));
}

#[test]
fn cr_245_a_reference_is_added_once_to_a_bill_and_to_a_sale() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 1, 1_000, None);
    let add = |id: u32, text: &str| W::new().u64(T0).u32(id).string(text).0;
    assert!(Rd::error(&rig.call(75, handle, &add(number, "A-1"))).is_none());
    assert_eq!(
        refused(&rig.call(75, handle, &add(number, "A-2"))),
        (1, "CR-245".to_string())
    );
    assert_eq!(
        refused(&rig.call(75, handle, &add(number, "no spaces"))),
        (1, "CR-245".to_string())
    );
    assert_eq!(bills(&rig, handle)[0].reference.as_deref(), Some("A-1"));

    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "USD", RATE, "test-fixed").0,
    );
    let sale = Rd::ok(&response).opened().index;
    assert!(Rd::error(&rig.call(73, handle, &add(sale, "T-7"))).is_none());
    // CR-254: the Paid screen reads the reference and the number.
    let response = rig.call(63, handle, &W::new().u32(sale).0);
    let mut r = Rd::ok(&response);
    let _ = (r.opt(Rd::string), r.u8(), r.u64(), r.bool());
    assert_eq!(r.opt(Rd::string).as_deref(), Some("T-7"));
    assert_eq!(r.opt(Rd::u32), None);
    r.done();
}

#[test]
fn cr_248_a_part_opens_a_sale_that_the_info_says_is_in_the_bill() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 1, 1_200, None);
    let response = rig.call(77, handle, &part_fields(number, 500, Some("T-14/1")));
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.needs_sats, 5_000);
    let response = rig.call(63, handle, &W::new().u32(opened.index).0);
    let mut r = Rd::ok(&response);
    let _ = (r.opt(Rd::string), r.u8(), r.u64(), r.bool());
    assert_eq!(r.opt(Rd::string).as_deref(), Some("T-14/1"));
    assert_eq!(r.opt(Rd::u32), Some(number));
    r.done();
    let rows = bills(&rig, handle);
    assert_eq!(rows[0].pending, 1);
    assert_eq!(rows[0].parts, vec![(Some(opened.index), 500, None, false)]);
    // A bill that does not exist opens no sale.
    let response = rig.call(77, handle, &part_fields(99, 500, None));
    assert_eq!(refused(&response), (1, "CR-246".to_string()));
}

#[test]
fn cr_247_a_part_paid_another_way_counts_at_once_and_settles_the_bill() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 1, 1_200, None);
    assert!(Rd::error(&other_part(&rig, handle, number, 700, 1)).is_none());
    let row = &bills(&rig, handle)[0];
    assert_eq!((row.other, row.left, row.state), (700, 500, 0));
    assert_eq!(row.parts, vec![(None, 700, Some(1), true)]);
    assert!(Rd::error(&other_part(&rig, handle, number, 500, 2)).is_none());
    let row = &bills(&rig, handle)[0];
    assert_eq!((row.state, row.left), (1, 0));
    // A settled bill takes no more.
    assert_eq!(
        refused(&other_part(&rig, handle, number, 1, 3)),
        (1, "CR-246".to_string())
    );
}

#[test]
fn cr_250_the_split_gives_each_share_and_the_last_takes_the_rest() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 1, 1_000, None);
    let response = rig.call(79, handle, &W::new().u32(number).u8(3).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.list(Rd::u64), vec![334, 334, 332]);
    r.done();
    let response = rig.call(79, handle, &W::new().u32(number).u8(1).0);
    assert_eq!(refused(&response), (1, "CR-250".to_string()));
}

#[test]
fn cr_251_a_tab_is_raised_and_finished_when_it_has_its_amount() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 2, 500, None);
    let raise = |amount: u64| W::new().u64(T0).u32(number).u64(amount).0;
    assert!(Rd::error(&rig.call(80, handle, &raise(800))).is_none());
    assert!(Rd::error(&rig.call(80, handle, &raise(1_100))).is_none());
    assert_eq!(bills(&rig, handle)[0].amount, 1_100);
    let finish = W::new().u64(T0).u32(number).0;
    // Not finished while it has less than its amount.
    assert_eq!(
        refused(&rig.call(81, handle, &finish)),
        (1, "CR-251".to_string())
    );
    assert!(Rd::error(&other_part(&rig, handle, number, 1_100, 2)).is_none());
    // Reaching the amount does not settle a tab; the cashier does.
    assert_eq!(bills(&rig, handle)[0].state, 0);
    assert!(Rd::error(&rig.call(81, handle, &finish)).is_none());
    assert_eq!(bills(&rig, handle)[0].state, 1);
    // Lowering below what it received is refused on a tab that is open.
    let open = open_bill(&rig, handle, 2, 500, None);
    assert!(Rd::error(&other_part(&rig, handle, open, 400, 1)).is_none());
    let lower = W::new().u64(T0).u32(open).u64(300).0;
    assert_eq!(
        refused(&rig.call(80, handle, &lower)),
        (1, "CR-251".to_string())
    );
}

#[test]
fn cr_252_close_short_needs_a_manager_and_cancel_is_refused_once_a_part_counts() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 1, 1_000, None);
    assert!(Rd::error(&other_part(&rig, handle, number, 400, 1)).is_none());
    let close = |manager: bool| {
        W::new()
            .u64(T0)
            .u32(number)
            .actor(manager, "Ana")
            .0
    };
    assert_eq!(
        refused(&rig.call(82, handle, &close(false))),
        (1, "CR-252".to_string())
    );
    let response = rig.call(82, handle, &close(true));
    let mut r = Rd::ok(&response);
    assert_eq!(r.u64(), 600);
    r.done();
    let row = &bills(&rig, handle)[0];
    assert_eq!((row.state, row.unpaid), (2, 600));

    // A bill with a part that counts is not cancelled; one without is.
    let counted = open_bill(&rig, handle, 1, 1_000, None);
    assert!(Rd::error(&other_part(&rig, handle, counted, 100, 3)).is_none());
    let cancel = |n: u32| W::new().u64(T0).u32(n).0;
    assert_eq!(
        refused(&rig.call(83, handle, &cancel(counted))),
        (1, "CR-252".to_string())
    );
    let empty = open_bill(&rig, handle, 1, 1_000, None);
    assert!(Rd::error(&rig.call(83, handle, &cancel(empty))).is_none());
    let rows = bills(&rig, handle);
    assert_eq!(rows.iter().find(|r| r.number == empty).map(|r| r.state), Some(3));
}

#[test]
fn cr_248_a_sale_that_is_open_joins_a_bill_once() {
    let (rig, handle) = ready();
    let number = open_bill(&rig, handle, 1, 1_000, None);
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "USD", RATE, "test-fixed").0,
    );
    let sale = Rd::ok(&response).opened().index;
    let join = |n: u32| W::new().u64(T0).u32(n).u32(sale).0;
    assert!(Rd::error(&rig.call(76, handle, &join(number))).is_none());
    let other = open_bill(&rig, handle, 1, 1_000, None);
    assert_eq!(
        refused(&rig.call(76, handle, &join(other))),
        (1, "CR-248".to_string())
    );
}

#[test]
fn cr_253_the_bills_stay_after_the_node_is_stopped_and_started_again() {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    let number = open_bill(&rig, handle, 1, 900, Some("T-1"));
    assert!(Rd::error(&other_part(&rig, handle, number, 300, 1)).is_none());
    let response = rig.call(2, handle, &[]);
    Rd::ok(&response);
    let handle = rig.start();
    let rows = bills(&rig, handle);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].reference.as_deref(), Some("T-1"));
    assert_eq!((rows[0].left, rows[0].other), (600, 300));
    // The next number is not the old one.
    assert_eq!(open_bill(&rig, handle, 1, 100, None), number + 1);
}
