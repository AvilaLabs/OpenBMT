//! The starter line the owner node makes (`docs/LINK-RULES.md` LK-79,
//! `docs/START-SIMPLE-RULES.md` SS-32, SS-36, SS-38).
//!
//! A shop on the starter wallet tells each counter what the wallet holds,
//! the limit, the stop setting and the last 16 payments out of it, in a line
//! the owner root signs. The app gives the node the limit and the stop
//! setting, which are the owner phone's own record, with
//! [`OwnerNode::set_starter_line`] when the node starts and when they change;
//! until it does, the node makes no line.
//!
//! Where the rules are silent:
//!
//! - What a payment is: a transaction that spends coins of the wallet. Its
//!   amount is what it took from the wallet less what its outputs pay back
//!   to the wallet (the fee is part of it). A payment of the owner phone's
//!   own is seen when the node holds it, the others when a block holds it
//!   (SS-36). It is "made by the owner phone" when the node held it or the
//!   queue's outbox took it.
//! - The time of a payment is the owner's clock at the first step that found
//!   it. The last 16 are kept, with the line's number, in the file
//!   [`STARTER_LINE_FILE`] (the frame, magic `OBM` 0x1b, of the last line made, nothing
//!   else), so a start goes on from where the last one stopped. The number of
//!   a line is the greater of the last one plus one and the time now, so it
//!   rises even if the file is lost.
//! - The block's payments are taken when the block is applied, in memory, and
//!   put in the line at the next step. Only blocks above the height the node
//!   had when the setting was first given count, so a rescan of old blocks
//!   adds no payment, and a reorganization lowers that height to the fork.
//! - A line goes to each counter the link follows that has no `switched`
//!   record in the owner's copy of its chain (CR-262, CR-275). A line whose
//!   send did not reach the relay is made again at the next step.

use std::collections::BTreeSet;

use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Txid};
use openbmt_core::starter_line::{LinePayment, StarterLine, MAX_LINE_PAYMENTS};
use openbmt_courier::Post;
use openbmt_owner::Delivery;
use openbmt_watch::HeaderChain;

use crate::coins::BlockEffect;
use crate::link::OwnerLink;
use crate::node::{OwnerNode, OwnerPort};
use crate::watcher::write_file;

/// The file the last line made is kept in, in the node's folder.
pub const STARTER_LINE_FILE: &str = "starter-line";

/// Most spends of blocks that wait for the next step.
const MAX_QUEUED: usize = 64;

/// One spend of the wallet a block held, until the next step takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Queued {
    txid: Txid,
    sats: u64,
    by_owner: bool,
}

/// What the last line said, to tell whether the next one has anything new.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Said {
    number: u64,
    sent_at: u64,
    /// Every counter the line was for was reached.
    reached: bool,
    held: u64,
    limit: u64,
    stop: bool,
    payments: Vec<LinePayment>,
}

/// What the node keeps for the starter line.
#[derive(Debug, Default)]
pub(crate) struct StarterLiner {
    /// The limit and the stop setting the app gave; `None` until it does.
    setting: Option<(u64, bool)>,
    /// The last payments, oldest first, at most [`MAX_LINE_PAYMENTS`].
    payments: Vec<LinePayment>,
    said: Option<Said>,
    queued: Vec<Queued>,
    /// Blocks at or below this height add no payment.
    high_water: u32,
}

/// What a step of the starter line did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StarterStep {
    /// The number of the line made at this step, if one was.
    pub number: Option<u64>,
    /// Counters the line was sent to and the relay took it for.
    pub sent: usize,
    /// Counters the line could not be sent to now.
    pub failed: usize,
}

impl<P: OwnerPort> OwnerNode<P> {
    /// LK-79, SS-38: the limit and the stop setting, from the owner phone's
    /// own record. The app gives them when the node starts and when they
    /// change. The first call reads the file of the last line made and goes
    /// on from it.
    pub fn set_starter_line(&mut self, limit_sats: u64, stop: bool) {
        if self.starter.setting.is_none() {
            self.starter.high_water = self.view.checked();
            self.starter_restore();
        }
        self.starter.setting = Some((limit_sats, stop));
    }

    /// Read the last line made, if the file is there and is the owner's.
    fn starter_restore(&mut self) {
        let Ok(bytes) = std::fs::read(self.dir.join(STARTER_LINE_FILE)) else {
            return;
        };
        let owner = match self.owner_identity() {
            Ok(identity) => identity.device_id(),
            Err(_) => return,
        };
        match StarterLine::from_bytes(&bytes) {
            Ok(line) if line.verify(&owner) => {
                self.starter.payments = line.payments.clone();
                self.starter.said = Some(Said {
                    number: line.number,
                    // The first step after a start sends a line.
                    sent_at: 0,
                    reached: true,
                    held: line.held_sats,
                    limit: line.limit_sats,
                    stop: line.stop,
                    payments: line.payments,
                });
            }
            _ => self.note(format!(
                "LK-79: the file {STARTER_LINE_FILE} is not a line of the owner's: not used"
            )),
        }
    }

    /// A block was applied: what it spent of the wallet is kept for the next
    /// step, when the block is above the height the setting counts from.
    pub(crate) fn starter_note_block(&mut self, effect: &BlockEffect, block: &BlockHash) {
        if self.starter.setting.is_none() {
            return;
        }
        let Some(height) = self.watcher.chain().height_of(block) else {
            return;
        };
        if height <= self.starter.high_water {
            return;
        }
        self.starter.high_water = height;
        let in_outbox: BTreeSet<Txid> = self
            .owner
            .outbox()
            .iter()
            .filter_map(|d| match &d.what {
                Delivery::Transaction { txid, .. } => Some(*txid),
                _ => None,
            })
            .collect();
        for spent in &effect.spends {
            let by_owner =
                effect.confirmed.contains(&spent.txid) || in_outbox.contains(&spent.txid);
            if self.starter.queued.len() >= MAX_QUEUED {
                self.starter.queued.remove(0);
            }
            self.starter.queued.push(Queued {
                txid: spent.txid,
                sats: spent.out(),
                by_owner,
            });
        }
    }

    /// Blocks above `fork` left the chain: they count again if they come
    /// back, with other payments in them.
    pub(crate) fn starter_note_reorg(&mut self, fork: u32) {
        self.starter.high_water = self.starter.high_water.min(fork);
    }

    /// The owner phone's own payments the node holds and no block has
    /// confirmed: each spends coins of the wallet.
    fn starter_held_spends(&self) -> Vec<Queued> {
        let owned: Vec<_> = self.view.owned().collect();
        self.view
            .pending()
            .filter_map(|p| {
                let taken = owned
                    .iter()
                    .filter(|c| p.tx.input.iter().any(|i| i.previous_output == c.outpoint))
                    .fold(0u64, |sum, c| sum.saturating_add(c.sats()));
                if taken == 0 {
                    return None;
                }
                let back = p.own.iter().fold(0u64, |sum, (vout, _, _)| {
                    let at = usize::try_from(*vout).ok();
                    let value = at.and_then(|at| p.tx.output.get(at));
                    sum.saturating_add(value.map_or(0, |o| o.value.to_sat()))
                });
                Some(Queued {
                    txid: p.tx.compute_txid(),
                    sats: taken.saturating_sub(back),
                    by_owner: true,
                })
            })
            .collect()
    }

    /// LK-79: one turn. The payments the node saw since the last turn are
    /// added; when the line says anything new, or the last one was made a
    /// heartbeat interval ago, a new line is signed, kept and sent to each
    /// starter counter. Nothing is sent before the app has given the limit
    /// and the stop setting, or while the view cannot say what the wallet
    /// holds.
    pub fn starter_line_step<R: Post>(&mut self, link: &mut OwnerLink<R>, now: u64) -> StarterStep {
        let mut step = StarterStep::default();
        let Some((limit, stop)) = self.starter.setting else {
            return step;
        };
        let Some(held) = self.starter_held() else {
            return step;
        };
        // The payments: the node's own held ones, then the blocks'.
        let mut found = self.starter_held_spends();
        found.append(&mut self.starter.queued);
        for q in found {
            if self
                .starter
                .payments
                .iter()
                .any(|p| p.txid == q.txid.to_byte_array())
            {
                continue;
            }
            self.starter.payments.push(LinePayment {
                txid: q.txid.to_byte_array(),
                sats: q.sats,
                time: now,
                by_owner: q.by_owner,
            });
        }
        let extra = self
            .starter
            .payments
            .len()
            .saturating_sub(MAX_LINE_PAYMENTS);
        self.starter.payments.drain(..extra);

        let (changed, due) = match &self.starter.said {
            None => (true, true),
            Some(s) => (
                !s.reached
                    || s.held != held
                    || s.limit != limit
                    || s.stop != stop
                    || s.payments != self.starter.payments,
                now.saturating_sub(s.sent_at) >= link.heartbeat_every_secs(),
            ),
        };
        if !changed && !due {
            return step;
        }
        let Ok(identity) = self.owner_identity() else {
            return step;
        };
        let number = self
            .starter
            .said
            .as_ref()
            .map_or(now, |s| s.number.saturating_add(1).max(now));
        let line = StarterLine::sign(
            &identity,
            number,
            now,
            held,
            limit,
            stop,
            &self.starter.payments,
        );
        let bytes = line.to_bytes();
        if let Err(e) = write_file(&self.dir, STARTER_LINE_FILE, &bytes) {
            self.note(format!("LK-79: the starter line could not be kept: {e}"));
        }
        step.number = Some(number);
        for counter in link.starter_targets() {
            match link.send_starter(&counter, &bytes) {
                Ok(()) => step.sent += 1,
                Err(_) => step.failed += 1,
            }
        }
        // A line that did not reach every counter is made again at the next
        // step.
        self.starter.said = Some(Said {
            number,
            sent_at: now,
            reached: step.failed == 0,
            held,
            limit,
            stop,
            payments: self.starter.payments.clone(),
        });
        step
    }
}
