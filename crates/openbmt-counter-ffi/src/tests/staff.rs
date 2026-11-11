//! The ops of the cashiers and the day (CR-143, CR-151 to CR-153):
//! `staff_record`, `day_totals`, and the person at the till in `open` and
//! `cancel`.

use super::kit::*;
use super::wire::{Day, Rd, W};

const DAY: u64 = 86_400;

fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    (rig, handle)
}

fn totals(rig: &Rig, handle: u64, from: u64, to: u64, people: bool) -> Day {
    let response = rig.call(
        58,
        handle,
        &W::new().u64(from).u64(to).u8(u8::from(people)).0,
    );
    let mut r = Rd::ok(&response);
    let day = r.day_totals();
    r.done();
    day
}

fn open_by(rig: &Rig, handle: u64, name: &str) -> u32 {
    let response = rig.call(
        11,
        handle,
        &W::new()
            .open_by(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed", name)
            .0,
    );
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    opened.index
}

#[test]
fn cr_143_a_staff_change_is_a_record_and_a_refusal_writes_nothing() {
    let (rig, handle) = ready();
    for (added, name) in [(1u8, "Ann"), (0, "Ann")] {
        let response = rig.call(
            57,
            handle,
            &W::new().u64(T0).u8(added).string(name).string("Max").0,
        );
        Rd::ok(&response).done();
    }
    // A name that is not a label is refused with the counter's words.
    let response = rig.call(
        57,
        handle,
        &W::new().u64(T0).u8(1).string("").string("Max").0,
    );
    let f = Rd::error(&response).expect("refused");
    assert_eq!(f.status, 1);
    assert_eq!(f.rule, "CR-143");
    assert!(!f.words.heading.is_empty() && !f.words.next.is_empty());
    // A flag that is neither 0 nor 1 is refused before the node is asked.
    let response = rig.call(
        57,
        handle,
        &W::new().u64(T0).u8(2).string("Ann").string("Max").0,
    );
    assert!(Rd::error(&response).is_some());
    // No node, no record.
    let response = rig.call(57, 0, &W::new().u64(T0).u8(1).string("Ann").string("Max").0);
    assert!(Rd::error(&response).is_some());
}

#[test]
fn cr_151_a_sale_and_a_cancel_name_the_person_at_the_till() {
    let (rig, handle) = ready();
    let index = open_by(&rig, handle, "Ann");
    let day = totals(&rig, handle, T0 - 10, T0 + DAY, true);
    assert_eq!(day.waiting.0, 1);
    assert_eq!(day.unnamed_sales, 0);
    assert_eq!(day.people.len(), 1);
    let line = &day.people[0];
    assert_eq!(
        (line.manager, line.name.as_str(), line.sales),
        (false, "Ann", 1)
    );
    // A bad name is refused and the cancel names the person.
    let response = rig.call(
        14,
        handle,
        &W::new().u64(T0).u32(index).u8(1).actor(false, "").0,
    );
    assert_eq!(Rd::error(&response).expect("refused").rule, "CR-61");
    let response = rig.call(
        14,
        handle,
        &W::new().u64(T0).u32(index).u8(1).actor(false, "Ann").0,
    );
    Rd::ok(&response).done();
    let day = totals(&rig, handle, T0 - 10, T0 + DAY, false);
    assert_eq!((day.ended, day.waiting.0), (1, 0));
    assert!(day.people.is_empty());
}

#[test]
fn cr_152_the_day_is_counted_between_two_times_the_app_gives() {
    let (rig, handle) = ready();
    open_by(&rig, handle, "Ann");
    let none = totals(&rig, handle, T0 + DAY, T0 + 2 * DAY, false);
    assert_eq!((none.waiting.0, none.paid.0, none.ended), (0, 0, 0));
    let some = totals(&rig, handle, T0 - 10, T0 + DAY, false);
    assert_eq!(some.waiting.0, 1);
    assert_eq!(some.currency.as_deref(), Some("USD"));
    // A window that is empty is refused with words.
    let response = rig.call(58, handle, &W::new().u64(T0).u64(T0).u8(0).0);
    let f = Rd::error(&response).expect("refused");
    assert_eq!(f.rule, "CR-152");
    assert!(!f.words.heading.is_empty());
}

#[test]
fn cr_153_the_new_ops_refuse_every_cut_of_a_request() {
    let (rig, handle) = ready();
    let fields = [
        (57u16, W::new().u64(T0).u8(1).string("Ann").string("Max").0),
        (58, W::new().u64(T0).u64(T0 + DAY).u8(1).0),
        (14, W::new().u64(T0).u32(0).u8(1).actor(false, "Ann").0),
    ];
    for (op, body) in fields {
        for n in 0..body.len() {
            let response = rig.call(op, handle, &body[..n]);
            assert!(Rd::error(&response).is_some(), "op {op} cut at {n}");
        }
        let mut longer = body.clone();
        longer.push(0);
        let response = rig.call(op, handle, &longer);
        assert!(Rd::error(&response).is_some(), "op {op} with a byte over");
    }
}
