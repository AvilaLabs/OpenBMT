//! BX-40 to BX-43 against the POS side without a socket: files in memory,
//! a connector the test controls and a clock the test sets.

use openbmt_pos::{
    AlertKind, CloseOutcome, IdempotencyKey, NoticeState, PosError, PosItem, SaleMismatch,
    MAX_SALES,
};

use super::kit::*;
use super::pos_kit::*;
use crate::files::{BoxFiles, MemFiles};
use crate::pos::{Reading, LEDGER_FILE, MAX_OUT, NOTICES_FILE, POLL_SECS, PRUNE_CHUNK};
use crate::state::{BoxError, POS_TOKEN_FILE};

fn c1() -> openbmt_core::identity::Identity {
    counter1()
}

fn sale_key(index: u32) -> openbmt_pos::SaleKey {
    openbmt_pos::SaleKey {
        counter: c1().device_id(),
        index,
    }
}

// ---- BX-40: the notices ----

#[test]
fn bx_40_a_notice_from_a_counter_is_taken_and_gives_the_sale_fact() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let items = [
        item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500)),
        item(&c1(), 2, &draft(0, NoticeState::Paid, NOW + 30, 500)),
    ];
    let notes = p.take_notices(&items, &roles(), NOW + 40);
    assert!(notes.is_empty(), "{notes:?}");
    let sales = p.sales();
    assert_eq!(sales.len(), 1);
    assert_eq!(
        (sales[0].index, sales[0].amount.0, sales[0].paid_at),
        (0, 500, Some(NOW + 30))
    );
    assert!(sales[0].is_paid_now());
    assert_eq!(p.last_number(&c1().device_id()), 2);
    assert_eq!(p.state().notices_taken, 2);
}

#[test]
fn bx_40_a_notice_signed_by_a_key_that_is_not_a_counter_is_refused() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let d = draft(0, NoticeState::Paid, NOW, 500);
    // A stranger, the owner's key and the Box's own key, each signing and
    // sending its own notice.
    let items = [
        item(&stranger(), 1, &d),
        item(&owner(), 1, &d),
        item(&the_box(), 1, &d),
    ];
    let notes = p.take_notices(&items, &roles(), NOW);
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(
        notes.iter().all(|n| n.contains("not a counter")),
        "{notes:?}"
    );
    assert!(p.sales().is_empty());
    assert_eq!(p.state().notices_refused, 3);
}

#[test]
fn bx_40_a_notice_that_does_not_check_or_names_another_counter_is_refused() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let d = draft(0, NoticeState::Paid, NOW, 500);
    // Signed by counter 1, sent by counter 2: not the sender's.
    let by_one = notice(&c1(), 1, &d).to_bytes();
    let stolen = (counter2().device_id(), by_one.clone());
    // A signature that does not check: the amount is changed after signing.
    let mut forged = by_one.clone();
    forged[5 + 32 + 8 + 4] ^= 1;
    let forged = (c1().device_id(), forged);
    // Bytes that are not a notice, and one a byte too long.
    let junk = (c1().device_id(), b"junk".to_vec());
    let mut long = by_one;
    long.push(0);
    let long = (c1().device_id(), long);
    let notes = p.take_notices(&[stolen, forged, junk, long], &roles(), NOW);
    assert_eq!(notes.len(), 4, "{notes:?}");
    assert!(notes[0].contains("another counter"), "{notes:?}");
    assert!(notes[1].contains("signature"), "{notes:?}");
    assert!(notes[2].contains("does not read"), "{notes:?}");
    assert!(p.sales().is_empty());
}

#[test]
fn bx_40_a_replayed_notice_and_an_older_number_are_refused() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let first = item(&c1(), 5, &draft(0, NoticeState::Opened, NOW, 500));
    assert!(p
        .take_notices(std::slice::from_ref(&first), &roles(), NOW)
        .is_empty());
    // The same notice again, and one with a lower number.
    let older = item(&c1(), 4, &draft(1, NoticeState::Opened, NOW, 700));
    let notes = p.take_notices(&[first, older], &roles(), NOW);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes.iter().all(|n| n.contains("not above 5")), "{notes:?}");
    assert_eq!(p.sales().len(), 1);
    // A higher number is taken; a gap is allowed.
    let next = item(&c1(), 9, &draft(1, NoticeState::Opened, NOW, 700));
    assert!(p.take_notices(&[next], &roles(), NOW).is_empty());
    assert_eq!(p.last_number(&c1().device_id()), 9);
    // Numbers are kept for each counter.
    let other = item(&counter2(), 1, &draft(0, NoticeState::Opened, NOW, 100));
    assert!(p.take_notices(&[other], &roles(), NOW).is_empty());
}

#[test]
fn bx_40_a_notice_that_changes_a_sale_is_refused_and_noted() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let ok = item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500));
    assert!(p.take_notices(&[ok], &roles(), NOW).is_empty());
    // The amount changed.
    let amount = item(&c1(), 2, &draft(0, NoticeState::Paid, NOW + 5, 900));
    // The opening time changed.
    let mut moved = draft(0, NoticeState::Paid, NOW + 5, 500);
    moved.opened_at += 1;
    let moved = item(&c1(), 3, &moved);
    // The currency changed.
    let mut eur = draft(0, NoticeState::Paid, NOW + 5, 500);
    eur.currency = openbmt_counter::Currency::new(*b"EUR").unwrap();
    let eur = item(&c1(), 4, &eur);
    let notes = p.take_notices(&[amount, moved, eur], &roles(), NOW + 6);
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(
        notes[0].contains(&SaleMismatch::Amount.to_string()),
        "{notes:?}"
    );
    assert!(notes[1].contains(&SaleMismatch::OpenedAt.to_string()));
    assert!(notes[2].contains(&SaleMismatch::Currency.to_string()));
    // The sale is as it was, and the refused numbers were not used up.
    let sales = p.sales();
    assert_eq!((sales[0].amount.0, sales[0].paid_at), (500, None));
    assert_eq!(p.last_number(&c1().device_id()), 1);
}

#[test]
fn bx_40_a_counter_removed_by_new_settings_is_refused_after_that() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let d = draft(0, NoticeState::Opened, NOW, 500);
    assert!(p
        .take_notices(&[item(&counter2(), 1, &d)], &roles(), NOW)
        .is_empty());
    // New settings take counter 2 off the list: the roles no longer hold it.
    let mut smaller = roles();
    smaller.remove(&counter2().device_id());
    let notes = p.take_notices(&[item(&counter2(), 2, &d)], &smaller, NOW + 1);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("not a counter"));
    // A key the settings now give another role is refused too.
    let mut changed = roles();
    changed.insert(c1().device_id(), openbmt_core::settings::DeviceRole::Box);
    let notes = p.take_notices(&[item(&c1(), 1, &d)], &changed, NOW + 1);
    assert_eq!(notes.len(), 1);
}

#[test]
fn bx_40_the_last_number_is_stored_before_the_notice_takes_effect() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let d = draft(0, NoticeState::Opened, NOW, 500);
    p.files_mut().fail_writes_from(1);
    let notes = p.take_notices(&[item(&c1(), 1, &d)], &roles(), NOW);
    assert!(notes.iter().any(|n| n.contains("not stored")), "{notes:?}");
    // The write failed, so nothing took effect: no sale and no number.
    assert!(p.sales().is_empty());
    assert_eq!(p.last_number(&c1().device_id()), 0);
    // With the disk back the same notice is taken, and a restart holds it.
    p.files_mut().stop_failing();
    assert!(p
        .take_notices(&[item(&c1(), 1, &d)], &roles(), NOW)
        .is_empty());
    let files = p.files().clone();
    let again = pos_over(files, &conn);
    assert_eq!(again.last_number(&c1().device_id()), 1);
    assert_eq!(again.sales().len(), 1);
    // And a notice replayed to the restarted Box is refused.
    let mut again = again;
    let notes = again.take_notices(&[item(&c1(), 1, &d)], &roles(), NOW);
    assert_eq!(notes.len(), 1);
}

#[test]
fn bx_40_ten_thousand_notices_are_held_within_the_bound() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    // Ten thousand sales, each opened and then final, in batches of what one
    // poll of the mailbox gives.
    let mut number = 0;
    for batch in 0..10u32 {
        let mut items = Vec::new();
        for i in 0..1_000u32 {
            let index = batch * 1_000 + i;
            number += 1;
            items.push(item(
                &c1(),
                number,
                &draft(index, NoticeState::Final, NOW, 500),
            ));
        }
        let notes = p.take_notices(&items, &roles(), NOW);
        assert!(notes.is_empty(), "{notes:?}");
    }
    assert_eq!(p.sales().len(), 10_000);
    assert!(p.sales().len() <= MAX_SALES);
    // The file is about what the bound says: under 2 MB for the most sales
    // it will ever hold.
    let size = p.files().read(NOTICES_FILE).unwrap().unwrap().len();
    assert!(size < 2 * 1024 * 1024, "{size}");
}

#[test]
fn bx_40_a_full_book_drops_the_oldest_final_sales_and_never_a_live_one() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    // The book is full of sales that were final long ago.
    let mut number = 0;
    for chunk in (0..MAX_SALES as u32).collect::<Vec<_>>().chunks(1_000) {
        let items: Vec<_> = chunk
            .iter()
            .map(|index| {
                number += 1;
                // Older sales have lower indexes, so they have lower times.
                item(
                    &c1(),
                    number,
                    &draft(*index, NoticeState::Final, NOW + u64::from(*index), 500),
                )
            })
            .collect();
        assert!(p.take_notices(&items, &roles(), NOW + 100_000).is_empty());
    }
    assert_eq!(p.sales().len(), MAX_SALES);
    // A new sale an hour later makes room: the oldest chunk goes.
    let new = item(
        &c1(),
        number + 1,
        &draft(MAX_SALES as u32, NoticeState::Opened, NOW + 90_000, 500),
    );
    let notes = p.take_notices(&[new], &roles(), NOW + 100_000);
    assert!(notes.is_empty(), "{notes:?}");
    let sales = p.sales();
    assert_eq!(sales.len(), MAX_SALES - PRUNE_CHUNK + 1);
    assert!(sales.iter().all(|s| s.index >= PRUNE_CHUNK as u32));
    assert!(sales.iter().any(|s| s.index == MAX_SALES as u32));

    // A book full of sales that are not final cannot make room: the notice
    // is refused and nothing is dropped.
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    let mut number = 0;
    for chunk in (0..MAX_SALES as u32).collect::<Vec<_>>().chunks(1_000) {
        let items: Vec<_> = chunk
            .iter()
            .map(|index| {
                number += 1;
                item(&c1(), number, &draft(*index, NoticeState::Paid, NOW, 500))
            })
            .collect();
        assert!(p.take_notices(&items, &roles(), NOW + 100_000).is_empty());
    }
    let extra = item(
        &c1(),
        number + 1,
        &draft(MAX_SALES as u32, NoticeState::Opened, NOW, 500),
    );
    let notes = p.take_notices(&[extra], &roles(), NOW + 100_000);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("PS-72"), "{notes:?}");
    assert_eq!(p.sales().len(), MAX_SALES);
}

#[test]
fn bx_40_the_pos_is_read_on_a_timer_and_at_once_after_a_notice() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    assert!(p.tick(NOW, token()).is_empty());
    assert_eq!(conn.probe().polls.len(), 1);
    // Not again before the interval.
    p.tick(NOW + POLL_SECS - 1, token());
    assert_eq!(conn.probe().polls.len(), 1);
    p.tick(NOW + POLL_SECS, token());
    assert_eq!(conn.probe().polls.len(), 2);
    // A notice makes the next tick read at once.
    let d = draft(0, NoticeState::Opened, NOW, 500);
    p.take_notices(&[item(&c1(), 1, &d)], &roles(), NOW + POLL_SECS + 1);
    p.tick(NOW + POLL_SECS + 1, token());
    assert_eq!(conn.probe().polls.len(), 3);
    // A refused notice does not.
    p.take_notices(&[item(&c1(), 1, &d)], &roles(), NOW + POLL_SECS + 2);
    p.tick(NOW + POLL_SECS + 2, token());
    assert_eq!(conn.probe().polls.len(), 3);
    assert_eq!(p.state().last_poll_at, Some(NOW + POLL_SECS + 1));
}

#[test]
fn bx_40_the_matching_runs_on_the_facts_the_notices_gave() {
    let conn = TestConnector::default();
    conn.probe()
        .pages
        .push_back(Ok(page(vec![order("o1", 500, NOW + 60)])));
    let mut p = pos(&conn);
    p.take_notices(
        &[
            item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500)),
            item(&c1(), 2, &draft(0, NoticeState::Paid, NOW + 30, 500)),
        ],
        &roles(),
        NOW + 61,
    );
    assert!(p.tick(NOW + 61, token()).is_empty());
    assert_eq!(p.ledger().matches().len(), 1);
    let m = p.ledger().matches().values().next().unwrap();
    assert_eq!(m.sale, sale_key(0));
}

// ---- BX-41: the token ----

#[test]
fn bx_41_the_token_is_read_only_from_a_file_only_the_boxs_user_can_read() {
    let (mut core, _) = paired_core();
    assert_eq!(core.read_pos_token(), Ok(None));
    core.store_pos_token("  sk_live_abc \n").unwrap();
    assert_eq!(core.read_pos_token(), Ok(Some("sk_live_abc".to_string())));
    for mode in [0o644, 0o640, 0o604, 0o660, 0o666, 0o601] {
        core.files_mut().set_mode(POS_TOKEN_FILE, mode);
        assert_eq!(
            core.read_pos_token(),
            Err(BoxError::TokenMode { mode }),
            "{mode:o}"
        );
    }
    for mode in [0o600, 0o400] {
        core.files_mut().set_mode(POS_TOKEN_FILE, mode);
        assert!(core.read_pos_token().unwrap().is_some(), "{mode:o}");
    }
    // The error names the file and the way out.
    core.files_mut().set_mode(POS_TOKEN_FILE, 0o644);
    let text = core.read_pos_token().unwrap_err().to_string();
    assert!(text.contains("pos.token") && text.contains("0644") && text.contains("0600"));
}

#[test]
fn bx_41_what_is_not_a_token_is_not_stored_and_not_read() {
    let (mut core, _) = paired_core();
    for bad in ["", "   ", "two\nlines", "tab\there", &"x".repeat(1_025)] {
        assert!(
            matches!(core.store_pos_token(bad), Err(BoxError::BadToken(_))),
            "{bad:?}"
        );
    }
    assert!(core.files().read(POS_TOKEN_FILE).unwrap().is_none());
    // A file put there by hand that is not a token is refused when read.
    core.files_mut().put(POS_TOKEN_FILE, b"one\ntwo");
    assert!(matches!(core.read_pos_token(), Err(BoxError::BadToken(_))));
    core.files_mut().put(POS_TOKEN_FILE, &[0xff, 0xfe]);
    assert!(matches!(core.read_pos_token(), Err(BoxError::BadToken(_))));
}

#[test]
fn bx_41_an_unpaired_or_turned_off_box_takes_and_gives_no_token() {
    let mut core = fresh_core();
    assert_eq!(core.store_pos_token("t"), Err(BoxError::NotPaired));
    assert_eq!(core.read_pos_token(), Ok(None));
    let (mut core, _) = paired_core();
    core.store_pos_token("t").unwrap();
    let mut s = settings_with(2, &core.device_id());
    s.devices.remove(&core.device_id());
    s.boxes.clear();
    core.take_settings(&sign_owner(s).to_bytes()).unwrap();
    // BX-23 erased it, and none is taken or read now.
    assert!(core.files().read(POS_TOKEN_FILE).unwrap().is_none());
    assert_eq!(core.store_pos_token("t"), Err(BoxError::TurnedOff));
    assert_eq!(core.read_pos_token(), Ok(None));
}

#[test]
fn bx_41_without_a_usable_token_nothing_is_read_and_the_token_is_never_shown() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    p.tick(NOW, Ok(None));
    assert_eq!(p.state().reading, Reading::NoToken);
    p.tick(
        NOW + 100,
        Err("BX-41: pos.token can be read by others".into()),
    );
    assert!(matches!(p.state().reading, Reading::TokenRefused(_)));
    assert!(conn.probe().polls.is_empty());
    // With a token, it is given to the connector and the POS is read.
    let notes = p.tick(NOW + 200, token());
    assert_eq!(p.state().reading, Reading::Reading);
    assert_eq!(conn.probe().polls.len(), 1);
    assert_eq!(
        conn.probe().tokens.last().cloned().flatten().as_deref(),
        Some("sk_test_token")
    );
    // Nothing the Box says about itself holds the token.
    let shown = format!("{}{notes:?}", p.state());
    assert!(!shown.contains("sk_test_token"), "{shown}");
    // A token that goes away is taken from the connector again.
    p.tick(NOW + 300, Ok(None));
    assert_eq!(conn.probe().tokens.last().cloned().flatten(), None);
}

// ---- BX-42: what goes to the owner ----

#[test]
fn bx_42_a_match_and_an_alert_are_queued_as_numbered_signed_pos_notices() {
    let conn = TestConnector::default();
    {
        let mut c = conn.probe();
        c.pages.push_back(Ok(page(vec![
            order("o1", 500, NOW + 60),
            order("o2", 700, NOW + 60),
        ])));
    }
    let mut p = pos(&conn);
    p.take_notices(
        &[
            item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500)),
            item(&c1(), 2, &draft(0, NoticeState::Paid, NOW + 30, 500)),
        ],
        &roles(),
        NOW + 61,
    );
    p.tick(NOW + 61, token());
    // o1 matches sale 0. o2 has no sale: its alert comes after ten minutes.
    let first = p.pending_reports();
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0].item, PosItem::Match { .. }));
    p.tick(NOW + 61 + POLL_SECS + 600, token());
    let all = p.pending_reports();
    assert_eq!(all.len(), 2);
    assert!(
        matches!(&all[1].item, PosItem::Alert(a) if a.key.kind == AlertKind::TenderWithoutPayment)
    );
    // Numbered from 1 without a gap, signed by the Box.
    assert_eq!(all.iter().map(|n| n.number).collect::<Vec<_>>(), vec![1, 2]);
    for n in &all {
        assert!(n.verify());
        assert_eq!(n.from, the_box().device_id());
        let back = openbmt_pos::PosNotice::from_bytes(&n.to_bytes()).unwrap();
        assert_eq!(&back, n);
    }
    // The relay took the first: it is forgotten, and stored so.
    p.reports_sent(1).unwrap();
    assert_eq!(p.pending_reports().len(), 1);
    let again = pos_over(p.files().clone(), &conn);
    assert_eq!(again.pending_reports().len(), 1);
    assert_eq!(again.out_number(), 2);
}

#[test]
fn bx_42_a_poll_that_finds_nothing_new_reports_nothing() {
    let conn = TestConnector::default();
    conn.probe()
        .pages
        .push_back(Ok(page(vec![order("o1", 500, NOW + 60)])));
    let mut p = pos(&conn);
    p.take_notices(
        &[
            item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500)),
            item(&c1(), 2, &draft(0, NoticeState::Paid, NOW + 30, 500)),
        ],
        &roles(),
        NOW + 61,
    );
    p.tick(NOW + 61, token());
    assert_eq!(p.pending_reports().len(), 1);
    p.tick(NOW + 100, token());
    p.tick(NOW + 200, token());
    assert_eq!(p.pending_reports().len(), 1);
}

#[test]
fn bx_42_reports_waiting_for_the_relay_are_bounded_and_the_oldest_go_first() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    // Close more orders than the queue holds, each telling the owner.
    {
        let mut c = conn.probe();
        c.can_close = true;
        c.order = Some(openbmt_pos::Id::new("o1").unwrap());
    }
    let mut number = 0;
    for chunk in (0..(MAX_OUT as u32 + 20)).collect::<Vec<_>>().chunks(16) {
        let items: Vec<_> = chunk
            .iter()
            .map(|i| {
                number += 1;
                item(&c1(), number, &draft(*i, NoticeState::Paid, NOW, 500))
            })
            .collect();
        p.take_notices(&items, &roles(), NOW);
        p.tick(NOW + number * POLL_SECS, token());
    }
    let waiting = p.pending_reports();
    assert!(waiting.len() <= MAX_OUT);
    assert_eq!(waiting.len(), MAX_OUT);
    assert!(waiting.windows(2).all(|w| w[0].number + 1 == w[1].number));
    assert!(waiting[0].number > 1);
}

#[test]
fn bx_42_the_connector_failing_timing_out_or_answering_with_garbage_changes_nothing() {
    let conn = TestConnector::default();
    conn.probe()
        .pages
        .push_back(Ok(page(vec![order("o1", 500, NOW + 60)])));
    let mut p = pos(&conn);
    p.take_notices(
        &[
            item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500)),
            item(&c1(), 2, &draft(0, NoticeState::Paid, NOW + 30, 500)),
        ],
        &roles(),
        NOW + 61,
    );
    p.tick(NOW + 61, token());
    let ledger = p.files().read(LEDGER_FILE).unwrap().unwrap();
    let held = p.ledger().clone();
    let mut now = NOW + 61;
    for failure in [
        PosError::Unavailable,
        PosError::BadResponse("not an order"),
        PosError::SignInRefused,
        PosError::Unsupported,
    ] {
        now += POLL_SECS;
        conn.probe().pages.push_back(Err(failure.clone()));
        let notes = p.tick(now, token());
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("not read"), "{notes:?}");
        assert_eq!(p.state().last_error.as_deref(), Some(notes[0].as_str()));
        assert_eq!(p.ledger(), &held);
        assert_eq!(p.files().read(LEDGER_FILE).unwrap().unwrap(), ledger);
    }
    // A page that says there is more and does not move on is refused as
    // the rules say, and so is one that is too large.
    now += POLL_SECS;
    conn.probe().pages.push_back(Ok(openbmt_pos::Page {
        orders: Vec::new(),
        next: p.ledger().cursor().clone(),
        more: true,
    }));
    let notes = p.tick(now, token());
    assert!(notes[0].contains("PS-80"), "{notes:?}");
    // The next good reading goes on from where it was.
    now += POLL_SECS;
    assert!(p.tick(now, token()).is_empty());
    assert_eq!(p.state().last_error, None);
}

#[test]
fn bx_42_a_pos_that_asks_to_wait_is_not_asked_again_before_then() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    conn.probe().pages.push_back(Err(PosError::RateLimited {
        retry_after_secs: 300,
    }));
    p.tick(NOW, token());
    assert_eq!(conn.probe().polls.len(), 1);
    p.tick(NOW + 299, token());
    assert_eq!(conn.probe().polls.len(), 1);
    p.tick(NOW + 300, token());
    assert_eq!(conn.probe().polls.len(), 2);
}

#[test]
fn bx_42_damaged_pos_files_are_not_read_over_and_not_replaced() {
    let conn = TestConnector::default();
    let mut files = MemFiles::new();
    files.put(NOTICES_FILE, b"junk");
    let setup = crate::pos::PosSetup::new(pos_settings(), Box::new(conn.clone()));
    let err = crate::pos::BoxPos::open(files, the_box(), setup)
        .err()
        .unwrap();
    assert!(err.to_string().contains("pos-notices"), "{err}");
    let mut files = MemFiles::new();
    files.put(LEDGER_FILE, b"junk");
    let setup = crate::pos::PosSetup::new(pos_settings(), Box::new(conn));
    let err = crate::pos::BoxPos::open(files, the_box(), setup)
        .err()
        .unwrap();
    assert!(err.to_string().contains("pos-ledger"), "{err}");
}

// ---- BX-43: closing an order ----

/// Notices that open a sale of 500 and report it paid.
fn paid_sale(p: &mut crate::pos::BoxPos<MemFiles>, number: u64, index: u32) {
    let items = [
        item(&c1(), number, &draft(index, NoticeState::Opened, NOW, 500)),
        item(
            &c1(),
            number + 1,
            &draft(index, NoticeState::Paid, NOW + 10, 500),
        ),
    ];
    assert!(p.take_notices(&items, &roles(), NOW + 11).is_empty());
}

#[test]
fn bx_43_an_order_is_closed_once_for_a_sale_reported_paid_with_the_key_of_ps_81() {
    let conn = TestConnector::closing("o9");
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    p.tick(NOW + 12, token());
    {
        let c = conn.probe();
        assert_eq!(c.closes.len(), 1);
        assert_eq!(c.closes[0].order.as_str(), "o9");
        assert_eq!(c.closes[0].sale, sale_key(0));
        assert_eq!(c.closes[0].amount_minor, 500);
        assert_eq!(
            c.closes[0].key,
            IdempotencyKey::for_sale(&c1().device_id(), 0)
        );
    }
    // Asked again, however often, the connector is not called again.
    for i in 1..5 {
        p.tick(NOW + 12 + i * POLL_SECS, token());
    }
    assert_eq!(conn.probe().closes.len(), 1);
    assert_eq!(conn.probe().find_calls, 1);
    // The owner is told once.
    let told: Vec<_> = p
        .pending_reports()
        .into_iter()
        .filter(|n| matches!(n.item, PosItem::Closed { .. }))
        .collect();
    assert_eq!(told.len(), 1);
    // And not again after a restart.
    let mut again = pos_over(p.files().clone(), &conn);
    again.tick(NOW + 500, token());
    assert_eq!(conn.probe().closes.len(), 1);
}

#[test]
fn bx_43_a_stop_between_the_attempt_and_the_result_is_retried_with_the_same_key() {
    let conn = TestConnector::closing("o9");
    // The connector fails the first call, as a stop in the middle would
    // leave it: the attempt is stored and the result is not.
    conn.probe()
        .close_script
        .push_back(Err(PosError::Unavailable));
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    let notes = p.tick(NOW + 12, token());
    assert!(notes.iter().any(|n| n.contains("not closed")), "{notes:?}");
    assert_eq!(conn.probe().closes.len(), 1);
    assert!(!p
        .pending_reports()
        .iter()
        .any(|n| matches!(n.item, PosItem::Closed { .. })));
    // The Box restarts. The connector had closed it after all: the answer is
    // that it was already closed.
    conn.probe()
        .close_script
        .push_back(Ok(CloseOutcome::AlreadyClosed));
    let mut again = pos_over(p.files().clone(), &conn);
    let notes = again.tick(NOW + 100, token());
    assert!(
        notes.iter().any(|n| n.contains("already closed")),
        "{notes:?}"
    );
    let c = conn.probe();
    assert_eq!(c.closes.len(), 2);
    assert_eq!(c.closes[0].key, c.closes[1].key);
    assert_eq!(c.closes[0].order, c.closes[1].order);
    // The order was found once: the retry names the order that was stored.
    assert_eq!(c.find_calls, 1);
}

#[test]
fn bx_43_a_close_that_could_not_be_recorded_is_not_made() {
    let conn = TestConnector::closing("o9");
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    p.files_mut().fail_writes_from(1);
    let notes = p.tick(NOW + 12, token());
    assert!(
        notes.iter().any(|n| n.contains("not recorded")),
        "{notes:?}"
    );
    assert!(conn.probe().closes.is_empty());
}

#[test]
fn bx_43_with_no_closing_connector_nothing_is_closed() {
    let conn = TestConnector::default();
    conn.probe().order = Some(openbmt_pos::Id::new("o9").unwrap());
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    p.tick(NOW + 12, token());
    {
        let c = conn.probe();
        assert!(c.closes.is_empty());
        assert_eq!(c.find_calls, 0);
    }
    assert!(!p.state().can_close);
    // Nor does the default connector, which refuses a close.
    struct Reader;
    impl openbmt_pos::PosReader for Reader {
        fn poll(&mut self, c: &openbmt_pos::Cursor) -> Result<openbmt_pos::Page, PosError> {
            Ok(openbmt_pos::Page {
                orders: Vec::new(),
                next: c.clone(),
                more: false,
            })
        }
    }
    impl openbmt_pos::Connector for Reader {}
    let mut r = Reader;
    assert!(!openbmt_pos::Connector::can_close(&r));
    let sale = openbmt_pos::SaleFact::opened(
        c1().device_id(),
        0,
        openbmt_counter::Amount(500),
        usd(),
        NOW,
    );
    let mut paid = sale;
    paid.paid_at = Some(NOW);
    let req = openbmt_pos::CloseRequest::for_sale(
        &paid,
        openbmt_pos::Id::new("o").unwrap(),
        openbmt_pos::Text::new("Bitcoin").unwrap(),
    )
    .unwrap();
    assert_eq!(
        openbmt_pos::Connector::close(&mut r, &req),
        Err(PosError::Unsupported)
    );
}

#[test]
fn bx_43_a_sale_that_was_never_paid_or_is_unpaid_again_is_not_closed() {
    let conn = TestConnector::closing("o9");
    let mut p = pos(&conn);
    // Sale 0 is only open. Sale 1 was paid and then reported unpaid again.
    let items = [
        item(&c1(), 1, &draft(0, NoticeState::Opened, NOW, 500)),
        item(&c1(), 2, &draft(1, NoticeState::Opened, NOW, 500)),
        item(&c1(), 3, &draft(1, NoticeState::Paid, NOW + 5, 500)),
        item(&c1(), 4, &draft(1, NoticeState::Unpaid, NOW + 6, 500)),
    ];
    assert!(p.take_notices(&items, &roles(), NOW + 7).is_empty());
    p.tick(NOW + 8, token());
    // Refused before the connector is asked anything.
    let c = conn.probe();
    assert!(c.closes.is_empty());
    assert_eq!(c.find_calls, 0);
}

#[test]
fn bx_43_a_sale_that_is_final_is_closed_too() {
    let conn = TestConnector::closing("o9");
    let mut p = pos(&conn);
    let items = [item(&c1(), 1, &draft(0, NoticeState::Final, NOW + 5, 500))];
    assert!(p.take_notices(&items, &roles(), NOW + 7).is_empty());
    p.tick(NOW + 8, token());
    assert_eq!(conn.probe().closes.len(), 1);
}

#[test]
fn bx_43_a_sale_that_goes_unpaid_again_after_a_close_tells_the_owner_and_undoes_nothing() {
    let conn = TestConnector::closing("o9");
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    p.tick(NOW + 12, token());
    assert_eq!(conn.probe().closes.len(), 1);
    // The counter reports the sale unpaid again.
    assert!(p
        .take_notices(
            &[item(
                &c1(),
                3,
                &draft(0, NoticeState::Unpaid, NOW + 20, 500)
            )],
            &roles(),
            NOW + 21
        )
        .is_empty());
    p.tick(NOW + 22, token());
    p.tick(NOW + 22 + POLL_SECS, token());
    let alerts: Vec<_> = p
        .pending_reports()
        .into_iter()
        .filter(|n| matches!(n.item, PosItem::UnpaidAfterClose { .. }))
        .collect();
    assert_eq!(alerts.len(), 1, "told once");
    // Nothing was undone and nothing was closed again, even when the sale
    // is paid once more.
    assert!(p
        .take_notices(
            &[item(&c1(), 4, &draft(0, NoticeState::Paid, NOW + 10, 500))],
            &roles(),
            NOW + 40
        )
        .is_empty());
    p.tick(NOW + 41 + POLL_SECS, token());
    assert_eq!(conn.probe().closes.len(), 1);
    assert_eq!(
        p.pending_reports()
            .iter()
            .filter(|n| matches!(n.item, PosItem::UnpaidAfterClose { .. }))
            .count(),
        1
    );
}

#[test]
fn bx_43_no_order_to_name_means_no_close_and_the_sale_is_tried_again_later() {
    let conn = TestConnector::closing("o9");
    conn.probe().order = None;
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    p.tick(NOW + 12, token());
    assert!(conn.probe().closes.is_empty());
    assert_eq!(conn.probe().find_calls, 1);
    // The cashier rings the order: the next reading finds it.
    conn.probe().order = Some(openbmt_pos::Id::new("o9").unwrap());
    p.tick(NOW + 12 + POLL_SECS, token());
    assert_eq!(conn.probe().closes.len(), 1);
}

#[test]
fn bx_43_a_stop_clears_the_token_from_the_connector() {
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    p.tick(NOW, token());
    p.stop();
    assert_eq!(conn.probe().tokens.last().cloned().flatten(), None);
    assert_eq!(p.state().reading, Reading::Stopped);
    // Stopping twice says nothing more to the connector.
    let n = conn.probe().tokens.len();
    p.stop();
    assert_eq!(conn.probe().tokens.len(), n);
}

#[test]
fn bx_40_a_notice_is_the_only_thing_stored_about_a_sale_and_it_holds_no_satoshis() {
    // What the Box keeps about a sale is the fact of PS-05, and nothing in a
    // fact is an amount in satoshis, a rate, an address or a transaction ID.
    let conn = TestConnector::default();
    let mut p = pos(&conn);
    paid_sale(&mut p, 1, 0);
    let stored = p.files().read(NOTICES_FILE).unwrap().unwrap();
    // One counter, one sale, no closes, no reports: the size says it.
    // 5 header + 8 number + 4 + (32 + 8) + 4 + (32 + 4 + 8 + 3 + 8 + 9 + 2)
    // + 4 + 4.
    assert_eq!(stored.len(), 5 + 8 + 4 + 40 + 4 + 66 + 4 + 4);
}
