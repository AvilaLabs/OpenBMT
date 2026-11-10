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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use openbmt_core::identity::{DeviceId, Identity, Sig};

use crate::proto::{Refusal, Request, Response, FETCH_BATCH, FETCH_BYTES};
use crate::seal::{inspect, OpenError, MAX_SEALED_LEN};
use crate::store::{Event, Loaded, MailStore};

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

/// The bytes a reader signs for LK-23: the challenge, the relay's key, the
/// action and the action's number (cursor or `up_to`).
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
    /// Members, and so mailboxes (LK-21).
    pub max_mailboxes: usize,
    /// Messages in a mailbox (LK-22).
    pub max_messages: usize,
    /// Sealed bytes in a mailbox (LK-22).
    pub max_bytes: usize,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            max_mailboxes: 64,
            max_messages: 256,
            max_bytes: 4 * 1024 * 1024,
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

/// What a relay owner installs to take a welcome (LK-27): it is given the
/// welcome, the settings and the time, and answers with the member list that
/// now applies, or `None` for any refusal.
pub type WelcomeHook = Box<dyn FnMut(&[u8], &[u8], u64) -> Option<Vec<DeviceId>> + Send>;

/// What a relay is asked and what it keeps.
pub struct Relay<S: MailStore> {
    identity: Identity,
    cfg: RelayConfig,
    store: S,
    members: BTreeSet<DeviceId>,
    mailboxes: BTreeMap<DeviceId, Mailbox>,
    /// The one challenge each connection may hold, with the time given
    /// (LK-23).
    challenges: BTreeMap<u64, ([u8; 32], u64)>,
    /// The highest number taken per sender and reader (LK-20). Not stored:
    /// after a restart it starts from the messages still held.
    taken: BTreeMap<(DeviceId, DeviceId), u64>,
    /// The claim of an open invitation, when this relay is a Box (LK-27).
    claim: Option<Vec<u8>>,
    /// Takes a welcome, when this relay is a Box (LK-27).
    welcome: Option<WelcomeHook>,
    /// With no members, give out no challenge either (BX-03).
    quiet_without_members: bool,
}

/// Why a member list was not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembersError<E> {
    /// More members than the relay allows.
    TooMany,
    /// The storage said no, so the list is unchanged.
    Store(E),
}

impl<E: fmt::Display> fmt::Display for MembersError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany => write!(f, "LK-21: more members than the relay allows"),
            Self::Store(e) => write!(f, "LK-41: the member list could not be stored: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for MembersError<E> {}

impl<S: MailStore> Relay<S> {
    /// A relay that loads what `store` holds.
    pub fn new(identity: Identity, cfg: RelayConfig, mut store: S) -> Result<Self, S::Error> {
        let Loaded {
            members,
            mailboxes: loaded,
        } = store.load()?;
        let members: BTreeSet<DeviceId> = members.into_iter().collect();
        let mut mailboxes = BTreeMap::new();
        let mut taken = BTreeMap::new();
        for (reader, events) in loaded {
            if members.contains(&reader) {
                let mailbox = Mailbox::replay(events, &cfg);
                for (_, sealed) in mailbox.messages() {
                    if let Ok(seen) = inspect(sealed) {
                        let last = taken.entry((seen.sender, reader)).or_insert(0);
                        *last = seen.number.max(*last);
                    }
                }
                mailboxes.insert(reader, mailbox);
            }
        }
        Ok(Self {
            identity,
            cfg,
            store,
            members,
            mailboxes,
            challenges: BTreeMap::new(),
            taken,
            claim: None,
            welcome: None,
            quiet_without_members: false,
        })
    }

    /// Offer this claim to whoever asks for one, or stop offering (LK-27).
    /// A plain relay never has one.
    pub fn set_claim(&mut self, claim: Option<Vec<u8>>) {
        self.claim = claim;
    }

    /// The claim on offer.
    pub fn claim(&self) -> Option<&[u8]> {
        self.claim.as_deref()
    }

    /// Install the hook that takes a welcome (LK-27). A relay with none
    /// refuses every welcome request.
    pub fn set_welcome_hook(&mut self, hook: Option<WelcomeHook>) {
        self.welcome = hook;
    }

    /// Whether a challenge is refused while the member list is empty
    /// (BX-03). Off by default: a plain relay gives a challenge to anyone.
    pub fn set_quiet_without_members(&mut self, quiet: bool) {
        self.quiet_without_members = quiet;
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

    /// The members (LK-21).
    pub fn members(&self) -> impl Iterator<Item = &DeviceId> {
        self.members.iter()
    }

    /// Replace the member list (LK-20, LK-21). It is stored first; only if
    /// that works does it take effect, and then the mailboxes of devices
    /// that are no longer members are gone. An empty list makes the relay
    /// take nothing.
    pub fn set_members(&mut self, members: &[DeviceId]) -> Result<(), MembersError<S::Error>> {
        let list: BTreeSet<DeviceId> = members.iter().copied().collect();
        if list.len() > self.cfg.max_mailboxes {
            return Err(MembersError::TooMany);
        }
        let ordered: Vec<DeviceId> = list.iter().copied().collect();
        self.store
            .set_members(&ordered)
            .map_err(MembersError::Store)?;
        self.mailboxes.retain(|reader, _| list.contains(reader));
        self.taken
            .retain(|(sender, reader), _| list.contains(sender) && list.contains(reader));
        self.members = list;
        Ok(())
    }

    /// Forget the numbers taken (LK-20), as a restart with empty mailboxes
    /// does. A reader must not depend on the relay for refusing copies
    /// (LK-06), and a test of a reader uses this to hand it some.
    pub fn forget_taken(&mut self) {
        self.taken.clear();
    }

    /// A connection has closed: its challenge is gone (LK-23).
    pub fn close(&mut self, conn: u64) {
        self.challenges.remove(&conn);
    }

    /// Answer one request.
    pub fn handle<E>(
        &mut self,
        conn: u64,
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
            Request::Challenge if self.quiet_without_members && self.members.is_empty() => {
                Response::Refused(Refusal::NotMember)
            }
            Request::Challenge => self.give_challenge(conn, now, random),
            Request::Claim => match &self.claim {
                Some(bytes) => Response::Claim {
                    bytes: bytes.clone(),
                },
                None => Response::Refused(Refusal::NoClaim),
            },
            Request::Welcome { welcome, settings } => self.take_welcome(&welcome, &settings, now),
            Request::MakeMailbox {
                reader,
                challenge,
                sig,
            } => match self.check_request(conn, &reader, &challenge, &sig, Action::Make, 0, now) {
                Err(why) => Response::Refused(why),
                Ok(()) => self.make(reader),
            },
            Request::Put { sealed } => self.put(sealed),
            Request::Fetch {
                reader,
                challenge,
                sig,
                cursor,
            } => match self.check_request(
                conn,
                &reader,
                &challenge,
                &sig,
                Action::Fetch,
                cursor,
                now,
            ) {
                Err(why) => Response::Refused(why),
                Ok(()) => self.fetch(&reader, cursor),
            },
            Request::Ack {
                reader,
                challenge,
                sig,
                up_to,
            } => match self.check_request(conn, &reader, &challenge, &sig, Action::Ack, up_to, now)
            {
                Err(why) => Response::Refused(why),
                Ok(()) => self.ack(&reader, up_to),
            },
        }
    }

    /// LK-27: hand the welcome to the hook. Whatever it says, the claim is
    /// no longer offered: the invitation is spent (BX-15). Only if it says
    /// yes does the member list change.
    fn take_welcome(&mut self, welcome: &[u8], settings: &[u8], now: u64) -> Response {
        let Some(hook) = self.welcome.as_mut() else {
            return Response::Refused(Refusal::WelcomeRefused);
        };
        let answer = hook(welcome, settings, now);
        self.claim = None;
        match answer {
            None => Response::Refused(Refusal::WelcomeRefused),
            Some(members) => match self.set_members(&members) {
                Ok(()) => Response::Welcomed,
                Err(_) => Response::Refused(Refusal::Storage),
            },
        }
    }

    fn give_challenge<E>(
        &mut self,
        conn: u64,
        now: u64,
        random: &mut impl FnMut(&mut [u8; 32]) -> Result<(), E>,
    ) -> Response {
        let mut challenge = [0u8; 32];
        if random(&mut challenge).is_err() {
            return Response::Refused(Refusal::NoRandomness);
        }
        // A new challenge replaces the connection's old one.
        self.challenges.insert(conn, (challenge, now));
        Response::Challenge { challenge }
    }

    /// LK-23: the challenge is one this relay gave, unused and under 60
    /// seconds old, and the reader signed it for this request. The challenge
    /// is used up whatever the outcome.
    #[allow(clippy::too_many_arguments)]
    fn check_request(
        &mut self,
        conn: u64,
        reader: &DeviceId,
        challenge: &[u8; 32],
        sig: &Sig,
        action: Action,
        number: u64,
        now: u64,
    ) -> Result<(), Refusal> {
        let (given, at) = self
            .challenges
            .remove(&conn)
            .ok_or(Refusal::ChallengeUnknown)?;
        if given != *challenge {
            return Err(Refusal::ChallengeUnknown);
        }
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
        if !self.members.contains(&reader) {
            return Response::Refused(Refusal::NotMember);
        }
        if self.mailboxes.contains_key(&reader) {
            return Response::Made;
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
        if !self.members.contains(&seen.sender) {
            return Response::Refused(Refusal::SenderNotMember);
        }
        if !self.members.contains(&seen.reader) {
            return Response::Refused(Refusal::ReaderNotMember);
        }
        let Some(mailbox) = self.mailboxes.get_mut(&seen.reader) else {
            return Response::Refused(Refusal::NoMailbox);
        };
        // A sealed message that was seen on the wire can be put again by
        // anyone. The reader refuses it (LK-06), but until then it would
        // push newer messages out of a full mailbox.
        let pair = (seen.sender, seen.reader);
        if self
            .taken
            .get(&pair)
            .is_some_and(|last| seen.number <= *last)
        {
            return Response::Refused(Refusal::AlreadyTaken);
        }
        let id = mailbox.next_id();
        if self.store.put(&seen.reader, id, &sealed).is_err() {
            return Response::Refused(Refusal::Storage);
        }
        self.taken.insert(pair, seen.number);
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
