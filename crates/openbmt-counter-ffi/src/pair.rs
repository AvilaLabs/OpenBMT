//! The pairing ceremony on the counter, behind the registry
//! (`docs/COUNTER-RULES.md`, CR-91 to CR-96).
//!
//! One ceremony at a time lives in the [`Registry`], in memory only: a
//! restart of the process forgets it, and so does `pair_differ`,
//! `pair_cancel`, a second `pair_begin`, a finished or refused
//! `pair_finish`. The counter's key and the fresh number are drawn here from
//! the system's random source, never taken from the app, and the call is
//! refused when that source fails (CR-91). The rules and the checks are the
//! counter node's (`openbmt_counter_node::paired`); this file holds the
//! slot, the draw and the conversion of errors.

use std::sync::MutexGuard;

use openbmt_core::batch::BatchPolicy;
use openbmt_counter_node::{batch_policy, Ceremony, PairRefused};
use openbmt_net_io::{OsRandomness, Randomness};

use crate::call::Registry;
use crate::error::{from_rule, CounterError};
use crate::views::{PairBeginArgs, PairStartedView, WordsView};

/// A refusal of a pairing step as the error the app gets, with the words of
/// the refusal.
pub(crate) fn refused(e: PairRefused) -> CounterError {
    from_rule(e.rule(), &e, e.words())
}

/// A key's secret and a number of 32 bytes each, from the system's random
/// source. A source that fell back to anything that can be guessed is
/// refused.
fn draw() -> Result<([u8; 32], [u8; 32]), PairRefused> {
    let mut random = OsRandomness::new().map_err(|_| PairRefused::NoRandomness)?;
    let mut bytes = [0u8; 64];
    for chunk in bytes.chunks_mut(8) {
        chunk.copy_from_slice(&random.next_u64().to_le_bytes());
    }
    if !random.unpredictable() {
        return Err(PairRefused::NoRandomness);
    }
    let (secret, nonce) = bytes.split_at(32);
    let secret = <[u8; 32]>::try_from(secret).map_err(|_| PairRefused::NoRandomness)?;
    let nonce = <[u8; 32]>::try_from(nonce).map_err(|_| PairRefused::NoRandomness)?;
    Ok((secret, nonce))
}

impl Registry {
    fn ceremony(&self) -> MutexGuard<'_, Option<Ceremony>> {
        self.ceremony.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The batch rule of this counter (CR-96). In the library's own tests it
    /// can be lowered, so that small batches can be made.
    pub(crate) fn batch_policy(&self) -> BatchPolicy {
        #[cfg(test)]
        {
            if let Some(min_count) = *self.batch_min.lock().unwrap_or_else(|e| e.into_inner()) {
                return BatchPolicy { min_count };
            }
        }
        batch_policy()
    }

    /// CR-91: begin a ceremony from the Key app's offer. A ceremony already
    /// under way is forgotten first, with its key and number.
    pub(crate) fn pair_begin(&self, a: PairBeginArgs) -> Result<PairStartedView, CounterError> {
        *self.ceremony() = None;
        let (secret, nonce) = draw().map_err(refused)?;
        let (ceremony, started) =
            Ceremony::begin(&a.offer, &a.name, secret, nonce, a.now).map_err(refused)?;
        *self.ceremony() = Some(ceremony);
        Ok(PairStartedView {
            request: started.request,
            words: started.words.iter().map(|w| (*w).to_string()).collect(),
        })
    }

    /// CR-92: the person says the words match. A refusal ends the ceremony.
    pub(crate) fn pair_match(&self, now: u64) -> Result<(), CounterError> {
        let mut slot = self.ceremony();
        let Some(c) = slot.as_mut() else {
            return Err(refused(PairRefused::NoCeremony));
        };
        let result = c.confirm(now);
        if result.is_err() {
            *slot = None;
        }
        result.map_err(refused)
    }

    /// CR-92: the person says the words differ. The ceremony, its key and its
    /// number are forgotten, and the words say what happened and what to do.
    pub(crate) fn pair_differ(&self) -> Result<WordsView, CounterError> {
        *self.ceremony() = None;
        Ok(WordsView(openbmt_counter_node::say::PairStep::Differ.words()))
    }

    /// The person leaves the ceremony: it is forgotten.
    pub(crate) fn pair_cancel(&self) -> Result<(), CounterError> {
        *self.ceremony() = None;
        Ok(())
    }

    /// CR-92 to CR-96: take the owner phone's bundle and return the pairing
    /// record. A call before "They match" and a bundle that does not read
    /// leave the ceremony as it was; any other outcome ends it.
    pub(crate) fn pair_finish(&self, now: u64, bundle: &[u8]) -> Result<Vec<u8>, CounterError> {
        let policy = self.batch_policy();
        let mut slot = self.ceremony();
        let Some(c) = slot.as_mut() else {
            return Err(refused(PairRefused::NoCeremony));
        };
        let result = c.finish(bundle, &policy, now);
        if !c.is_open() {
            *slot = None;
        }
        result.map(|record| record.to_bytes()).map_err(refused)
    }
}
