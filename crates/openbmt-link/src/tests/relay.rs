//! LK-20 to LK-26: the relay's state machine.

use openbmt_core::identity::{DeviceId, Identity};

use super::kit::*;
use crate::proto::{Refusal, Request, Response};
use crate::relay::{
    sign_challenge, Action, MembersError, Relay, RelayConfig, WelcomeAnswer, CHALLENGE_SECS,
    LEAVING_SECS,
};
use crate::seal::{open, seal, Kind, OpenError, Role, MAX_CONTENT_LEN};
use crate::store::{Event, MailStore, MemoryStore};

pub struct Rig<S: MailStore = MemoryStore> {
    pub relay: Relay<S>,
    pub now: u64,
    /// The connection the helpers speak on.
    pub conn: u64,
    rng: Counting,
}

impl Rig {
    pub fn new() -> Self {
        Self::with(RelayConfig::default())
    }

    pub fn with(cfg: RelayConfig) -> Self {
        Rig::with_store(cfg, MemoryStore::new())
    }
}

impl<S: MailStore> Rig<S> {
    pub fn with_store(cfg: RelayConfig, store: S) -> Self
    where
        S::Error: std::fmt::Debug,
    {
        let mut relay = Relay::new(id(99), cfg, store).unwrap();
        let members = [owner(), counter(), the_box()].map(|i| i.device_id());
        relay.set_members(&members, 1_000).unwrap();
        Self {
            relay,
            now: 1_000,
            conn: 1,
            rng: Counting(100),
        }
    }

    /// A rig over a relay that already exists, as after a restart: the
    /// members are not set again.
    pub fn over(relay: Relay<S>) -> Self {
        Self {
            relay,
            now: 1_000,
            conn: 1,
            rng: Counting(100),
        }
    }

    pub fn relay_key(&self) -> DeviceId {
        self.relay.device_id()
    }

    pub fn ask(&mut self, r: Request) -> Response {
        self.ask_on(self.conn, r)
    }

    pub fn ask_on(&mut self, conn: u64, r: Request) -> Response {
        let rng = &mut self.rng;
        self.relay.handle(conn, r, self.now, &mut |b| rng.fill(b))
    }

    pub fn challenge(&mut self) -> [u8; 32] {
        match self.ask(Request::Challenge) {
            Response::Challenge { challenge } => challenge,
            other => panic!("{other:?}"),
        }
    }

    pub fn make(&mut self, who: &Identity) -> Response {
        let challenge = self.challenge();
        let sig = sign_challenge(who, &challenge, &self.relay_key(), Action::Make, 0);
        self.ask(Request::MakeMailbox {
            reader: who.device_id(),
            challenge,
            sig,
        })
    }

    pub fn put(&mut self, sealed: Vec<u8>) -> Response {
        self.ask(Request::Put { sealed })
    }

    pub fn fetch(&mut self, who: &Identity, cursor: u64) -> Response {
        let challenge = self.challenge();
        let sig = sign_challenge(who, &challenge, &self.relay_key(), Action::Fetch, cursor);
        self.ask(Request::Fetch {
            reader: who.device_id(),
            challenge,
            sig,
            cursor,
        })
    }

    pub fn ack(&mut self, who: &Identity, up_to: u64) -> Response {
        let challenge = self.challenge();
        let sig = sign_challenge(who, &challenge, &self.relay_key(), Action::Ack, up_to);
        self.ask(Request::Ack {
            reader: who.device_id(),
            challenge,
            sig,
            up_to,
        })
    }
}

pub fn items(r: Response) -> Vec<(u64, Vec<u8>)> {
    match r {
        Response::Messages { items, .. } => items,
        other => panic!("{other:?}"),
    }
}

pub fn ids(r: Response) -> Vec<u64> {
    items(r).into_iter().map(|(i, _)| i).collect()
}

/// A message for the counter from the owner with a given number.
pub fn msg(n: u64) -> Vec<u8> {
    owner_to_counter(n, Kind::Heartbeat, b"x")
}

fn rig_with_counter_mailbox() -> Rig {
    let mut rig = Rig::new();
    assert_eq!(rig.make(&counter()), Response::Made);
    rig
}

#[test]
fn lk_20_a_relay_takes_a_well_formed_signed_message_for_an_existing_mailbox() {
    let mut rig = rig_with_counter_mailbox();
    // A message from a member to a member is taken; one from a key that is
    // not a member is not.
    let mut rng = Counting(0);
    let from_stranger = seal(
        &stranger(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        1,
        Kind::Command,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert_eq!(
        rig.put(from_stranger),
        Response::Refused(Refusal::SenderNotMember)
    );
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
}

#[test]
fn lk_20_a_message_for_a_key_that_is_not_a_member_is_refused() {
    let mut rig = rig_with_counter_mailbox();
    let mut rng = Counting(0);
    let to_stranger = seal(
        &owner(),
        Role::Owner,
        &stranger().device_id(),
        Role::Counter,
        1,
        Kind::Command,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert_eq!(
        rig.put(to_stranger),
        Response::Refused(Refusal::ReaderNotMember)
    );
}

#[test]
fn lk_20_a_relay_with_no_members_takes_nothing() {
    let mut rig = rig_with_counter_mailbox();
    rig.relay.set_members(&[], rig.now).unwrap();
    assert_eq!(rig.relay.members().count(), 0);
    assert_eq!(rig.put(msg(1)), Response::Refused(Refusal::SenderNotMember));
    assert_eq!(rig.make(&counter()), Response::Refused(Refusal::NotMember));
}

#[test]
fn lk_20_a_key_outside_the_list_cannot_fill_a_mailbox_with_junk() {
    let mut rig = rig_with_counter_mailbox();
    // The real message is there; an outside key seals as many validly
    // signed messages to the counter as it likes.
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    let mut rng = Counting(0);
    for n in 1..=100u64 {
        let junk = seal(
            &stranger(),
            Role::Owner,
            &counter().device_id(),
            Role::Counter,
            n,
            Kind::Command,
            b"junk",
            &mut |b| rng.fill(b),
        )
        .unwrap();
        assert_eq!(rig.put(junk), Response::Refused(Refusal::SenderNotMember));
    }
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1]);
}

#[test]
fn lk_20_a_relay_refuses_what_is_malformed_unsigned_oversize_or_unaddressed() {
    let mut rig = rig_with_counter_mailbox();
    let good = msg(1);
    // Not well formed.
    assert_eq!(
        rig.put(good[..good.len() - 1].to_vec()),
        Response::Refused(Refusal::NotWellFormed)
    );
    assert_eq!(rig.put(vec![]), Response::Refused(Refusal::NotWellFormed));
    // A bad signature, and a ciphertext that is not the signed one.
    let mut bad_sig = good.clone();
    bad_sig[PREFIX_SIG] ^= 1;
    assert_eq!(rig.put(bad_sig), Response::Refused(Refusal::BadSignature));
    let mut bad_ct = good.clone();
    let last = bad_ct.len() - 1;
    bad_ct[last] ^= 1;
    assert_eq!(rig.put(bad_ct), Response::Refused(Refusal::BadSignature));
    // Over the largest step plus the header.
    let mut big = good.clone();
    big.resize(crate::seal::MAX_SEALED_LEN + 1, 0);
    assert_eq!(rig.put(big), Response::Refused(Refusal::TooLarge));
    // The largest allowed is taken.
    let largest = owner_to_counter(2, Kind::Batch, &vec![1u8; MAX_CONTENT_LEN]);
    assert_eq!(largest.len(), crate::seal::MAX_SEALED_LEN);
    assert!(matches!(rig.put(largest), Response::Stored { .. }));
    // No mailbox for the reader.
    let mut rng = Counting(0);
    let to_box = seal(
        &owner(),
        Role::Owner,
        &the_box().device_id(),
        Role::Box,
        1,
        Kind::Settings,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert_eq!(rig.put(to_box), Response::Refused(Refusal::NoMailbox));
    // An unknown kind byte is not well formed.
    let mut unknown = good;
    unknown[77] = 0;
    assert_eq!(rig.put(unknown), Response::Refused(Refusal::NotWellFormed));
}

const PREFIX_SIG: usize = crate::seal::PREFIX_LEN + 32;

#[test]
fn lk_21_a_mailbox_is_made_when_the_reader_proves_its_key() {
    let mut rig = Rig::new();
    assert_eq!(rig.relay.mailbox_count(), 0);
    // A signature by another key does not make it.
    let challenge = rig.challenge();
    let sig = sign_challenge(&stranger(), &challenge, &rig.relay_key(), Action::Make, 0);
    let r = rig.ask(Request::MakeMailbox {
        reader: counter().device_id(),
        challenge,
        sig,
    });
    assert_eq!(r, Response::Refused(Refusal::ChallengeSignature));
    assert_eq!(rig.relay.mailbox_count(), 0);
    assert_eq!(rig.make(&counter()), Response::Made);
    assert_eq!(rig.relay.mailbox_count(), 1);
    // Making it again changes nothing.
    assert_eq!(rig.make(&counter()), Response::Made);
    assert_eq!(rig.relay.mailbox_count(), 1);
}

#[test]
fn lk_21_a_mailbox_exists_only_for_a_member() {
    let mut rig = Rig::new();
    assert_eq!(rig.make(&stranger()), Response::Refused(Refusal::NotMember));
    assert_eq!(rig.relay.mailbox_count(), 0);
    assert_eq!(rig.make(&counter()), Response::Made);
}

#[test]
fn lk_21_a_relay_has_at_most_64_members() {
    let mut rig = Rig::new();
    let ids64: Vec<DeviceId> = (1..=64u8).map(|n| id(n).device_id()).collect();
    rig.relay.set_members(&ids64, rig.now).unwrap();
    for n in 1..=64u8 {
        assert_eq!(rig.make(&id(n)), Response::Made, "mailbox {n}");
    }
    assert_eq!(rig.relay.mailbox_count(), 64);
    let mut ids65 = ids64.clone();
    ids65.push(id(65).device_id());
    assert!(matches!(
        rig.relay.set_members(&ids65, rig.now),
        Err(MembersError::TooMany)
    ));
    // The list is as it was.
    assert_eq!(rig.relay.members().count(), 64);
    assert_eq!(rig.make(&id(65)), Response::Refused(Refusal::NotMember));
}

#[test]
fn lk_21_the_mailbox_of_a_removed_member_is_not_a_member_mailbox() {
    let mut rig = rig_with_counter_mailbox();
    assert_eq!(rig.make(&the_box()), Response::Made);
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    let keep = [owner().device_id(), the_box().device_id()];
    rig.relay.set_members(&keep, rig.now).unwrap();
    // It is leaving (LK-28): held, but not a member's.
    assert!(rig.relay.mailbox(&counter().device_id()).is_some());
    assert_eq!(rig.relay.members().count(), 2);
    assert_eq!(rig.make(&counter()), Response::Refused(Refusal::NotMember));
    // Added back: a normal mailbox again.
    let all = [
        owner().device_id(),
        counter().device_id(),
        the_box().device_id(),
    ];
    rig.relay.set_members(&all, rig.now).unwrap();
    assert_eq!(rig.relay.leaving().count(), 0);
    assert_eq!(rig.make(&counter()), Response::Made);
    assert_eq!(rig.put(msg(2)), Response::Stored { id: 2 });
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1, 2]);
    // A list the store refuses changes nothing.
    rig.relay.store_mut().fail_writes = true;
    assert!(matches!(
        rig.relay.set_members(&keep, rig.now),
        Err(MembersError::Store(_))
    ));
    assert_eq!(rig.relay.members().count(), 3);
    assert_eq!(rig.relay.leaving().count(), 0);
}

fn without_counter(rig: &Rig) -> Vec<DeviceId> {
    rig.relay
        .members()
        .copied()
        .filter(|m| *m != counter().device_id())
        .collect()
}

#[test]
fn lk_28_a_removed_device_fetches_what_was_queued_for_it() {
    let mut rig = rig_with_counter_mailbox();
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    assert_eq!(rig.put(msg(2)), Response::Stored { id: 2 });
    let keep = without_counter(&rig);
    rig.relay.set_members(&keep, rig.now).unwrap();
    assert_eq!(
        rig.relay.leaving().map(|(d, _)| *d).collect::<Vec<_>>(),
        vec![counter().device_id()]
    );
    // The same proof of key (LK-23) fetches it.
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1, 2]);
}

#[test]
fn lk_28_a_leaving_mailbox_takes_no_new_message_and_allows_a_fetch_and_nothing_else() {
    let mut rig = rig_with_counter_mailbox();
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    rig.relay
        .set_members(&without_counter(&rig), rig.now)
        .unwrap();
    // From a member to the leaving device, and from the leaving device.
    assert_eq!(rig.put(msg(2)), Response::Refused(Refusal::ReaderNotMember));
    let mut rng = Counting(0);
    let from_counter = seal(
        &counter(),
        Role::Counter,
        &owner().device_id(),
        Role::Owner,
        1,
        Kind::CheckIn,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert_eq!(
        rig.put(from_counter),
        Response::Refused(Refusal::SenderNotMember)
    );
    // It cannot be made again, and nothing is acknowledged in it.
    assert_eq!(rig.make(&counter()), Response::Refused(Refusal::NotMember));
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1]);
    assert!(rig.relay.mailbox(&counter().device_id()).is_none());
    let mut rig = rig_with_counter_mailbox();
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    rig.relay
        .set_members(&without_counter(&rig), rig.now)
        .unwrap();
    assert_eq!(
        rig.ack(&counter(), 1),
        Response::Refused(Refusal::NotMember)
    );
    assert_eq!(rig.relay.mailbox(&counter().device_id()).unwrap().len(), 1);
}

#[test]
fn lk_29_a_leaving_mailbox_is_gone_after_a_full_fetch() {
    let mut rig = rig_with_counter_mailbox();
    for n in 1..=70u64 {
        assert!(matches!(rig.put(msg(n)), Response::Stored { .. }));
    }
    rig.relay
        .set_members(&without_counter(&rig), rig.now)
        .unwrap();
    // 64 at a time: the first fetch is not the end.
    assert_eq!(ids(rig.fetch(&counter(), 0)).len(), 64);
    assert_eq!(rig.relay.leaving().count(), 1);
    assert_eq!(ids(rig.fetch(&counter(), 64)).len(), 6);
    assert_eq!(rig.relay.leaving().count(), 0);
    assert_eq!(rig.relay.mailbox_count(), 0);
    assert_eq!(rig.relay.store().len(), 0);
    assert_eq!(
        rig.fetch(&counter(), 0),
        Response::Refused(Refusal::NoMailbox)
    );
}

#[test]
fn lk_29_a_leaving_mailbox_is_gone_after_seven_days() {
    let mut rig = rig_with_counter_mailbox();
    assert_eq!(rig.put(msg(1)), Response::Stored { id: 1 });
    let at = rig.now;
    rig.relay.set_members(&without_counter(&rig), at).unwrap();
    rig.now = at + LEAVING_SECS - 1;
    assert!(matches!(
        rig.ask(Request::Hello { nonce: [1; 32] }),
        Response::Hello { .. }
    ));
    assert_eq!(rig.relay.leaving().count(), 1);
    rig.now = at + LEAVING_SECS;
    assert!(matches!(
        rig.ask(Request::Hello { nonce: [1; 32] }),
        Response::Hello { .. }
    ));
    assert_eq!(rig.relay.leaving().count(), 0);
    assert_eq!(rig.relay.mailbox_count(), 0);
    assert_eq!(rig.relay.store().len(), 0);
    assert_eq!(
        rig.fetch(&counter(), 0),
        Response::Refused(Refusal::NoMailbox)
    );
}

#[test]
fn lk_22_a_mailbox_holds_256_messages_and_the_oldest_goes() {
    let mut rig = rig_with_counter_mailbox();
    for n in 1..=257u64 {
        assert!(matches!(rig.put(msg(n)), Response::Stored { .. }));
    }
    let mb = rig.relay.mailbox(&counter().device_id()).unwrap();
    assert_eq!(mb.len(), 256);
    let first = mb.messages().next().unwrap().0;
    assert_eq!(first, 2);
    // The reader sees from the second on.
    let got = ids(rig.fetch(&counter(), 0));
    assert_eq!(got.first(), Some(&2));
}

#[test]
fn lk_22_a_mailbox_holds_4_mib_and_the_oldest_goes() {
    let mut rig = rig_with_counter_mailbox();
    let big = |n: u64| owner_to_counter(n, Kind::Batch, &vec![7u8; MAX_CONTENT_LEN]);
    for n in 1..=70u64 {
        assert!(matches!(rig.put(big(n)), Response::Stored { .. }));
    }
    let mb = rig.relay.mailbox(&counter().device_id()).unwrap();
    assert!(mb.bytes() <= 4 * 1024 * 1024);
    // 4 MiB holds 63 of these; one more would not fit.
    assert_eq!(mb.len(), 63);
    assert_eq!(mb.messages().next().unwrap().0, 8);
}

#[test]
fn lk_23_nobody_can_read_list_or_empty_a_mailbox_without_a_challenge() {
    let mut rig = rig_with_counter_mailbox();
    rig.put(msg(1));
    let sig = counter().sign_tagged("OpenBMT/link-fetch/v1", b"anything");
    for req in [
        Request::Fetch {
            reader: counter().device_id(),
            challenge: [9; 32],
            sig,
            cursor: 0,
        },
        Request::Ack {
            reader: counter().device_id(),
            challenge: [9; 32],
            sig,
            up_to: 1,
        },
        Request::MakeMailbox {
            reader: id(50).device_id(),
            challenge: [9; 32],
            sig,
        },
    ] {
        assert_eq!(rig.ask(req), Response::Refused(Refusal::ChallengeUnknown));
    }
    assert_eq!(rig.relay.mailbox(&counter().device_id()).unwrap().len(), 1);
}

#[test]
fn lk_23_a_challenge_is_used_once() {
    let mut rig = rig_with_counter_mailbox();
    let challenge = rig.challenge();
    let sig = sign_challenge(&counter(), &challenge, &rig.relay_key(), Action::Fetch, 0);
    let req = Request::Fetch {
        reader: counter().device_id(),
        challenge,
        sig,
        cursor: 0,
    };
    assert!(matches!(rig.ask(req.clone()), Response::Messages { .. }));
    assert_eq!(rig.ask(req), Response::Refused(Refusal::ChallengeUnknown));
}

#[test]
fn lk_23_a_challenge_is_good_for_sixty_seconds() {
    let mut rig = rig_with_counter_mailbox();
    for (wait, ok) in [(CHALLENGE_SECS, true), (CHALLENGE_SECS + 1, false)] {
        let challenge = rig.challenge();
        let sig = sign_challenge(&counter(), &challenge, &rig.relay_key(), Action::Fetch, 0);
        rig.now += wait;
        let r = rig.ask(Request::Fetch {
            reader: counter().device_id(),
            challenge,
            sig,
            cursor: 0,
        });
        if ok {
            assert!(matches!(r, Response::Messages { .. }), "{r:?}");
        } else {
            assert_eq!(r, Response::Refused(Refusal::ChallengeExpired));
        }
    }
}

#[test]
fn lk_23_a_signature_by_another_key_or_for_another_request_is_refused() {
    let mut rig = rig_with_counter_mailbox();
    // Another key signs a fetch for the counter's mailbox.
    let c = rig.challenge();
    let sig = sign_challenge(&stranger(), &c, &rig.relay_key(), Action::Fetch, 0);
    let r = rig.ask(Request::Fetch {
        reader: counter().device_id(),
        challenge: c,
        sig,
        cursor: 0,
    });
    assert_eq!(r, Response::Refused(Refusal::ChallengeSignature));
    // The counter's signature for a fetch from 0 does not serve a fetch
    // from 5, nor an ack.
    let c = rig.challenge();
    let sig = sign_challenge(&counter(), &c, &rig.relay_key(), Action::Fetch, 0);
    let r = rig.ask(Request::Fetch {
        reader: counter().device_id(),
        challenge: c,
        sig,
        cursor: 5,
    });
    assert_eq!(r, Response::Refused(Refusal::ChallengeSignature));
    let c = rig.challenge();
    let sig = sign_challenge(&counter(), &c, &rig.relay_key(), Action::Fetch, 3);
    let r = rig.ask(Request::Ack {
        reader: counter().device_id(),
        challenge: c,
        sig,
        up_to: 3,
    });
    assert_eq!(r, Response::Refused(Refusal::ChallengeSignature));
    // A signature made for another relay is no good here.
    let c = rig.challenge();
    let sig = sign_challenge(&counter(), &c, &id(77).device_id(), Action::Fetch, 0);
    let r = rig.ask(Request::Fetch {
        reader: counter().device_id(),
        challenge: c,
        sig,
        cursor: 0,
    });
    assert_eq!(r, Response::Refused(Refusal::ChallengeSignature));
}

fn make_request(rig: &Rig, challenge: [u8; 32]) -> Request {
    let sig = sign_challenge(&counter(), &challenge, &rig.relay_key(), Action::Make, 0);
    Request::MakeMailbox {
        reader: counter().device_id(),
        challenge,
        sig,
    }
}

fn challenge_on(rig: &mut Rig, conn: u64) -> [u8; 32] {
    match rig.ask_on(conn, Request::Challenge) {
        Response::Challenge { challenge } => challenge,
        other => panic!("{other:?}"),
    }
}

#[test]
fn lk_23_a_challenge_is_good_only_on_the_connection_that_asked() {
    let mut rig = Rig::new();
    let c = challenge_on(&mut rig, 1);
    let req = make_request(&rig, c);
    assert_eq!(
        rig.ask_on(2, req.clone()),
        Response::Refused(Refusal::ChallengeUnknown)
    );
    // The attempt on another connection did not use it up.
    assert_eq!(rig.ask_on(1, req), Response::Made);
}

#[test]
fn lk_23_a_connection_holds_one_challenge_and_a_new_one_replaces_it() {
    let mut rig = Rig::new();
    let first = challenge_on(&mut rig, 1);
    let second = challenge_on(&mut rig, 1);
    assert_ne!(first, second);
    let old = make_request(&rig, first);
    assert_eq!(
        rig.ask_on(1, old),
        Response::Refused(Refusal::ChallengeUnknown)
    );
    // The wrong value used up the connection's challenge too: ask again.
    let third = challenge_on(&mut rig, 1);
    let ok = make_request(&rig, third);
    assert_eq!(rig.ask_on(1, ok), Response::Made);
    // Other connections have their own.
    let a = challenge_on(&mut rig, 5);
    let b = challenge_on(&mut rig, 6);
    assert_eq!(rig.ask_on(6, make_request(&rig, b)), Response::Made);
    assert_eq!(rig.ask_on(5, make_request(&rig, a)), Response::Made);
}

#[test]
fn lk_23_a_challenge_dies_with_its_connection() {
    let mut rig = Rig::new();
    let c = challenge_on(&mut rig, 1);
    rig.relay.close(1);
    assert_eq!(
        rig.ask_on(1, make_request(&rig, c)),
        Response::Refused(Refusal::ChallengeUnknown)
    );
    // And randomness that fails gives no challenge.
    let r = rig
        .relay
        .handle(1, Request::Challenge, 0, &mut |_: &mut [u8; 32]| {
            Err::<(), _>("no")
        });
    assert_eq!(r, Response::Refused(Refusal::NoRandomness));
}

#[test]
fn lk_24_fetch_gives_what_is_after_the_cursor_oldest_first_and_deletes_nothing() {
    let mut rig = rig_with_counter_mailbox();
    for n in 1..=5u64 {
        rig.put(msg(n));
    }
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1, 2, 3, 4, 5]);
    assert_eq!(ids(rig.fetch(&counter(), 2)), vec![3, 4, 5]);
    assert_eq!(ids(rig.fetch(&counter(), 5)), Vec::<u64>::new());
    // Again: nothing was deleted.
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1, 2, 3, 4, 5]);
    assert_eq!(rig.relay.mailbox(&counter().device_id()).unwrap().len(), 5);
}

#[test]
fn lk_24_fetch_gives_at_most_64_at_a_time() {
    let mut rig = rig_with_counter_mailbox();
    for n in 1..=100u64 {
        rig.put(msg(n));
    }
    let Response::Messages { items, more } = rig.fetch(&counter(), 0) else {
        panic!()
    };
    assert_eq!(items.len(), 64);
    assert!(more);
    assert_eq!(items.last().unwrap().0, 64);
    let Response::Messages { items, more } = rig.fetch(&counter(), 64) else {
        panic!()
    };
    assert_eq!(items.len(), 36);
    assert!(!more);
}

#[test]
fn lk_24_an_acknowledgement_drops_only_what_was_fetched() {
    let mut rig = rig_with_counter_mailbox();
    for n in 1..=5u64 {
        rig.put(msg(n));
    }
    // Nothing fetched yet: an ack drops nothing, whatever it says.
    assert_eq!(rig.ack(&counter(), 5), Response::Acked { dropped: 0 });
    assert_eq!(rig.relay.mailbox(&counter().device_id()).unwrap().len(), 5);
    // Fetch the first three, then say 5: only 1 to 3 go.
    let Response::Messages { items, .. } = rig.fetch(&counter(), 0) else {
        panic!()
    };
    assert_eq!(items.len(), 5);
    let mut rig2 = rig_with_counter_mailbox();
    for n in 1..=5u64 {
        rig2.put(msg(n));
    }
    rig2.fetch(&counter(), 0);
    assert_eq!(rig2.ack(&counter(), 3), Response::Acked { dropped: 3 });
    assert_eq!(
        rig2.relay
            .mailbox(&counter().device_id())
            .unwrap()
            .messages()
            .map(|(i, _)| i)
            .collect::<Vec<_>>(),
        vec![4, 5]
    );
    // A later arrival is not dropped by an old ack.
    rig2.put(msg(6));
    assert_eq!(rig2.ack(&counter(), 100), Response::Acked { dropped: 2 });
    assert_eq!(rig2.relay.mailbox(&counter().device_id()).unwrap().len(), 1);
    // Numbers are not reused after the mailbox empties.
    let r = rig2.fetch(&counter(), 0);
    assert_eq!(ids(r), vec![6]);
    assert_eq!(rig2.put(msg(7)), Response::Stored { id: 7 });
}

#[test]
fn lk_25_a_relay_keeps_the_sealed_bytes_and_nothing_else() {
    let mut rig = rig_with_counter_mailbox();
    let sealed = msg(1);
    rig.put(sealed.clone());
    let events = rig.relay.store().events(&counter().device_id());
    assert_eq!(
        events,
        vec![Event::Put {
            id: 1,
            sealed: sealed.clone()
        }]
    );
    // What the relay could see is the header; the content is not in it.
    let content = b"the shop's private content";
    let m = owner_to_counter(2, Kind::Command, content);
    assert!(!m.windows(content.len()).any(|w| w == content));
    let seen = crate::seal::inspect(&m).unwrap();
    assert_eq!((seen.kind, seen.step), (Kind::Command, 256));
}

#[test]
fn lk_26_a_relay_cannot_return_another_readers_messages() {
    let mut rig = rig_with_counter_mailbox();
    assert_eq!(rig.make(&the_box()), Response::Made);
    rig.put(msg(1));
    // The box fetches: it gets its own mailbox, which is empty.
    assert!(items(rig.fetch(&the_box(), 0)).is_empty());
    // The box signs a fetch but names the counter: refused.
    let c = rig.challenge();
    let sig = sign_challenge(&the_box(), &c, &rig.relay_key(), Action::Fetch, 0);
    let r = rig.ask(Request::Fetch {
        reader: counter().device_id(),
        challenge: c,
        sig,
        cursor: 0,
    });
    assert_eq!(r, Response::Refused(Refusal::ChallengeSignature));
    // Acks the same.
    assert_eq!(rig.ack(&the_box(), 99), Response::Acked { dropped: 0 });
    assert_eq!(rig.relay.mailbox(&counter().device_id()).unwrap().len(), 1);
}

#[test]
fn lk_26_a_relay_that_hands_out_an_old_message_again_is_refused_by_the_reader() {
    let mut rig = rig_with_counter_mailbox();
    rig.put(msg(1));
    let first = items(rig.fetch(&counter(), 0));
    // The reader takes it (numbers 1), and acknowledges.
    let o = open(&counter(), Role::Counter, &first[0].1, allowed, none_taken).unwrap();
    assert_eq!(o.number, 1);
    // A lying relay gives the same sealed bytes again: the reader's last
    // number is 1 now.
    let again = open(&counter(), Role::Counter, &first[0].1, allowed, |_| 1);
    assert_eq!(again.unwrap_err(), OpenError::Replay { last: 1, got: 1 });
    // It cannot change a message: any change is refused.
    let mut changed = first[0].1.clone();
    let n = changed.len();
    changed[n - 1] ^= 1;
    assert!(open(&counter(), Role::Counter, &changed, allowed, none_taken).is_err());
}

#[test]
fn lk_41_a_failed_write_is_refused_and_leaves_memory_as_it_was() {
    let mut rig = rig_with_counter_mailbox();
    rig.put(msg(1));
    rig.relay.store_mut().fail_writes = true;
    assert_eq!(rig.put(msg(2)), Response::Refused(Refusal::Storage));
    assert_eq!(rig.make(&the_box()), Response::Refused(Refusal::Storage));
    rig.fetch(&counter(), 0);
    assert_eq!(rig.ack(&counter(), 1), Response::Refused(Refusal::Storage));
    let mb = rig.relay.mailbox(&counter().device_id()).unwrap();
    assert_eq!(mb.len(), 1);
    assert_eq!(rig.relay.mailbox_count(), 1);
    rig.relay.store_mut().fail_writes = false;
    assert_eq!(rig.put(msg(2)), Response::Stored { id: 2 });
}

#[test]
fn lk_41_a_relay_restarted_over_its_store_has_the_same_mailboxes() {
    let mut rig = rig_with_counter_mailbox();
    for n in 1..=4u64 {
        rig.put(msg(n));
    }
    rig.fetch(&counter(), 0);
    rig.ack(&counter(), 2);
    let mut store = MemoryStore::new();
    let loaded = rig.relay.store_mut().load().unwrap();
    store.set_members(&loaded.members, &[]).unwrap();
    for (reader, events) in loaded.mailboxes {
        for e in events {
            match e {
                Event::Put { id, sealed } => store.put(&reader, id, &sealed).unwrap(),
                Event::Floor(n) => store.ack(&reader, n).unwrap(),
            }
        }
    }
    let again = Relay::new(id(99), RelayConfig::default(), store).unwrap();
    let a = again.mailbox(&counter().device_id()).unwrap();
    let b = rig.relay.mailbox(&counter().device_id()).unwrap();
    assert_eq!(
        a.messages().collect::<Vec<_>>(),
        b.messages().collect::<Vec<_>>()
    );
    assert_eq!(a.next_id(), 5);
}

#[test]
fn lk_20_a_copy_of_a_message_already_taken_is_refused_and_pushes_nothing_out() {
    let cfg = RelayConfig {
        max_messages: 3,
        ..RelayConfig::default()
    };
    let mut rig = Rig::with(cfg);
    assert_eq!(rig.make(&counter()), Response::Made);
    let first = msg(1);
    assert!(matches!(rig.put(first.clone()), Response::Stored { .. }));
    assert!(matches!(rig.put(msg(2)), Response::Stored { .. }));
    assert!(matches!(rig.put(msg(3)), Response::Stored { .. }));
    // The same bytes again, and an older number, as often as one likes.
    for _ in 0..10 {
        assert_eq!(
            rig.put(first.clone()),
            Response::Refused(Refusal::AlreadyTaken)
        );
        assert_eq!(rig.put(msg(2)), Response::Refused(Refusal::AlreadyTaken));
    }
    assert_eq!(ids(rig.fetch(&counter(), 0)), vec![1, 2, 3]);
    // A gap is fine, and what was skipped is then too old.
    assert!(matches!(rig.put(msg(9)), Response::Stored { .. }));
    assert_eq!(rig.put(msg(5)), Response::Refused(Refusal::AlreadyTaken));
    // Another sender's numbers are its own.
    let mut rng = Counting(0);
    let from_box = seal(
        &the_box(),
        Role::Box,
        &counter().device_id(),
        Role::Counter,
        1,
        Kind::PosNotice,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert!(matches!(rig.put(from_box), Response::Stored { .. }));
    // A member that leaves and comes back starts again.
    let members: Vec<DeviceId> = rig.relay.members().copied().collect();
    rig.relay.set_members(&[], rig.now).unwrap();
    rig.relay.set_members(&members, rig.now).unwrap();
    assert_eq!(rig.make(&counter()), Response::Made);
    assert!(matches!(rig.put(msg(1)), Response::Stored { .. }));
}

#[test]
fn lk_27_a_plain_relay_refuses_a_claim_and_a_welcome() {
    let mut rig = Rig::new();
    assert_eq!(rig.ask(Request::Claim), Response::Refused(Refusal::NoClaim));
    let welcome = Request::Welcome {
        welcome: vec![1; 101],
        settings: vec![2; 10],
    };
    assert_eq!(rig.ask(welcome), Response::Refused(Refusal::WelcomeRefused));
}

#[test]
fn lk_27_a_relay_answers_a_claim_while_one_is_set() {
    let mut rig = Rig::new();
    rig.relay.set_claim(Some(vec![9; 101]));
    assert_eq!(
        rig.ask(Request::Claim),
        Response::Claim {
            bytes: vec![9; 101]
        }
    );
    // Asking does not use it up.
    assert_eq!(
        rig.ask(Request::Claim),
        Response::Claim {
            bytes: vec![9; 101]
        }
    );
    rig.relay.set_claim(None);
    assert_eq!(rig.ask(Request::Claim), Response::Refused(Refusal::NoClaim));
}

#[test]
fn lk_27_a_welcome_goes_to_the_hook_and_a_yes_sets_the_members() {
    let mut rig = Rig::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = std::sync::Arc::clone(&seen);
    let members = vec![owner().device_id(), counter().device_id()];
    rig.relay.set_welcome_hook(Some(Box::new(move |w, s, now| {
        log.lock().unwrap().push((w.to_vec(), s.to_vec(), now));
        WelcomeAnswer::Welcomed(members.clone())
    })));
    rig.relay.set_claim(Some(vec![1; 101]));
    let answer = rig.ask(Request::Welcome {
        welcome: vec![3; 5],
        settings: vec![4; 6],
    });
    assert_eq!(answer, Response::Welcomed);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![(vec![3; 5], vec![4; 6], rig.now)]
    );
    assert_eq!(rig.relay.members().count(), 2);
    // Taken: the claim is no longer offered.
    assert_eq!(rig.ask(Request::Claim), Response::Refused(Refusal::NoClaim));
}

#[test]
fn lk_27_a_refusal_keeps_the_claim_unless_the_hook_withdraws_it() {
    let mut rig = Rig::new();
    rig.relay.set_claim(Some(vec![1; 101]));
    let before: Vec<DeviceId> = rig.relay.members().copied().collect();
    let welcome = || Request::Welcome {
        welcome: vec![3; 5],
        settings: vec![4; 6],
    };
    for (answer, claim_stays) in [
        (WelcomeAnswer::Refused, true),
        (WelcomeAnswer::Withdrawn, false),
    ] {
        rig.relay.set_claim(Some(vec![1; 101]));
        let a = answer.clone();
        rig.relay
            .set_welcome_hook(Some(Box::new(move |_, _, _| a.clone())));
        // One refusal for every answer, and the members never change.
        assert_eq!(
            rig.ask(welcome()),
            Response::Refused(Refusal::WelcomeRefused)
        );
        let after: Vec<DeviceId> = rig.relay.members().copied().collect();
        assert_eq!(before, after);
        assert_eq!(rig.relay.claim().is_some(), claim_stays, "{answer:?}");
    }
}

#[test]
fn lk_27_a_relay_can_be_quiet_while_it_has_no_members() {
    let mut rig = Rig::new();
    rig.relay.set_quiet_without_members(true);
    assert!(matches!(
        rig.ask(Request::Challenge),
        Response::Challenge { .. }
    ));
    rig.relay.set_members(&[], rig.now).unwrap();
    assert_eq!(
        rig.ask(Request::Challenge),
        Response::Refused(Refusal::NotMember)
    );
    // The hello and the claim are still answered.
    assert!(matches!(
        rig.ask(Request::Hello { nonce: [1; 32] }),
        Response::Hello { .. }
    ));
    rig.relay.set_claim(Some(vec![1; 101]));
    assert!(matches!(rig.ask(Request::Claim), Response::Claim { .. }));
}
