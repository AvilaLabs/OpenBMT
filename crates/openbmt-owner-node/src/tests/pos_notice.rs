//! BX-42 on the owner's side: the Box's POS notices are kept as the Box's
//! report, and only from a Box, signed, in rising order.

use openbmt_core::identity::Identity;
use openbmt_core::records::DeviceList;
use openbmt_courier::{Courier, FirstStart, MemoryPost, MemoryRelay, SendError};
use openbmt_link::{Kind, Role};
use openbmt_pos::{PosItem, PosNotice, SaleKey};

use super::*;
use crate::{OwnerLink, OwnerLinkConfig, MAX_BOX_REPORTS};

fn owner() -> DeviceId {
    id(1).device_id()
}

fn the_box() -> Identity {
    id(7)
}

struct World {
    link: OwnerLink<MemoryPost>,
    box_side: Courier<MemoryPost>,
    counter_side: Courier<MemoryPost>,
    rig: Rig,
    _dirs: Vec<Dir>,
}

fn world() -> World {
    let relay = MemoryRelay::new(id(99), &[owner(), counter(), the_box().device_id()]).unwrap();
    let dirs: Vec<Dir> = (0..4).map(|_| Dir::new()).collect();
    let rig = start(dirs[0].path());
    let owner_courier = Courier::start_with(
        relay.post(),
        id(1),
        Role::Owner,
        [
            (counter(), Role::Counter),
            (the_box().device_id(), Role::Box),
        ]
        .into_iter()
        .collect(),
        dirs[1].path(),
        FirstStart::Yes,
    )
    .unwrap();
    let mut link = OwnerLink::new(
        owner_courier,
        dirs[1].path(),
        DeviceList::new([counter(), owner()]),
        vec![counter()],
        OwnerLinkConfig {
            poll_secs: 0,
            retry_secs: 0,
            ..OwnerLinkConfig::default()
        },
    )
    .unwrap();
    let mut box_side = Courier::start_with(
        relay.post(),
        the_box(),
        Role::Box,
        [(owner(), Role::Owner)].into_iter().collect(),
        dirs[2].path(),
        FirstStart::Yes,
    )
    .unwrap();
    let mut counter_side = Courier::start_with(
        relay.post(),
        id(3),
        Role::Counter,
        [(owner(), Role::Owner)].into_iter().collect(),
        dirs[3].path(),
        FirstStart::Yes,
    )
    .unwrap();
    box_side.poll().unwrap();
    counter_side.poll().unwrap();
    link.courier_mut().poll().unwrap();
    World {
        link,
        box_side,
        counter_side,
        rig,
        _dirs: dirs,
    }
}

impl World {
    fn step(&mut self, now: u64) {
        self.link.step(&mut self.rig.watcher, now);
    }

    fn send(&mut self, notice: &PosNotice) {
        self.box_side
            .send(&owner(), Kind::PosNotice, &notice.to_bytes())
            .unwrap();
    }
}

fn closed(number: u64, by: &Identity) -> PosNotice {
    PosNotice::sign(
        by,
        number,
        T0,
        PosItem::Closed {
            sale: SaleKey {
                counter: counter(),
                index: number as u32,
            },
            order: openbmt_pos::Id::new("o1").unwrap(),
        },
    )
    .unwrap()
}

#[test]
fn bx_42_a_box_notice_is_kept_as_the_boxs_report_and_not_as_an_alert() {
    let mut w = world();
    w.send(&closed(1, &the_box()));
    w.send(&closed(2, &the_box()));
    w.step(T0);
    let kept: Vec<_> = w.link.box_reports().collect();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].notice.number, 1);
    assert_eq!(kept[1].notice.number, 2);
    assert_eq!(kept[0].from, the_box().device_id());
    assert_eq!(kept[0].taken_at, T0);
    // The owner's own alerts are not touched and the notes say nothing is
    // wrong.
    assert!(w.link.alerts().is_empty());
    assert!(w.link.take_notes().iter().all(|n| !n.contains("BX-42")));
}

#[test]
fn bx_42_a_replayed_or_older_number_is_refused_and_noted() {
    let mut w = world();
    w.send(&closed(5, &the_box()));
    w.step(T0);
    w.send(&closed(5, &the_box()));
    w.send(&closed(3, &the_box()));
    w.send(&closed(6, &the_box()));
    w.step(T0 + 1);
    let numbers: Vec<u64> = w.link.box_reports().map(|r| r.notice.number).collect();
    assert_eq!(numbers, vec![5, 6]);
    let notes = w.link.take_notes();
    assert_eq!(notes.iter().filter(|n| n.contains("not above")).count(), 2);
}

#[test]
fn bx_42_a_notice_signed_by_another_key_is_refused() {
    let mut w = world();
    // The Box carries a notice that someone else signed, and one that
    // names the Box but carries a signature that does not check.
    w.send(&closed(1, &id(8)));
    let mut forged = closed(2, &the_box());
    forged.at += 1;
    w.send(&forged);
    w.step(T0);
    assert_eq!(w.link.box_reports().count(), 0);
    let notes = w.link.take_notes();
    assert_eq!(
        notes.iter().filter(|n| n.contains("not signed")).count(),
        2,
        "{notes:?}"
    );
    // Bytes that are not a notice are noted too.
    w.box_side
        .send(&owner(), Kind::PosNotice, b"not a notice")
        .unwrap();
    w.step(T0 + 1);
    assert!(w
        .link
        .take_notes()
        .iter()
        .any(|n| n.contains("does not decode")));
}

#[test]
fn bx_42_a_pos_notice_from_a_counter_key_is_refused_by_role() {
    let mut w = world();
    // A counter may not send the kind at all (LK-08): the courier refuses it
    // before the relay sees it.
    let notice = closed(1, &id(3));
    let err = w
        .counter_side
        .send(&owner(), Kind::PosNotice, &notice.to_bytes())
        .unwrap_err();
    assert_eq!(err, SendError::KindNotAllowed);
    w.step(T0);
    assert_eq!(w.link.box_reports().count(), 0);
}

#[test]
fn bx_42_the_list_of_the_boxs_reports_is_bounded() {
    let mut w = world();
    let total = MAX_BOX_REPORTS as u64 + 10;
    for n in 1..=total {
        w.send(&closed(n, &the_box()));
        if n % 50 == 0 {
            w.step(T0 + n);
        }
    }
    w.step(T0 + total + 1);
    let kept: Vec<u64> = w.link.box_reports().map(|r| r.notice.number).collect();
    assert_eq!(kept.len(), MAX_BOX_REPORTS);
    assert_eq!(kept.last().copied(), Some(total));
    assert_eq!(
        kept.first().copied(),
        Some(total - MAX_BOX_REPORTS as u64 + 1)
    );
}
