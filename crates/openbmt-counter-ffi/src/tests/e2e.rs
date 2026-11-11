//! A sale from open to paid, and a restart, through `dispatch` alone, on the
//! node's own rules and a driver the test feeds by hand.

use bitcoin::{Address, Network};
use openbmt_qr::{encode_text, Ecc};

use super::kit::*;
use super::wire::{Rd, Report, W};

const TONE_INFO: u8 = 0;
const TONE_GOOD: u8 = 1;

fn pump(rig: &Rig, handle: u64) -> (Vec<Report>, Vec<String>, bool) {
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    let out = r.pump();
    r.done();
    out
}

fn judge(rig: &Rig, handle: u64, index: u32) -> super::wire::Screen {
    let response = rig.call(12, handle, &W::new().u64(T0).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    screen
}

#[test]
fn a_sale_goes_from_open_to_paid_and_survives_a_restart() {
    let rig = Rig::new();
    let handle = rig.start();
    assert_eq!(handle, 1);

    // The check needs the network's answer before a sale can open.
    let refused = rig.call(
        11,
        handle,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    let refusal = Rd::error(&refused).expect("not healthy yet");
    assert_eq!((refusal.status, refusal.rule.as_str()), (1, "CR-01"));
    assert!(!refusal.words.heading.is_empty());

    rig.send(vec![peers_event()]);
    assert_eq!(pump(&rig, handle), (vec![], vec![], false));

    // Open: the address is watched before the customer sees it.
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    let mut r = Rd::ok(&response);
    let opened = r.opened();
    r.done();
    assert_eq!(opened.index, 0);
    assert_eq!(opened.needs_sats, 5_000);
    assert_eq!(opened.quote_rate, RATE);
    assert_eq!(opened.quote_from, T0);
    assert_eq!(opened.quote_until, T0 + openbmt_counter::QUOTE_SECS);
    assert_eq!(rig.calls.lock().expect("lock").watched, vec![script_of(0)]);

    // The payment request: the address, the URI with the amount, and a code
    // that is the QR encoder's own for that text.
    let address = Address::from_script(&script_of(0), Network::Regtest)
        .expect("an address")
        .to_string();
    assert_eq!(opened.payment.address, address);
    assert_eq!(
        opened.payment.copy_uri,
        format!("bitcoin:{address}?amount=0.00005")
    );
    assert_eq!(
        opened.payment.uri,
        format!("BITCOIN:{}?amount=0.00005", address.to_ascii_uppercase())
    );
    let code = encode_text(&opened.payment.uri, Ecc::M).expect("a code");
    assert_eq!(opened.payment.size as usize, code.size());
    let n = code.size();
    for y in 0..n {
        for x in 0..n {
            let bit = y * n + x;
            let set = opened.payment.bits[bit / 8] & (0x80 >> (bit % 8)) != 0;
            assert_eq!(set, code.get(x, y), "module ({x}, {y})");
        }
    }
    assert_eq!(opened.payment.bits.len(), (n * n).div_ceil(8));

    // Nothing has arrived: blue, with the words and the same request.
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 0, "NotPaidYet");
    assert_eq!(screen.words.tone, TONE_INFO);
    assert_eq!(screen.words.heading, "Not paid yet");
    assert_eq!(screen.payment.as_ref(), Some(&opened.payment));
    assert!(screen.reports.is_empty() && screen.flags.is_empty());

    // One peer has seen the payment: waiting.
    let pay = spend(1, &[(&script_of(0), 5_000)]);
    rig.send(vec![sighting(&pay, vec![peer(20)])]);
    assert_eq!(pump(&rig, handle).0, vec![]);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 2, "Waiting");
    assert_eq!(screen.words.tone, TONE_INFO);
    assert_eq!(screen.received, 5_000);

    // One block with the payment: confirmed, and the POS is told once.
    let block = mine(genesis(), 1, std::slice::from_ref(&pay));
    rig.send(vec![joined(1, &block), block_event(&block)]);
    let (reports, _, stopped) = pump(&rig, handle);
    assert_eq!(reports, vec![Report::PosPaid(0)]);
    assert!(!stopped);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 4, "Confirmed");
    assert_eq!(screen.words.tone, TONE_GOOD);
    assert_eq!(screen.received, 5_000);
    assert!(screen.reports.is_empty(), "each report is returned once");

    // Status: the sale, the checked height and the health answer.
    let response = rig.call(30, handle, &[]);
    let mut r = Rd::ok(&response);
    let _checked = r.u32();
    let tip = r.u32();
    let health = r.opt(|r| (r.bool(), r.bool(), r.words()));
    assert!(
        r.opt(|r| (r.bool(), r.words())).is_none(),
        "no own node set"
    );
    let sales = r.list(Rd::u32);
    let refused_below = r.u64();
    let stopped = r.bool();
    assert!(r.opt(|r| (r.bool(), r.words())).is_none());
    r.done();
    assert_eq!(tip, 1);
    let (healthy, _behind, words) = health.expect("an answer");
    assert!(healthy && !words.heading.is_empty());
    assert_eq!((sales, refused_below, stopped), (vec![0], 0, false));

    // Stop, and start again on the same folder: the sale is still there and
    // is still confirmed, and the folder's lock was let go by the stop.
    let response = rig.call(2, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(!r.pump().2);
    assert!(rig.calls.lock().expect("lock").shutdown);
    let gone = rig.call(30, handle, &[]);
    assert_eq!(Rd::error(&gone).expect("gone").rule, "handle-unknown");

    // The driver saves the headers as it stops; the fake one does not.
    std::fs::write(rig.dir().join("headers.bin"), stored_headers(&[block])).expect("written");
    rig.arm();
    let again = rig.start();
    assert_eq!(again, 2, "handles are never reused");
    // The sale and its block are read back from the disk: confirmed before
    // the network has said anything.
    let screen = judge(&rig, again, 0);
    assert_eq!(screen.state, 4, "Confirmed, from the disk");
    assert!(screen.reports.is_empty(), "nothing is reported again");
    rig.send(vec![peers_event()]);
    pump(&rig, again);
    let screen = judge(&rig, again, 0);
    assert_eq!(screen.state, 4, "Confirmed, from the disk");
    assert!(screen.reports.is_empty(), "nothing is reported again");
}
