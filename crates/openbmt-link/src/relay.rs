//! The relay's state machine (LK-20 to LK-26).
//!
//! [`Relay::handle`] takes one request, the time and a source of randomness,
//! and gives one answer. It opens no sockets and reads no clock. Its
//! storage is behind [`MailStore`](crate::MailStore); a change is made in
//! storage first and in memory only if storage said yes.
//!
//! Each message in a mailbox has a number the relay gives it, rising and never
//! reused in that mailbox. A fetch names the last number the reader has; an
//! acknowledgement names the last number the reader has taken. The relay
//! drops only numbers it has already handed out in a fetch, so an
//! acknowledgement cannot drop a message the reader never saw.
//!
//! A relay keeps the sealed bytes and nothing else about a message (LK-25).

use std::collections::{BTreeMap, VecDeque};

use openbmt_core::identity::{DeviceId, Identity, Sig};

use crate::proto::{Refusal, Request, Response, FETCH_BATCH, FETCH_BYTES};
use crate::seal::{inspect, OpenError, MAX_SEALED_LEN};
use crate::store::{Event, MailStore};

/// The tag of a reader's signature over a challenge (LK-23).
pub const TAG_FETCH: &str = "OpenBMT/link-fetch/v1";
/// The tag of the relay's signature over a device's nonce (LK-32).
pub const TAG_HELLO: &str = "OpenBMT/link-hello/v1";
/// Seconds a challenge is good for (LK-23).
pub const CHALLENGE_SECS: u64 = 60;

/// What a reader's signature on a challenge asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Make the mailbox (LK-21). The number is 0.
    Make,
    /// Fetch after this cursor (LK-24).
    Fetch,
    /// Acknowledge up to this number (LK-24).
    Ack,
}

impl Action {
    fn byte(self) -> u8 {
        match self {
            Self::Make => 1,
            Self::Fetch => 2,
            Self::Ack => 3,
        }
    }
}

/// The bytes a reader signs for LK-23: the challenge, the relay's key, and
/// what is asked (the action and its number). LK-23 names the first two;
/// the last two are added so that a frame changed on the way, on a network
/// where frames are not encrypted (LK-33), cannot turn one request into
/// another under the same signature.
pub fn challenge_message(
    challenge: &[u8; 32],
    relay: &DeviceId,
    action: Action,
    number: u64,
) -> Vec<u8> {
    let mut m = Vec::with_capacity(73);
    m.extend_from_slice(challenge);
    m.extend_from_slice(&relay.to_bytes());
    m.push(action.byte());
    m.extend_from_slice(&number.to_le_bytes());
    m
}

/// A reader's signature for a request (LK-23).
pub fn sign_challenge(
    reader: &Identity,
    challenge: &[u8; 32],
    relay: &DeviceId,
    action: Action,
    number: u64,
) -> Sig {
    reader.sign_tagged(
        TAG_FETCH,
        &challenge_message(challenge, relay, action, number),
    )
}

/// The bytes the relay signs in its hello (LK-32): the nonce and its own key.
pub fn hello_message(nonce: &[u8; 32], relay: &DeviceId) -> Vec<u8> {
    let mut m = Vec::with_capacity(64);
    m.extend_from_slice(nonce);
    m.extend_from_slice(&relay.to_bytes());
    m
}

/// A relay's limits. The defaults are the ones in the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayConfig {
    /// Mailboxes (LK-21).
    pub max_mailboxes: usize,
    /// Messages in a mailbox (LK-22).
    pub max_messages: usize,
    /// Sealed bytes in a mailbox (LK-22).
    pub max_bytes: usize,
    /// Challenges given and not yet used. Not in the rules: without a bound
    /// the relay's memory is the limit (see the notes in `lib.rs`).
    pub max_challenges: usize,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            max_mailboxes: 64,
            max_messages: 256,
            max_bytes: 4 * 1024 * 1024,
            max_challenges: 256,
        }
    }
}

/// One reader's mailbox.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailbox {
    messages: VecDeque<(u64, Vec<u8>)>,
    bytes: usize,
    /// The number the next message gets.
    next_id: u64,
    /// The highest number handed out in a fetch. Not stored: after a
    /// restart nothing can be acknowledged until it is fetched again.
    fetched: u64,
}

impl Mailbox {
    /// Messages held.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether no message is held.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Sealed bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The number the next message gets.
    pub fn next_id(&self) -> u64 {
        self.next_id.max(1)
    }

    /// The held messages, oldest first, with their numbers.
    pub fn messages(&self) -> impl Iterator<Item = (u64, &[u8])> {
        self.messages.iter().map(|(id, m)| (*id, m.as_slice()))
    }

    fn push(&mut self, id: u64, sealed: Vec<u8>, cfg: &RelayConfig) {
        self.bytes = self.bytes.saturating_add(sealed.len());
        self.messages.push_back((id, sealed));
        self.next_id = self.next_id.max(id.saturating_add(1));
        while self.messages.len() > cfg.max_messages || self.bytes > cfg.max_bytes {
            match self.messages.pop_front() {
                Some((_, m)) => self.bytes = self.bytes.saturating_sub(m.len()),
                None => break,
            }
        }
    }

    fn drop_through(&mut self, up_to: u64) -> usize {
        let mut dropped = 0usize;
        while self.messages.front().is_some_and(|(id, _)| *id <= up_to) {
            if let Some((_, m)) = self.messages.pop_front() {
                self.bytes = self.bytes.saturating_sub(m.len());
                dropped = dropped.saturating_add(1);
            }
        }
        self.next_id = self.next_id.max(up_to.saturating_add(1));
        dropped
    }

    /// Build a mailbox from what storage kept, in order.
    pub fn replay(events: Vec<Event>, cfg: &RelayConfig) -> Self {
        let mut mailbox = Self::default();
        for event in events {
            match event {
                Event::Put { id, sealed } => mailbox.push(id, sealed, cfg),
                Event::Floor(up_to) => {
                    mailbox.drop_through(up_to);
                }
            }
        }
        mailbox
    }
}

/// What a relay is asked and what it keeps.
pub struct Relay<S: MailStore> {
    identity: Identity,
    cfg: RelayConfig,
    store: S,
    mailboxes: BTreeMap<DeviceId, Mailbox>,
    /// Challenges given and not used, oldest first, with the time given.
    challenges: VecDeque<([u8; 32], u64)>,
}

impl<S: MailStore> Relay<S> {
    /// A relay that loads what `store` holds.
    pub fn new(identity: Identity, cfg: RelayConfig, mut store: S) -> Result<Self, S::Error> {
        let mut mailboxes = BTreeMap::new();
        for (reader, events) in store.load()? {
            mailboxes.insert(reader, Mailbox::replay(events, &cfg));
        }
        Ok(Self {
            identity,
            cfg,
            store,
            mailboxes,
            challenges: VecDeque::new(),
        })
    }

    /// The relay's own identity key (LK-32).
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// How many mailboxes there are.
    pub fn mailbox_count(&self) -> usize {
        self.mailboxes.len()
    }

    /// A reader's mailbox, if it has one.
    pub fn mailbox(&self, reader: &DeviceId) -> Option<&Mailbox> {
        self.mailboxes.get(reader)
    }

    /// The storage, for tests and for tools.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// The storage, for tests.
    #[cfg(test)]
    pub(crate) fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Remove every mailbox: the merchant's way to clear them (LK-21).
    pub fn clear(&mut self) -> Result<(), S::Error> {
        self.store.clear()?;
        self.mailboxes.clear();
        Ok(())
    }

    /// Answer one request.
    pub fn handle<E>(
        &mut self,
        request: Request,
        now: u64,
        random: &mut impl FnMut(&mut [u8; 32]) -> Result<(), E>,
    ) -> Response {
        match request {
            Request::Hello { nonce } => {
                let relay = self.identity.device_id();
                let sig = self
                    .identity
                    .sign_tagged(TAG_HELLO, &hello_message(&nonce, &relay));
                Response::Hello { relay, sig }
            }
            Request::Challenge => self.give_challenge(now, random),
            Request::MakeMailbox {
                reader,
                challenge,
                sig,
            } => match self.check_request(&reader, &challenge, &sig, Action::Make, 0, now) {
                Err(why) => Response::Refused(why),
                Ok(()) => self.make(reader),
            },
            Request::Put { sealed } => self.put(sealed),
            Request::Fetch {
                reader,
                challenge,
                sig,
                cursor,
            } => match self.check_request(&reader, &challenge, &sig, Action::Fetch, cursor, now) {
                Err(why) => Response::Refused(why),
                Ok(()) => self.fetch(&reader, cursor),
            },
            Request::Ack {
                reader,
                challenge,
                sig,
                up_to,
            } => match self.check_request(&reader, &challenge, &sig, Action::Ack, up_to, now) {
                Err(why) => Response::Refused(why),
                Ok(()) => self.ack(&reader, up_to),
            },
        }
    }

    fn give_challenge<E>(
        &mut self,
        now: u64,
        random: &mut impl FnMut(&mut [u8; 32]) -> Result<(), E>,
    ) -> Response {
        let mut challenge = [0u8; 32];
        if random(&mut challenge).is_err() {
            return Response::Refused(Refusal::NoRandomness);
        }
        while self
            .challenges
            .front()
            .is_some_and(|(_, at)| now.saturating_sub(*at) > CHALLENGE_SECS)
        {
            self.challenges.pop_front();
        }
        while self.challenges.len() >= self.cfg.max_challenges.max(1) {
            self.challenges.pop_front();
        }
        self.challenges.push_back((challenge, now));
        Response::Challenge { challenge }
    }

    /// LK-23: the challenge is one this relay gave, unused and under 60
    /// seconds old, and the reader signed it for this request. The challenge
    /// is used up whatever the outcome.
    fn check_request(
        &mut self,
        reader: &DeviceId,
        challenge: &[u8; 32],
        sig: &Sig,
        action: Action,
        number: u64,
        now: u64,
    ) -> Result<(), Refusal> {
        let at = self
            .challenges
            .iter()
            .position(|(c, _)| c == challenge)
            .and_then(|i| self.challenges.remove(i))
            .map(|(_, at)| at)
            .ok_or(Refusal::ChallengeUnknown)?;
        if now.saturating_sub(at) > CHALLENGE_SECS {
            return Err(Refusal::ChallengeExpired);
        }
        let msg = challenge_message(challenge, &self.identity.device_id(), action, number);
        if reader.verify_tagged(TAG_FETCH, &msg, sig) {
            Ok(())
        } else {
            Err(Refusal::ChallengeSignature)
        }
    }

    fn make(&mut self, reader: DeviceId) -> Response {
        if self.mailboxes.contains_key(&reader) {
            return Response::Made;
        }
        if self.mailboxes.len() >= self.cfg.max_mailboxes {
            return Response::Refused(Refusal::MailboxesFull);
        }
        if self.store.create(&reader).is_err() {
            return Response::Refused(Refusal::Storage);
        }
        self.mailboxes.insert(reader, Mailbox::default());
        Response::Made
    }

    fn put(&mut self, sealed: Vec<u8>) -> Response {
        if sealed.len() > MAX_SEALED_LEN {
            return Response::Refused(Refusal::TooLarge);
        }
        let seen = match inspect(&sealed) {
            Ok(seen) => seen,
            Err(OpenError::BadSignature | OpenError::CiphertextHash) => {
                return Response::Refused(Refusal::BadSignature)
            }
            Err(_) => return Response::Refused(Refusal::NotWellFormed),
        };
        let Some(mailbox) = self.mailboxes.get_mut(&seen.reader) else {
            return Response::Refused(Refusal::NoMailbox);
        };
        let id = mailbox.next_id();
        if self.store.put(&seen.reader, id, &sealed).is_err() {
            return Response::Refused(Refusal::Storage);
        }
        mailbox.push(id, sealed, &self.cfg);
        // Best effort: a failed compaction loses nothing.
        let _ = self.store.compact(&seen.reader, mailbox);
        Response::Stored { id }
    }

    fn fetch(&mut self, reader: &DeviceId, cursor: u64) -> Response {
        let Some(mailbox) = self.mailboxes.get_mut(reader) else {
            return Response::Refused(Refusal::NoMailbox);
        };
        let mut items: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut bytes = 0usize;
        let mut more = false;
        for (id, sealed) in mailbox.messages().filter(|(id, _)| *id > cursor) {
            let next_bytes = bytes.saturating_add(sealed.len());
            if items.len() >= FETCH_BATCH || (!items.is_empty() && next_bytes > FETCH_BYTES) {
                more = true;
                break;
            }
            bytes = next_bytes;
            items.push((id, sealed.to_vec()));
        }
        if let Some((last, _)) = items.last() {
            mailbox.fetched = mailbox.fetched.max(*last);
        }
        Response::Messages { items, more }
    }

    fn ack(&mut self, reader: &DeviceId, up_to: u64) -> Response {
        let Some(mailbox) = self.mailboxes.get_mut(reader) else {
            return Response::Refused(Refusal::NoMailbox);
        };
        let through = up_to.min(mailbox.fetched);
        let due = mailbox.messages().any(|(id, _)| id <= through);
        if !due {
            return Response::Acked { dropped: 0 };
        }
        if self.store.ack(reader, through).is_err() {
            return Response::Refused(Refusal::Storage);
        }
        let dropped = mailbox.drop_through(through);
        let _ = self.store.compact(reader, mailbox);
        Response::Acked {
            dropped: u32::try_from(dropped).unwrap_or(u32::MAX),
        }
    }
}
