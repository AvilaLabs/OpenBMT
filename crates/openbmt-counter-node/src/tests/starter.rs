//! CR-272 to CR-276: the starter line at the counter. The node of a counter
//! paired with a starter wallet is started on a fake driver; the owner
//! phone's part is the signed line of `openbmt_core`.

use std::sync::atomic::Ordering;

use openbmt_core::starter_line::{LinePayment, StarterLine, MAX_LINE_PAYMENTS};

use super::kit::*;
use super::paired_own::record;
use super::switch::{fresh_rig, good, new_wallet, rig_for, switch_path};
use crate::dirstore::{STARTER_CARDS_FILE, STARTER_LINE_FILE};
use crate::say::{all_words, every_starter_refused, is_placeholder};
use crate::starter::{stop_decision, Cards, StarterRefused, StopDecision, MAX_CARDS};

fn payment(n: u8, time: u64) -> LinePayment {
    LinePayment {
        txid: [n; 32],
        sats: 1_000 * u64::from(n),
        time,
        by_owner: n.is_multiple_of(2),
    }
}

/// A line signed by `signer`.
fn line_by(
    signer: u8,
    number: u64,
    time: u64,
    held: u64,
    limit: u64,
    stop: bool,
    p: &[LinePayment],
) -> StarterLine {
    StarterLine::sign(&id(signer), number, time, held, limit, stop, p)
}

/// A line of the owner root (1).
fn line(
    number: u64,
    time: u64,
    held: u64,
    limit: u64,
    stop: bool,
    p: &[LinePayment],
) -> StarterLine {
    line_by(1, number, time, held, limit, stop, p)
}

fn path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    dir.join(name)
}

// CR-273: the decision is a pure function.

#[test]
fn cr_273_the_stop_decision_is_a_function_of_the_line_the_time_and_the_age_limit() {
    let age = 600;
    let now = T0;
    let l = |time, held, limit, stop| line(1, time, held, limit, stop, &[]);
    // No line: bitcoin is offered, and nothing is said.
    assert_eq!(stop_decision(None, now, age), StopDecision::Open);
    // A fresh line that says stop and holds the limit, or more: stopped.
    assert_eq!(
        stop_decision(Some(&l(now, 100, 100, true)), now, age),
        StopDecision::Stopped
    );
    assert_eq!(
        stop_decision(Some(&l(now, 101, 100, true)), now, age),
        StopDecision::Stopped
    );
    // Under the limit: open.
    assert_eq!(
        stop_decision(Some(&l(now, 99, 100, true)), now, age),
        StopDecision::Open
    );
    // The owner did not ask to stop: open, over the limit or not.
    assert_eq!(
        stop_decision(Some(&l(now, 500, 100, false)), now, age),
        StopDecision::Open
    );
    // The edge of the age limit: at it the line is fresh, one second past it
    // the limit cannot be checked.
    assert_eq!(
        stop_decision(Some(&l(now - age, 500, 100, true)), now, age),
        StopDecision::Stopped
    );
    assert_eq!(
        stop_decision(Some(&l(now - age - 1, 500, 100, true)), now, age),
        StopDecision::Unchecked
    );
    // A line from a clock far ahead is no fresher.
    assert_eq!(
        stop_decision(Some(&l(now + age, 500, 100, true)), now, age),
        StopDecision::Stopped
    );
    assert_eq!(
        stop_decision(Some(&l(now + age + 1, 500, 100, true)), now, age),
        StopDecision::Unchecked
    );
    // An old line that did not ask to stop says nothing.
    assert_eq!(
        stop_decision(Some(&l(now - 10_000, 500, 100, false)), now, age),
        StopDecision::Open
    );
    // An old line that said stop but is under the limit still cannot be
    // checked: the wallet may have grown since.
    assert_eq!(
        stop_decision(Some(&l(now - 10_000, 1, 100, true)), now, age),
        StopDecision::Unchecked
    );
    // The limit in force moves the edge.
    assert_eq!(
        stop_decision(Some(&l(now - 1_000, 500, 100, true)), now, 3_600),
        StopDecision::Stopped
    );
}

#[test]
fn cr_273_the_node_uses_the_time_now_and_the_age_limit_of_its_settings() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    assert_eq!(rig.node.starter_decision(), StopDecision::Open);
    let age = u64::from(rig.node.settings().heartbeat_max_age_secs);
    rig.node
        .take_starter_line(&line(1, T0, 300, 200, true, &[]).to_bytes())
        .unwrap();
    assert_eq!(rig.node.starter_decision(), StopDecision::Stopped);
    rig.time.store(T0 + age, Ordering::SeqCst);
    assert_eq!(rig.node.starter_decision(), StopDecision::Stopped);
    rig.time.store(T0 + age + 1, Ordering::SeqCst);
    assert_eq!(rig.node.starter_decision(), StopDecision::Unchecked);
    // A newer line makes it fresh again, and a sale already open is not
    // touched by any of it.
    let sale = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    rig.node
        .take_starter_line(&line(2, T0 + age + 1, 100, 200, true, &[]).to_bytes())
        .unwrap();
    assert_eq!(rig.node.starter_decision(), StopDecision::Open);
    assert!(rig.node.counter().sale(sale.index).unwrap().is_live());
}

// CR-272: taking a line.

#[test]
fn cr_272_a_good_line_is_kept_in_its_own_file_and_read_back_at_start() {
    let dir = Scratch::new();
    let bytes = line(7, T0, 300, 200, true, &[payment(1, T0 - 50)]).to_bytes();
    {
        let mut rig = fresh_rig(dir.path());
        rig.node.take_starter_line(&bytes).unwrap();
        assert_eq!(rig.node.starter_line().unwrap().number, 7);
    }
    // The frame and nothing else.
    assert_eq!(
        std::fs::read(path(dir.path(), STARTER_LINE_FILE)).unwrap(),
        bytes
    );
    let rig = fresh_rig(dir.path());
    assert_eq!(rig.node.starter_line().unwrap().number, 7);
    assert_eq!(rig.node.starter_decision(), StopDecision::Stopped);
}

#[test]
fn cr_272_a_line_that_fails_a_check_is_dropped_and_changes_nothing() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    let first = line(5, T0, 1, 2, false, &[payment(1, T0)]).to_bytes();
    rig.node.take_starter_line(&first).unwrap();
    let file = std::fs::read(path(dir.path(), STARTER_LINE_FILE)).unwrap();
    // Not a line, cut, or with a byte over.
    assert!(matches!(
        rig.node.take_starter_line(b"junk"),
        Err(StarterRefused::Frame(_))
    ));
    assert!(matches!(
        rig.node.take_starter_line(&first[..first.len() - 1]),
        Err(StarterRefused::Frame(_))
    ));
    // Not signed by the owner root: another key, or a changed field.
    assert_eq!(
        rig.node
            .take_starter_line(&line_by(9, 6, T0, 1, 2, false, &[]).to_bytes()),
        Err(StarterRefused::NotFromOwner)
    );
    let mut forged = line(6, T0, 1, 2, false, &[]);
    forged.held_sats += 1;
    assert_eq!(
        rig.node.take_starter_line(&forged.to_bytes()),
        Err(StarterRefused::NotFromOwner)
    );
    // A number that is not above the last one, the same line again included.
    for number in [4, 5] {
        assert_eq!(
            rig.node
                .take_starter_line(&line(number, T0, 9, 9, true, &[]).to_bytes()),
            Err(StarterRefused::Old {
                last: 5,
                got: number
            })
        );
    }
    assert_eq!(
        rig.node.take_starter_line(&first),
        Err(StarterRefused::Old { last: 5, got: 5 })
    );
    // Nothing changed.
    assert_eq!(rig.node.starter_line().unwrap().number, 5);
    assert_eq!(
        std::fs::read(path(dir.path(), STARTER_LINE_FILE)).unwrap(),
        file
    );
    assert_eq!(rig.node.starter_payments().len(), 1);
    // A higher number is taken.
    rig.node
        .take_starter_line(&line(6, T0, 9, 9, true, &[]).to_bytes())
        .unwrap();
    assert_eq!(rig.node.starter_line().unwrap().number, 6);
}

#[test]
fn cr_272_a_counter_that_is_not_a_starter_counter_takes_no_line() {
    let dir = Scratch::new();
    // Paired with the shop's own wallet, with no starter flag (CR-256).
    let mut rig = rig_for(dir.path(), &record());
    assert_eq!(
        rig.node
            .take_starter_line(&line(1, T0, 1, 2, true, &[payment(1, T0)]).to_bytes()),
        Err(StarterRefused::NotStarter)
    );
    assert!(rig.node.starter_line().is_none());
    assert!(!path(dir.path(), STARTER_LINE_FILE).exists());
    assert!(!path(dir.path(), STARTER_CARDS_FILE).exists());
    assert_eq!(rig.node.starter_decision(), StopDecision::Open);
}

#[test]
fn cr_272_a_file_that_does_not_read_or_does_not_check_is_removed_at_start() {
    let dir = Scratch::new();
    drop(fresh_rig(dir.path()));
    let good_bytes = line(3, T0, 1, 2, true, &[]).to_bytes();
    let cases: Vec<Vec<u8>> = vec![
        good_bytes[..good_bytes.len() - 1].to_vec(),
        [good_bytes.clone(), vec![0]].concat(),
        line_by(9, 3, T0, 1, 2, true, &[]).to_bytes(),
    ];
    for bytes in cases {
        std::fs::write(path(dir.path(), STARTER_LINE_FILE), &bytes).unwrap();
        let mut rig = fresh_rig(dir.path());
        assert!(rig.node.starter_line().is_none());
        assert!(!path(dir.path(), STARTER_LINE_FILE).exists() || rig.node.starter_line().is_none());
        assert!(rig.node.take_notes().iter().any(|n| n.contains("CR-272")));
        assert_eq!(rig.node.starter_decision(), StopDecision::Open);
    }
}

// CR-274: the payments.

#[test]
fn cr_274_a_payment_not_shown_before_becomes_a_card_once() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    assert!(rig.node.starter_payments().is_empty());
    rig.node
        .take_starter_line(
            &line(
                1,
                T0,
                0,
                0,
                false,
                &[payment(1, T0 - 100), payment(2, T0 - 50)],
            )
            .to_bytes(),
        )
        .unwrap();
    let cards = rig.node.starter_payments();
    assert_eq!(cards.len(), 2);
    // Newest first, with what the owner phone said of each.
    assert_eq!(cards[0].payment.txid, [2; 32]);
    assert!(cards[0].payment.by_owner && !cards[1].payment.by_owner);
    assert!(cards.iter().all(|c| !c.seen));
    // A later line with the same payments and one more adds only the new.
    rig.node
        .take_starter_line(
            &line(
                2,
                T0,
                0,
                0,
                false,
                &[
                    payment(1, T0 - 100),
                    payment(2, T0 - 50),
                    payment(3, T0 - 10),
                ],
            )
            .to_bytes(),
        )
        .unwrap();
    assert_eq!(rig.node.starter_payments().len(), 3);
    // Seen: the card goes, and the same payment in the next line does not
    // bring it back.
    rig.node.starter_seen(&[2; 32]).unwrap();
    assert_eq!(
        rig.node.starter_seen(&[2; 32]),
        Err(StarterRefused::NoSuchPayment)
    );
    assert_eq!(
        rig.node.starter_seen(&[9; 32]),
        Err(StarterRefused::NoSuchPayment)
    );
    rig.node
        .take_starter_line(
            &line(
                3,
                T0,
                0,
                0,
                false,
                &[payment(2, T0 - 50), payment(3, T0 - 10)],
            )
            .to_bytes(),
        )
        .unwrap();
    let unseen: Vec<[u8; 32]> = rig
        .node
        .starter_payments()
        .iter()
        .filter(|c| !c.seen)
        .map(|c| c.payment.txid)
        .collect();
    assert_eq!(unseen, vec![[3; 32], [1; 32]]);
}

#[test]
fn cr_274_the_cards_and_what_was_seen_are_kept_across_a_restart() {
    let dir = Scratch::new();
    {
        let mut rig = fresh_rig(dir.path());
        rig.node
            .take_starter_line(
                &line(
                    1,
                    T0,
                    0,
                    0,
                    false,
                    &[payment(1, T0 - 100), payment(2, T0 - 50)],
                )
                .to_bytes(),
            )
            .unwrap();
        rig.node.starter_seen(&[1; 32]).unwrap();
    }
    let mut rig = fresh_rig(dir.path());
    let cards = rig.node.starter_payments();
    assert_eq!(cards.len(), 2);
    assert!(!cards[0].seen && cards[1].seen);
    // The last line comes again after the restart: nothing is new.
    assert_eq!(rig.node.starter_line().unwrap().number, 1);
    rig.node
        .take_starter_line(
            &line(
                2,
                T0,
                0,
                0,
                false,
                &[payment(1, T0 - 100), payment(2, T0 - 50)],
            )
            .to_bytes(),
        )
        .unwrap();
    assert_eq!(rig.node.starter_payments(), cards);
}

#[test]
fn cr_274_today_lists_the_payments_of_the_day() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    rig.node
        .take_starter_line(
            &line(
                1,
                T0,
                0,
                0,
                false,
                &[payment(1, 1_000), payment(2, 2_000), payment(3, 3_000)],
            )
            .to_bytes(),
        )
        .unwrap();
    let day = rig.node.starter_payments_between(1_500, 3_000);
    assert_eq!(day.len(), 1);
    assert_eq!(day[0].payment.txid, [2; 32]);
    assert_eq!(rig.node.starter_payments_between(0, 3_001).len(), 3);
    assert!(rig.node.starter_payments_between(3_001, 9_000).is_empty());
}

#[test]
fn cr_274_the_file_of_the_payments_is_read_strictly_and_holds_at_most_256() {
    let many: Vec<LinePayment> = (0..=u8::MAX)
        .map(|n| payment(n, 10_000 + u64::from(n)))
        .collect();
    let mut cards = Cards::default();
    for chunk in many.chunks(MAX_LINE_PAYMENTS) {
        assert!(cards.add(chunk));
    }
    assert_eq!(cards.len(), MAX_CARDS);
    assert!(!cards.add(&many[..4]));
    // One more drops the oldest that was seen, or else the oldest.
    let mut one_more = payment(9, 20_000);
    one_more.txid = [9; 32];
    one_more.txid[31] = 8;
    let mut with_seen = cards.clone();
    with_seen.mark_seen(&[200; 32]);
    assert!(with_seen.add(&[one_more]));
    assert_eq!(with_seen.len(), MAX_CARDS);
    let kept = |c: &Cards, txid: [u8; 32]| c.newest_first().iter().any(|x| x.payment.txid == txid);
    assert!(!kept(&with_seen, [200; 32]) && kept(&with_seen, [0; 32]));
    assert!(cards.add(&[one_more]));
    assert_eq!(cards.len(), MAX_CARDS);
    assert!(!kept(&cards, [0; 32]) && kept(&cards, [200; 32]));
    // The wire form: back to the same bytes, every cut refused, flags 0 or 1,
    // the order and repeats refused.
    let mut small = Cards::default();
    small.add(&[payment(1, 5), payment(2, 6)]);
    small.mark_seen(&[2; 32]);
    let bytes = small.to_bytes();
    assert_eq!(&bytes[..4], b"OBM\x1c");
    assert_eq!(Cards::from_bytes(&bytes).unwrap(), small);
    assert_eq!(Cards::from_bytes(&bytes).unwrap().to_bytes(), bytes);
    for n in 0..bytes.len() {
        assert!(Cards::from_bytes(&bytes[..n]).is_err(), "cut {n}");
    }
    assert!(Cards::from_bytes(&[bytes.clone(), vec![0]].concat()).is_err());
    let mut flag = bytes.clone();
    *flag.last_mut().unwrap() = 2;
    assert!(Cards::from_bytes(&flag).is_err());
    let mut repeated = bytes.clone();
    let second = 7 + 50;
    let first = repeated[7..7 + 32].to_vec();
    repeated[second..second + 32].copy_from_slice(&first);
    assert!(Cards::from_bytes(&repeated).is_err());
    let mut count = bytes.clone();
    count[5] = 0xff;
    count[6] = 0xff;
    assert!(Cards::from_bytes(&count).is_err());
}

#[test]
fn cr_274_a_file_of_payments_that_does_not_read_is_removed_with_a_note() {
    let dir = Scratch::new();
    drop(fresh_rig(dir.path()));
    std::fs::write(path(dir.path(), STARTER_CARDS_FILE), b"OBM\x1c\x01\x05").unwrap();
    let mut rig = fresh_rig(dir.path());
    assert!(rig.node.starter_payments().is_empty());
    assert!(rig.node.take_notes().iter().any(|n| n.contains("CR-274")));
}

// CR-275: after the switch.

#[test]
fn cr_275_a_switch_drops_the_line_and_the_cards_and_no_more_lines_are_taken() {
    let dir = Scratch::new();
    let mut rig = fresh_rig(dir.path());
    rig.node
        .take_starter_line(&line(1, T0, 300, 200, true, &[payment(1, T0)]).to_bytes())
        .unwrap();
    assert!(path(dir.path(), STARTER_LINE_FILE).exists());
    assert!(path(dir.path(), STARTER_CARDS_FILE).exists());
    assert_eq!(rig.node.starter_decision(), StopDecision::Stopped);
    rig.node.take_switch(&good().to_bytes()).unwrap();
    // While the switch waits, the line stands.
    assert_eq!(rig.node.starter_decision(), StopDecision::Stopped);
    rig.node.switch_same().unwrap();
    assert!(rig.node.starter_line().is_none());
    assert!(rig.node.starter_payments().is_empty());
    assert_eq!(rig.node.starter_decision(), StopDecision::Open);
    assert!(!path(dir.path(), STARTER_LINE_FILE).exists());
    assert!(!path(dir.path(), STARTER_CARDS_FILE).exists());
    assert_eq!(
        rig.node
            .take_starter_line(&line(2, T0, 300, 200, true, &[payment(2, T0)]).to_bytes()),
        Err(StarterRefused::NotStarter)
    );
    assert!(!path(dir.path(), STARTER_LINE_FILE).exists());
    let _ = switch_path(dir.path());
}

#[test]
fn cr_275_a_start_after_the_switch_drops_what_a_stop_left() {
    let dir = Scratch::new();
    {
        let mut rig = fresh_rig(dir.path());
        rig.node.take_switch(&good().to_bytes()).unwrap();
        rig.node.switch_same().unwrap();
    }
    // A line written before the stop that the switch could not remove.
    std::fs::write(
        path(dir.path(), STARTER_LINE_FILE),
        line(1, T0, 300, 200, true, &[]).to_bytes(),
    )
    .unwrap();
    std::fs::write(
        path(dir.path(), STARTER_CARDS_FILE),
        Cards::default().to_bytes(),
    )
    .unwrap();
    // The record the app kept after the switch has no starter flag.
    let mut kept = record();
    kept.wallet = new_wallet();
    let rig = rig_for(dir.path(), &kept);
    assert!(rig.node.starter_line().is_none());
    assert!(!path(dir.path(), STARTER_LINE_FILE).exists());
    assert!(!path(dir.path(), STARTER_CARDS_FILE).exists());
}

// CR-276.

#[test]
fn cr_276_every_state_and_refusal_has_written_words_that_pass_the_checks() {
    let mut seen = Vec::new();
    for (name, e) in every_starter_refused() {
        let w = e.words();
        assert!(!is_placeholder(&w), "{name}");
        assert_eq!(openbmt_core::say::check(&w), Ok(()), "{name}");
        assert_eq!(w.code, Some(e.rule()), "{name}");
        seen.push((w.heading.clone(), w.what.clone()));
    }
    for w in [
        crate::say::starter_stopped_words(),
        crate::say::starter_unchecked_words(),
        crate::say::starter_cards_words(),
    ] {
        assert!(!is_placeholder(&w), "{}", w.heading);
        assert_eq!(openbmt_core::say::check(&w), Ok(()), "{}", w.heading);
        seen.push((w.heading.clone(), w.what.clone()));
    }
    let n = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), n, "no two starter words read the same");
    let names: Vec<String> = all_words().into_iter().map(|(n, _)| n).collect();
    for name in ["StarterStopped", "StarterUnchecked", "StarterCards"] {
        assert!(names.iter().any(|n| n == name), "{name}");
    }
}
