//! The counter's end of the link (LK-55 to LK-57, LK-61, LK-63).
//!
//! [`CounterLink`] owns a [`Courier`] and is driven by the app loop next to
//! `Node::pump`. Each [`CounterLink::step`] polls the mailbox, hands what came
//! to the node, sends the check-in when one is due, and sends the records the
//! owner does not hold yet.
//!
//! ## What comes in
//!
//! - Heartbeat: `Node::heartbeat`. What the node reports when it judges the
//!   sales again is returned by `step`.
//! - Command: decoded as a `SignedCommand` and given to `Node::command`.
//! - Batch: decoded as a `SignedBatch` and given to `Node::add_batch`.
//! - Records received: the owner's mark (LK-55), which is also given to the
//!   node as the mark that settling goes by (CR-76).
//! - Settings and device lists: noted as not handled yet, and dropped.
//! - POS notices from a Box: opened and dropped with a note.
//!
//! A refusal by the node, or a content that does not decode, is noted and the
//! step goes on (LK-63).
//!
//! ## What goes out
//!
//! The records after the owner's mark (LK-56), a check-in (LK-57) and, when
//! the link has a Box, the paid notices (BX-40, see [`crate::notices`]). The
//! step does nothing before its time: the relay is polled at most every
//! `poll_secs`, and after a failure not again before `retry_secs`, so a loop
//! that calls `step` ten times a second neither connects ten times a second
//! nor waits for a dead relay each time.
//!
//! Where the rules are silent:
//!
//! - After a start the first step checks in at once, and a record that is
//!   too long for one message is noted and never sent.
//! - Nothing here is stored. After a restart the mark is "the owner holds
//!   nothing", which the first Records received message corrects; until then
//!   the counter sends from the first record, and the owner skips what it
//!   holds (LK-59).

use std::collections::VecDeque;

use openbmt_core::batch::SignedBatch;
use openbmt_core::command::SignedCommand;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::records::GENESIS;
use openbmt_counter::Report;
use openbmt_courier::{
    decode_received, records_content, Courier, PollError, Post, Received, RelayPost, SendError,
};
use openbmt_link::seal::MAX_CONTENT_LEN;
use openbmt_link::{ClientError, Kind, Role};

use crate::node::{DriverPort, Node};
use crate::notices::{NoticeOutbox, NoticeStoreError};

/// Most lines the link's own notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 64;
/// How many records are read at once for a message of records (CR-81). A
/// message takes as many of them as fit.
const SEND_CHUNK: usize = 512;

/// The intervals and limits of the link (LK-56, LK-57). The defaults are the
/// ones in the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterLinkConfig {
    /// Seconds after a send of records before they are sent again (LK-56).
    pub resend_secs: u64,
    /// Most messages of records in one step (LK-56).
    pub max_messages_per_step: usize,
    /// Most content bytes in one message of records. At most what one message
    /// holds.
    pub max_content: usize,
    /// Fewest seconds between two check-ins (LK-57).
    pub check_in_min_secs: u64,
    /// Seconds after which a check-in is made though nothing changed
    /// (LK-57).
    pub check_in_max_secs: u64,
    /// Fewest seconds between two polls of the relay.
    pub poll_secs: u64,
    /// Seconds to wait after a failure to reach the relay.
    pub retry_secs: u64,
}

impl Default for CounterLinkConfig {
    fn default() -> Self {
        Self {
            resend_secs: 30,
            max_messages_per_step: 4,
            max_content: MAX_CONTENT_LEN,
            check_in_min_secs: 900,
            check_in_max_secs: 900,
            poll_secs: 1,
            retry_secs: 5,
        }
    }
}

/// What the link can say about itself, for a screen or a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkState {
    /// The number of records the owner says it holds (LK-55).
    pub owner_records: u64,
    /// The head after them.
    pub owner_head: [u8; 32],
    /// When the last check-in was made, by the clock the steps were given.
    pub last_check_in_at: Option<u64>,
    /// Messages the courier refused (LK-52).
    pub refused: u64,
    /// Marks from the owner that did not fit the chain (LK-55).
    pub marks_ignored: u64,
    /// Messages of records sent.
    pub record_messages_sent: u64,
    /// Heartbeats the node took through the link.
    pub heartbeats_taken: u64,
    /// Heartbeats, commands and batches the node refused, or that did not
    /// decode (LK-63).
    pub node_refusals: u64,
    /// Whether the last call to the relay worked.
    pub relay_reachable: bool,
    /// Whether the last call that failed was refused by the relay, which
    /// answered: the counter is not a member of it. False when the relay
    /// could not be reached, and after a call that worked.
    pub relay_refused: bool,
}

/// What the first part of a step brought: see [`CounterLink::poll_step`].
pub struct Polled {
    received: Vec<Received>,
    reached: bool,
}

/// What the second part of a step decided to send: see
/// [`CounterLink::take_step`].
#[derive(Default)]
pub struct Outgoing {
    /// A connection was opened for this step and must be closed.
    active: bool,
    /// The relay was reached by the poll.
    reached: bool,
    check_in: Option<Vec<u8>>,
    /// Where the records to send start, and each message with the number of
    /// records in it.
    records_from: u64,
    records: Vec<(Vec<u8>, usize)>,
}

/// The counter's end of the link. See the module docs.
pub struct CounterLink<R: Post = RelayPost> {
    courier: Courier<R>,
    owner: DeviceId,
    cfg: CounterLinkConfig,
    mark: (u64, [u8; 32]),
    sent_upto: u64,
    last_send_at: Option<u64>,
    last_check_in_at: Option<u64>,
    len_after_check_in: u64,
    /// LK-57: the check-in record that did not reach the relay, sent again
    /// at each step until it does or a newer check-in replaces it.
    unsent_check_in: Option<Vec<u8>>,
    next_relay_at: u64,
    too_long_noted: bool,
    relay_refused: bool,
    marks_ignored: u64,
    record_messages_sent: u64,
    heartbeats_taken: u64,
    node_refusals: u64,
    notes: VecDeque<String>,
    notices: Option<NoticeOutbox>,
    notice_noted: Option<String>,
    /// The last line about a failed read of the records (CR-81), so that a
    /// fault that lasts is noted once, not at each step.
    read_noted: Option<String>,
}

impl<R: Post> CounterLink<R> {
    /// A link that talks to `owner` through `courier`.
    pub fn new(courier: Courier<R>, owner: DeviceId, cfg: CounterLinkConfig) -> Self {
        Self {
            courier,
            owner,
            cfg,
            mark: (0, GENESIS),
            sent_upto: 0,
            last_send_at: None,
            last_check_in_at: None,
            len_after_check_in: 0,
            unsent_check_in: None,
            next_relay_at: 0,
            too_long_noted: false,
            relay_refused: false,
            marks_ignored: 0,
            record_messages_sent: 0,
            heartbeats_taken: 0,
            node_refusals: 0,
            notes: VecDeque::new(),
            notices: None,
            notice_noted: None,
            read_noted: None,
        }
    }

    /// Send paid notices (BX-40) to each Box in the courier's table, signed
    /// with `identity`, which must be the counter's own. The numbers and the
    /// queue are kept in `dir`. With no Box in the table nothing is sent.
    pub fn enable_notices<P: DriverPort>(
        &mut self,
        identity: Identity,
        dir: impl Into<std::path::PathBuf>,
        node: &Node<P>,
    ) -> Result<(), NoticeStoreError> {
        if identity.device_id() != self.courier.device_id() {
            return Err(NoticeStoreError::NotThisCounter);
        }
        self.notices = Some(NoticeOutbox::open(identity, dir, &node.record_source())?);
        Ok(())
    }

    /// The notices, for reading.
    pub fn notices(&self) -> Option<&NoticeOutbox> {
        self.notices.as_ref()
    }

    /// The courier, for reading.
    pub fn courier(&self) -> &Courier<R> {
        &self.courier
    }

    /// The courier, for the tests that change what it knows.
    pub fn courier_mut(&mut self) -> &mut Courier<R> {
        &mut self.courier
    }

    /// What the link says about itself.
    pub fn state(&self) -> LinkState {
        LinkState {
            owner_records: self.mark.0,
            owner_head: self.mark.1,
            last_check_in_at: self.last_check_in_at,
            refused: self.courier.refused(),
            marks_ignored: self.marks_ignored,
            record_messages_sent: self.record_messages_sent,
            heartbeats_taken: self.heartbeats_taken,
            node_refusals: self.node_refusals,
            relay_reachable: self.courier.reachable(),
            relay_refused: self.relay_refused,
        }
    }

    /// Make the next step call the relay whatever the pause after the last
    /// call or failure was (the manager's Try again, CR-136).
    pub fn retry_now(&mut self) {
        self.next_relay_at = 0;
    }

    /// The lines the link and its courier have logged, oldest first.
    pub fn notes(&self) -> impl Iterator<Item = &str> {
        self.notes
            .iter()
            .map(String::as_str)
            .chain(self.courier.notes())
    }

    /// Take the logged lines.
    pub fn take_notes(&mut self) -> Vec<String> {
        let mut lines: Vec<String> = self.notes.drain(..).collect();
        lines.extend(self.courier.take_notes());
        lines
    }

    fn note(&mut self, line: String) {
        if self.notes.len() >= NOTE_LOG {
            self.notes.pop_front();
        }
        self.notes.push_back(line);
    }

    /// One turn of the link at `now` (seconds). Returns what the node
    /// reported when a heartbeat made it judge the sales again; the app acts
    /// on those as on the reports of `Node::pump`.
    ///
    /// The turn is [`CounterLink::poll_step`], [`CounterLink::take_step`] and
    /// [`CounterLink::send_step`] one after the other. A caller that shares
    /// the node with other threads calls the three itself, so that the node
    /// is held only for the second, which makes no call to the relay.
    pub fn step<P: DriverPort>(&mut self, node: &mut Node<P>, now: u64) -> Vec<Report> {
        let polled = self.poll_step(now);
        let (reports, out) = self.take_step(node, polled, now);
        self.send_step(out, now);
        reports
    }

    /// The first part of a step: call the relay and fetch what is waiting.
    /// `None` when it is not yet time (the relay is polled at most every
    /// `poll_secs`, and after a failure not before `retry_secs`). This part
    /// does not touch the node and can wait as long as the relay does.
    pub fn poll_step(&mut self, now: u64) -> Option<Polled> {
        if now < self.next_relay_at {
            return None;
        }
        self.next_relay_at = now.saturating_add(self.cfg.poll_secs);
        self.courier.begin();
        let polled = match self.courier.poll() {
            Ok(received) => {
                self.relay_refused = false;
                Polled {
                    received,
                    reached: true,
                }
            }
            Err(PollError::Relay(e)) => {
                self.relay_refused = matches!(e, ClientError::Refused(_));
                Polled {
                    received: Vec::new(),
                    reached: false,
                }
            }
        };
        if !polled.reached {
            self.next_relay_at = now.saturating_add(self.cfg.retry_secs);
        }
        Some(polled)
    }

    /// The second part: hand what came to the node, and decide what goes
    /// out (the check-in, the records after the owner's mark, LK-56, LK-57).
    /// Nothing here calls the relay. Returns what the node reported, and
    /// what [`CounterLink::send_step`] is to send.
    pub fn take_step<P: DriverPort>(
        &mut self,
        node: &mut Node<P>,
        polled: Option<Polled>,
        now: u64,
    ) -> (Vec<Report>, Outgoing) {
        let mut reports = Vec::new();
        let Some(polled) = polled else {
            return (reports, Outgoing::default());
        };
        self.queue_notices(node);
        for message in polled.received {
            self.route(node, message, &mut reports);
        }
        let mut out = Outgoing {
            active: true,
            reached: polled.reached,
            ..Outgoing::default()
        };
        if polled.reached {
            out.check_in = self.prepare_check_in(node, now);
            self.prepare_records(node, now, &mut out);
        }
        (reports, out)
    }

    /// The third part: send what [`CounterLink::take_step`] decided, then
    /// the paid notices, and let go of the connection. Does not touch the
    /// node.
    pub fn send_step(&mut self, out: Outgoing, now: u64) {
        if !out.active {
            return;
        }
        let reached = out.reached
            && self.send_check_in(out.check_in)
            && self.send_records(out.records_from, out.records, now)
            && self.send_notices();
        self.courier.end();
        if !reached {
            self.next_relay_at = now.saturating_add(self.cfg.retry_secs);
        }
    }

    fn route<P: DriverPort>(
        &mut self,
        node: &mut Node<P>,
        message: Received,
        reports: &mut Vec<Report>,
    ) {
        match message.kind {
            Kind::Heartbeat => match node.heartbeat(&message.content) {
                Ok(taken) => {
                    self.heartbeats_taken = self.heartbeats_taken.saturating_add(1);
                    reports.extend(taken.reports);
                }
                Err(e) => self.refused_by_node(format!("heartbeat not taken: {e}")),
            },
            Kind::Command => match SignedCommand::from_bytes(&message.content) {
                Ok(signed) => {
                    if let Err(e) = node.command(&signed) {
                        self.refused_by_node(format!("command not taken: {e}"));
                    }
                }
                Err(e) => self.refused_by_node(format!("command does not decode: {e}")),
            },
            Kind::Batch => match SignedBatch::from_bytes(&message.content) {
                Ok(signed) => {
                    if let Err(e) = node.add_batch(signed) {
                        self.refused_by_node(format!("batch not taken: {e}"));
                    }
                }
                Err(e) => self.refused_by_node(format!("batch does not decode: {e}")),
            },
            Kind::RecordsReceived => self.take_mark(node, &message.content),
            Kind::Settings => self.note("settings from the owner: not handled yet".to_string()),
            Kind::DeviceList => {
                self.note("a device list from the owner: not handled yet".to_string())
            }
            Kind::PosNotice => self.note("a POS notice from the Box: dropped".to_string()),
            other => self.note(format!("a {other:?} message: not handled yet")),
        }
    }

    fn refused_by_node(&mut self, line: String) {
        self.node_refusals = self.node_refusals.saturating_add(1);
        self.note(line);
    }

    /// LK-55.
    fn take_mark<P: DriverPort>(&mut self, node: &mut Node<P>, content: &[u8]) {
        let (count, head) = match decode_received(content) {
            Ok(mark) => mark,
            Err(e) => {
                self.marks_ignored = self.marks_ignored.saturating_add(1);
                self.note(format!("the owner's mark was ignored: {e}"));
                return;
            }
        };
        let have = node.records_len();
        // A hash below what memory holds is read from the record file
        // (CR-81).
        match node.hash_at(count) {
            Err(e) => {
                // Not the owner's fault and not a mark that was wrong: the
                // last one stands, and the next mark is looked at again.
                self.note_read(format!(
                    "LK-55: the owner's mark of {count} records could not be checked: {e}"
                ));
            }
            Ok(Some(own)) if own == head => {
                self.mark = (count, head);
                // CR-76: the counter settles only what the owner holds.
                node.set_owner_mark(count);
            }
            Ok(Some(_)) => {
                self.marks_ignored = self.marks_ignored.saturating_add(1);
                self.note(format!(
                    "LK-55: the owner's mark of {count} records has a head that is not ours: \
                     ignored"
                ));
            }
            Ok(None) => {
                self.marks_ignored = self.marks_ignored.saturating_add(1);
                self.note(format!(
                    "LK-55: the owner's mark of {count} records is more than the {have} we have: \
                     ignored"
                ));
            }
        }
    }

    /// LK-57: write the check-in when one is due, and give the record to
    /// send: the one just written, or the one an earlier step could not
    /// deliver.
    fn prepare_check_in<P: DriverPort>(&mut self, node: &mut Node<P>, now: u64) -> Option<Vec<u8>> {
        let len = node.records_len();
        let due = match self.last_check_in_at {
            None => true,
            Some(at) => {
                now >= at.saturating_add(self.cfg.check_in_min_secs)
                    && (len != self.len_after_check_in
                        || now >= at.saturating_add(self.cfg.check_in_max_secs))
            }
        };
        if due {
            // Whatever comes of it, no new check-in before the minimum has
            // passed.
            self.last_check_in_at = Some(now);
            match node.check_in() {
                Ok(record) => {
                    self.len_after_check_in = node.records_len();
                    // A newer check-in replaces one that was never delivered.
                    self.unsent_check_in = Some(record.to_bytes());
                }
                Err(e) => {
                    self.note(format!("the check-in was not written: {e}"));
                }
            }
        }
        // A check-in that did not reach the relay is sent again, the same
        // record, until it does: no new record is written for it.
        self.unsent_check_in.take()
    }

    /// LK-57. `false` if the relay could not be reached.
    fn send_check_in(&mut self, bytes: Option<Vec<u8>>) -> bool {
        let Some(bytes) = bytes else {
            return true;
        };
        match self.courier.send(&self.owner, Kind::CheckIn, &bytes) {
            Ok(()) => true,
            Err(e) => {
                if e.is_retryable() {
                    self.unsent_check_in = Some(bytes);
                }
                self.send_failed(&e, "the check-in")
            }
        }
    }

    /// LK-56: read the messages of records to send, when some are due.
    fn prepare_records<P: DriverPort>(&mut self, node: &Node<P>, now: u64, out: &mut Outgoing) {
        let len = node.records_len();
        if len <= self.mark.0 {
            return;
        }
        let base = self.sent_upto.max(self.mark.0);
        let unsent = len > base;
        let overdue = self
            .last_send_at
            .is_some_and(|at| now >= at.saturating_add(self.cfg.resend_secs));
        if !unsent && !overdue {
            return;
        }
        let start = if overdue { self.mark.0 } else { base };
        out.records_from = start;
        let mut end = start;
        for _ in 0..self.cfg.max_messages_per_step {
            // The records come from memory, and from the record file below
            // what memory holds (CR-81). A read that fails is a note and the
            // step goes on: nothing here changes what the counter holds.
            let rest = match node.records_from(end, SEND_CHUNK) {
                Ok(rest) => rest,
                Err(e) => {
                    self.note_read(format!("the records after {end} could not be read: {e}"));
                    break;
                }
            };
            if rest.is_empty() {
                break;
            }
            let (content, count) = records_content(&rest, self.cfg.max_content);
            if count == 0 {
                if !self.too_long_noted {
                    self.too_long_noted = true;
                    self.note(format!(
                        "the record after {end} is too long for one message: not sent"
                    ));
                }
                break;
            }
            out.records.push((content, count));
            end = end.saturating_add(count as u64);
        }
    }

    /// LK-56. `false` if the relay could not be reached.
    fn send_records(&mut self, start: u64, messages: Vec<(Vec<u8>, usize)>, now: u64) -> bool {
        let mut end = start;
        let mut sent_any = false;
        let mut reached = true;
        for (content, count) in messages {
            match self.courier.send(&self.owner, Kind::Records, &content) {
                Ok(()) => {
                    sent_any = true;
                    self.record_messages_sent = self.record_messages_sent.saturating_add(1);
                    end = end.saturating_add(count as u64);
                }
                Err(e) => {
                    reached = self.send_failed(&e, "records");
                    break;
                }
            }
        }
        if sent_any {
            self.sent_upto = end;
            self.last_send_at = Some(now);
        }
        reached
    }

    /// BX-40: the Boxes in the courier's table.
    fn boxes(&self) -> Vec<DeviceId> {
        self.courier
            .peers()
            .iter()
            .filter(|(_, role)| **role == Role::Box)
            .map(|(d, _)| *d)
            .collect()
    }

    /// BX-40: queue the notices the counter's records call for and store
    /// them, before anything is sent and whether or not the relay can be
    /// reached. Nothing here changes a sale or fails the step: whatever goes
    /// wrong is a note.
    fn queue_notices<P: DriverPort>(&mut self, node: &Node<P>) {
        if self.boxes().is_empty() {
            return;
        }
        let Some(out) = self.notices.as_mut() else {
            return;
        };
        let line = match out.collect(&node.record_source()) {
            Ok(0) => return,
            Ok(dropped) => {
                format!("BX-40: {dropped} notices were dropped from a queue that was too long")
            }
            Err(e) => format!("the notices were not queued: {e}"),
        };
        self.note_once(line);
    }

    /// A line about a failed read of the records is noted once, until
    /// another one is.
    fn note_read(&mut self, line: String) {
        if self.read_noted.as_deref() != Some(line.as_str()) {
            self.read_noted = Some(line.clone());
            self.note(line);
        }
    }

    /// A line about the notices is noted once, until another one is.
    fn note_once(&mut self, line: String) {
        if self.notice_noted.as_deref() != Some(line.as_str()) {
            self.notice_noted = Some(line.clone());
            self.note(line);
        }
    }

    /// BX-40: send each Box the notices the relay has not taken for it. A
    /// failure is a note and never an error to the counter. `false` if the
    /// relay could not be reached.
    fn send_notices(&mut self) -> bool {
        let boxes = self.boxes();
        let Some(out) = self.notices.as_mut() else {
            return true;
        };
        if boxes.is_empty() {
            return true;
        }
        let mut lines = Vec::new();
        let mut reached = true;
        'boxes: for to in &boxes {
            for notice in out.unsent(to) {
                match self.courier.send(to, Kind::PaidNotice, &notice.to_bytes()) {
                    Ok(()) => out.mark_sent(to, notice.number),
                    Err(e) if e.is_retryable() => {
                        reached = false;
                        break 'boxes;
                    }
                    Err(e) => {
                        lines.push(format!("a paid notice could not be sent: {e}"));
                        continue 'boxes;
                    }
                }
            }
        }
        if let Err(e) = out.settle(&boxes) {
            lines.push(format!("the notices were not stored: {e}"));
        }
        for line in lines {
            self.note_once(line);
        }
        reached
    }

    /// A send failed. A relay that cannot be reached is the courier's note
    /// already; anything else is noted here. `true` if the relay is not the
    /// problem.
    fn send_failed(&mut self, e: &SendError, what: &str) -> bool {
        if e.is_retryable() {
            self.relay_refused = matches!(e, SendError::Relay(ClientError::Refused(_)));
            false
        } else {
            self.note(format!("{what} could not be sent: {e}"));
            true
        }
    }
}
