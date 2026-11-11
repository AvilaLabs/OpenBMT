//! The owner's end of the link (LK-55, LK-58 to LK-63) and the two ends
//! together: a watcher on a fake driver, a link on a relay in memory, and a
//! courier standing in for the counter, or a real counter node.

use std::fs;
use std::sync::mpsc::{channel, Sender};

use bitcoin::hashes::Hash;
use bitcoin::{Network, ScriptBuf, WScriptHash};
use openbmt_core::batch::{AddressBatch, BatchPolicy, SignedBatch};
use openbmt_core::command::{Action, OwnerCommand, SignedCommand};
use openbmt_core::records::{Chain, DeviceList, Record, RecordKind, GENESIS};
use openbmt_core::settings::{Change, SettingNote, Settings, SettingsError, SignedSettings};
use openbmt_counter::{BatchHeld, Pairing};
use openbmt_counter_node::{CounterLink, CounterLinkConfig, Node, NodeConfig};
use openbmt_courier::{
    decode_received, records_content, Courier, FirstStart, MemoryPost, MemoryRelay, Received,
};
use openbmt_link::{Kind, Role};

use super::*;
use crate::copy::{CheckInTaken, CopyError, CounterCopy, Stop};
use crate::{Alert, OwnerLink, OwnerLinkConfig, SettingsNotCarried};

fn owner() -> DeviceId {
    id(1).device_id()
}

fn devices() -> DeviceList {
    DeviceList::new([counter(), owner()])
}

fn config() -> OwnerLinkConfig {
    OwnerLinkConfig {
        poll_secs: 0,
        retry_secs: 0,
        ..OwnerLinkConfig::default()
    }
}

fn courier(
    relay: &MemoryRelay,
    who: Identity,
    role: Role,
    peer: (DeviceId, Role),
    dir: &Dir,
    first: FirstStart,
) -> Courier<MemoryPost> {
    Courier::start_with(
        relay.post(),
        who,
        role,
        [peer].into_iter().collect(),
        dir.path(),
        first,
    )
    .unwrap()
}

/// The counter's chain: `n` records of a body that depends on `salt`.
fn chain(n: usize, salt: u8) -> Chain {
    let mut chain = Chain::new();
    for i in 0..n {
        chain
            .append(&id(3), RecordKind::Sale, vec![salt, i as u8, 7])
            .unwrap();
    }
    chain
}

struct World {
    rig: Rig,
    link: OwnerLink<MemoryPost>,
    counter_side: Courier<MemoryPost>,
    relay: MemoryRelay,
    dir: Dir,
    _watcher_dir: Dir,
    _counter_dir: Dir,
}

fn world() -> World {
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let (dir, watcher_dir, counter_dir) = (Dir::new(), Dir::new(), Dir::new());
    let rig = start(watcher_dir.path());
    let owner_courier = courier(
        &relay,
        id(1),
        Role::Owner,
        (counter(), Role::Counter),
        &dir,
        FirstStart::Yes,
    );
    let mut link = OwnerLink::new(
        owner_courier,
        dir.path(),
        devices(),
        vec![counter()],
        config(),
    )
    .unwrap();
    let mut counter_side = courier(
        &relay,
        id(3),
        Role::Counter,
        (owner(), Role::Owner),
        &counter_dir,
        FirstStart::Yes,
    );
    counter_side.poll().unwrap();
    // The owner's mailbox exists before the counter sends.
    link.courier_mut().poll().unwrap();
    World {
        rig,
        link,
        counter_side,
        relay,
        dir,
        _watcher_dir: watcher_dir,
        _counter_dir: counter_dir,
    }
}

impl World {
    fn step(&mut self, now: u64) {
        self.link.step(&mut self.rig.watcher, now);
    }

    fn send_records(&mut self, records: &[Record]) {
        let (content, count) = records_content(records, usize::MAX);
        assert_eq!(count, records.len());
        self.counter_side
            .send(&owner(), Kind::Records, &content)
            .unwrap();
    }

    fn send_check_in(&mut self, record: &Record) {
        self.counter_side
            .send(&owner(), Kind::CheckIn, &record.to_bytes())
            .unwrap();
    }

    fn counter_gets(&mut self) -> Vec<Received> {
        self.counter_side.poll().unwrap()
    }

    fn notes(&mut self) -> Vec<String> {
        self.link.take_notes()
    }

    fn copy(&self) -> &CounterCopy {
        self.link.copy(&counter()).unwrap()
    }
}

/// The receipts among `got`, as (count, head).
fn receipts(got: &[Received]) -> Vec<(u64, [u8; 32])> {
    got.iter()
        .filter(|r| r.kind == Kind::RecordsReceived)
        .map(|r| decode_received(&r.content).unwrap())
        .collect()
}

fn diverged(alerts: &[Alert]) -> Vec<u64> {
    alerts
        .iter()
        .filter_map(|a| match a {
            Alert::Diverged { seq, .. } => Some(*seq),
            _ => None,
        })
        .collect()
}

#[test]
fn lk_58_records_extend_the_copy_and_the_counter_is_told_what_the_copy_holds() {
    let mut w = world();
    let c = chain(5, 1);
    w.send_records(c.records());
    w.step(T0);
    assert_eq!(w.copy().len(), 5);
    assert_eq!(w.copy().head(), c.head());
    assert_eq!(w.copy().records(), c.records());
    assert!(w.notes().is_empty());
    assert_eq!(receipts(&w.counter_gets()), vec![(5, c.head())]);
    // The file is where the rules say, and a link started again reads it.
    let hex: String = counter()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let file = w.dir.path().join("counters").join(&hex).join("records");
    assert!(file.exists());
    let (again, torn) = CounterCopy::open(w.dir.path(), counter(), &devices()).unwrap();
    assert!(torn.is_none());
    assert_eq!(again.records(), c.records());
    // More records, on top.
    let mut longer = c.clone();
    longer
        .append(&id(3), RecordKind::Sale, b"more".to_vec())
        .unwrap();
    w.send_records(&longer.records()[3..]);
    w.step(T0 + 1);
    assert_eq!(w.copy().len(), 6);
    assert_eq!(receipts(&w.counter_gets()), vec![(6, longer.head())]);
}

#[test]
fn lk_58_the_first_record_that_fails_stops_the_message_and_the_ones_before_it_stay() {
    let c = chain(6, 2);
    let rs = c.records().to_vec();
    let dir = Dir::new();
    let (mut copy, _) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();

    // A bad signature on record 3: 0 to 2 are taken.
    let mut bad = rs.clone();
    bad[3].sig.0[5] ^= 1;
    let taken = copy.take(bad, &devices());
    assert_eq!(taken.appended, 3);
    assert_eq!(taken.stopped, Some(Stop::BadSignature { seq: 3 }));
    assert_eq!(copy.len(), 3);
    assert_eq!(copy.head(), rs[2].hash());
    // The right ones go on from there.
    let taken = copy.take(rs[3..].to_vec(), &devices());
    assert_eq!((taken.appended, taken.stopped), (3, None));

    // A gap, an author that is not on the list, a body above the largest.
    let dir = Dir::new();
    let (mut copy, _) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
    let taken = copy.take(rs[1..].to_vec(), &devices());
    assert_eq!(
        taken.stopped,
        Some(Stop::Gap {
            expected: 0,
            found: 1
        })
    );
    assert_eq!(copy.len(), 0);
    let mut foreign = Chain::new();
    foreign
        .append(&id(7), RecordKind::Sale, b"x".to_vec())
        .unwrap();
    let taken = copy.take(foreign.records().to_vec(), &devices());
    assert_eq!(taken.stopped, Some(Stop::UnknownAuthor { seq: 0 }));
    // A check-in that says something other than its place says.
    let mut wrong = Chain::new();
    wrong
        .append(&id(3), RecordKind::CheckIn, vec![9; 40])
        .unwrap();
    let taken = copy.take(wrong.records().to_vec(), &devices());
    assert_eq!(taken.stopped, Some(Stop::BadCheckIn { seq: 0 }));
    assert_eq!(copy.len(), 0);
    // The stop is in words and names its rule.
    assert!(Stop::BadSignature { seq: 3 }.to_string().contains("LK-58"));
}

#[test]
fn lk_58_a_message_that_stops_is_still_answered_with_what_the_copy_holds() {
    let mut w = world();
    let c = chain(4, 3);
    let mut bad = c.records().to_vec();
    bad[2].sig.0[0] ^= 1;
    w.send_records(&bad);
    w.step(T0);
    assert_eq!(w.copy().len(), 2);
    assert_eq!(
        receipts(&w.counter_gets()),
        vec![(2, c.records()[1].hash())]
    );
    assert!(w
        .notes()
        .iter()
        .any(|n| n.contains("stopped after 2 taken")));
}

#[test]
fn lk_58_a_torn_tail_of_the_copy_is_cut_off_at_the_start() {
    let c = chain(4, 4);
    let dir = Dir::new();
    {
        let (mut copy, _) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
        copy.take(c.records().to_vec(), &devices());
    }
    let hex: String = counter()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let file = dir.path().join("counters").join(hex).join("records");
    let whole = fs::read(&file).unwrap();
    // Half of a fifth record, written by a crash.
    let next = chain(5, 4).records()[4].to_bytes();
    let mut torn = whole.clone();
    torn.extend_from_slice(&(next.len() as u32).to_le_bytes());
    torn.extend_from_slice(&next[..next.len() / 2]);
    fs::write(&file, &torn).unwrap();

    let (mut copy, tail) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
    let tail = tail.unwrap();
    assert_eq!(tail.kept_records, 4);
    assert_eq!(tail.cut_bytes, (torn.len() - whole.len()) as u64);
    assert_eq!(copy.len(), 4);
    assert_eq!(fs::read(&file).unwrap(), whole, "the file was cut");
    // The fifth record is taken after the last whole one.
    let taken = copy.take(chain(5, 4).records()[4..].to_vec(), &devices());
    assert_eq!(taken.appended, 1);
    let (copy, tail) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
    assert!(tail.is_none());
    assert_eq!(copy.len(), 5);
    // A cut inside the length itself.
    let mut torn = fs::read(&file).unwrap();
    torn.extend_from_slice(&[1, 0]);
    fs::write(&file, &torn).unwrap();
    let (_, tail) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
    assert_eq!(tail.unwrap().cut_bytes, 2);

    // The link says so at its start.
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let cd = Dir::new();
    let mut torn = fs::read(&file).unwrap();
    torn.extend_from_slice(&[5, 0, 0]);
    fs::write(&file, &torn).unwrap();
    let mut link = OwnerLink::new(
        courier(
            &relay,
            id(1),
            Role::Owner,
            (counter(), Role::Counter),
            &cd,
            FirstStart::Yes,
        ),
        dir.path(),
        devices(),
        vec![counter()],
        config(),
    )
    .unwrap();
    assert!(link.take_notes().iter().any(|n| n.contains("cut short")));
}

#[test]
fn lk_58_a_copy_that_was_edited_on_disk_is_found_when_it_is_read_back() {
    let c = chain(3, 5);
    let dir = Dir::new();
    {
        let (mut copy, _) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
        copy.take(c.records().to_vec(), &devices());
    }
    let hex: String = counter()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let file = dir.path().join("counters").join(hex).join("records");
    let mut bytes = fs::read(&file).unwrap();
    // A byte of the second record's body.
    let first = c.records()[0].to_bytes().len() + 4;
    bytes[first + 4 + 5 + 8 + 32 + 32 + 1 + 4] ^= 1;
    fs::write(&file, &bytes).unwrap();
    let err = CounterCopy::open(dir.path(), counter(), &devices()).unwrap_err();
    assert!(matches!(err, CopyError::NotAChain(_)), "{err}");
    assert!(err.to_string().contains("LK-58"));
    // Garbage that is not a record at all.
    fs::write(&file, [5, 0, 0, 0, 1, 2, 3, 4, 5]).unwrap();
    let err = CounterCopy::open(dir.path(), counter(), &devices()).unwrap_err();
    assert!(matches!(err, CopyError::Corrupt { .. }), "{err}");
}

#[test]
fn lk_59_a_record_that_is_held_already_is_skipped_without_a_word() {
    let mut w = world();
    let c = chain(5, 6);
    w.send_records(c.records());
    w.step(T0);
    // The same again, a part of it again, and the whole with one more.
    w.send_records(c.records());
    w.send_records(&c.records()[1..3]);
    let mut longer = c.clone();
    longer
        .append(&id(3), RecordKind::Sale, b"six".to_vec())
        .unwrap();
    w.send_records(longer.records());
    w.step(T0 + 1);
    assert_eq!(w.copy().len(), 6);
    assert_eq!(w.copy().head(), longer.head());
    assert!(w.notes().is_empty(), "no word: {:?}", w.notes());
    assert!(w.link.alerts().is_empty());
    // One answer for the three messages of the step.
    let got = receipts(&w.counter_gets());
    assert_eq!(got.last(), Some(&(6, longer.head())));
    assert_eq!(got.len(), 2, "one for each step");
}

#[test]
fn lk_59_a_different_record_at_the_same_number_is_a_divergence_and_nothing_is_overwritten() {
    let mut w = world();
    let honest = chain(5, 7);
    w.send_records(honest.records());
    w.step(T0);
    let before = fs::read(w.copy().dir().join("records")).unwrap();

    // The counter shows another history from record 3 on.
    let mut fork = Chain::new();
    for r in &honest.records()[..3] {
        fork.append(&id(3), r.kind, r.body.clone()).unwrap();
    }
    fork.append(&id(3), RecordKind::Sale, b"rewritten".to_vec())
        .unwrap();
    fork.append(&id(3), RecordKind::Sale, b"and more".to_vec())
        .unwrap();
    assert_eq!(fork.records()[2], honest.records()[2]);
    w.send_records(&fork.records()[3..]);
    w.step(T0 + 1);
    assert_eq!(
        w.link.alerts(),
        &[Alert::Diverged {
            counter: counter(),
            seq: 3
        }]
    );
    assert_eq!(w.copy().head(), honest.head());
    assert_eq!(w.copy().len(), 5);
    assert_eq!(w.copy().records(), honest.records());
    assert_eq!(
        fs::read(w.copy().dir().join("records")).unwrap(),
        before,
        "nothing was written"
    );
    assert!(w.notes().iter().any(|n| n.contains("LK-59")));
    // Raised once, however often it is shown.
    w.send_records(&fork.records()[3..]);
    w.step(T0 + 2);
    assert_eq!(diverged(w.link.alerts()), vec![3]);
    // The counter is told what the copy holds, which is the honest chain.
    assert_eq!(
        receipts(&w.counter_gets()).last(),
        Some(&(5, honest.head()))
    );
    // It stays until the owner clears it.
    let alert = w.link.alerts()[0].clone();
    w.link.clear(&alert);
    assert!(w.link.alerts().is_empty());
}

#[test]
fn lk_59_a_record_that_follows_another_than_the_one_held_is_a_divergence_too() {
    let mut w = world();
    let honest = chain(3, 8);
    w.send_records(honest.records());
    w.step(T0);
    let mut fork = Chain::new();
    for r in &honest.records()[..2] {
        fork.append(&id(3), r.kind, r.body.clone()).unwrap();
    }
    fork.append(&id(3), RecordKind::Sale, b"other third".to_vec())
        .unwrap();
    fork.append(&id(3), RecordKind::Sale, b"fourth".to_vec())
        .unwrap();
    // Only the fourth: it is the next number, and it follows the fork.
    w.send_records(&fork.records()[3..]);
    w.step(T0 + 1);
    assert_eq!(diverged(w.link.alerts()), vec![2]);
    assert_eq!(w.copy().len(), 3);
    assert_eq!(w.copy().head(), honest.head());
}

#[test]
fn lk_60_a_check_in_is_kept_as_the_latest_and_an_older_one_is_ignored() {
    let mut w = world();
    let mut c = chain(3, 9);
    c.append_check_in(&id(3));
    let first = c.records()[3].clone();
    c.append(&id(3), RecordKind::Sale, b"x".to_vec()).unwrap();
    c.append_check_in(&id(3));
    let second = c.records()[5].clone();

    // The check-in comes before the records it speaks of: kept, and not yet
    // matched.
    w.send_check_in(&first);
    w.step(T0);
    assert_eq!(w.copy().check_in(), Some(&first));
    assert_eq!(w.link.state(&counter()).unwrap().last_check_in_at, Some(T0));
    assert_eq!(w.link.state(&counter()).unwrap().check_in_seq, Some(3));
    w.send_records(c.records());
    w.send_check_in(&second);
    w.step(T0 + 1);
    assert_eq!(w.copy().len(), 6);
    assert_eq!(w.copy().check_in(), Some(&second));
    assert!(w.link.alerts().is_empty());
    assert!(w.copy().diverged_at().is_none());
    // The older one again: ignored, and the latest stays.
    w.send_check_in(&first);
    w.step(T0 + 2);
    assert_eq!(w.copy().check_in(), Some(&second));
    assert_eq!(
        w.link.state(&counter()).unwrap().last_check_in_at,
        Some(T0 + 1)
    );
    assert!(w.notes().iter().any(|n| n.contains("older check-in")));
    // A restart finds it on disk.
    let (again, _) = CounterCopy::open(w.dir.path(), counter(), &devices()).unwrap();
    assert_eq!(again.check_in(), Some(&second));
}

#[test]
fn lk_60_a_check_in_that_is_not_genuine_is_refused() {
    let dir = Dir::new();
    let (mut copy, _) = CounterCopy::open(dir.path(), counter(), &devices()).unwrap();
    let mut c = chain(2, 10);
    c.append_check_in(&id(3));
    let good = c.records()[2].clone();
    // A record that is not a check-in.
    let r = copy.take_check_in(c.records()[0].clone(), &devices());
    assert!(matches!(r, CheckInTaken::Refused(_)), "{r:?}");
    // A bad signature.
    let mut bad = good.clone();
    bad.sig.0[3] ^= 1;
    assert!(matches!(
        copy.take_check_in(bad, &devices()),
        CheckInTaken::Refused(_)
    ));
    // The owner's own key, which is on the list, is not this counter.
    let mut other = Chain::new();
    other.append_check_in(&id(1));
    assert!(matches!(
        copy.take_check_in(other.records()[0].clone(), &devices()),
        CheckInTaken::Refused(_)
    ));
    // A key that is not on the list.
    let mut stranger = Chain::new();
    stranger.append_check_in(&id(7));
    assert!(matches!(
        copy.take_check_in(stranger.records()[0].clone(), &devices()),
        CheckInTaken::Refused(_)
    ));
    // A check-in that says something other than its place.
    let mut lying = Chain::new();
    lying
        .append(
            &id(3),
            RecordKind::CheckIn,
            [&5u64.to_le_bytes()[..], &[1; 32]].concat(),
        )
        .unwrap();
    assert!(matches!(
        copy.take_check_in(lying.records()[0].clone(), &devices()),
        CheckInTaken::Refused(_)
    ));
    assert!(copy.check_in().is_none());
    assert_eq!(
        copy.take_check_in(good, &devices()),
        CheckInTaken::Taken { matches: false }
    );
}

#[test]
fn lk_60_a_check_in_that_the_copy_holds_differently_is_a_divergence() {
    let mut w = world();
    let honest = chain(4, 11);
    w.send_records(honest.records());
    w.step(T0);

    // The counter's chain forks at record 2 and checks in at record 4.
    let mut fork = Chain::new();
    for r in &honest.records()[..2] {
        fork.append(&id(3), r.kind, r.body.clone()).unwrap();
    }
    fork.append(&id(3), RecordKind::Sale, b"forked".to_vec())
        .unwrap();
    fork.append(&id(3), RecordKind::Sale, b"on".to_vec())
        .unwrap();
    fork.append_check_in(&id(3));
    let check_in = fork.records()[4].clone();
    w.send_check_in(&check_in);
    w.step(T0 + 1);
    // The check-in claims 4 records with a head the copy does not have at
    // record 3: it differs from record 3 on (the copy is the honest one).
    assert_eq!(diverged(w.link.alerts()), vec![3]);
    assert_eq!(w.copy().records(), honest.records());
    // The evidence is kept, and found again after a restart.
    let (again, _) = CounterCopy::open(w.dir.path(), counter(), &devices()).unwrap();
    assert_eq!(again.check_in(), Some(&check_in));
    assert_eq!(again.diverged_at(), Some(3));
}

#[test]
fn lk_60_the_counter_is_answered_once_for_each_step_however_many_messages_came() {
    let mut w = world();
    let c = chain(30, 12);
    for part in c.records().chunks(10) {
        w.send_records(part);
    }
    w.step(T0);
    assert_eq!(w.copy().len(), 30);
    assert_eq!(receipts(&w.counter_gets()), vec![(30, c.head())]);
    // A step with no records says nothing.
    w.step(T0 + 1);
    assert!(receipts(&w.counter_gets()).is_empty());
}

fn command(seq: u64) -> SignedCommand {
    let command = OwnerCommand {
        counter: counter(),
        seq,
        action: Action::ClearCancelAlarm,
    };
    SignedCommand {
        owner_sig: command.sign(&id(1)),
        command,
    }
}

fn batch() -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: counter(),
        start: 0,
        count: 2,
        scripts: (1..=2u8)
            .map(|n| ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32])))
            .collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

#[test]
fn lk_61_a_heartbeat_goes_to_each_counter_every_twenty_seconds() {
    let mut w = world();
    w.rig.feed(vec![peers()]);
    let heartbeats = |w: &mut World| -> Vec<u64> {
        w.counter_gets()
            .iter()
            .filter(|r| r.kind == Kind::Heartbeat)
            .map(|r| {
                SignedHeartbeat::from_bytes(&r.content)
                    .unwrap()
                    .verify(&roots(), &counter(), 0)
                    .unwrap()
                    .seq
            })
            .collect()
    };
    w.step(T0);
    let first = heartbeats(&mut w);
    assert_eq!(first.len(), 1);
    for dt in [1, 10, 19] {
        w.step(T0 + dt);
        assert!(heartbeats(&mut w).is_empty(), "at +{dt}");
    }
    w.step(T0 + 20);
    let second = heartbeats(&mut w);
    assert_eq!(second.len(), 1);
    assert!(second[0] > first[0]);
    // A relay that is down costs no number.
    w.relay.set_down(true);
    w.step(T0 + 40);
    assert_eq!(w.rig.watcher.last_seq(), second[0]);
}

#[test]
fn lk_61_a_command_and_a_batch_are_sent_when_asked() {
    let mut w = world();
    w.link.send_command(&counter(), &command(4)).unwrap();
    w.link.send_batch(&counter(), &batch(), T0).unwrap();
    let got = w.counter_gets();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].kind, Kind::Command);
    assert_eq!(
        SignedCommand::from_bytes(&got[0].content).unwrap(),
        command(4)
    );
    assert_eq!(got[1].kind, Kind::Batch);
    assert_eq!(SignedBatch::from_bytes(&got[1].content).unwrap(), batch());
    // Not to a device that is not the owner's counter.
    assert!(w
        .link
        .send_command(&id(8).device_id(), &command(5))
        .is_err());
    // A relay that is down is an error the caller may try again.
    w.relay.set_down(true);
    let err = w.link.send_command(&counter(), &command(5)).unwrap_err();
    assert!(err.is_retryable());
}

#[test]
fn lk_62_a_missed_check_in_is_one_alert_until_a_check_in_clears_it() {
    let mut w = world();
    w.step(T0);
    w.step(T0 + 1799);
    assert!(w.link.alerts().is_empty());
    w.step(T0 + 1800);
    let alert = Alert::MissedCheckIn {
        counter: counter(),
        last_at: T0,
    };
    assert_eq!(w.link.alerts(), std::slice::from_ref(&alert));
    // Raised once, not on every step.
    for dt in [1801, 2000, 5000] {
        w.step(T0 + dt);
    }
    assert_eq!(w.link.alerts(), &[alert]);
    // A check-in clears it.
    let mut c = chain(1, 13);
    c.append_check_in(&id(3));
    w.send_check_in(&c.records()[1]);
    w.step(T0 + 5001);
    assert!(w.link.alerts().is_empty());
    // And a new silence is a new alert, from that check-in.
    w.step(T0 + 5001 + 1799);
    assert!(w.link.alerts().is_empty());
    w.step(T0 + 5001 + 1800);
    assert_eq!(
        w.link.alerts(),
        &[Alert::MissedCheckIn {
            counter: counter(),
            last_at: T0 + 5001
        }]
    );
}

#[test]
fn lk_62_the_alert_comes_though_the_relay_is_down() {
    let mut w = world();
    w.relay.set_down(true);
    w.step(T0);
    w.step(T0 + 1800);
    assert_eq!(w.link.alerts().len(), 1);
    assert!(!w.link.courier().reachable());
}

#[test]
fn lk_63_a_message_that_does_not_decode_is_noted_and_the_step_goes_on() {
    let mut w = world();
    w.counter_side
        .send(&owner(), Kind::Records, b"not records")
        .unwrap();
    w.counter_side
        .send(&owner(), Kind::CheckIn, b"not a check-in")
        .unwrap();
    // A check-in that is a record but not a check-in.
    let c = chain(2, 14);
    w.send_check_in(&c.records()[0]);
    w.send_records(c.records());
    w.step(T0);
    assert_eq!(w.copy().len(), 2, "the good message is still taken");
    let notes = w.notes();
    assert!(
        notes.iter().any(|n| n.contains("do not decode")),
        "{notes:?}"
    );
    assert!(notes.iter().any(|n| n.contains("does not decode")));
    assert!(notes.iter().any(|n| n.contains("was refused")));
    assert!(w.link.alerts().is_empty());
}

fn counter_node(dir: &Dir) -> (Node<FakePort>, Sender<AppEvent>) {
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter());
    settings.counters.insert(counter(), 10_000);
    let pairing = Pairing {
        roots: roots(),
        wallet_checksum: [9; 32],
        batch_policy: BatchPolicy { min_count: 1 },
    };
    let mut cfg = NodeConfig::new(
        dir.path(),
        id(3),
        pairing,
        settings,
        DriverConfig::new(Network::Regtest, 0),
    );
    cfg.health = Box::new(|_| healthy());
    let (tx, rx) = channel();
    let node = Node::start_with(cfg, move |_| Ok(FakePort(rx.into()))).unwrap();
    (node, tx)
}

fn counter_link(
    relay: &MemoryRelay,
    dir: &Dir,
    first: FirstStart,
    max_content: usize,
) -> CounterLink<MemoryPost> {
    CounterLink::new(
        courier(
            relay,
            id(3),
            Role::Counter,
            (owner(), Role::Owner),
            dir,
            first,
        ),
        owner(),
        CounterLinkConfig {
            poll_secs: 0,
            retry_secs: 0,
            max_content,
            ..CounterLinkConfig::default()
        },
    )
}

fn owner_link(relay: &MemoryRelay, dir: &Dir, first: FirstStart) -> OwnerLink<MemoryPost> {
    OwnerLink::new(
        courier(
            relay,
            id(1),
            Role::Owner,
            (counter(), Role::Counter),
            dir,
            first,
        ),
        dir.path(),
        devices(),
        vec![counter()],
        config(),
    )
    .unwrap()
}

/// How many records the copy's file holds, counted from the bytes.
fn records_in_file(copy: &CounterCopy) -> usize {
    let bytes = fs::read(copy.dir().join("records")).unwrap();
    let mut rest = bytes.as_slice();
    let mut n = 0;
    while !rest.is_empty() {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        rest = &rest[4 + len..];
        n += 1;
    }
    n
}

#[test]
fn lk_56_three_hundred_records_reach_the_owner_across_several_messages() {
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let (node_dir, link_dir, owner_dir, watcher_dir) =
        (Dir::new(), Dir::new(), Dir::new(), Dir::new());
    let (mut node, _events) = counter_node(&node_dir);
    for _ in 0..300 {
        node.check_in().unwrap();
    }
    let mut counter_side = counter_link(&relay, &link_dir, FirstStart::Yes, 4_000);
    let mut owner_side = owner_link(&relay, &owner_dir, FirstStart::Yes);
    let mut rig = start(watcher_dir.path());
    // The owner's mailbox exists before the counter sends.
    owner_side.courier_mut().poll().unwrap();
    let mut rounds = 0;
    for now in T0..T0 + 100 {
        counter_side.step(&mut node, now);
        owner_side.step(&mut rig.watcher, now);
        rounds += 1;
        let copy = owner_side.copy(&counter()).unwrap();
        // The link's own check-in is a record too.
        if copy.len() == node.records_len() && rounds > 2 {
            break;
        }
    }
    let theirs = node.records_from(0, usize::MAX).unwrap();
    let copy = owner_side.copy(&counter()).unwrap();
    assert_eq!(copy.len(), node.records_len());
    assert!(copy.len() >= 301);
    assert_eq!(copy.head(), node.records_head());
    assert_eq!(copy.records(), theirs.as_slice());
    assert_eq!(records_in_file(copy), copy.records().len());
    // It took several messages: more than one step's four.
    assert!(counter_side.state().record_messages_sent > 8);
    assert!(rounds > 3);
    // The owner's mark reaches the counter at its next step, and then the
    // counter stops sending.
    counter_side.step(&mut node, T0 + 99);
    let copy = owner_side.copy(&counter()).unwrap();
    assert_eq!(counter_side.state().owner_records, copy.len());
    let sent = counter_side.state().record_messages_sent;
    for now in T0 + 100..T0 + 110 {
        counter_side.step(&mut node, now);
        owner_side.step(&mut rig.watcher, now);
    }
    assert_eq!(counter_side.state().record_messages_sent, sent);
    // The check-in is held, and agrees.
    let copy = owner_side.copy(&counter()).unwrap();
    assert!(copy.check_in().is_some());
    assert!(copy.diverged_at().is_none());
    assert!(owner_side.alerts().is_empty());
    assert_eq!(
        decode_received(&openbmt_courier::received_content(1, &GENESIS))
            .unwrap()
            .0,
        1
    );
}

#[test]
fn lk_58_a_restart_of_both_sides_in_the_middle_goes_on_without_duplicates() {
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let (node_dir, link_dir, owner_dir, watcher_dir) =
        (Dir::new(), Dir::new(), Dir::new(), Dir::new());
    let (mut node, events) = counter_node(&node_dir);
    for _ in 0..300 {
        node.check_in().unwrap();
    }
    let mut rig = start(watcher_dir.path());
    let mut counter_side = counter_link(&relay, &link_dir, FirstStart::Yes, 4_000);
    let mut owner_side = owner_link(&relay, &owner_dir, FirstStart::Yes);
    owner_side.courier_mut().poll().unwrap();
    // Two rounds: part of the chain has arrived, not all.
    let mut now = T0;
    for _ in 0..2 {
        counter_side.step(&mut node, now);
        owner_side.step(&mut rig.watcher, now);
        now += 1;
    }
    let partway = owner_side.copy(&counter()).unwrap().len();
    assert!(partway > 0 && partway < 300, "{partway}");

    // Both sides restart: the counter's node and link, and the owner's link.
    drop(counter_side);
    drop(owner_side);
    drop(node);
    drop(events);
    let (mut node, _events) = counter_node(&node_dir);
    let mut counter_side = counter_link(&relay, &link_dir, FirstStart::No, 4_000);
    let mut owner_side = owner_link(&relay, &owner_dir, FirstStart::No);
    assert_eq!(owner_side.copy(&counter()).unwrap().len(), partway);
    // The counter has forgotten the owner's mark and sends from the start.
    assert_eq!(counter_side.state().owner_records, 0);
    for _ in 0..60 {
        counter_side.step(&mut node, now);
        owner_side.step(&mut rig.watcher, now);
        now += 1;
    }
    assert!(owner_side.alerts().is_empty());
    let notes: Vec<String> = owner_side.take_notes();
    let copy = owner_side.copy(&counter()).unwrap();
    assert_eq!(
        copy.records(),
        node.records_from(0, usize::MAX).unwrap().as_slice()
    );
    assert_eq!(records_in_file(copy), copy.records().len());
    assert!(
        !notes.iter().any(|n| n.contains("stopped")),
        "what was held twice was skipped in silence: {notes:?}"
    );
    // The files read back as the same chain.
    let (again, torn) = CounterCopy::open(owner_dir.path(), counter(), &devices()).unwrap();
    assert!(torn.is_none());
    assert_eq!(again.records(), copy.records());
    // And the counter's numbers went on rather than starting again.
    assert!(counter_side.courier().outbox().last(&owner()) > 2);
}

// LK-64 to LK-66: the settings the owner signed travel to the devices.

fn box_key() -> DeviceId {
    id(5).device_id()
}

/// Settings of `version` that list the counter and the Box, changed by `f`.
fn listed(version: u32, f: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    s.devices.insert(counter());
    s.devices.insert(box_key());
    s.counters.insert(counter(), 10_000);
    s.boxes.insert(box_key());
    f(&mut s);
    s
}

fn signed_by_owner(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
    }
}

fn signed_by_both(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

/// A world with a Box that is a member of the relay and reads its mailbox.
struct WithBox {
    w: World,
    box_side: Courier<MemoryPost>,
    _box_dir: Dir,
}

fn with_box() -> WithBox {
    let relay =
        MemoryRelay::new(id(99), &[owner(), counter(), box_key(), id(7).device_id()]).unwrap();
    let (dir, watcher_dir, counter_dir, box_dir) = (Dir::new(), Dir::new(), Dir::new(), Dir::new());
    let rig = start(watcher_dir.path());
    let owner_courier = courier(
        &relay,
        id(1),
        Role::Owner,
        (counter(), Role::Counter),
        &dir,
        FirstStart::Yes,
    );
    let mut link = OwnerLink::new(
        owner_courier,
        dir.path(),
        devices(),
        vec![counter()],
        config(),
    )
    .unwrap();
    let mut counter_side = courier(
        &relay,
        id(3),
        Role::Counter,
        (owner(), Role::Owner),
        &counter_dir,
        FirstStart::Yes,
    );
    counter_side.poll().unwrap();
    let mut box_side = courier(
        &relay,
        id(5),
        Role::Box,
        (owner(), Role::Owner),
        &box_dir,
        FirstStart::Yes,
    );
    box_side.poll().unwrap();
    link.courier_mut().poll().unwrap();
    WithBox {
        w: World {
            rig,
            link,
            counter_side,
            relay,
            dir,
            _watcher_dir: watcher_dir,
            _counter_dir: counter_dir,
        },
        box_side,
        _box_dir: box_dir,
    }
}

fn settings_in(got: &[Received]) -> Vec<Vec<u8>> {
    got.iter()
        .filter(|r| r.kind == Kind::Settings)
        .map(|r| r.content.clone())
        .collect()
}

/// The counter's chain, which grows by `Setting` records; `sent` is how many
/// of its records the owner was given.
struct Shown {
    chain: Chain,
    sent: usize,
}

impl Shown {
    fn new() -> Self {
        Self {
            chain: Chain::new(),
            sent: 0,
        }
    }

    /// The counter writes a `Setting` record for `version` and sends the new
    /// records to the owner.
    fn holds(&mut self, w: &mut World, version: u32) {
        let note = SettingNote {
            version,
            change: Change::Tightening,
            hash: [version as u8; 32],
        };
        self.chain
            .append(&id(3), RecordKind::Setting, note.to_body())
            .unwrap();
        self.flush(w);
    }

    fn flush(&mut self, w: &mut World) {
        let all = self.chain.records();
        w.send_records(&all[self.sent..]);
        self.sent = all.len();
    }
}

#[test]
fn lk_64_settings_go_to_the_counters_and_boxes_on_the_list_once_they_check() {
    let mut b = with_box();
    let first = listed(1, |_| {});
    let tighter = signed_by_owner(listed(2, |s| s.light_per_sale.sats -= 1));
    b.w.link
        .carry_settings(7, &tighter, &roots(), Some(&first), T0)
        .unwrap();
    // Sent at once, to the counter and to the Box, as the bytes signed.
    assert_eq!(settings_in(&b.w.counter_gets()), vec![tighter.to_bytes()]);
    assert_eq!(
        settings_in(&b.box_side.poll().unwrap()),
        vec![tighter.to_bytes()]
    );
    // A loosening without the Key app's signature is never sent.
    let looser = signed_by_owner(listed(3, |s| s.light_per_sale.sats += 50));
    let err =
        b.w.link
            .carry_settings(8, &looser, &roots(), Some(&tighter.settings), T0)
            .unwrap_err();
    assert!(err.to_string().starts_with("LK-64: "), "{err}");
    assert_eq!(
        err,
        SettingsNotCarried::Check(SettingsError::ApproverRequired)
    );
    // Nor are settings signed by a key that is not the owner root.
    let mut forged = signed_by_both(listed(3, |s| s.light_per_sale.sats += 50));
    forged.owner_sig = forged.settings.sign(&id(9));
    assert!(b
        .w
        .link
        .carry_settings(9, &forged, &roots(), Some(&tighter.settings), T0)
        .is_err());
    // Without the settings before them, the signatures there are checked.
    assert!(b
        .w
        .link
        .carry_settings(9, &forged, &roots(), None, T0)
        .is_err());
    assert!(settings_in(&b.w.counter_gets()).is_empty());
    assert!(settings_in(&b.box_side.poll().unwrap()).is_empty());
}

#[test]
fn lk_64_a_device_takes_the_versions_in_order_and_is_not_sent_one_every_step() {
    let mut b = with_box();
    let v1 = listed(1, |_| {});
    let v2 = signed_by_owner(listed(2, |s| s.light_per_sale.sats -= 1));
    let v3 = signed_by_both(listed(3, |s| s.light_per_sale.sats += 5));
    b.w.link
        .carry_settings(1, &v2, &roots(), Some(&v1), T0)
        .unwrap();
    b.w.link
        .carry_settings(2, &v3, &roots(), Some(&v2.settings), T0)
        .unwrap();
    // The counter gets version 2 only; the Box takes each once, in order.
    assert_eq!(settings_in(&b.w.counter_gets()), vec![v2.to_bytes()]);
    assert_eq!(
        settings_in(&b.box_side.poll().unwrap()),
        vec![v2.to_bytes(), v3.to_bytes()]
    );
    // Not again within the resend time, however many steps.
    for t in 1..5 {
        b.w.step(T0 + t);
    }
    assert!(settings_in(&b.w.counter_gets()).is_empty());
    // After it, the version the counter does not show is sent again.
    b.w.step(T0 + 130);
    assert_eq!(settings_in(&b.w.counter_gets()), vec![v2.to_bytes()]);
    // The counter shows version 2 by its record: version 3 is next.
    let mut shown = Shown::new();
    shown.holds(&mut b.w, 2);
    b.w.step(T0 + 131);
    assert_eq!(b.w.copy().settings_held(), Some(2));
    assert_eq!(settings_in(&b.w.counter_gets()), vec![v3.to_bytes()]);
    // And nothing more once it shows version 3; the Box is not sent again.
    shown.holds(&mut b.w, 3);
    b.w.step(T0 + 400);
    assert!(settings_in(&b.w.counter_gets()).is_empty());
    assert!(settings_in(&b.box_side.poll().unwrap()).is_empty());
}

#[test]
fn lk_64_a_device_the_new_settings_remove_gets_the_settings_that_remove_it() {
    let mut b = with_box();
    let v1 = listed(1, |_| {});
    let removed = signed_by_both(listed(2, |s| {
        s.counters.clear();
        s.devices.remove(&counter());
    }));
    b.w.link
        .carry_settings(4, &removed, &roots(), Some(&v1), T0)
        .unwrap();
    assert_eq!(settings_in(&b.w.counter_gets()), vec![removed.to_bytes()]);
    assert_eq!(
        settings_in(&b.box_side.poll().unwrap()),
        vec![removed.to_bytes()]
    );
}

#[test]
fn lk_65_a_counter_shows_it_holds_a_version_by_its_setting_record_and_a_box_by_the_relay() {
    let mut b = with_box();
    let v1 = listed(1, |_| {});
    let v2 = signed_by_owner(listed(2, |s| s.light_per_sale.sats -= 1));
    assert!(b.w.link.settings_missing(3).is_empty(), "nothing carried");
    b.w.link
        .carry_settings(3, &v2, &roots(), Some(&v1), T0)
        .unwrap();
    // The Box is counted once the relay took the message; the counter is
    // missing until its record is in the copy.
    assert_eq!(
        b.w.link.settings_missing(3),
        vec![(counter(), Role::Counter)]
    );
    // A record of another kind does not show it.
    let mut shown = Shown::new();
    shown
        .chain
        .append(&id(3), RecordKind::Sale, vec![2, 0, 7])
        .unwrap();
    shown.flush(&mut b.w);
    b.w.step(T0 + 1);
    assert_eq!(b.w.copy().settings_held(), None);
    assert_eq!(b.w.link.settings_missing(3).len(), 1);
    shown.holds(&mut b.w, 2);
    b.w.step(T0 + 2);
    assert_eq!(b.w.copy().settings_held(), Some(2));
    assert!(b.w.link.settings_missing(3).is_empty());
}

#[test]
fn lk_66_settings_stay_until_every_device_holds_them_and_are_then_forgotten() {
    let mut b = with_box();
    let v1 = listed(1, |_| {});
    let v2 = signed_by_owner(listed(2, |s| s.light_per_sale.sats -= 1));
    b.w.link
        .carry_settings(3, &v2, &roots(), Some(&v1), T0)
        .unwrap();
    assert!(!b.w.link.settings_missing(3).is_empty());
    let mut shown = Shown::new();
    shown.holds(&mut b.w, 2);
    b.w.step(T0 + 1);
    assert!(b.w.link.settings_missing(3).is_empty());
    b.w.link.settings_done(3);
    assert!(b.w.link.settings_missing(3).is_empty());
    // Carried again, it is a new delivery: nothing is left of the old one.
    b.w.link
        .carry_settings(3, &v2, &roots(), Some(&v1), T0 + 2)
        .unwrap();
    assert!(b.w.link.settings_missing(3).is_empty());

    // The link carries only so many settings.
    let mut b = with_box();
    let mut previous = listed(1, |_| {});
    for i in 0..crate::MAX_CARRIED_SETTINGS as u32 {
        let next = signed_by_owner(listed(2 + i, |s| s.light_per_sale.sats -= 1 + u64::from(i)));
        b.w.link
            .carry_settings(i, &next, &roots(), Some(&previous), T0)
            .unwrap();
        previous = next.settings;
    }
    let over = signed_by_owner(listed(99, |s| s.light_per_sale.sats = 1));
    assert_eq!(
        b.w.link
            .carry_settings(99, &over, &roots(), Some(&previous), T0),
        Err(SettingsNotCarried::Full)
    );
}

#[test]
fn lk_67_the_link_talks_to_the_counters_of_the_settings_and_drops_a_removed_one_when_done() {
    let mut b = with_box();
    let second = id(7).device_id();
    // A counter the settings add is talked to at once.
    b.w.link.follow(&[counter(), second], &[box_key()]).unwrap();
    assert_eq!(b.w.link.counters(), [counter(), second]);
    assert!(b.w.link.copy(&second).is_some());
    // With nothing carried for it, a counter the settings drop is dropped.
    b.w.link.follow(&[counter()], &[box_key()]).unwrap();
    assert_eq!(b.w.link.counters(), [counter()]);
    assert!(b.w.link.copy(&second).is_none());

    // Settings that remove it are sent to it; it stays until they are done.
    b.w.link.follow(&[counter(), second], &[box_key()]).unwrap();
    let both = listed(1, |s| {
        s.devices.insert(second);
        s.counters.insert(counter(), 5_000);
        s.counters.insert(second, 5_000);
    });
    let removal = signed_by_both(listed(2, |s| {
        s.counters.insert(counter(), 10_000);
    }));
    b.w.link
        .carry_settings(5, &removal, &roots(), Some(&both), T0)
        .unwrap();
    b.w.link.follow(&[counter()], &[box_key()]).unwrap();
    assert_eq!(b.w.link.counters(), [counter(), second]);
    assert!(b
        .w
        .link
        .settings_missing(5)
        .iter()
        .any(|(d, _)| *d == second));
    let notes = b.w.notes();
    assert!(
        !notes.iter().any(|n| n.contains("were not sent")),
        "{notes:?}"
    );
    b.w.link.settings_done(5);
    assert_eq!(b.w.link.counters(), [counter()]);
    assert!(b.w.link.copy(&second).is_none());
}

// Section 6, batches that wait to be held (LK-70 to LK-73).

/// A batch of two addresses from `start` for the test counter.
fn batch_at(start: u32) -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: counter(),
        start,
        count: 2,
        scripts: (0..2u8)
            .map(|n| {
                ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array(
                    [(start as u8) * 2 + n + 1; 32],
                ))
            })
            .collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

/// The counter's chain with one record that says it holds each of `batches`.
fn chain_holding(batches: &[&SignedBatch]) -> Chain {
    let mut chain = Chain::new();
    for b in batches {
        let held = BatchHeld::of(b).unwrap();
        chain.append(&id(3), held.kind(), held.encode()).unwrap();
    }
    chain
}

fn batch_messages(got: &[Received]) -> Vec<SignedBatch> {
    got.iter()
        .filter(|r| r.kind == Kind::Batch)
        .map(|r| SignedBatch::from_bytes(&r.content).unwrap())
        .collect()
}

fn batch_alerts(link: &OwnerLink<MemoryPost>) -> Vec<Alert> {
    link.alerts()
        .iter()
        .filter(|a| matches!(a, Alert::BatchUndelivered { .. }))
        .cloned()
        .collect()
}

fn restart_error(w: &World) -> Option<crate::OwnerLinkError> {
    OwnerLink::new(
        courier(
            &w.relay,
            id(1),
            Role::Owner,
            (counter(), Role::Counter),
            &w.dir,
            FirstStart::No,
        ),
        w.dir.path(),
        devices(),
        vec![counter()],
        config(),
    )
    .err()
}

#[test]
fn lk_70_a_batch_is_kept_until_the_copy_shows_the_counter_holds_it() {
    let mut w = world();
    let b = batch_at(0);
    w.link.send_batch(&counter(), &b, T0).unwrap();
    let waiting = w.link.batches_waiting(&counter()).unwrap();
    assert_eq!((waiting.batches, waiting.since), (1, T0));
    // Records that say nothing about it leave it pending.
    let mut c = chain(3, 1);
    w.send_records(c.records());
    w.step(T0 + 1);
    assert!(w.link.batches_waiting(&counter()).is_some());
    // A record for the same range with another digest drops nothing.
    let mut other = batch_at(0);
    other.batch.scripts.reverse();
    let wrong = BatchHeld::of(&other).unwrap();
    c.append(&id(3), wrong.kind(), wrong.encode()).unwrap();
    w.send_records(&c.records()[3..]);
    w.step(T0 + 2);
    assert_eq!(w.copy().len(), 4);
    assert!(w.link.batches_waiting(&counter()).is_some());
    // The record of this batch drops it.
    let held = BatchHeld::of(&b).unwrap();
    c.append(&id(3), held.kind(), held.encode()).unwrap();
    w.send_records(&c.records()[4..]);
    w.step(T0 + 3);
    assert!(w.link.batches_waiting(&counter()).is_none());
    assert!(w.link.alerts().is_empty());
    // Nothing is sent again.
    w.counter_gets();
    w.step(T0 + 1000);
    assert!(batch_messages(&w.counter_gets()).is_empty());
}

#[test]
fn lk_70_a_batch_the_counter_has_passed_is_dropped() {
    let mut w = world();
    let (first, second) = (batch_at(0), batch_at(2));
    w.link.send_batch(&counter(), &first, T0).unwrap();
    w.link.send_batch(&counter(), &second, T0).unwrap();
    assert_eq!(w.link.batches_waiting(&counter()).unwrap().batches, 2);
    // The counter holds the later one only.
    w.send_records(chain_holding(&[&second]).records());
    w.step(T0 + 1);
    let waiting = w.link.batches_waiting(&counter());
    assert!(waiting.is_none(), "{waiting:?}");
    // A batch that is later than the one held stays.
    let mut w = world();
    w.link.send_batch(&counter(), &batch_at(4), T0).unwrap();
    w.send_records(chain_holding(&[&first]).records());
    w.step(T0 + 1);
    assert_eq!(w.link.batches_waiting(&counter()).unwrap().batches, 1);
}

#[test]
fn lk_71_a_pending_batch_is_sent_again_once_in_a_hundred_and_twenty_seconds() {
    let mut w = world();
    let b = batch_at(0);
    w.link.send_batch(&counter(), &b, T0).unwrap();
    assert_eq!(batch_messages(&w.counter_gets()), vec![b.clone()]);
    for dt in [1, 60, 119] {
        w.step(T0 + dt);
        assert!(batch_messages(&w.counter_gets()).is_empty(), "at +{dt}");
    }
    w.step(T0 + 120);
    assert_eq!(batch_messages(&w.counter_gets()), vec![b.clone()]);
    // Once, not at every step, and again after the next 120 seconds.
    w.step(T0 + 121);
    w.step(T0 + 239);
    assert!(batch_messages(&w.counter_gets()).is_empty());
    w.step(T0 + 240);
    assert_eq!(batch_messages(&w.counter_gets()), vec![b]);
}

#[test]
fn lk_71_the_pending_batches_of_a_counter_go_in_the_order_of_their_ranges() {
    let mut w = world();
    let (a, b, c) = (batch_at(0), batch_at(2), batch_at(4));
    // Handed over out of order.
    for x in [&c, &a, &b] {
        w.link.send_batch(&counter(), x, T0).unwrap();
    }
    w.counter_gets();
    w.step(T0 + 120);
    assert_eq!(batch_messages(&w.counter_gets()), vec![a, b, c]);
}

#[test]
fn lk_71_a_send_that_did_not_reach_the_relay_is_tried_again_at_the_next_step() {
    let mut w = world();
    let b = batch_at(0);
    w.relay.set_down(true);
    let err = w.link.send_batch(&counter(), &b, T0).unwrap_err();
    assert!(err.is_retryable());
    assert!(w.link.batches_waiting(&counter()).is_some());
    w.step(T0 + 1);
    w.relay.set_down(false);
    w.step(T0 + 2);
    assert_eq!(batch_messages(&w.counter_gets()), vec![b]);
    // A relay that is down when the resend is due does not use up the turn.
    w.relay.set_down(true);
    w.step(T0 + 122);
    w.relay.set_down(false);
    w.step(T0 + 123);
    assert_eq!(batch_messages(&w.counter_gets()).len(), 1);
}

#[test]
fn lk_72_the_pending_batches_survive_a_restart_and_read_back_strictly() {
    let mut w = world();
    let (a, b) = (batch_at(0), batch_at(2));
    w.link.send_batch(&counter(), &b, T0 + 5).unwrap();
    w.link.send_batch(&counter(), &a, T0).unwrap();
    let file = w.dir.path().join(crate::batch_outbox::BATCH_OUTBOX_FILE);
    let bytes = fs::read(&file).unwrap();
    assert_eq!(&bytes[..5], b"OBM0\x01");
    let waiting = w.link.batches_waiting(&counter()).unwrap();
    assert_eq!((waiting.batches, waiting.since), (2, T0));
    // A link started again on the same directory has them, with the times.
    let mut again = owner_link(&w.relay, &w.dir, FirstStart::No);
    assert_eq!(again.batches_waiting(&counter()), Some(waiting));
    // The first step after the start sends them, oldest range first.
    w.counter_gets();
    again.step(&mut w.rig.watcher, T0 + 10);
    assert_eq!(batch_messages(&w.counter_gets()), vec![a, b]);
    drop(again);
    // A file that is cut anywhere, or has a byte left over, refuses the
    // start.
    for n in 0..bytes.len() {
        fs::write(&file, &bytes[..n]).unwrap();
        assert!(restart_error(&w).is_some(), "cut at {n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    fs::write(&file, &longer).unwrap();
    let err = restart_error(&w).unwrap();
    assert!(err.to_string().contains("LK-72"), "{err}");
    // The bytes as they were read back.
    fs::write(&file, &bytes).unwrap();
    assert!(restart_error(&w).is_none());
}

#[test]
fn lk_72_a_batch_for_a_device_that_is_not_a_counter_is_not_kept() {
    let mut w = world();
    let other = id(8).device_id();
    assert!(w.link.send_batch(&other, &batch_at(0), T0).is_err());
    assert!(w.link.batches_waiting(&other).is_none());
    assert!(!w
        .dir
        .path()
        .join(crate::batch_outbox::BATCH_OUTBOX_FILE)
        .exists());
}

#[test]
fn lk_73_a_batch_that_waits_a_day_raises_one_alert_that_clears_when_it_is_held() {
    let mut w = world();
    let b = batch_at(0);
    w.link.send_batch(&counter(), &b, T0).unwrap();
    let day = 24 * 60 * 60;
    w.step(T0 + day - 1);
    assert!(batch_alerts(&w.link).is_empty());
    w.step(T0 + day);
    let alert = Alert::BatchUndelivered {
        counter: counter(),
        since: T0,
    };
    assert_eq!(batch_alerts(&w.link), vec![alert]);
    // Once, not on every step.
    w.step(T0 + day + 1);
    w.step(T0 + day + 500);
    assert_eq!(batch_alerts(&w.link).len(), 1);
    // The counter shows it holds the batch: the alert goes.
    w.send_records(chain_holding(&[&b]).records());
    w.step(T0 + day + 501);
    assert!(batch_alerts(&w.link).is_empty());
}

#[test]
fn lk_73_the_alert_is_raised_though_the_relay_is_down() {
    let mut w = world();
    w.link.send_batch(&counter(), &batch_at(0), T0).unwrap();
    w.relay.set_down(true);
    w.step(T0 + 24 * 60 * 60);
    assert_eq!(batch_alerts(&w.link).len(), 1);
}

#[test]
fn lk_70_a_counter_node_that_missed_the_first_message_holds_the_batch_after_the_resend() {
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let (node_dir, link_dir, owner_dir, watcher_dir) =
        (Dir::new(), Dir::new(), Dir::new(), Dir::new());
    let (mut node, _events) = counter_node(&node_dir);
    let mut counter_side = counter_link(&relay, &link_dir, FirstStart::Yes, 4_000);
    let mut owner_side = owner_link(&relay, &owner_dir, FirstStart::Yes);
    let mut rig = start(watcher_dir.path());
    owner_side.courier_mut().poll().unwrap();
    let b = batch_at(0);
    let empty = node.counter().batches().to_bytes();
    // The relay is down when the batch is handed over: the counter never has
    // the first message.
    relay.set_down(true);
    assert!(owner_side.send_batch(&counter(), &b, T0).is_err());
    relay.set_down(false);
    for now in T0..T0 + 600 {
        counter_side.step(&mut node, now);
        owner_side.step(&mut rig.watcher, now);
    }
    assert!(owner_side.batches_waiting(&counter()).is_none());
    assert!(owner_side.alerts().is_empty());
    assert_ne!(node.counter().batches().to_bytes(), empty);
    // The counter's records show it once, and the owner's copy is the same.
    let ours = node.records_from(0, usize::MAX).unwrap();
    let held = BatchHeld::of(&b).unwrap();
    let showing = |records: &[Record]| {
        records
            .iter()
            .filter(|r| r.kind == RecordKind::Setting && r.body == held.encode())
            .count()
    };
    assert_eq!(showing(&ours), 1);
    assert_eq!(
        owner_side.copy(&counter()).unwrap().records(),
        ours.as_slice()
    );
    // The owner hands the same batch over again: nothing changes.
    owner_side.send_batch(&counter(), &b, T0 + 700).unwrap();
    for now in T0 + 700..T0 + 900 {
        counter_side.step(&mut node, now);
        owner_side.step(&mut rig.watcher, now);
    }
    let after = node.records_from(0, usize::MAX).unwrap();
    assert_eq!(showing(&after), 1, "no second record");
    assert!(owner_side.batches_waiting(&counter()).is_none());
}
