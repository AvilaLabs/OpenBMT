//! The counter's end of the link (LK-55 to LK-57, LK-61, LK-63): a node on a
//! fake driver, a link on a relay in memory, and a courier standing in for
//! the owner.

use bitcoin::hashes::Hash;
use openbmt_core::command::{Action, OwnerCommand, SignedCommand};
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::{Record, RecordKind, GENESIS};
use openbmt_courier::{
    decode_records, head_at, received_content, Courier, FirstStart, MemoryPost, MemoryRelay,
    Received,
};
use openbmt_link::{Kind, Role};

use super::kit::*;
use crate::{CounterLink, CounterLinkConfig};

/// Fast polling, and the rules' own intervals for everything else.
fn config() -> CounterLinkConfig {
    CounterLinkConfig {
        poll_secs: 0,
        retry_secs: 0,
        ..CounterLinkConfig::default()
    }
}

fn owner() -> DeviceId {
    id(1).device_id()
}

fn counter() -> DeviceId {
    id(3).device_id()
}

struct World {
    rig: Rig,
    link: CounterLink<MemoryPost>,
    owner: Courier<MemoryPost>,
    relay: MemoryRelay,
    link_dir: Scratch,
    _owner_dir: Scratch,
    node_dir: Scratch,
}

fn courier(
    relay: &MemoryRelay,
    who: Identity,
    role: Role,
    peer: (DeviceId, Role),
    dir: &Scratch,
) -> Courier<MemoryPost> {
    Courier::start_with(
        relay.post(),
        who,
        role,
        [peer].into_iter().collect(),
        dir.path(),
        FirstStart::Yes,
    )
    .unwrap()
}

fn world(cfg: CounterLinkConfig) -> World {
    world_with(cfg, true)
}

fn world_with(cfg: CounterLinkConfig, owner_mailbox_first: bool) -> World {
    let relay = MemoryRelay::new(id(99), &[owner(), counter()]).unwrap();
    let node_dir = Scratch::new();
    let link_dir = Scratch::new();
    let owner_dir = Scratch::new();
    let rig = start(node_dir.path());
    let mut link = CounterLink::new(
        courier(
            &relay,
            id(3),
            Role::Counter,
            (owner(), Role::Owner),
            &link_dir,
        ),
        owner(),
        cfg,
    );
    let mut owner_side = courier(
        &relay,
        id(1),
        Role::Owner,
        (counter(), Role::Counter),
        &owner_dir,
    );
    // Both mailboxes exist before anyone sends, unless the test asks for
    // the owner's to be made later.
    if owner_mailbox_first {
        owner_side.poll().unwrap();
    }
    link.courier_mut().poll().unwrap();
    World {
        rig,
        link,
        owner: owner_side,
        relay,
        link_dir,
        _owner_dir: owner_dir,
        node_dir,
    }
}

impl World {
    fn step(&mut self, now: u64) {
        self.link.step(&mut self.rig.node, now);
    }

    fn records(&self) -> Vec<Record> {
        self.rig.node.records_from(0, usize::MAX).unwrap()
    }

    /// Everything the owner's courier takes now.
    fn owner_gets(&mut self) -> Vec<Received> {
        self.owner.poll().unwrap()
    }

    fn mark(&mut self, count: u64, head: [u8; 32]) {
        self.owner
            .send(
                &counter(),
                Kind::RecordsReceived,
                &received_content(count, &head),
            )
            .unwrap();
    }

    fn send(&mut self, kind: Kind, content: &[u8]) {
        self.owner.send(&counter(), kind, content).unwrap();
    }

    fn notes(&mut self) -> Vec<String> {
        self.link.take_notes()
    }
}

fn of_kind(got: &[Received], kind: Kind) -> Vec<&Received> {
    got.iter().filter(|r| r.kind == kind).collect()
}

/// The records in the Records messages among `got`, in the order they came.
fn carried(got: &[Received]) -> Vec<Record> {
    of_kind(got, Kind::Records)
        .into_iter()
        .flat_map(|r| decode_records(&r.content).unwrap())
        .collect()
}

fn heartbeat_bytes(seq: u64) -> Vec<u8> {
    WatcherHeartbeat {
        counter: counter(),
        seq,
        time: T0,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
    .sign(&id(1))
    .to_bytes()
}

fn command(signer: &Identity, seq: u64) -> SignedCommand {
    let command = OwnerCommand {
        counter: counter(),
        seq,
        action: Action::ClearCancelAlarm,
    };
    SignedCommand {
        owner_sig: command.sign(signer),
        command,
    }
}

#[test]
fn lk_61_a_heartbeat_comes_through_the_relay_and_is_taken_by_the_node() {
    let mut w = world(config());
    assert!(w.rig.feed(vec![Rig::peers()]).is_empty());
    assert!(w.rig.node.counter().heartbeat_view().is_none());
    w.send(Kind::Heartbeat, &heartbeat_bytes(1));
    w.step(T0);
    assert!(matches!(
        w.rig.node.counter().heartbeat_view(),
        Some(openbmt_counter::HeartbeatView::Verified { .. })
    ));
    assert_eq!(w.link.state().heartbeats_taken, 1);
    assert_eq!(w.rig.node.counter().desk().last_heartbeat_seq(), 1);
    // The same bytes again are the node's to refuse, by its own rule.
    w.send(Kind::Heartbeat, &heartbeat_bytes(1));
    w.step(T0 + 1);
    assert_eq!(w.link.state().heartbeats_taken, 1);
    assert_eq!(w.link.state().node_refusals, 1);
    assert!(w.notes().iter().any(|n| n.contains("CR-50")));
}

#[test]
fn lk_61_a_command_and_a_batch_are_given_to_the_node() {
    let mut w = world(config());
    let before = w.rig.node.counter().batches().remaining();
    w.send(Kind::Command, &command(&id(1), 5).to_bytes());
    w.send(Kind::Batch, &batch(10, 5).to_bytes());
    w.step(T0);
    assert_eq!(w.rig.node.counter().desk().last_command_seq(), 5);
    assert_eq!(w.rig.node.counter().batches().remaining(), before + 5);
    assert_eq!(w.link.state().node_refusals, 0);
}

#[test]
fn lk_63_what_the_node_refuses_or_cannot_decode_is_noted_and_the_step_goes_on() {
    let mut w = world(config());
    w.send(Kind::Heartbeat, b"not a heartbeat");
    // A command signed by someone who is not the owner root.
    w.send(Kind::Command, &command(&id(5), 1).to_bytes());
    w.send(Kind::Command, b"junk");
    w.send(Kind::Batch, b"junk");
    w.send(Kind::Settings, b"settings");
    w.send(Kind::DeviceList, b"list");
    w.send(Kind::Heartbeat, &heartbeat_bytes(1));
    assert!(w.rig.feed(vec![Rig::peers()]).is_empty());
    w.step(T0);
    // The last message is still taken.
    assert_eq!(w.link.state().heartbeats_taken, 1);
    assert_eq!(w.link.state().node_refusals, 4);
    assert_eq!(w.rig.node.counter().desk().last_command_seq(), 0);
    let notes = w.notes();
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.contains("not handled yet"))
            .count(),
        2,
        "{notes:?}"
    );
    assert!(notes.iter().any(|n| n.contains("heartbeat")));
    assert!(notes.iter().any(|n| n.contains("command not taken")));
    // And the link still works afterwards.
    w.send(Kind::Command, &command(&id(1), 2).to_bytes());
    w.step(T0 + 1);
    assert_eq!(w.rig.node.counter().desk().last_command_seq(), 2);
}

#[test]
fn lk_55_a_mark_that_fits_the_chain_is_taken_and_one_that_does_not_is_ignored() {
    let mut w = world(config());
    for _ in 0..3 {
        w.rig.node.check_in().unwrap();
    }
    // A step with no mark: the link starts with "the owner holds nothing".
    w.step(T0);
    let chain = w.records();
    assert_eq!(chain.len(), 4, "three, and the link's own check-in");
    assert_eq!(w.link.state().owner_records, 0);
    assert_eq!(w.link.state().owner_head, GENESIS);

    // More records than the counter has; a head that is not ours.
    w.mark(5, [1; 32]);
    w.mark(u64::MAX, chain[3].hash());
    w.mark(2, chain[2].hash());
    w.mark(4, [9; 32]);
    w.step(T0 + 1);
    assert_eq!(w.link.state().marks_ignored, 4);
    assert_eq!(w.link.state().owner_records, 0);
    let notes = w.notes();
    assert_eq!(notes.iter().filter(|n| n.contains("LK-55")).count(), 4);

    // One that fits, then a lower one that fits (the owner lost some).
    w.mark(3, chain[2].hash());
    w.step(T0 + 2);
    assert_eq!(w.link.state().owner_records, 3);
    assert_eq!(w.link.state().owner_head, chain[2].hash());
    w.mark(1, chain[0].hash());
    w.step(T0 + 3);
    assert_eq!(w.link.state().owner_records, 1);
    // A mark that is not 40 bytes.
    w.send(Kind::RecordsReceived, b"short");
    w.step(T0 + 4);
    assert_eq!(w.link.state().marks_ignored, 5);
    assert_eq!(w.link.state().owner_records, 1);
    assert_eq!(head_at(&chain, 0), Some(GENESIS));
}

#[test]
fn cr_76_a_mark_the_link_takes_is_the_counter_s_owner_mark_and_one_it_ignores_is_not() {
    let mut w = world(config());
    for _ in 0..3 {
        w.rig.node.check_in().unwrap();
    }
    w.step(T0);
    let chain = w.records();
    assert_eq!(w.rig.node.counter().owner_mark(), None);
    w.mark(4, [9; 32]);
    w.step(T0 + 1);
    assert_eq!(w.rig.node.counter().owner_mark(), None);
    w.mark(3, chain[2].hash());
    w.step(T0 + 2);
    assert_eq!(w.rig.node.counter().owner_mark(), Some(3));
    // A lower valid mark replaces it (LK-55).
    w.mark(1, chain[0].hash());
    w.step(T0 + 3);
    assert_eq!(w.rig.node.counter().owner_mark(), Some(1));
}

#[test]
fn lk_56_a_lost_send_is_sent_again_after_30_seconds_and_not_before() {
    let mut w = world(config());
    w.relay.set_lose_puts(true);
    w.step(T0);
    let puts = w.relay.puts();
    assert_eq!(puts, 2, "the check-in and the records");
    w.relay.set_lose_puts(false);
    // Nothing is due: the records were sent, and the owner has not said
    // otherwise.
    for dt in [1, 10, 29] {
        w.step(T0 + dt);
        assert_eq!(w.relay.puts(), puts, "at +{dt}");
    }
    assert!(w.owner_gets().is_empty());
    w.step(T0 + 30);
    assert_eq!(w.relay.puts(), puts + 1);
    let got = w.owner_gets();
    assert_eq!(carried(&got), w.records());
    // The owner's receipt ends it.
    let chain = w.records();
    w.mark(1, chain[0].hash());
    let after_mark = w.relay.puts();
    w.step(T0 + 31);
    w.step(T0 + 100);
    assert_eq!(w.relay.puts(), after_mark);
}

#[test]
fn lk_56_a_mark_that_never_moves_costs_a_resend_every_30_seconds_and_no_more() {
    let mut w = world(config());
    w.step(T0);
    let puts = w.relay.puts();
    // The owner takes nothing and says nothing.
    for (i, dt) in [30u64, 31, 59, 60, 61, 90].into_iter().enumerate() {
        w.step(T0 + dt);
        let want = puts + [1, 1, 1, 2, 2, 3][i];
        assert_eq!(w.relay.puts(), want, "at +{dt}");
        w.owner_gets();
    }
}

#[test]
fn lk_56_at_most_four_messages_a_step_and_whole_records_oldest_first() {
    let mut cfg = config();
    // Five of the 190-byte check-in records fit.
    cfg.max_content = 1_000;
    let mut w = world(cfg);
    for _ in 0..99 {
        w.rig.node.check_in().unwrap();
    }
    w.step(T0);
    let got = w.owner_gets();
    let messages = of_kind(&got, Kind::Records);
    assert_eq!(messages.len(), 4);
    for m in &messages {
        assert_eq!(decode_records(&m.content).unwrap().len(), 5);
    }
    // The next step goes on after what was sent, without a mark.
    w.step(T0 + 1);
    let got2 = w.owner_gets();
    assert_eq!(of_kind(&got2, Kind::Records).len(), 4);
    let mut seqs: Vec<u64> = carried(&got).iter().map(|r| r.seq).collect();
    seqs.extend(carried(&got2).iter().map(|r| r.seq));
    assert_eq!(seqs, (0..40).collect::<Vec<u64>>());
    assert_eq!(w.link.state().record_messages_sent, 8);
}

#[test]
fn lk_56_a_record_too_long_for_a_message_is_noted_and_the_link_goes_on() {
    let mut cfg = config();
    cfg.max_content = 100;
    let mut w = world(cfg);
    w.step(T0);
    assert!(w.notes().iter().any(|n| n.contains("too long")));
    w.send(Kind::Command, &command(&id(1), 3).to_bytes());
    w.step(T0 + 1);
    assert_eq!(w.rig.node.counter().desk().last_command_seq(), 3);
    assert!(carried(&w.owner_gets()).is_empty());
}

#[test]
fn lk_57_a_check_in_is_a_record_sent_as_a_check_in_and_among_the_records() {
    let mut w = world(config());
    w.step(T0);
    let got = w.owner_gets();
    let check_ins = of_kind(&got, Kind::CheckIn);
    assert_eq!(check_ins.len(), 1);
    let record = Record::from_bytes(&check_ins[0].content).unwrap();
    assert_eq!(record.kind, RecordKind::CheckIn);
    assert_eq!(record.author, counter());
    assert_eq!(record.check_in_claim().unwrap(), (0, GENESIS));
    // It is in the chain, and so in the records.
    assert_eq!(w.records(), vec![record.clone()]);
    assert_eq!(carried(&got), vec![record]);
    assert_eq!(w.link.state().last_check_in_at, Some(T0));
}

#[test]
fn lk_57_check_ins_are_at_most_one_a_minute_and_only_when_something_changed_or_ten_minutes_passed()
{
    // The defaults are 15 minutes for both; the two intervals are set apart
    // here to show each at work.
    let defaults = CounterLinkConfig::default();
    assert_eq!(
        (defaults.check_in_min_secs, defaults.check_in_max_secs),
        (900, 900)
    );
    let mut cfg = config();
    cfg.check_in_min_secs = 60;
    cfg.check_in_max_secs = 600;
    let mut w = world(cfg);
    let count = |w: &mut World| of_kind(&w.owner_gets(), Kind::CheckIn).len();
    w.step(T0);
    assert_eq!(count(&mut w), 1);
    // Nothing changed: not at 30 or 60 seconds, nor at 599.
    for dt in [30, 60, 599] {
        w.step(T0 + dt);
        assert_eq!(count(&mut w), 0, "at +{dt}");
    }
    // A sale is a change, and more than a minute has passed: a check-in.
    assert!(w.rig.feed(vec![Rig::peers()]).is_empty());
    w.rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&w.rig.time))
        .unwrap();
    w.step(T0 + 600);
    assert_eq!(count(&mut w), 1);
    // The check-in is the record after the sale.
    assert_eq!(w.records().last().unwrap().kind, RecordKind::CheckIn);
    // A change soon after the last one waits for the minute.
    w.rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&w.rig.time))
        .unwrap();
    w.step(T0 + 630);
    assert_eq!(count(&mut w), 0);
    w.step(T0 + 660);
    assert_eq!(count(&mut w), 1);
    // And with no change, ten minutes after that one.
    w.step(T0 + 660 + 599);
    assert_eq!(count(&mut w), 0);
    w.step(T0 + 660 + 600);
    assert_eq!(count(&mut w), 1);
}

#[test]
fn lk_57_a_check_in_refused_before_the_owner_s_mailbox_exists_is_sent_again_as_the_same_record() {
    let mut cfg = config();
    cfg.retry_secs = 5;
    let mut w = world_with(cfg, false);
    // The owner's mailbox is not made yet: the relay refuses the check-in.
    w.step(T0);
    assert_eq!(w.records().len(), 1);
    let first = w.records()[0].clone();
    // The owner's first poll makes its mailbox; nothing has reached it.
    assert!(of_kind(&w.owner_gets(), Kind::CheckIn).is_empty());
    // The next step sends the same record again, and writes no new one.
    w.step(T0 + 5);
    let got = w.owner_gets();
    let check_ins = of_kind(&got, Kind::CheckIn);
    assert_eq!(check_ins.len(), 1);
    assert_eq!(Record::from_bytes(&check_ins[0].content).unwrap(), first);
    assert_eq!(w.records(), vec![first]);
    // Delivered once: not sent again.
    w.step(T0 + 10);
    assert!(of_kind(&w.owner_gets(), Kind::CheckIn).is_empty());
}

#[test]
fn lk_57_the_node_stores_a_check_in_like_any_record() {
    let mut w = world(config());
    let rec = w.rig.node.check_in().unwrap();
    assert_eq!(rec.seq, 0);
    assert_eq!(w.rig.node.records_len(), 1);
    let node_dir = w.node_dir.path().to_path_buf();
    let head = w.rig.node.records_head();
    // A node started again on the same directory reads it back as a chain.
    drop(w.rig);
    let again = start(&node_dir);
    assert_eq!(again.node.records_len(), 1);
    assert_eq!(again.node.records_head(), head);
    drop(w.link_dir);
}

#[test]
fn lk_56_a_relay_that_cannot_be_reached_is_tried_again_after_the_pause() {
    let mut cfg = config();
    cfg.retry_secs = 5;
    let mut w = world(cfg);
    w.relay.set_down(true);
    w.step(T0);
    let calls = w.relay.calls();
    assert!(!w.link.state().relay_reachable);
    for dt in [1, 2, 4] {
        w.step(T0 + dt);
        assert_eq!(w.relay.calls(), calls, "no call at +{dt}");
    }
    w.step(T0 + 5);
    assert!(w.relay.calls() > calls);
    w.relay.set_down(false);
    w.step(T0 + 10);
    assert!(w.link.state().relay_reachable);
    assert_eq!(carried(&w.owner_gets()), w.records());
}

/// A world whose counter holds 1,500 records, the owner having taken them
/// all, so that the first records are below the counter's base (CR-81).
fn long_world() -> World {
    let mut cfg = config();
    // Five of the 190-byte check-in records fit one message.
    cfg.max_content = 1_000;
    let mut w = world(cfg);
    for _ in 0..1_499 {
        w.rig.node.check_in().unwrap();
    }
    w.step(T0);
    assert_eq!(w.rig.node.records_len(), 1_500);
    w.mark(1_500, w.rig.node.records_head());
    w.step(T0 + 1);
    assert_eq!(w.rig.node.counter().owner_mark(), Some(1_500));
    assert!(w.rig.node.counter().records_base() >= 400);
    w.owner_gets();
    w.notes();
    w
}

#[test]
fn lk_55_a_mark_below_the_base_is_checked_against_the_record_file() {
    let mut w = long_world();
    let base = w.rig.node.counter().records_base();
    let in_memory = w.rig.node.counter().records_in_memory();
    let h10 = w.rig.node.record_at(9).unwrap().unwrap().hash();
    assert_eq!(w.rig.node.hash_at(10), Ok(Some(h10)));
    // A head that is wrong for a place below the base is ignored; a right
    // one is taken, and so is one at the base itself.
    w.mark(10, [7; 32]);
    w.mark(base, [7; 32]);
    w.step(T0 + 2);
    assert_eq!(w.link.state().marks_ignored, 2);
    assert_eq!(w.link.state().owner_records, 1_500);
    assert_eq!(w.notes().iter().filter(|n| n.contains("LK-55")).count(), 2);
    w.mark(10, h10);
    w.step(T0 + 3);
    assert_eq!(w.link.state().marks_ignored, 2);
    assert_eq!(
        (w.link.state().owner_records, w.link.state().owner_head),
        (10, h10)
    );
    assert_eq!(w.rig.node.counter().owner_mark(), Some(10));
    let below = w.rig.node.record_at(base - 1).unwrap().unwrap().hash();
    w.mark(base, below);
    w.step(T0 + 4);
    assert_eq!(w.link.state().owner_records, base);
    // Memory did not take the records back.
    assert_eq!(w.rig.node.counter().records_base(), base);
    assert_eq!(w.rig.node.counter().records_in_memory(), in_memory);
}

#[test]
fn lk_56_a_mark_that_goes_down_below_the_base_resends_from_the_record_file() {
    let mut w = long_world();
    let base = w.rig.node.counter().records_base();
    assert!(base > 10);
    let h10 = w.rig.node.record_at(9).unwrap().unwrap().hash();
    w.mark(10, h10);
    w.step(T0 + 2);
    assert_eq!(w.link.state().owner_records, 10);
    // The records the owner was never sent go first, from memory.
    let seqs: Vec<u64> = carried(&w.owner_gets()).iter().map(|r| r.seq).collect();
    assert_eq!(seqs, (20..40).collect::<Vec<u64>>());
    // Thirty seconds after the last send the records after the mark go
    // again, oldest first, from the file: four messages of five, and the
    // next step goes on after them.
    w.step(T0 + 33);
    let got = w.owner_gets();
    assert_eq!(of_kind(&got, Kind::Records).len(), 4);
    let seqs: Vec<u64> = carried(&got).iter().map(|r| r.seq).collect();
    assert_eq!(seqs, (10..30).collect::<Vec<u64>>());
    assert_eq!(carried(&got), w.rig.node.records_from(10, 20).unwrap());
    w.step(T0 + 34);
    let seqs: Vec<u64> = carried(&w.owner_gets()).iter().map(|r| r.seq).collect();
    assert_eq!(seqs, (30..50).collect::<Vec<u64>>());
}

#[test]
fn cr_81_a_store_fault_below_the_base_is_noted_and_the_step_goes_on() {
    let mut w = long_world();
    let h10 = w.rig.node.record_at(9).unwrap().unwrap().hash();
    let h20 = w.rig.node.record_at(19).unwrap().unwrap().hash();
    w.mark(10, h10);
    w.step(T0 + 2);
    assert_eq!(w.link.state().owner_records, 10);
    w.owner_gets();
    w.notes();
    let (len, head, base, held) = (
        w.rig.node.records_len(),
        w.rig.node.records_head(),
        w.rig.node.counter().records_base(),
        w.rig.node.counter().records_in_memory(),
    );
    // The record file is gone from under the node.
    std::fs::OpenOptions::new()
        .write(true)
        .open(w.node_dir.path().join("records.bin"))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(w.rig.node.hash_at(20).is_err());
    w.mark(20, h20);
    w.send(Kind::Command, &command(&id(1), 2).to_bytes());
    w.step(T0 + 3);
    // The mark could not be checked: the last one stands, and it is not
    // counted as the owner's mistake. The rest of the step was done.
    assert_eq!(w.link.state().owner_records, 10);
    assert_eq!(w.link.state().marks_ignored, 0);
    assert_eq!(w.rig.node.counter().owner_mark(), Some(10));
    assert_eq!(w.rig.node.counter().desk().last_command_seq(), 2);
    let notes = w.notes();
    let checked = notes.iter().filter(|n| n.contains("could not be checked"));
    assert_eq!(checked.count(), 1, "{notes:?}");
    // A resend that needs the records below the base is noted once and
    // sends nothing.
    let puts = w.relay.puts();
    w.step(T0 + 40);
    w.step(T0 + 41);
    assert_eq!(w.relay.puts(), puts);
    let notes = w.notes();
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.contains("could not be read"))
            .count(),
        1,
        "{notes:?}"
    );
    // The counter holds what it held.
    assert_eq!(
        (
            w.rig.node.records_len(),
            w.rig.node.records_head(),
            w.rig.node.counter().records_base(),
            w.rig.node.counter().records_in_memory(),
        ),
        (len, head, base, held)
    );
}
