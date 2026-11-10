//! The owner's watcher against a driver that the test feeds by hand.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use bitcoin::blockdata::constants::genesis_block;
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Network, ScriptBuf};
use openbmt_core::batch::PinnedRoots;
use openbmt_core::heartbeat::SignedHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_counter_node::DriverPort;
use openbmt_net_io::{AppEvent, Clock, DriverConfig};
use openbmt_watch::{HeaderChain, Health, Peer, PeerAddr};

use crate::{OwnerConfig, OwnerStartError, OwnerWatcher, HEARTBEAT_FILE};

const T0: u64 = 1_700_000_000;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fn counter() -> DeviceId {
    id(3).device_id()
}

fn roots() -> PinnedRoots {
    PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap()
}

fn genesis() -> BlockHash {
    genesis_block(Network::Regtest).block_hash()
}

struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "openbmt-owner-node-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct FakePort(Receiver<AppEvent>);

impl DriverPort for FakePort {
    fn events(&self) -> &Receiver<AppEvent> {
        &self.0
    }
    fn watch_script(&self, _: ScriptBuf) {}
    fn set_all_blocks(&self, _: bool) {}
    fn shutdown(&mut self) {}
}

#[derive(Clone)]
struct TestClock(Arc<AtomicU64>);

impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct Rig {
    watcher: OwnerWatcher<FakePort>,
    events: Sender<AppEvent>,
    time: Arc<AtomicU64>,
}

impl Rig {
    fn feed(&mut self, events: Vec<AppEvent>) {
        for event in events {
            self.events.send(event).unwrap();
        }
        self.watcher.pump(Duration::ZERO);
    }
}

fn healthy() -> Health {
    Health {
        healthy: true,
        problem: None,
        behind: false,
    }
}

/// A watcher whose health rule is `rule`.
fn start_with_rule(
    dir: &Path,
    time: &Arc<AtomicU64>,
    rule: impl Fn(&openbmt_watch::HealthInput<'_>) -> Health + 'static,
) -> Rig {
    let mut config = OwnerConfig::new(dir, id(1), DriverConfig::new(Network::Regtest, 0));
    config.clock = Box::new(TestClock(time.clone()));
    config.health = Box::new(rule);
    let (events, rx) = channel();
    let watcher = OwnerWatcher::start_with(config, move |_, _| Ok(FakePort(rx))).unwrap();
    Rig {
        watcher,
        events,
        time: time.clone(),
    }
}

fn start(dir: &Path) -> Rig {
    start_with_rule(dir, &Arc::new(AtomicU64::new(T0)), |_| healthy())
}

fn peers() -> AppEvent {
    AppEvent::Peers(vec![], None, (0, 0))
}

fn open(bytes: &[u8]) -> SignedHeartbeat {
    SignedHeartbeat::from_bytes(bytes).unwrap()
}

#[test]
fn the_heartbeat_is_signed_by_the_owner_and_names_the_tip_and_the_time() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    rig.feed(vec![peers()]);
    let bytes = rig.watcher.heartbeat(counter()).unwrap();
    let signed = open(&bytes);
    let hb = signed.verify(&roots(), &counter(), 0).unwrap();
    assert_eq!(hb.counter, counter());
    assert_eq!(hb.time, T0);
    assert_eq!(hb.tip_height, 0);
    assert_eq!(hb.tip_hash, genesis().to_byte_array());
    assert!(hb.healthy);
    assert_eq!(rig.watcher.device_id(), id(1).device_id());
    // only the owner root's signature verifies, not another key's
    let other = PinnedRoots::new(id(5).device_id(), id(2).device_id()).unwrap();
    assert!(signed.verify(&other, &counter(), 0).is_err());
}

#[test]
fn before_the_first_peers_event_the_heartbeat_says_unhealthy() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    let hb = *open(&rig.watcher.heartbeat(counter()).unwrap())
        .verify(&roots(), &counter(), 0)
        .unwrap();
    assert!(!hb.healthy);
    rig.feed(vec![peers()]);
    let hb = *open(&rig.watcher.heartbeat(counter()).unwrap())
        .verify(&roots(), &counter(), 0)
        .unwrap();
    assert!(hb.healthy);
}

#[test]
fn the_health_rules_run_on_every_peers_event() {
    let dir = Dir::new();
    // the real rules, with no peers: not enough groups
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with_rule(dir.path(), &time, openbmt_watch::check_health);
    rig.feed(vec![peers()]);
    let answer = rig.watcher.health().unwrap();
    assert!(!answer.healthy);
    assert_eq!(
        answer.problem,
        Some(openbmt_watch::Problem::TooFewPeerGroups)
    );
    let hb = *open(&rig.watcher.heartbeat(counter()).unwrap())
        .verify(&roots(), &counter(), 0)
        .unwrap();
    assert!(!hb.healthy);

    // three groups, two with filters, and a tip that is old: still not
    // healthy, by WR-51
    let peer = |n: u8, filters: bool| Peer {
        addr: PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap()),
        serves_filters: filters,
        best_height: 0,
    };
    let three = vec![peer(20, true), peer(21, true), peer(22, false)];
    rig.feed(vec![AppEvent::Peers(three.clone(), None, (0, T0 - 8_000))]);
    assert_eq!(
        rig.watcher.health().unwrap().problem,
        Some(openbmt_watch::Problem::TipIsOld)
    );
    // the same peers with a tip that is current: healthy
    rig.feed(vec![AppEvent::Peers(three, None, (0, T0 - 100))]);
    assert!(rig.watcher.health().unwrap().healthy);
    let hb = *open(&rig.watcher.heartbeat(counter()).unwrap())
        .verify(&roots(), &counter(), 0)
        .unwrap();
    assert!(hb.healthy);
    assert_eq!(rig.watcher.chain().tip_time(), T0 - 100);
}

#[test]
fn a_chain_event_moves_the_tip_the_heartbeat_names() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    let one = BlockHash::from_byte_array([1; 32]);
    let two = BlockHash::from_byte_array([2; 32]);
    rig.feed(vec![AppEvent::Chain {
        joined: vec![(1, one)],
        left: vec![],
    }]);
    let hb = *open(&rig.watcher.heartbeat(counter()).unwrap())
        .verify(&roots(), &counter(), 0)
        .unwrap();
    assert_eq!((hb.tip_height, hb.tip_hash), (1, [1; 32]));
    // a reorganization: block 1 leaves and another takes its place
    rig.feed(vec![AppEvent::Chain {
        joined: vec![(1, two)],
        left: vec![one],
    }]);
    let hb = *open(&rig.watcher.heartbeat(counter()).unwrap())
        .verify(&roots(), &counter(), 0)
        .unwrap();
    assert_eq!((hb.tip_height, hb.tip_hash), (1, [2; 32]));
}

#[test]
fn numbers_rise_and_are_stored_before_the_bytes_are_returned() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    let stored = |dir: &Dir| {
        u64::from_le_bytes(
            std::fs::read(dir.path().join(HEARTBEAT_FILE))
                .unwrap()
                .try_into()
                .unwrap(),
        )
    };
    let mut last = 0;
    for _ in 0..4 {
        let bytes = rig.watcher.heartbeat(counter()).unwrap();
        let seq = open(&bytes).heartbeat.seq;
        assert!(seq > last, "{seq} after {last}");
        assert_eq!(stored(&dir), seq, "the file already holds the number");
        assert_eq!(rig.watcher.last_seq(), seq);
        last = seq;
    }
    // the clock is the floor, and with the same clock the numbers still rise
    assert_eq!(last, T0 + 3);
    rig.time.store(T0 + 1_000, Ordering::SeqCst);
    let seq = open(&rig.watcher.heartbeat(counter()).unwrap())
        .heartbeat
        .seq;
    assert_eq!(seq, T0 + 1_000);
}

#[test]
fn a_restart_goes_on_above_the_stored_number_and_a_lost_file_starts_from_the_clock() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    let last = open(&rig.watcher.heartbeat(counter()).unwrap())
        .heartbeat
        .seq;
    drop(rig);

    // The clock has gone back: the stored number still decides.
    let time = Arc::new(AtomicU64::new(T0 - 500));
    let mut rig = start_with_rule(dir.path(), &time, |_| healthy());
    assert_eq!(rig.watcher.last_seq(), last);
    let seq = open(&rig.watcher.heartbeat(counter()).unwrap())
        .heartbeat
        .seq;
    assert_eq!(seq, last + 1);
    drop(rig);

    // The file is lost: the number comes from the clock, which is above
    // what any earlier heartbeat used.
    std::fs::remove_file(dir.path().join(HEARTBEAT_FILE)).unwrap();
    let time = Arc::new(AtomicU64::new(T0 + 7_000));
    let mut rig = start_with_rule(dir.path(), &time, |_| healthy());
    assert_eq!(rig.watcher.last_seq(), 0);
    let seq = open(&rig.watcher.heartbeat(counter()).unwrap())
        .heartbeat
        .seq;
    assert_eq!(seq, T0 + 7_000);
}

#[test]
fn a_number_that_cannot_be_stored_gives_no_heartbeat() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    // a directory where the temporary file must go
    std::fs::create_dir(dir.path().join(format!("{HEARTBEAT_FILE}.tmp"))).unwrap();
    assert!(rig.watcher.heartbeat(counter()).is_none());
    assert_eq!(rig.watcher.last_seq(), 0);
    let notes = rig.watcher.take_notes();
    assert!(
        notes.iter().any(|n| n.contains("could not be stored")),
        "{notes:?}"
    );
    std::fs::remove_dir(dir.path().join(format!("{HEARTBEAT_FILE}.tmp"))).unwrap();
    assert!(rig.watcher.heartbeat(counter()).is_some());
}

#[test]
fn a_number_file_that_does_not_read_back_is_logged_and_taken_as_zero() {
    let dir = Dir::new();
    std::fs::write(dir.path().join(HEARTBEAT_FILE), b"abc").unwrap();
    let mut rig = start(dir.path());
    assert_eq!(rig.watcher.last_seq(), 0);
    let notes = rig.watcher.take_notes();
    assert!(
        notes.iter().any(|n| n.contains("does not read back")),
        "{notes:?}"
    );
    assert!(rig.watcher.heartbeat(counter()).is_some());
}

#[test]
fn notes_and_stop_events_are_taken_and_other_events_are_ignored() {
    let dir = Dir::new();
    let mut rig = start(dir.path());
    rig.feed(vec![
        AppEvent::Note("a peer was dropped".to_string()),
        AppEvent::Checked(3),
        AppEvent::Sighting {
            tx: vec![1, 2, 3],
            from: vec![],
        },
    ]);
    assert_eq!(rig.watcher.take_notes(), vec!["a peer was dropped"]);
    assert!(!rig.watcher.stopped());
    rig.feed(vec![AppEvent::Stopped]);
    assert!(rig.watcher.stopped());
}

#[test]
fn nr_79_a_second_start_on_the_same_directory_fails_and_a_later_one_succeeds() {
    let dir = Dir::new();
    let first = start(dir.path());
    let config = || {
        let mut c = OwnerConfig::new(dir.path(), id(1), DriverConfig::new(Network::Regtest, 0));
        c.health = Box::new(|_| healthy());
        c
    };
    let err = OwnerWatcher::start_with(config(), |_, _| -> Result<FakePort, String> {
        Err("the driver must not start".to_string())
    })
    .err();
    match err {
        Some(OwnerStartError::Locked(why)) => {
            assert!(
                why.starts_with("another OpenBMT process is using "),
                "{why}"
            );
        }
        other => panic!("{other:?}"),
    }
    drop(first);
    let (_events, rx) = channel();
    assert!(OwnerWatcher::start_with(config(), move |_, _| Ok(FakePort(rx))).is_ok());
}
