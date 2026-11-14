//! LK-41: the relay's files.

use std::fs;
use std::io::Write;

use super::kit::*;
use super::relay::{ids, items, msg, Rig};
use crate::proto::{Request, Response};
use crate::relay::{Relay, RelayConfig, LEAVING_SECS};
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
    rig.relay
        .set_members(&[the_box().device_id()], rig.now)
        .unwrap();
    // The file stays while the mailbox is leaving (LK-28), and goes with it.
    assert!(log_path(&dir).exists());
    rig.now += LEAVING_SECS;
    rig.ask(Request::Hello { nonce: [1; 32] });
    assert!(!log_path(&dir).exists());
    assert!(dir.0.join("notes.txt").exists());
    let mut store = DirStore::new(&dir.0).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.members, vec![the_box().device_id()]);
    assert!(loaded.leaving.is_empty());
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

#[test]
fn lk_41_a_file_left_from_a_removed_member_is_emptied_when_the_member_returns() {
    let dir = TempDir::new("leftover");
    let mut rig = dir_rig(&dir);
    assert_eq!(rig.make(&counter()), Response::Made);
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    let old = fs::read(log_path(&dir)).unwrap();
    assert!(!old.is_empty());
    // The counter leaves, and its file comes back as if it had not been
    // removed.
    let members: Vec<_> = rig.relay.members().copied().collect();
    let without: Vec<_> = members
        .iter()
        .copied()
        .filter(|m| *m != counter().device_id())
        .collect();
    rig.relay.set_members(&without, rig.now).unwrap();
    rig.now += LEAVING_SECS;
    rig.ask(Request::Hello { nonce: [1; 32] });
    assert!(!log_path(&dir).exists());
    fs::write(log_path(&dir), &old).unwrap();
    // It returns: the mailbox is empty in memory and on disk.
    rig.relay.set_members(&members, rig.now).unwrap();
    assert_eq!(rig.make(&counter()), Response::Made);
    assert_eq!(fs::metadata(log_path(&dir)).unwrap().len(), 0);
    drop(rig);
    let mut again = dir_rig(&dir);
    assert!(items(again.fetch(&counter(), 0)).is_empty());
}

#[test]
fn lk_42_the_leaving_list_and_its_mailbox_survive_a_restart() {
    let dir = TempDir::new("leaving");
    let mut rig = dir_rig(&dir);
    rig.make(&counter());
    rig.put(msg(1));
    let at = rig.now;
    let members: Vec<_> = rig.relay.members().copied().collect();
    let without: Vec<_> = members
        .iter()
        .copied()
        .filter(|m| *m != counter().device_id())
        .collect();
    rig.relay.set_members(&without, at).unwrap();
    drop(rig);
    let mut store = DirStore::new(&dir.0).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.leaving, vec![(counter().device_id(), at)]);
    assert_eq!(loaded.mailboxes.len(), 1);
    // The restarted relay still hands the removed device its message, and
    // keeps the time it became leaving.
    let relay = Relay::new(
        id(99),
        RelayConfig::default(),
        DirStore::new(&dir.0).unwrap(),
    )
    .unwrap();
    let mut again = Rig::over(relay);
    assert_eq!(
        again
            .relay
            .leaving()
            .map(|(d, t)| (*d, t))
            .collect::<Vec<_>>(),
        vec![(counter().device_id(), at)]
    );
    assert_eq!(ids(again.fetch(&counter(), 0)), vec![1]);
    // Read to its end: the file and the list entry are gone.
    assert!(!log_path(&dir).exists());
    drop(again);
    assert!(DirStore::new(&dir.0)
        .unwrap()
        .load()
        .unwrap()
        .leaving
        .is_empty());
}

#[test]
fn lk_42_a_directory_without_a_leaving_file_has_no_leaving_mailboxes() {
    let dir = TempDir::new("noleaving");
    let mut rig = dir_rig(&dir);
    rig.make(&counter());
    drop(rig);
    fs::remove_file(dir.0.join("leaving.set")).unwrap();
    let loaded = DirStore::new(&dir.0).unwrap().load().unwrap();
    assert!(loaded.leaving.is_empty());
    assert_eq!(loaded.mailboxes.len(), 1);
}

#[test]
fn lk_42_a_damaged_leaving_file_stops_the_start() {
    let dir = TempDir::new("badleaving");
    let mut rig = dir_rig(&dir);
    rig.make(&counter());
    let members: Vec<_> = rig.relay.members().copied().collect();
    let without: Vec<_> = members
        .iter()
        .copied()
        .filter(|m| *m != counter().device_id())
        .collect();
    rig.relay.set_members(&without, rig.now).unwrap();
    drop(rig);
    let path = dir.0.join("leaving.set");
    let good = fs::read(&path).unwrap();
    assert_eq!(good.len(), 40 + 4);
    for n in 0..good.len() {
        fs::write(&path, &good[..n]).unwrap();
        assert!(DirStore::new(&dir.0).unwrap().load().is_err(), "cut at {n}");
    }
    let mut flipped = good.clone();
    flipped[5] ^= 1;
    fs::write(&path, flipped).unwrap();
    assert!(DirStore::new(&dir.0).unwrap().load().is_err());
}

#[test]
fn lk_42_the_leaving_list_reads_back_to_the_same_bytes() {
    use crate::store::{decode_leaving, encode_leaving};
    let list = vec![(counter().device_id(), 5), (owner().device_id(), u64::MAX)];
    let bytes = encode_leaving(&list);
    assert_eq!(decode_leaving(&bytes), Some(list));
    assert_eq!(decode_leaving(&encode_leaving(&[])), Some(Vec::new()));
    for n in 0..bytes.len() {
        assert!(decode_leaving(&bytes[..n]).is_none(), "cut at {n}");
    }
    let mut longer = bytes;
    longer.push(0);
    assert!(decode_leaving(&longer).is_none());
}

#[cfg(unix)]
#[test]
fn lk_41_the_directory_and_every_file_in_it_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = TempDir::new("private");
    let inner = dir.0.join("relay/mail");
    // A stale temporary of the list and of the log, left with a wider mode.
    fs::create_dir_all(&inner).unwrap();
    let list_tmp = inner.join("members.set.tmp");
    fs::write(&list_tmp, b"stale").unwrap();
    fs::set_permissions(&list_tmp, fs::Permissions::from_mode(0o644)).unwrap();
    let mut rig = Rig::with_store(RelayConfig::default(), DirStore::new(&inner).unwrap());
    rig.make(&counter());
    for n in 1..=10u64 {
        rig.put(msg(n));
    }
    rig.fetch(&counter(), 0);
    let log = DirStore::new(&inner).unwrap().path(&counter().device_id());
    let log_tmp = log.with_extension("mbx.tmp");
    fs::write(&log_tmp, b"stale").unwrap();
    fs::set_permissions(&log_tmp, fs::Permissions::from_mode(0o666)).unwrap();
    // Over half acknowledged: the log is rewritten through the temporary.
    rig.ack(&counter(), 8);
    assert_eq!(mode(&log), 0o600);
    assert_eq!(mode(&inner.join("members.set")), 0o600);
    assert!(!list_tmp.exists() && !log_tmp.exists());
    // A directory made fresh is 0700, parents included.
    let fresh = dir.0.join("new/deeper");
    DirStore::new(&fresh).unwrap();
    assert_eq!(mode(&fresh), 0o700);
    assert_eq!(mode(&dir.0.join("new")), 0o700);
}
