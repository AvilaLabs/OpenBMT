//! A block that leaves the chain (RQ-37).
//!
//! The book keeps no chain of its own: the caller says the height of the
//! last block still in the chain, and every payment recorded from a block
//! above it is taken back.

use std::collections::HashMap;

use crate::book::RequestBook;
use crate::model::{Credit, Return, ReturnState};

/// What a reorg changed (RQ-37).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reorged {
    /// Every payment taken back, with the number of its request, in the
    /// order of the requests and then of their payments. Late and after-end
    /// receipts are among them.
    pub credits: Vec<(u64, Credit)>,
    /// The queued returns taken off the queue, newest first.
    pub removed_returns: Vec<Return>,
    /// The requests that now show more returned than received, with the
    /// excess in satoshis, by number.
    pub over_returned: Vec<(u64, u64)>,
}

impl RequestBook {
    /// RQ-37: the checked chain dropped every block above `fork_height`.
    /// Takes back each payment recorded from a higher block, in every
    /// request. Then, for each request whose returns (queued, sent and done)
    /// come to more than it now received, takes its queued returns off the
    /// queue, newest first, until they no longer do or none are queued. A
    /// return in a spend or done stays. Nothing is paid or cancelled, and a
    /// request that had ended stays ended. A payment in a block of the new
    /// chain is counted by [`credit`](Self::credit) as any other.
    pub fn take_back_above(&mut self, fork_height: u32) -> Reorged {
        let mut done = Reorged::default();
        let mut changed: Vec<u64> = Vec::new();
        for request in &mut self.requests {
            if request.credits.iter().all(|c| c.height <= fork_height) {
                continue;
            }
            let number = request.number;
            let (kept, gone): (Vec<Credit>, Vec<Credit>) = request
                .credits
                .iter()
                .partition(|c| c.height <= fork_height);
            request.credits = kept;
            done.credits.extend(gone.into_iter().map(|c| (number, c)));
            changed.push(number);
        }
        if changed.is_empty() {
            return done;
        }
        done.removed_returns = self.trim_queued(&changed);
        for number in changed {
            if let Some(over) = self.over_returned(number) {
                done.over_returned.push((number, over));
            }
        }
        done
    }

    /// RQ-37: for each of these requests whose returns come to more than it
    /// received, take its queued returns off, newest first, until they no
    /// longer do or none are queued. Gives the returns taken off, newest
    /// first.
    pub(crate) fn trim_queued(&mut self, numbers: &[u64]) -> Vec<Return> {
        let mut returned: HashMap<u64, u64> = HashMap::new();
        for r in self.returns.iter().filter(|r| r.state.counts()) {
            let total = returned.entry(r.request).or_insert(0);
            *total = total.saturating_add(r.sats);
        }
        // What each request still has to shed, in satoshis.
        let mut excess: HashMap<u64, u64> = HashMap::new();
        for number in numbers {
            let received = self.request(*number).map_or(0, |r| r.received_sats());
            let total = returned.get(number).copied().unwrap_or(0);
            if total > received {
                excess.insert(*number, total - received);
            }
        }
        let mut removed: Vec<Return> = Vec::new();
        for r in self.returns.iter().rev() {
            if r.state != ReturnState::Queued {
                continue;
            }
            if let Some(left) = excess.get_mut(&r.request) {
                if *left > 0 {
                    *left = left.saturating_sub(r.sats);
                    removed.push(r.clone());
                }
            }
        }
        self.returns
            .retain(|r| !removed.iter().any(|gone| gone.id == r.id));
        removed
    }

    /// RQ-37: how much more was returned from a request (queued, sent or
    /// done) than it received, in satoshis; none when it is not more, or
    /// there is no such request.
    pub fn over_returned(&self, number: u64) -> Option<u64> {
        let request = self.request(number)?;
        let returned = self
            .returns
            .iter()
            .filter(|r| r.request == number && r.state.counts())
            .fold(0u64, |t, r| t.saturating_add(r.sats));
        returned
            .checked_sub(request.received_sats())
            .filter(|over| *over > 0)
    }
}
