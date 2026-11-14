//! The starter line through `dispatch` (`docs/COUNTER-RULES.md` CR-272 to
//! CR-275): a counter paired with a starter wallet is started and linked to
//! a relay; the owner phone, which the test plays with the functions of
//! `openbmt_core` and a courier, sends starter lines; the app asks what "New
//! sale" does and which payments to show, and marks one seen.

use openbmt_core::starter_line::{LinePayment, StarterLine};
use openbmt_link::Kind;

use super::confirm::{owner, step};
use super::kit::T0;
use super::switch::{open_sale, shop, Shop};
use super::wire::{Rd, W};

const OP_STARTER_STATE: u16 = 204;
const OP_STARTER_SEEN: u16 = 205;
const OP_SWITCH_SAME: u16 = 201;

/// What `starter_state` returns, as the app reads it.
struct State {
    decision: u8,
    words: Option<String>,
    cards: Option<String>,
    held: Option<u64>,
    limit: Option<u64>,
    line_time: Option<u64>,
    /// Transaction ID (first byte), sats, time, by the owner, seen.
    payments: Vec<(u8, u64, u64, bool, bool)>,
}

fn state(s: &Shop, now: u64) -> State {
    let response = s.rig.call(OP_STARTER_STATE, s.handle, &W::new().u64(now).0);
    let mut r = Rd::ok(&response);
    let decision = r.u8();
    let words = r.opt(|r| r.words().heading);
    let cards = r.opt(|r| r.words().heading);
    let held = r.opt(Rd::u64);
    let limit = r.opt(Rd::u64);
    let line_time = r.opt(Rd::u64);
    let payments = r.list(|r| {
        let txid = r.bytes();
        assert_eq!(txid.len(), 32);
        (txid[0], r.u64(), r.u64(), r.bool(), r.bool())
    });
    r.done();
    State {
        decision,
        words,
        cards,
        held,
        limit,
        line_time,
        payments,
    }
}

fn seen(s: &Shop, now: u64, txid: &[u8]) -> Result<(), String> {
    let response = s
        .rig
        .call(OP_STARTER_SEEN, s.handle, &W::new().u64(now).bytes(txid).0);
    match Rd::error(&response) {
        Some(e) => Err(e.rule),
        None => {
            Rd::ok(&response).done();
            Ok(())
        }
    }
}

fn payment(n: u8, time: u64) -> LinePayment {
    LinePayment {
        txid: [n; 32],
        sats: 1_000 * u64::from(n),
        time,
        by_owner: n == 1,
    }
}

fn send(
    s: &mut Shop,
    number: u64,
    time: u64,
    held: u64,
    limit: u64,
    stop: bool,
    p: &[LinePayment],
) {
    let line = StarterLine::sign(&owner(), number, time, held, limit, stop, p);
    s.w.owner
        .send(&s.counter, Kind::Starter, &line.to_bytes())
        .unwrap();
    step(&s.rig, s.handle, time);
}

#[test]
fn cr_272_to_cr_274_the_app_reads_the_stop_and_the_cards_and_marks_a_card_seen() {
    let mut s = shop();
    // No line: bitcoin is offered, nothing is said, nothing to show.
    let st = state(&s, T0 + 200);
    assert_eq!(st.decision, 0);
    assert!(st.words.is_none() && st.cards.is_none() && st.held.is_none());
    assert!(st.payments.is_empty());

    // A line over the limit that asks to stop, with two payments.
    send(
        &mut s,
        1,
        T0 + 210,
        300,
        200,
        true,
        &[payment(1, T0 + 100), payment(2, T0 + 150)],
    );
    assert!(s.rig.dir().join("starter-line.bin").exists());
    assert!(s.rig.dir().join("starter-cards.bin").exists());
    let st = state(&s, T0 + 211);
    assert_eq!(st.decision, 1, "stopped");
    assert_eq!(st.words.unwrap(), "No bitcoin sales now");
    assert_eq!(st.cards.unwrap(), "Starter wallet payments");
    assert_eq!(
        (st.held, st.limit, st.line_time),
        (Some(300), Some(200), Some(T0 + 210))
    );
    // Newest first, with what the owner phone said of each.
    assert_eq!(
        st.payments,
        vec![
            (2, 2_000, T0 + 150, false, false),
            (1, 1_000, T0 + 100, true, false)
        ]
    );
    // A sale already open is not changed (CR-273).
    let open = open_sale(&s.rig, s.handle, T0 + 220);
    assert!(!open.payment.address.is_empty());

    // The owner phone's older line is refused; the state stays.
    send(&mut s, 1, T0 + 230, 0, 200, false, &[]);
    assert_eq!(state(&s, T0 + 231).decision, 1);

    // The line ages: the limit cannot be checked, bitcoin is offered again
    // and Home says so.
    let st = state(&s, T0 + 210 + 601);
    assert_eq!(st.decision, 2);
    assert_eq!(st.words.unwrap(), "Limit cannot be checked");

    // Someone at the counter has seen the newest payment.
    seen(&s, T0 + 300, &[2; 32]).unwrap();
    assert_eq!(seen(&s, T0 + 300, &[2; 32]), Err("CR-274".to_string()));
    assert_eq!(seen(&s, T0 + 300, &[7; 32]), Err("CR-274".to_string()));
    assert!(seen(&s, T0 + 300, &[2; 5]).is_err());
    let st = state(&s, T0 + 301);
    assert!(st.payments[0].4 && !st.payments[1].4);
    assert!(st.cards.is_some());
    seen(&s, T0 + 302, &[1; 32]).unwrap();
    assert!(state(&s, T0 + 303).cards.is_none());

    // A fresh line under the limit: open. The payments are not shown again.
    send(
        &mut s,
        2,
        T0 + 700,
        100,
        200,
        true,
        &[payment(2, T0 + 150), payment(3, T0 + 650)],
    );
    let st = state(&s, T0 + 701);
    assert_eq!(st.decision, 0);
    assert!(st.words.is_none());
    assert_eq!(st.payments.iter().filter(|p| !p.4).count(), 1);
    assert_eq!(st.payments[0].0, 3);

    // A restart keeps the line, the cards and what was seen.
    let record = s.record.clone();
    s.restart(&record, T0 + 800);
    let again = state(&s, T0 + 801);
    assert_eq!(again.decision, 0);
    assert_eq!(again.payments, st.payments);
    assert_eq!(again.held, Some(100));
}

#[test]
fn cr_275_the_switch_drops_the_line_and_the_cards() {
    let mut s = shop();
    send(&mut s, 1, T0 + 210, 300, 200, true, &[payment(1, T0 + 100)]);
    assert_eq!(state(&s, T0 + 211).decision, 1);
    let sw = s.switch((1_000, 2_000));
    s.send(&sw, T0 + 220);
    // While the switch waits the line stands.
    assert_eq!(state(&s, T0 + 221).decision, 1);
    Rd::ok(
        &s.rig
            .call(OP_SWITCH_SAME, s.handle, &W::new().u64(T0 + 230).0),
    );
    let st = state(&s, T0 + 231);
    assert_eq!(st.decision, 0);
    assert!(st.payments.is_empty() && st.held.is_none());
    assert!(!s.rig.dir().join("starter-line.bin").exists());
    assert!(!s.rig.dir().join("starter-cards.bin").exists());
    // No more lines are taken.
    send(&mut s, 2, T0 + 240, 300, 200, true, &[payment(2, T0 + 235)]);
    let st = state(&s, T0 + 241);
    assert_eq!(st.decision, 0);
    assert!(st.payments.is_empty());
    assert!(!s.rig.dir().join("starter-line.bin").exists());
}
