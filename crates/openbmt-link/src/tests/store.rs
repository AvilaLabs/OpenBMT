//! LK-41: the relay's files.

use std::fs;
use std::io::Write;

use super::kit::*;
use super::relay::{ids, items, msg, Rig};
use crate::proto::Response;
use crate::relay::RelayConfig;
use crate::store::{DirStore, Event, MailStore};

fn dir_rig(dir: &TempDir) -> Rig<DirStore> {
    Rig::with_store(RelayConfig::default(), DirStore::new(&dir.0).unwrap())
}

fn log_path(dir: &TempDir) -> std::path::PathBuf {
    DirStore::new(&dir.0).unwrap().path(&counter().device_id())
}

#[test]
fn lk_41_the_log_is_append_only_and_survives_a_restart() {
    let dir = TempDir::new("restart");
    let mut rig = dir_rig(&dir);
    assert_eq!(rig.make(&counter()), Response::Made);
    let mut size = fs::metadata(log_path(&dir)).unwrap().len();
    assert_eq!(size, 0);
    let mut last = Vec::new();
    for n in 1..=3u64 {
        let m = msg(n);
        assert_eq!(rig.put(m.clone()), Response::Stored { id: n });
        let now = fs::metadata(log_path(&dir)).unwrap().len();
        // One record: length 4, type 1, id 8, the message, check 4.
        assert_eq!(now - size, (4 + 1 + 8 + m.len() + 4) as u64);
        size = now;
        last = m;
    }
    drop(rig);
    let mut again = dir_rig(&dir);
    let got = items(again.fetch(&counter(), 0));
    assert_eq!(got.len(), 3);
    assert_eq!(got.last().unwrap().1, last);
    assert_eq!(again.put(msg(4)), Response::Stored { id: 4 });
}

#[test]
fn lk_41_a_torn_tail_loses_at_most_the_message_being_written() {
    let dir = TempDir::new("torn");
    let mut rig = dir_rig(&dir);
    rig.make(&counter());
    rig.put(msg(1));
    rig.put(msg(2));
    drop(rig);
    let path = log_path(&dir);
    let whole = fs::read(&path).unwrap();
    // Cut the file at every point inside the last record.
    let last_start = whole.len() - (4 + 1 + 8 + msg(2).len() + 4);
    for cut in last_start..whole.len() {
        fs::write(&path, &whole[..cut]).unwrap();
        let mut store = DirStore::new(&dir.0).unwrap();
        let loaded = store.load().unwrap().mailboxes;
        assert_eq!(loaded.len(), 1);
        let events = &loaded[0].1;
        assert_eq!(events.len(), 1, "cut at {cut}");
        assert_eq!(fs::metadata(&path).unwrap().len() as usize, last_start);
    }
    // Garbage after whole records.
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(&[0xde, 0xad, 0xbe, 0xef, 1, 2, 3]).unwrap();
    drop(f);
    let mut store = DirStore::new(&dir.0).unwrap();
    assert_eq!(store.load().unwrap().mailboxes[0].1.len(), 1);
    // A flipped byte in the second record cuts there, not before.
    fs::write(&path, &whole).unwrap();
    let mut bad = whole.clone();
    let n = bad.len();
    bad[n - 10] ^= 1;
    fs::write(&path, &bad).unwrap();
    let mut store = DirStore::new(&dir.0).unwrap();
    assert_eq!(store.load().unwrap().mailboxes[0].1.len(), 1);
    // The relay starts from what is left and goes on numbering above it.
    let mut rig = dir_rig(&dir);
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1]);
    assert_eq!(rig.put(msg(2)), Response::Stored { id: 2 });
}

#[test]
fn lk_41_the_log_is_rewritten_when_acknowledged_messages_pass_half_of_it() {
    let dir = TempDir::new("compact");
    let mut rig = dir_rig(&dir);
    rig.make(&counter());
    for n in 1..=10u64 {
        rig.put(msg(n));
    }
    rig.fetch(&counter(), 0);
    let before = fs::metadata(log_path(&dir)).unwrap().len();
    // Four of ten acknowledged: under half, so the file only grows (by the
    // floor record).
    rig.ack(&counter(), 4);
    let mid = fs::metadata(log_path(&dir)).unwrap().len();
    assert!(mid > before);
    // Eight of ten: over half, so the file is rewritten and is smaller.
    rig.ack(&counter(), 8);
    let after = fs::metadata(log_path(&dir)).unwrap().len();
    assert!(after < before / 2, "{after} not under half of {before}");
    assert!(!log_path(&dir).with_extension("mbx.tmp").exists());
    drop(rig);
    let mut again = dir_rig(&dir);
    assert_eq!(ids(again.fetch(&counter(), 0)), vec![9, 10]);
    // Everything acknowledged: the numbers still do not start over.
    again.fetch(&counter(), 0);
    again.ack(&counter(), 10);
    assert_eq!(again.put(msg(11)), Response::Stored { id: 11 });
    drop(again);
    let mut third = dir_rig(&dir);
    assert_eq!(ids(third.fetch(&counter(), 0)), vec![11]);
    assert_eq!(third.put(msg(12)), Response::Stored { id: 12 });
}

#[test]
fn lk_41_removing_a_member_removes_the_file_and_other_files_are_left_alone() {
    let dir = TempDir::new("clear");
    fs::write(dir.0.join("notes.txt"), b"keep").unwrap();
    let mut rig = dir_rig(&dir);
    rig.make(&counter());
    rig.make(&the_box());
    assert!(log_path(&dir).exists());
    rig.relay.set_members(&[the_box().device_id()]).unwrap();
    assert!(!log_path(&dir).exists());
    assert!(dir.0.join("notes.txt").exists());
    let mut store = DirStore::new(&dir.0).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.members, vec![the_box().device_id()]);
    assert_eq!(loaded.mailboxes.len(), 1);
    // With none stored, there are no members.
    let empty = TempDir::new("nomembers");
    let mut store = DirStore::new(&empty.0).unwrap();
    assert_eq!(store.load().unwrap(), crate::store::Loaded::default());
    // A damaged list stops the start.
    let path = dir.0.join("members.set");
    let mut bytes = fs::read(&path).unwrap();
    bytes[3] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(DirStore::new(&dir.0).unwrap().load().is_err());
}

#[test]
fn lk_41_replay_applies_the_limits_the_same_way() {
    let dir = TempDir::new("limits");
    let cfg = RelayConfig {
        max_messages: 3,
        ..RelayConfig::default()
    };
    let mut rig = Rig::with_store(cfg, DirStore::new(&dir.0).unwrap());
    rig.make(&counter());
    for n in 1..=5u64 {
        rig.put(msg(n));
    }
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![3, 4, 5]);
    drop(rig);
    let mut again = Rig::with_store(cfg, DirStore::new(&dir.0).unwrap());
    assert_eq!(ids(again.fetch(&counter(), 0)), vec![3, 4, 5]);
    let _ = Event::Floor(0);
}
