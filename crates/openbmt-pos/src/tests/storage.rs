//! PS-70 to PS-74: storage, bounds, retirement and replay.

use std::convert::Infallible;

use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::ledger::{MAGIC_POS_LEDGER, TAG_LEDGER};
use crate::reference::tagged_hash;
use crate::*;

const DAY: u64 = 86_400;

/// A ledger with orders, matches, open and acknowledged alerts, a cursor and
/// a retired count.
pub fn rich() -> Ledger {
    let s1 = paid(sale(3, 1, 500, T0), T0 + 60);
    let s2 = paid(sale(3, 2, 800, T0), T0 + 60);
    let mut tipped = tender("t", 800, T0 + 90);
    tipped.tip_minor = 40;
    tipped.reference = Some(text("btc note"));
    let orders = [
        simple("a", "Main", 500, T0 + 90),
        order("b", "Main", vec![tipped, cash("c", 100, T0 + 90)]),
        simple("lost", "Main", 123, T0 + 90),
        simple("far", "Elsewhere", 5, T0 + 90),
    ];
    let mut l = Ledger::new();
    let cursor = Cursor::new(vec![7, 8, 9]).unwrap();
    l.update_stored(&orders, cursor, &[s1, s2], &settings(), T0 + 2_000, |_| {
        Ok::<(), Infallible>(())
    })
    .unwrap();
    let key = l.open_alerts()[0].key.clone();
    l.acknowledge_stored(&key, T0 + 2_100, |_| Ok::<(), Infallible>(()))
        .unwrap();
    assert!(l.summary().open_alerts >= 2 && l.summary().acknowledged_alerts == 1);
    assert!(l.summary().matches == 2);
    l
}

#[test]
fn ps_70_a_failed_store_leaves_the_ledger_and_the_cursor_as_they_were() {
    let mut l = Ledger::new();
    let before = l.clone();
    let r = l.update_stored(
        &[simple("o", "Main", 500, T0)],
        Cursor::new(vec![1]).unwrap(),
        &[],
        &settings(),
        T0 + 700,
        |_| Err::<(), _>("disk full"),
    );
    assert_eq!(r, Err(LedgerWriteError::Persist("disk full")));
    assert_eq!(l, before);
    assert!(l.cursor().as_bytes().is_empty());
}

#[test]
fn ps_70_what_is_stored_is_the_value_that_is_swapped_in() {
    let mut l = Ledger::new();
    let mut stored: Option<Vec<u8>> = None;
    l.update_stored(
        &[simple("o", "Main", 500, T0)],
        Cursor::new(vec![1, 2]).unwrap(),
        &[],
        &settings(),
        T0 + 700,
        |bytes| {
            stored = Some(bytes.to_vec());
            Ok::<(), Infallible>(())
        },
    )
    .unwrap();
    assert_eq!(Ledger::from_bytes(&stored.unwrap()).unwrap(), l);
    assert_eq!(l.cursor().as_bytes(), &[1, 2]);
}

#[test]
fn ps_70_a_refused_change_stores_nothing() {
    let mut l = Ledger::new();
    let mut called = false;
    let mut s = settings();
    s.timing.grace_secs = 0;
    let r = l.update_stored(&[], Cursor::start(), &[], &s, T0, |_| {
        called = true;
        Ok::<(), Infallible>(())
    });
    assert_eq!(
        r,
        Err(LedgerWriteError::Refused(LedgerError::Settings(
            SettingsError::Timing
        )))
    );
    assert!(!called);
}

#[test]
fn ps_71_the_stored_form_reads_back_equal() {
    let l = rich();
    let bytes = l.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC_POS_LEDGER);
    assert_eq!(bytes[4], 1);
    let back = Ledger::from_bytes(&bytes).unwrap();
    assert_eq!(back, l);
    assert_eq!(back.to_bytes(), bytes);
    let empty = Ledger::new();
    assert_eq!(Ledger::from_bytes(&empty.to_bytes()).unwrap(), empty);
}

#[test]
fn ps_71_every_truncation_is_refused() {
    let bytes = rich().to_bytes();
    for n in 0..bytes.len() {
        assert!(Ledger::from_bytes(&bytes[..n]).is_err(), "prefix {n}");
    }
}

#[test]
fn ps_71_every_single_byte_change_is_refused() {
    let bytes = rich().to_bytes();
    for i in 0..bytes.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut b = bytes.clone();
            b[i] ^= flip;
            assert!(Ledger::from_bytes(&b).is_err(), "byte {i} ^ {flip:#x}");
        }
    }
}

#[test]
fn ps_71_even_with_the_hash_mended_anything_read_is_canonical() {
    let bytes = rich().to_bytes();
    let body_len = bytes.len() - 32;
    let mut accepted = 0;
    for i in 0..body_len {
        for flip in [0x01u8, 0x02, 0x80, 0xff] {
            let mut body = bytes[..body_len].to_vec();
            body[i] ^= flip;
            let mut b = body.clone();
            b.extend_from_slice(&tagged_hash(TAG_LEDGER, &[&body]));
            if let Ok(l) = Ledger::from_bytes(&b) {
                // A changed number can be a valid ledger, but then it writes
                // back as exactly these bytes.
                assert_eq!(l.to_bytes(), b, "byte {i} ^ {flip:#x}");
                accepted += 1;
            }
        }
    }
    // Some changes (a time, an amount) are legitimately other ledgers.
    assert!(accepted > 0);
}

#[test]
fn ps_71_a_wrong_magic_version_or_tail_is_refused() {
    let bytes = rich().to_bytes();
    let body_len = bytes.len() - 32;
    let mend = |mut body: Vec<u8>| {
        let tag = tagged_hash(TAG_LEDGER, &[&body]);
        body.extend_from_slice(&tag);
        body
    };
    let mut body = bytes[..body_len].to_vec();
    body[0] = b'X';
    assert_eq!(Ledger::from_bytes(&mend(body)), Err(DecodeError::BadMagic));
    let mut body = bytes[..body_len].to_vec();
    body[4] = 2;
    assert_eq!(
        Ledger::from_bytes(&mend(body)),
        Err(DecodeError::UnsupportedVersion(2))
    );
    let mut body = bytes[..body_len].to_vec();
    body.push(0);
    assert_eq!(
        Ledger::from_bytes(&mend(body)),
        Err(DecodeError::TrailingBytes)
    );
    assert!(Ledger::from_bytes(&[]).is_err());
}

#[test]
fn ps_71_keys_out_of_order_or_unlinked_are_refused() {
    // Two orders written in the wrong order, hash mended.
    let s = settings();
    let mut l = Ledger::new();
    l.update_stored(
        &[simple("a", "Main", 1, T0), simple("b", "Main", 2, T0)],
        Cursor::start(),
        &[],
        &s,
        T0,
        |_| Ok::<(), Infallible>(()),
    )
    .unwrap();
    let bytes = l.to_bytes();
    let (a, b) = {
        let mut one = Vec::new();
        l.orders()[&oid("a")].encode(&mut one);
        let mut two = Vec::new();
        l.orders()[&oid("b")].encode(&mut two);
        (one, two)
    };
    let at = bytes
        .windows(a.len())
        .position(|w| w == a.as_slice())
        .unwrap();
    let mut body = bytes[..bytes.len() - 32].to_vec();
    body.splice(
        at..at + a.len() + b.len(),
        b.iter().chain(a.iter()).copied(),
    );
    body.extend_from_slice(&tagged_hash(TAG_LEDGER, &[&body]));
    assert!(Ledger::from_bytes(&body).is_err());
}

#[test]
fn ps_72_the_ledger_refuses_to_grow_past_a_bound() {
    let orders: Vec<PosOrder> = (0..=MAX_TRACKED_ORDERS)
        .map(|i| {
            simple(
                &format!("o{i:05}"),
                "Main",
                100 + u64::try_from(i).unwrap(),
                T0,
            )
        })
        .collect();
    let mut l = Ledger::new();
    let r = l.update_stored(&orders, Cursor::start(), &[], &settings(), T0 + 10, ok);
    assert!(matches!(
        r,
        Err(LedgerWriteError::Refused(LedgerError::Full {
            what: "tracked orders",
            ..
        }))
    ));
    assert_eq!(l, Ledger::new());
    // One fewer fits.
    let r = l.update_stored(&orders[1..], Cursor::start(), &[], &settings(), T0 + 10, ok);
    assert!(r.is_ok());
    assert_eq!(l.orders().len(), MAX_TRACKED_ORDERS);
}

#[test]
fn ps_72_too_many_sales_or_two_facts_for_one_sale_are_refused() {
    let facts: Vec<SaleFact> = (0..=u32::try_from(MAX_SALES).unwrap())
        .map(|i| sale(3, i, 100, T0))
        .collect();
    let l = Ledger::new();
    assert_eq!(
        l.updated(&[], Cursor::start(), &facts, &settings(), T0)
            .unwrap_err(),
        LedgerError::TooManySales
    );
    let twice = [sale(3, 1, 100, T0), sale(3, 1, 200, T0)];
    assert!(matches!(
        l.updated(&[], Cursor::start(), &twice, &settings(), T0)
            .unwrap_err(),
        LedgerError::DuplicateSale(_)
    ));
}

#[test]
fn ps_72_a_cursor_is_at_most_256_bytes() {
    assert!(Cursor::new(vec![0; MAX_CURSOR_BYTES]).is_ok());
    assert_eq!(
        Cursor::new(vec![0; MAX_CURSOR_BYTES + 1]),
        Err(ModelError::CursorTooLong)
    );
}

fn settled(now: u64) -> (Ledger, SaleFact) {
    let mut s = paid(sale(3, 1, 500, T0), T0 + 60);
    s.is_final = true;
    let mut l = Ledger::new();
    step(
        &mut l,
        &[simple("o", "Main", 500, T0 + 90)],
        &[s.clone()],
        T0 + 100,
    );
    assert_eq!(l.matches().len(), 1);
    let _ = now;
    (l, s)
}

#[test]
fn ps_73_a_settled_match_retires_into_a_count_after_seven_days() {
    let (mut l, s) = settled(0);
    let d = step(&mut l, &[], &[s.clone()], T0 + 90 + 7 * DAY - 1);
    assert_eq!(l.matches().len(), 1, "not yet");
    assert_eq!(d.retired_matches, 0);
    let d = step(&mut l, &[], &[s.clone()], T0 + 90 + 7 * DAY);
    assert_eq!(d.retired_matches, 1);
    assert!(l.matches().is_empty() && l.orders().is_empty());
    assert_eq!(l.retired_matches(), 1);
    assert_eq!(l.summary().retired_matches, 1);
    // The sale is not an alert for want of its tender.
    let d = step(&mut l, &[], &[s], T0 + 8 * DAY);
    assert!(l.open_alerts().is_empty() && d.new_alerts.is_empty());
    // And the count survives storage.
    assert_eq!(
        Ledger::from_bytes(&l.to_bytes()).unwrap().retired_matches(),
        1
    );
}

#[test]
fn ps_73_not_while_the_sale_is_not_final_or_the_order_changed_or_an_alert_is_open() {
    // Not final.
    let (mut l, mut s) = settled(0);
    s.is_final = false;
    step(&mut l, &[], &[s], T0 + 30 * DAY);
    assert_eq!(l.matches().len(), 1);
    // Sale unknown.
    let (mut l, _) = settled(0);
    step(&mut l, &[], &[], T0 + 30 * DAY);
    assert_eq!(l.matches().len(), 1);
    // The order changed recently.
    let (mut l, s) = settled(0);
    let mut o = simple("o", "Main", 500, T0 + 90);
    o.updated_at = T0 + 29 * DAY;
    step(&mut l, &[o], &[s], T0 + 30 * DAY);
    assert_eq!(l.matches().len(), 1);
    // An open alert (a tip nobody looked at).
    let mut s = paid(sale(3, 1, 500, T0), T0 + 60);
    s.is_final = true;
    let mut t = tender("t", 500, T0 + 90);
    t.tip_minor = 10;
    let mut l = Ledger::new();
    step(
        &mut l,
        &[order("o", "Main", vec![t])],
        &[s.clone()],
        T0 + 100,
    );
    step(&mut l, &[], &[s.clone()], T0 + 30 * DAY);
    assert_eq!(l.matches().len(), 1);
    // Acknowledged, it goes, with its alert.
    let key = l.open_alerts()[0].key.clone();
    l.acknowledge_stored(&key, T0 + 30 * DAY, ok).unwrap();
    step(&mut l, &[], &[s], T0 + 30 * DAY + 1);
    assert!(l.matches().is_empty() && l.orders().is_empty());
    assert_eq!(l.summary().acknowledged_alerts, 0);
}

#[test]
fn ps_73_an_unmatched_tender_retires_only_once_its_alert_is_acknowledged() {
    let mut l = Ledger::new();
    step(&mut l, &[simple("o", "Main", 500, T0)], &[], T0 + 700);
    step(&mut l, &[], &[], T0 + 30 * DAY);
    assert_eq!(l.orders().len(), 1);
    let key = l.open_alerts()[0].key.clone();
    l.acknowledge_stored(&key, T0 + 30 * DAY, ok).unwrap();
    step(&mut l, &[], &[], T0 + 30 * DAY + 1);
    assert!(l.orders().is_empty());
    assert_eq!(l.retired_matches(), 0);
}

#[test]
fn ps_74_each_page_is_stored_with_its_cursor_and_matching_is_done_once_at_the_end() {
    let page = |orders: Vec<PosOrder>, next: u8, more| Page {
        orders,
        next: Cursor::new(vec![next]).unwrap(),
        more,
    };
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    let mut pos = MemoryPos::new([
        page(vec![simple("a", "Main", 500, T0 + 90)], 1, true),
        page(vec![simple("b", "Main", 999, T0 + 90)], 2, false),
    ]);
    let mut writes: Vec<Ledger> = Vec::new();
    let r = sync(&mut pos, &mut l, &[s], &settings(), T0 + 5_000, |b| {
        writes.push(Ledger::from_bytes(b).unwrap());
        Ok::<(), Infallible>(())
    })
    .unwrap();
    assert_eq!((r.pages, r.orders_read, r.more), (2, 2, false));
    // Two page stores and the final one; the pages hold orders and a cursor
    // and no matches, the last holds the matches and the alerts.
    assert_eq!(writes.len(), 3);
    assert_eq!(writes[0].cursor().as_bytes(), &[1]);
    assert_eq!(writes[0].orders().len(), 1);
    assert_eq!(writes[1].cursor().as_bytes(), &[2]);
    assert!(writes[1].matches().is_empty() && writes[1].open_alerts().is_empty());
    assert_eq!(writes[2].matches().len(), 1);
    assert_eq!(writes[2].open_alerts().len(), 1);
    assert_eq!(l, writes[2]);
}

#[test]
fn ps_74_a_pos_that_fails_on_a_later_page_still_makes_progress() {
    let page = |orders: Vec<PosOrder>, next: u8, more| Page {
        orders,
        next: Cursor::new(vec![next]).unwrap(),
        more,
    };
    let mut l = Ledger::new();
    let mut pos = MemoryPos::scripted([
        Ok(page(vec![simple("a", "Main", 100, T0)], 1, true)),
        Ok(page(vec![simple("b", "Main", 200, T0)], 2, true)),
        Err(PosError::Unavailable),
    ]);
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 10, ok);
    assert_eq!(
        r,
        Err(SyncFailed {
            error: SyncError::Pos(PosError::Unavailable),
            pages_kept: 2
        })
    );
    // Two pages are stored, with the cursor after the second.
    assert_eq!(l.orders().len(), 2);
    assert_eq!(l.cursor().as_bytes(), &[2]);
    // The next sync goes on from the stored cursor.
    let mut pos = MemoryPos::new([page(vec![simple("c", "Main", 300, T0)], 3, false)]);
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 20, ok).unwrap();
    assert_eq!((r.pages, pos.polled()[0].as_bytes()), (1, [2].as_slice()));
    assert_eq!(l.orders().len(), 3);
}

#[test]
fn ps_74_a_store_that_fails_keeps_the_pages_before_it_and_reports_them() {
    let page = |orders: Vec<PosOrder>, next: u8, more| Page {
        orders,
        next: Cursor::new(vec![next]).unwrap(),
        more,
    };
    let mut l = Ledger::new();
    let mut pos = MemoryPos::new([
        page(vec![simple("a", "Main", 100, T0)], 1, true),
        page(vec![simple("b", "Main", 200, T0)], 2, false),
    ]);
    let mut n = 0;
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 10, |_| {
        n += 1;
        if n == 2 {
            Err("disk full")
        } else {
            Ok(())
        }
    });
    assert_eq!(
        r,
        Err(SyncFailed {
            error: SyncError::Persist("disk full"),
            pages_kept: 1
        })
    );
    // The failed store changed nothing: only the first page is held.
    assert_eq!(l.orders().len(), 1);
    assert_eq!(l.cursor().as_bytes(), &[1]);
    assert!(r.unwrap_err().to_string().contains("1 pages were kept"));
}

#[test]
fn ps_74_a_page_with_a_bad_order_is_not_stored_and_earlier_pages_are() {
    let bad: Vec<PosTender> = (0..17).map(|i| tender(&format!("t{i}"), 1, T0)).collect();
    let mut l = Ledger::new();
    let mut pos = MemoryPos::new([
        Page {
            orders: vec![simple("a", "Main", 100, T0)],
            next: Cursor::new(vec![1]).unwrap(),
            more: true,
        },
        Page {
            orders: vec![order("bad", "Main", bad)],
            next: Cursor::new(vec![2]).unwrap(),
            more: false,
        },
    ]);
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 10, ok).unwrap_err();
    assert_eq!(r.pages_kept, 1);
    assert_eq!(l.cursor().as_bytes(), &[1]);
    assert_eq!(l.orders().len(), 1);
}

#[test]
fn ps_74_reading_the_same_pages_again_changes_nothing_and_raises_nothing() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let orders = vec![
        simple("a", "Main", 500, T0 + 90),
        simple("b", "Main", 999, T0 + 90),
    ];
    let pages = || {
        [Page {
            orders: orders.clone(),
            next: Cursor::new(vec![5]).unwrap(),
            more: false,
        }]
    };
    let mut l = Ledger::new();
    let now = T0 + 5_000;
    let first = sync(
        &mut MemoryPos::new(pages()),
        &mut l,
        &[s.clone()],
        &settings(),
        now,
        ok,
    )
    .unwrap();
    assert_eq!(first.delta.new_matches.len(), 1);
    assert_eq!(first.delta.new_alerts.len(), 1);
    let bytes = l.to_bytes();
    for later in [now, now + 1, now + 3_600] {
        let again = sync(
            &mut MemoryPos::new(pages()),
            &mut l,
            &[s.clone()],
            &settings(),
            later,
            ok,
        )
        .unwrap();
        assert_eq!(again.delta, Delta::default());
        assert_eq!(l.to_bytes(), bytes);
    }
}

#[test]
fn ps_74_an_older_version_of_an_order_does_not_undo_a_newer_one() {
    let o = simple("o", "Main", 500, T0 + 90);
    let mut voided = o.clone();
    voided.tenders[0].state = TenderState::Voided;
    voided.updated_at = T0 + 500;
    let mut l = Ledger::new();
    step(&mut l, &[voided.clone()], &[], T0 + 600);
    step(&mut l, &[o.clone()], &[], T0 + 700);
    assert_eq!(l.orders()[&oid("o")], voided);
    // Within one poll too, whatever the order.
    let mut l2 = Ledger::new();
    step(&mut l2, &[voided.clone(), o], &[], T0 + 700);
    assert_eq!(l2.orders()[&oid("o")], voided);
}

#[test]
fn ps_75_two_versions_with_the_same_time_give_the_same_result_in_any_order() {
    let mut a = simple("o", "Main", 500, T0 + 90);
    let mut b = simple("o", "Main", 600, T0 + 90);
    a.updated_at = T0 + 100;
    b.updated_at = T0 + 100;
    let mut one = Ledger::new();
    step(&mut one, &[a.clone(), b.clone()], &[], T0 + 110);
    let mut two = Ledger::new();
    step(&mut two, &[b, a], &[], T0 + 110);
    assert_eq!(one, two);
    // And a later poll of either does not change what is held.
    let held = one.clone();
    step(
        &mut one,
        &[simple("o", "Main", 500, T0 + 90)],
        &[],
        T0 + 120,
    );
    assert_eq!(one.orders(), held.orders());
}

#[test]
fn ps_76_a_full_refusal_names_the_oldest_alerts_and_they_can_be_acknowledged() {
    // 4,096 orders, each with a tip nobody has looked at.
    let orders: Vec<PosOrder> = (0..MAX_TRACKED_ORDERS)
        .map(|i| {
            let mut t = tender("t", 100, T0 + u64::try_from(i).unwrap());
            t.tip_minor = 5;
            order(&format!("o{i:05}"), "Main", vec![t])
        })
        .collect();
    let mut l = Ledger::new();
    l.update_stored(&orders, Cursor::start(), &[], &settings(), T0 + 100, ok)
        .unwrap();
    assert_eq!(l.summary().open_alerts, MAX_TRACKED_ORDERS);
    let r = l.update_stored(
        &[simple("one-more", "Main", 1, T0 + 150)],
        Cursor::start(),
        &[],
        &settings(),
        T0 + 200,
        ok,
    );
    let Err(LedgerWriteError::Refused(LedgerError::Full { what, oldest })) = r else {
        panic!("not refused: {r:?}");
    };
    assert_eq!(what, "tracked orders");
    assert_eq!(oldest.len(), MAX_NAMED_BLOCKERS);
    assert!(oldest.iter().all(|b| matches!(b, Blocker::OpenAlert(_))));
    // Each one named is an alert the owner can acknowledge now.
    for b in &oldest {
        let Blocker::OpenAlert(key) = b else { continue };
        l.acknowledge_stored(key, T0 + 300, ok).unwrap();
    }
    assert_eq!(l.summary().acknowledged_alerts, MAX_NAMED_BLOCKERS);
    // The refusal is plain words that name them.
    let words = LedgerError::Full { what, oldest }.to_string();
    assert!(words.contains("Tip on a Bitcoin tender") && words.contains("order o00000"));
}

#[test]
fn ps_76_a_full_refusal_also_names_orders_waiting_for_a_sale() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    step(&mut l, &[simple("o", "Main", 500, T0 + 90)], &[s], T0 + 100);
    let many: Vec<PosOrder> = (0..MAX_TRACKED_ORDERS)
        .map(|i| simple(&format!("p{i:05}"), "Main", 900, T0 + 95))
        .collect();
    let r = l.update_stored(&many, Cursor::start(), &[], &settings(), T0 + 110, ok);
    let Err(LedgerWriteError::Refused(LedgerError::Full { oldest, .. })) = r else {
        panic!("not refused");
    };
    assert_eq!(
        oldest[0],
        Blocker::SaleUnsettled {
            order: oid("o"),
            sale: sale(3, 1, 500, T0).key()
        }
    );
}
