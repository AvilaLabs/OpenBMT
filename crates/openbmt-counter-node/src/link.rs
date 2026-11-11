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
    decode_received, head_at, records_content, Courier, Post, Received, RelayPost, SendError,
};
use openbmt_link::seal::MAX_CONTENT_LEN;
use openbmt_link::{Kind, Role};

use crate::node::{DriverPort, Node};
use crate::notices::{NoticeOutbox, NoticeStoreError};

/// Most lines the link's own notes keep; the oldest goes first.
pub const NOTE_LOG: usize = 64;

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
    next_relay_at: u64,
    too_long_noted: bool,
    marks_ignored: u64,
    record_messages_sent: u64,
    heartbeats_taken: u64,
    node_refusals: u64,
    notes: VecDeque<String>,
    notices: Option<NoticeOutbox>,
    notice_noted: Option<String>,
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
            next_relay_at: 0,
            too_long_noted: false,
            marks_ignored: 0,
            record_messages_sent: 0,
            heartbeats_taken: 0,
            node_refusals: 0,
            notes: VecDeque::new(),
            notices: None,
            notice_noted: None,
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
        let records = node.counter().records().records();
        self.notices = Some(NoticeOutbox::open(identity, dir, records)?);
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
        }
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
    pub fn step<P: DriverPort>(&mut self, node: &mut Node<P>, now: u64) -> Vec<Report> {
        let mut reports = Vec::new();
        if now < self.next_relay_at {
            return reports;
        }
        self.next_relay_at = now.saturating_add(self.cfg.poll_secs);
        self.courier.begin();
        self.queue_notices(node);
        let reached = match self.courier.poll() {
            Ok(received) => {
                for message in received {
                    self.route(node, message, &mut reports);
                }
                true
            }
            Err(_) => false,
        };
        let reached = reached
            && self.send_check_in(node, now)
            && self.send_records(node, now)
            && self.send_notices();
        self.courier.end();
        if !reached {
            self.next_relay_at = now.saturating_add(self.cfg.retry_secs);
        }
        reports
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
        let have = node.counter().records().len();
        match head_at(node.counter().records().records(), count) {
            Some(own) if own == head => {
                self.mark = (count, head);
                // CR-76: the counter settles only what the owner holds.
                node.set_owner_mark(count);
            }
            Some(_) => {
                self.marks_ignored = self.marks_ignored.saturating_add(1);
                self.note(format!(
                    "LK-55: the owner's mark of {count} records has a head that is not ours: \
                     ignored"
                ));
            }
            None => {
                self.marks_ignored = self.marks_ignored.saturating_add(1);
                self.note(format!(
                    "LK-55: the owner's mark of {count} records is more than the {have} we have: \
                     ignored"
                ));
            }
        }
    }

    /// LK-57. `false` if the relay could not be reached.
    fn send_check_in<P: DriverPort>(&mut self, node: &mut Node<P>, now: u64) -> bool {
        let len = node.counter().records().len();
        let due = match self.last_check_in_at {
            None => true,
            Some(at) => {
                now >= at.saturating_add(self.cfg.check_in_min_secs)
                    && (len != self.len_after_check_in
                        || now >= at.saturating_add(self.cfg.check_in_max_secs))
            }
        };
        if !due {
            return true;
        }
        // Whatever comes of it, not again before the minimum has passed.
        self.last_check_in_at = Some(now);
        let record = match node.check_in() {
            Ok(record) => record,
            Err(e) => {
                self.note(format!("the check-in was not written: {e}"));
                return true;
            }
        };
        self.len_after_check_in = node.counter().records().len();
        match self
            .courier
            .send(&self.owner, Kind::CheckIn, &record.to_bytes())
        {
            Ok(()) => true,
            Err(e) => self.send_failed(&e, "the check-in"),
        }
    }

    /// LK-56. `false` if the relay could not be reached.
    fn send_records<P: DriverPort>(&mut self, node: &Node<P>, now: u64) -> bool {
        let all = node.counter().records().records();
        let len = all.len() as u64;
        if len <= self.mark.0 {
            return true;
        }
        let base = self.sent_upto.max(self.mark.0);
        let unsent = len > base;
        let overdue = self
            .last_send_at
            .is_some_and(|at| now >= at.saturating_add(self.cfg.resend_secs));
        if !unsent && !overdue {
            return true;
        }
        let start = if overdue { self.mark.0 } else { base };
        let Some(mut rest) = usize::try_from(start).ok().and_then(|n| all.get(n..)) else {
            return true;
        };
        let mut end = start;
        let mut sent_any = false;
        let mut reached = true;
        for _ in 0..self.cfg.max_messages_per_step {
            if rest.is_empty() {
                break;
            }
            let (content, count) = records_content(rest, self.cfg.max_content);
            if count == 0 {
                if !self.too_long_noted {
                    self.too_long_noted = true;
                    self.note(format!(
                        "the record after {end} is too long for one message: not sent"
                    ));
                }
                break;
            }
            match self.courier.send(&self.owner, Kind::Records, &content) {
                Ok(()) => {
                    sent_any = true;
                    self.record_messages_sent = self.record_messages_sent.saturating_add(1);
                    end = end.saturating_add(count as u64);
                    rest = rest.get(count..).unwrap_or(&[]);
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
        let line = match out.collect(node.counter().records().records()) {
            Ok(0) => return,
            Ok(dropped) => {
                format!("BX-40: {dropped} notices were dropped from a queue that was too long")
            }
            Err(e) => format!("the notices were not queued: {e}"),
        };
        self.note_once(line);
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
            false
        } else {
            self.note(format!("{what} could not be sent: {e}"));
            true
        }
    }
}
