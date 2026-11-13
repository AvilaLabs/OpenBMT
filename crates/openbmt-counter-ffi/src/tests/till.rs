//! CR-266 to CR-271 through `dispatch`: the waiting requests, Start, Not
//! now, and what a sale carries of the till.

use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_pos::{SaleCancel, SaleRequest, TillMessage};

use super::kit::*;
use super::wire::{Rd, W};

const SALE: [u8; 16] = [4; 16];

/// Settings that name the Box (BX-20), signed as first settings are.
fn settings_with_box() -> SignedSettings {
    let mut s = Settings::defaults(1_000);
    s.version = 1;
    s.customer_wifi = false;
    s.min_sale_sats = 0;
    s.devices.insert(id(3).device_id());
    s.counters.insert(id(3).device_id(), 10_000);
    s.devices.insert(id(7).device_id());
    s.boxes.insert(id(7).device_id());
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.record = record_with(settings_with_box()).to_bytes();
    let handle = rig.start_with(&config);
    rig.send(vec![peers_event()]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    r.pump();
    r.done();
    (rig, handle)
}

fn request(sale: [u8; 16], cents: u64, reference: &str) -> Vec<u8> {
    TillMessage::Request(SaleRequest {
        sale,
        amount: openbmt_counter::Amount(cents),
        currency: openbmt_counter::Currency::new(*b"USD").expect("USD"),
        reference: reference.to_string(),
        expires_at: T0 + 300,
    })
    .to_bytes()
}

/// The Box's POS notice, as the link hands it to the node.
fn from_the_box(rig: &Rig, handle: u64, content: &[u8]) {
    let entry = rig.registry().entry_for_test(handle).expect("a node");
    entry
        .run(Some(T0), |node, _, _| {
            node.take_till_notice(&id(7).device_id(), content)
                .expect("taken");
            Ok(())
        })
        .expect("run");
}

#[derive(Debug, PartialEq, Eq)]
struct Row {
    sale: Vec<u8>,
    amount: u64,
    reference: String,
    lapses_at: u64,
    minutes_left: u64,
    heading: String,
}

fn waiting(rig: &Rig, handle: u64, now: u64) -> Vec<Row> {
    let response = rig.call(85, handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| Row {
        sale: r.bytes(),
        amount: r.u64(),
        reference: r.string(),
        lapses_at: r.u64(),
        minutes_left: r.u64(),
        heading: r.words().heading,
    });
    r.done();
    rows
}

fn start_fields(now: u64, sale: &[u8]) -> Vec<u8> {
    W::new()
        .u64(now)
        .bytes(sale)
        .u64(RATE)
        .string("test-fixed")
        .u64(T0)
        .u8(0)
        .0
}

fn of_sale(rig: &Rig, handle: u64, index: u32) -> (Option<Vec<u8>>, bool, Option<String>) {
    let response = rig.call(88, handle, &W::new().u32(index).0);
    let mut r = Rd::ok(&response);
    let view = (r.opt(Rd::bytes), r.bool(), r.opt(|r| r.words().heading));
    r.done();
    view
}

#[test]
fn cr_267_the_waiting_request_is_listed_with_its_amount_reference_and_minutes() {
    let (rig, handle) = ready();
    assert!(waiting(&rig, handle, T0).is_empty());
    from_the_box(&rig, handle, &request(SALE, 500, "POS/0007"));
    let rows = waiting(&rig, handle, T0);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sale, SALE.to_vec());
    assert_eq!(rows[0].amount, 500);
    assert_eq!(rows[0].reference, "POS/0007");
    assert_eq!((rows[0].lapses_at, rows[0].minutes_left), (T0 + 300, 5));
    assert_eq!(rows[0].heading, "WORDS_NEEDED_CR_267_FROM_THE_TILL");
    // The minutes count down and the request leaves when it lapses.
    assert_eq!(waiting(&rig, handle, T0 + 241)[0].minutes_left, 1);
    assert!(waiting(&rig, handle, T0 + 300).is_empty());
}

#[test]
fn cr_267_not_now_hides_the_request_and_an_unknown_one_is_refused() {
    let (rig, handle) = ready();
    from_the_box(&rig, handle, &request(SALE, 500, ""));
    let response = rig.call(87, handle, &W::new().u64(T0).bytes(&SALE).0);
    Rd::ok(&response).done();
    assert!(waiting(&rig, handle, T0).is_empty());
    let response = rig.call(87, handle, &W::new().u64(T0).bytes(&[8; 16]).0);
    assert_eq!(Rd::error(&response).expect("refused").rule, "CR-266");
    // A sale id that is not 16 bytes is refused before anything is looked up.
    let response = rig.call(87, handle, &W::new().u64(T0).bytes(&[1; 3]).0);
    assert_eq!(Rd::error(&response).expect("refused").rule, "CR-266");
}

#[test]
fn cr_268_start_opens_the_sale_for_the_amount_and_the_sale_names_the_till_sale() {
    let (rig, handle) = ready();
    from_the_box(&rig, handle, &request(SALE, 500, "POS/0007"));
    let response = rig.call(86, handle, &start_fields(T0, &SALE));
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.needs_sats, 5_000);
    assert!(waiting(&rig, handle, T0).is_empty());
    let (till, cancelled, words) = of_sale(&rig, handle, opened.index);
    assert_eq!((till, cancelled, words), (Some(SALE.to_vec()), false, None));
    // A request that is not there is refused with its rule.
    let response = rig.call(86, handle, &start_fields(T0, &SALE));
    assert_eq!(Rd::error(&response).expect("refused").rule, "CR-266");
}

#[test]
fn cr_268_a_start_the_counter_refuses_keeps_the_request_waiting() {
    let (rig, handle) = ready();
    from_the_box(&rig, handle, &request(SALE, 500, ""));
    // A rate of zero is not a rate.
    let response = rig.call(
        86,
        handle,
        &W::new()
            .u64(T0)
            .bytes(&SALE)
            .u64(0)
            .string("test-fixed")
            .u64(T0)
            .u8(0)
            .0,
    );
    assert_eq!(Rd::error(&response).expect("refused").rule, "sale-rate");
    assert_eq!(waiting(&rig, handle, T0).len(), 1);
}

#[test]
fn cr_269_a_cancel_after_start_marks_the_sale_and_the_sale_goes_on() {
    let (rig, handle) = ready();
    from_the_box(&rig, handle, &request(SALE, 500, ""));
    let response = rig.call(86, handle, &start_fields(T0, &SALE));
    let index = Rd::ok(&response).opened().index;
    let entry = rig.registry().entry_for_test(handle).expect("a node");
    entry
        .run(Some(T0), |node, _, _| {
            let cancel = TillMessage::Cancel(SaleCancel { sale: SALE }).to_bytes();
            node.take_till_notice(&id(7).device_id(), &cancel)
                .expect("taken");
            Ok(())
        })
        .expect("run");
    let (till, cancelled, words) = of_sale(&rig, handle, index);
    assert_eq!(till, Some(SALE.to_vec()));
    assert!(cancelled);
    assert_eq!(words.as_deref(), Some("WORDS_NEEDED_CR_269_CANCELLED"));
    // The sale is still open: judging it says so.
    let response = rig.call(12, handle, &W::new().u64(T0).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    assert_eq!(screen.needs, 5_000);
}

#[test]
fn cr_269_a_sale_typed_at_the_counter_has_no_till_sale() {
    let (rig, handle) = ready();
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, 500, "USD", RATE, "test-fixed").0,
    );
    let index = Rd::ok(&response).opened().index;
    assert_eq!(of_sale(&rig, handle, index), (None, false, None));
}
