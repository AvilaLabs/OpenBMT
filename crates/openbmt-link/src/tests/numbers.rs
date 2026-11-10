//! LK-06 and LK-40: the numbers a device keeps.

use std::cell::RefCell;

use openbmt_core::wire::DecodeError;

use super::kit::*;
use crate::numbers::{Inbox, NextError, Outbox, TakeError};
use crate::seal::{Kind, OpenError, Role};

#[test]
fn lk_06_the_inbox_takes_rising_numbers_and_refuses_a_replay() {
    let mut inbox = Inbox::default();
    let reader = counter();
    let ok = |_: &[u8]| -> Result<(), ()> { Ok(()) };
    let m5 = owner_to_counter(5, Kind::Command, b"five");
    let m3 = owner_to_counter(3, Kind::Command, b"three");
    let m9 = owner_to_counter(9, Kind::Command, b"nine");

    let o = inbox
        .open(&reader, Role::Counter, &m5, allowed, ok)
        .unwrap();
    assert_eq!(o.content, b"five");
    assert_eq!(inbox.last(&owner().device_id()), 5);
    // The same again, and an older one.
    for m in [&m5, &m3] {
        let e = inbox
            .open(&reader, Role::Counter, m, allowed, ok)
            .unwrap_err();
        assert!(matches!(
            e,
            TakeError::Open(OpenError::Replay { last: 5, .. })
        ));
    }
    // A gap is fine.
    assert!(inbox.open(&reader, Role::Counter, &m9, allowed, ok).is_ok());
    assert_eq!(inbox.last(&owner().device_id()), 9);
    // Per sender: another sender's last is its own.
    assert_eq!(inbox.last(&the_box().device_id()), 0);
}

#[test]
fn lk_06_the_number_is_stored_before_the_content_is_returned() {
    let mut inbox = Inbox::default();
    let stored: RefCell<Option<Vec<u8>>> = RefCell::new(None);
    let m = owner_to_counter(4, Kind::Command, b"x");
    let o = inbox
        .open(&counter(), Role::Counter, &m, allowed, |bytes| {
            // At this point the content has not been returned.
            *stored.borrow_mut() = Some(bytes.to_vec());
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(o.number, 4);
    let bytes = stored.into_inner().unwrap();
    let from_disk = Inbox::from_bytes(&bytes).unwrap();
    assert_eq!(from_disk.last(&owner().device_id()), 4);
    assert_eq!(from_disk, inbox);
}

#[test]
fn lk_40_a_failed_store_returns_no_content_and_changes_nothing() {
    let mut inbox = Inbox::default();
    let m = owner_to_counter(4, Kind::Command, b"x");
    let e = inbox
        .open(&counter(), Role::Counter, &m, allowed, |_| Err("disk full"))
        .unwrap_err();
    assert_eq!(e, TakeError::Persist("disk full"));
    assert_eq!(inbox.last(&owner().device_id()), 0);
    assert!(e.to_string().contains("LK-40"));
    // After the failure the same message still opens.
    assert!(inbox
        .open(&counter(), Role::Counter, &m, allowed, |_| Ok::<(), ()>(()))
        .is_ok());
}

#[test]
fn lk_40_the_outbox_stores_a_number_before_it_hands_it_out() {
    let mut outbox = Outbox::default();
    let reader = counter().device_id();
    let seen: RefCell<Vec<Vec<u8>>> = RefCell::new(Vec::new());
    for want in 1..=3u64 {
        let n = outbox
            .next(&reader, |b| {
                seen.borrow_mut().push(b.to_vec());
                Ok::<(), ()>(())
            })
            .unwrap();
        assert_eq!(n, want);
    }
    let last = seen.borrow().last().cloned().unwrap();
    assert_eq!(Outbox::from_bytes(&last).unwrap(), outbox);
    // A failed store hands out nothing and the next call tries the same one.
    let e = outbox.next(&reader, |_| Err("disk full")).unwrap_err();
    assert_eq!(e, NextError::Persist("disk full"));
    assert_eq!(outbox.last(&reader), 3);
    assert_eq!(outbox.next(&reader, |_| Ok::<(), ()>(())).unwrap(), 4);
    // Per reader.
    assert_eq!(outbox.last(&owner().device_id()), 0);
}

#[test]
fn lk_40_a_restart_continues_from_the_stored_number() {
    let mut outbox = Outbox::default();
    let reader = counter().device_id();
    let mut file = Vec::new();
    for _ in 0..5 {
        outbox
            .next(&reader, |b| {
                file = b.to_vec();
                Ok::<(), ()>(())
            })
            .unwrap();
    }
    let mut after = Outbox::from_bytes(&file).unwrap();
    assert_eq!(after.next(&reader, |_| Ok::<(), ()>(())).unwrap(), 6);
}

#[test]
fn lk_40_the_outbox_does_not_wrap() {
    let mut bytes = Outbox::default().to_bytes();
    // One entry at u64::MAX.
    bytes[5..9].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&counter().device_id().to_bytes());
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    let mut outbox = Outbox::from_bytes(&bytes).unwrap();
    let e = outbox
        .next(&counter().device_id(), |_| Ok::<(), ()>(()))
        .unwrap_err();
    assert_eq!(e, NextError::Exhausted);
}

#[test]
fn lk_40_stored_tables_decode_strictly() {
    let mut inbox = Inbox::default();
    for n in [owner(), the_box()] {
        let m = {
            let mut rng = Counting(0);
            crate::seal::seal(
                &n,
                if n.device_id() == owner().device_id() {
                    Role::Owner
                } else {
                    Role::Box
                },
                &counter().device_id(),
                Role::Counter,
                2,
                if n.device_id() == owner().device_id() {
                    Kind::Heartbeat
                } else {
                    Kind::PosNotice
                },
                b"x",
                &mut |b| rng.fill(b),
            )
            .unwrap()
        };
        inbox
            .open(&counter(), Role::Counter, &m, allowed, |_| Ok::<(), ()>(()))
            .unwrap();
    }
    let bytes = inbox.to_bytes();
    assert_eq!(Inbox::from_bytes(&bytes).unwrap(), inbox);
    // Trailing byte, short, wrong magic (an outbox is not an inbox), and
    // entries out of order.
    let mut long = bytes.clone();
    long.push(0);
    assert_eq!(
        Inbox::from_bytes(&long).unwrap_err(),
        DecodeError::TrailingBytes
    );
    assert!(Inbox::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    assert_eq!(
        Inbox::from_bytes(&Outbox::default().to_bytes()).unwrap_err(),
        DecodeError::BadMagic
    );
    let mut swapped = bytes[..9].to_vec();
    swapped.extend_from_slice(&bytes[9 + 40..9 + 80]);
    swapped.extend_from_slice(&bytes[9..9 + 40]);
    assert!(matches!(
        Inbox::from_bytes(&swapped),
        Err(DecodeError::NonCanonical(_))
    ));
    assert_eq!(
        Inbox::from_bytes(&Inbox::default().to_bytes()).unwrap(),
        Inbox::default()
    );
}
