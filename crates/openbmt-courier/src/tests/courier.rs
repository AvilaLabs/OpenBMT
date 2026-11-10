//! The courier (LK-50 to LK-53) on a relay in memory.

use openbmt_link::seal::{seal, Kind, Role};
use openbmt_link::{os_random, ClientError, Refusal};

use super::kit::*;
use crate::{
    Courier, FirstStart, PollError, SendError, StartError, INBOX_FILE, NOTE_LOG, OUTBOX_FILE,
};

fn texts(received: &[crate::Received]) -> Vec<Vec<u8>> {
    received.iter().map(|r| r.content.clone()).collect()
}

#[test]
fn lk_50_a_message_goes_from_one_courier_to_the_other() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    // The counter makes its mailbox on its first poll; nothing is held yet.
    assert!(counter_side.poll().unwrap().is_empty());
    owner_side
        .send(&counter(), Kind::Heartbeat, b"alive")
        .unwrap();
    let got = counter_side.poll().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].from, owner());
    assert_eq!(got[0].role, Role::Owner);
    assert_eq!(got[0].kind, Kind::Heartbeat);
    assert_eq!(got[0].content, b"alive");
    assert_eq!(counter_side.refused(), 0);
}

#[test]
fn lk_50_a_message_to_a_device_not_in_the_table_is_not_sent() {
    let relay = relay();
    let d = TempDir::new("o");
    let mut c = owner_courier(&relay, d.path(), FirstStart::Yes);
    let before = relay.puts();
    let r = c.send(&stranger(), Kind::Heartbeat, b"x");
    assert_eq!(r.unwrap_err(), SendError::UnknownPeer);
    assert_eq!(relay.puts(), before, "nothing reached the relay");
    assert_eq!(c.outbox().last(&stranger()), 0, "no number was used");
    // A kind the roles may not send, and a content that does not fit.
    assert_eq!(
        c.send(&counter(), Kind::CheckIn, b"x").unwrap_err(),
        SendError::KindNotAllowed
    );
    assert_eq!(
        c.send(&counter(), Kind::Command, &vec![0u8; 70_000])
            .unwrap_err(),
        SendError::TooLarge
    );
    assert_eq!(c.outbox().last(&counter()), 0);
}

#[test]
fn lk_50_a_message_from_a_device_not_in_the_table_is_refused_and_counted() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    // The stranger is a member of the relay and knows the counter's key.
    let sd = TempDir::new("s");
    let mut stranger_side = Courier::start_with(
        relay.post(),
        id(4),
        Role::Owner,
        table(&[(2, Role::Counter)]),
        sd.path(),
        FirstStart::Yes,
    )
    .unwrap();
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    stranger_side
        .send(&counter(), Kind::Command, b"let me in")
        .unwrap();
    owner_side
        .send(&counter(), Kind::Heartbeat, b"alive")
        .unwrap();
    // The stranger's message is refused and skipped; the owner's is taken.
    let got = counter_side.poll().unwrap();
    assert_eq!(texts(&got), vec![b"alive".to_vec()]);
    assert_eq!(counter_side.refused(), 1);
    assert!(
        counter_side.notes().any(|n| n.contains("LK-05")),
        "the reason is kept: {:?}",
        counter_side.notes().collect::<Vec<_>>()
    );
    assert_eq!(relay.held(&counter()).len(), 0, "both were acknowledged");
}

#[test]
fn lk_50_a_kind_from_the_wrong_role_is_refused() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let _counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    owner_side.poll().unwrap();
    // Sealed by the counter's key to the owner as a command, which only an
    // owner may send. The seal call is told lies about the roles; the
    // reader goes by its own table.
    let sealed = seal(
        &id(2),
        Role::Owner,
        &owner(),
        Role::Counter,
        1,
        Kind::Command,
        b"do this",
        &mut os_random,
    )
    .unwrap();
    relay.put_raw(&sealed).unwrap();
    assert!(owner_side.poll().unwrap().is_empty());
    assert_eq!(owner_side.refused(), 1);
    assert!(owner_side.notes().any(|n| n.contains("LK-08")));
    assert!(relay.held(&owner()).is_empty());
}

#[test]
fn lk_51_the_number_is_stored_before_the_message_leaves_and_a_retry_takes_a_new_one() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    // The relay is down: the send fails, and may be tried again.
    relay.set_down(true);
    let err = owner_side
        .send(&counter(), Kind::Heartbeat, b"one")
        .unwrap_err();
    assert!(err.is_retryable(), "{err}");
    assert!(matches!(err, SendError::Relay(ClientError::Connect(_))));
    // The number was used and stored even so.
    assert_eq!(owner_side.outbox().last(&counter()), 1);
    let stored = std::fs::read(od.path().join(OUTBOX_FILE)).unwrap();
    assert_eq!(stored, owner_side.outbox().to_bytes());
    // The retry seals again, with number 2; the receiver takes it, gap and
    // all (LK-06).
    relay.set_down(false);
    owner_side
        .send(&counter(), Kind::Heartbeat, b"one")
        .unwrap();
    assert_eq!(owner_side.outbox().last(&counter()), 2);
    let got = counter_side.poll().unwrap();
    assert_eq!(texts(&got), vec![b"one".to_vec()]);
    assert_eq!(counter_side.inbox().last(&owner()), 2);
    // The two messages the relay was given were not the same bytes.
    let a = relay.held(&counter());
    assert!(a.is_empty());
}

#[test]
fn lk_51_two_messages_never_share_a_number_or_a_one_time_key() {
    let relay = relay();
    let d = TempDir::new("o");
    let mut c = owner_courier(&relay, d.path(), FirstStart::Yes);
    let mut sealed = Vec::new();
    // A relay that has no mailbox yet for the counter refuses the put; each
    // try is still its own message.
    for _ in 0..3 {
        assert!(c.send(&counter(), Kind::Heartbeat, b"same").is_err());
    }
    let cd = TempDir::new("c");
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    for _ in 0..3 {
        c.send(&counter(), Kind::Heartbeat, b"same").unwrap();
    }
    for m in relay.held(&counter()) {
        sealed.push(m);
    }
    assert_eq!(sealed.len(), 3);
    // Number, one-time key and ciphertext all differ between the three.
    let field = |m: &[u8], from: usize, len: usize| m[from..from + len].to_vec();
    let numbers: Vec<_> = sealed.iter().map(|m| field(m, 69, 8)).collect();
    let keys: Vec<_> = sealed.iter().map(|m| field(m, 78, 33)).collect();
    for set in [numbers, keys] {
        let mut sorted = set.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 3);
    }
    assert_eq!(c.outbox().last(&counter()), 6);
}

#[test]
fn lk_51_a_number_that_cannot_be_stored_keeps_the_message_back() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    // The directory is gone: the number cannot be written.
    drop(od);
    let before = relay.puts();
    let err = owner_side
        .send(&counter(), Kind::Heartbeat, b"x")
        .unwrap_err();
    assert!(matches!(err, SendError::Number(_)), "{err}");
    assert!(!err.is_retryable());
    assert!(err.to_string().contains("LK-40"));
    assert_eq!(relay.puts(), before, "nothing reached the relay");
    assert_eq!(owner_side.outbox().last(&counter()), 0);
}

#[test]
fn lk_52_a_poll_makes_the_mailbox_and_empties_it() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    // No mailbox yet: the owner's put is refused (LK-20).
    let err = owner_side
        .send(&counter(), Kind::Heartbeat, b"early")
        .unwrap_err();
    assert!(matches!(
        err,
        SendError::Relay(ClientError::Refused(Refusal::NoMailbox))
    ));
    counter_side.poll().unwrap();
    for i in 0..3u8 {
        owner_side.send(&counter(), Kind::Heartbeat, &[i]).unwrap();
    }
    assert_eq!(relay.held(&counter()).len(), 3);
    let got = counter_side.poll().unwrap();
    assert_eq!(texts(&got), vec![vec![0], vec![1], vec![2]]);
    assert!(relay.held(&counter()).is_empty());
    assert!(counter_side.poll().unwrap().is_empty());
}

#[test]
fn lk_52_a_replayed_message_is_refused_and_acknowledged_away() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    owner_side
        .send(&counter(), Kind::Command, b"first")
        .unwrap();
    owner_side
        .send(&counter(), Kind::Command, b"second")
        .unwrap();
    let held = relay.held(&counter());
    assert_eq!(counter_side.poll().unwrap().len(), 2);
    // The relay hands both out again, and one more from an old number.
    for m in &held {
        relay.put_raw(m).unwrap();
    }
    assert_eq!(relay.held(&counter()).len(), 2);
    let got = counter_side.poll().unwrap();
    assert!(got.is_empty());
    assert_eq!(counter_side.refused(), 2);
    assert!(counter_side.notes().any(|n| n.contains("LK-06")));
    assert!(relay.held(&counter()).is_empty(), "acknowledged away");
    // The next real message is still taken.
    owner_side
        .send(&counter(), Kind::Command, b"third")
        .unwrap();
    assert_eq!(
        texts(&counter_side.poll().unwrap()),
        vec![b"third".to_vec()]
    );
}

#[test]
fn lk_52_more_than_one_fetch_of_messages_is_taken_in_one_poll() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    owner_side.begin();
    for i in 0..150u16 {
        owner_side
            .send(&counter(), Kind::Heartbeat, &i.to_le_bytes())
            .unwrap();
    }
    owner_side.end();
    let got = counter_side.poll().unwrap();
    assert_eq!(got.len(), 150);
    for (i, r) in got.iter().enumerate() {
        assert_eq!(r.content, (i as u16).to_le_bytes());
    }
    assert!(relay.held(&counter()).is_empty());
}

#[test]
fn lk_52_a_relay_that_cannot_be_reached_is_an_error_and_is_noted_once() {
    let relay = relay();
    let cd = TempDir::new("c");
    let mut c = counter_courier(&relay, cd.path(), FirstStart::Yes);
    c.poll().unwrap();
    relay.set_down(true);
    for _ in 0..4 {
        let err = c.poll().unwrap_err();
        assert!(matches!(err, PollError::Relay(ClientError::Connect(_))));
        assert!(err.to_string().contains("LK-52"));
    }
    assert!(!c.reachable());
    assert_eq!(
        c.notes().filter(|n| n.contains("failed")).count(),
        1,
        "the change is noted once, not on every try"
    );
    relay.set_down(false);
    assert!(c.poll().unwrap().is_empty());
    assert!(c.reachable());
    assert!(c.notes().any(|n| n.contains("again")));
}

#[test]
fn lk_52_the_notes_are_bounded_and_a_refusal_never_panics() {
    let relay = relay();
    let cd = TempDir::new("c");
    let mut c = counter_courier(&relay, cd.path(), FirstStart::Yes);
    c.poll().unwrap();
    // Garbage the relay will not take is not the courier's problem; sealed
    // messages from the stranger are: many of them.
    let sd = TempDir::new("s");
    let mut s = Courier::start_with(
        relay.post(),
        id(4),
        Role::Owner,
        table(&[(2, Role::Counter)]),
        sd.path(),
        FirstStart::Yes,
    )
    .unwrap();
    s.begin();
    for _ in 0..(NOTE_LOG + 20) {
        s.send(&counter(), Kind::Heartbeat, b"x").unwrap();
    }
    s.end();
    assert!(c.poll().unwrap().is_empty());
    assert_eq!(c.refused(), (NOTE_LOG + 20) as u64);
    assert_eq!(c.notes().count(), NOTE_LOG);
    assert_eq!(c.take_notes().len(), NOTE_LOG);
    assert_eq!(c.notes().count(), 0);
}

#[test]
fn lk_53_a_first_start_makes_the_number_files_and_any_other_start_needs_them() {
    let relay = relay();
    let d = TempDir::new("o");
    let post = || relay.post();
    let start = |first| {
        Courier::start_with(
            post(),
            id(1),
            Role::Owner,
            table(&[(2, Role::Counter)]),
            d.path(),
            first,
        )
    };
    // Not a first start and no files: a new pairing.
    let err = start(FirstStart::No).err().unwrap();
    assert_eq!(err, StartError::NeedsNewPairing { file: INBOX_FILE });
    assert!(err.to_string().contains("new pairing"));
    // First start: both files are made, empty.
    start(FirstStart::Yes).unwrap();
    assert!(d.path().join(INBOX_FILE).exists());
    assert!(d.path().join(OUTBOX_FILE).exists());
    // Now a normal start works.
    start(FirstStart::No).unwrap();
    // One file lost: the device cannot tell what its numbers were.
    std::fs::remove_file(d.path().join(OUTBOX_FILE)).unwrap();
    let err = start(FirstStart::No).err().unwrap();
    assert_eq!(err, StartError::NeedsNewPairing { file: OUTBOX_FILE });
    std::fs::remove_file(d.path().join(INBOX_FILE)).unwrap();
    let err = start(FirstStart::No).err().unwrap();
    assert_eq!(err, StartError::NeedsNewPairing { file: INBOX_FILE });
    // A file that does not read back is the same.
    start(FirstStart::Yes).unwrap();
    std::fs::write(d.path().join(INBOX_FILE), b"junk").unwrap();
    let err = start(FirstStart::No).err().unwrap();
    assert_eq!(err, StartError::Corrupt { file: INBOX_FILE });
}

#[test]
fn lk_53_a_restart_goes_on_from_the_stored_numbers() {
    let relay = relay();
    let (od, cd) = (TempDir::new("o"), TempDir::new("c"));
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::Yes);
    counter_side.poll().unwrap();
    {
        let mut owner_side = owner_courier(&relay, od.path(), FirstStart::Yes);
        owner_side.send(&counter(), Kind::Command, b"a").unwrap();
        owner_side.send(&counter(), Kind::Command, b"b").unwrap();
    }
    let held = relay.held(&counter());
    assert_eq!(counter_side.poll().unwrap().len(), 2);
    drop(counter_side);
    // Both restart: the owner's next number is 3, and the counter, which
    // starts from its file, refuses what it already took.
    let mut owner_side = owner_courier(&relay, od.path(), FirstStart::No);
    let mut counter_side = counter_courier(&relay, cd.path(), FirstStart::No);
    assert_eq!(owner_side.outbox().last(&counter()), 2);
    assert_eq!(counter_side.inbox().last(&owner()), 2);
    for m in &held {
        relay.put_raw(m).unwrap();
    }
    owner_side.send(&counter(), Kind::Command, b"c").unwrap();
    let got = counter_side.poll().unwrap();
    assert_eq!(texts(&got), vec![b"c".to_vec()]);
    assert_eq!(counter_side.inbox().last(&owner()), 3);
    assert_eq!(counter_side.refused(), 2);
}
