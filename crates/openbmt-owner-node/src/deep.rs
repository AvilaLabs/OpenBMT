//! The deep check of the receive scripts that were set aside from the live
//! watch (`docs/OWNER-COINS-RULES.md`, OC-24 to OC-27).
//!
//! ## How it runs
//!
//! The check runs on a second driver of its own, started from the same
//! settings and spawner as the main driver, in `<dir>/deepcheck` with its own
//! directory lock and its own headers, as the sending driver does (NR-91). It
//! shares nothing with the driver that watches the wallet: the live watch,
//! the coin view's position and its state are not touched while the check
//! runs. A block the deep driver hands over is applied to the view with the
//! scripts of the current pass only, so the view's checked position does not
//! move. That is the whole difference from a rescan of the live watch, which
//! uses the same machinery: the driver's rescan (NR-90) and the view's
//! `apply_block`.
//!
//! The scripts are checked in passes of at most the live watch's bound. The
//! first pass is the second driver's starting set and its `watch_from` is the
//! height above which the scripts were not checked; later passes replace the
//! set with `unwatch_scripts` (NR-97) and `watch_script`, and ask for a
//! rescan from the same height. A pass is done when the driver has checked
//! every block up to the target, the node's tip when the check began.
//!
//! A payment found is applied by `apply_block` like any other: it is a
//! receipt (OC-12) and a coin. At the end of a pass the scripts that were
//! paid go back in the live watch, and the main driver rescans from the
//! target, so that a payment between the target and now is seen.
//!
//! ## What is kept
//!
//! The scripts set aside and the schedule are in the coin view (`OBMe`), so
//! a restart in the middle loses no script. The progress of a run is not
//! stored: a restart starts the check again from the stored height, which is
//! safe because a payment already found is a stored receipt and a block read
//! twice is applied once.

use std::path::PathBuf;
use std::time::Duration;

use bitcoin::ScriptBuf;
use openbmt_core::identity::Identity;
use openbmt_counter_node::MAX_EVENTS_PER_PUMP;
use openbmt_net_io::{AppEvent, DriverConfig};
use openbmt_watch::{HeaderChain, Refusal, WellFormedBlock};

use crate::coins::{ApplyError, ScriptSet, ViewState};
use crate::live::{DeepLine, DEEP_RETRY_SECS, DEEP_STALL_SECS};
use crate::node::{NodeError, OwnerNode, OwnerPort};
use crate::watcher::{write_file, OwnerConfig, OwnerWatcher};
use openbmt_core::wallet::Keychain;

/// The directory of the deep check's driver.
pub const DEEP_DIR: &str = "deepcheck";

/// How the node starts a driver for the deep check: the settings, the
/// directory and the scripts to start with.
pub type DeepSpawn<P> = Box<dyn FnMut(DriverConfig, PathBuf, Vec<ScriptBuf>) -> Result<P, String>>;

/// A deep check in progress.
pub struct DeepRun<P: OwnerPort> {
    watcher: OwnerWatcher<P>,
    /// Every index set aside when the check began, ascending.
    all: Vec<u32>,
    /// Where the current pass starts in `all`.
    pos: usize,
    /// The scripts of the current pass.
    pass: ScriptSet,
    /// How many indices the current pass holds.
    pass_len: usize,
    /// The height above which the check reads.
    from: u32,
    /// The height the check reads up to: the node's tip when it began.
    target: u32,
    /// Waiting for the driver to say that the rescan of a later pass began.
    awaiting_start: bool,
    /// The indices paid in the current pass.
    found: Vec<u32>,
    /// The owner's clock when the driver last made progress.
    progress_at: u64,
}

impl<P: OwnerPort> OwnerNode<P> {
    /// OC-24: ask for a deep check now. It starts at the next call of
    /// `pump` that finds the view ready and caught up. Returns false when no
    /// script is set aside. Asking while a check runs changes nothing.
    pub fn request_deep_check(&mut self) -> Result<bool, NodeError> {
        let dir = self.dir.clone();
        self.view
            .deep_request(|b| write_file(&dir, crate::node::COINS_FILE, b))
            .map_err(NodeError::Files)
    }

    /// Whether a deep check is running.
    pub fn deep_running(&self) -> bool {
        self.deep_run.is_some()
    }

    /// OC-25: what the owner is told about the scripts set aside, or nothing
    /// when none is.
    pub fn old_addresses_line(&self) -> Option<DeepLine> {
        let count = self.view.released().len();
        if count == 0 {
            return None;
        }
        let deep = self.view.deep();
        let now = self.watcher.now();
        let next = deep.due.max(now);
        Some(if self.deep_run.is_some() {
            DeepLine::Checking { count }
        } else if deep.failed {
            DeepLine::Failed { count, next }
        } else if deep.last == 0 {
            DeepLine::FirstCheckWaiting { count, next }
        } else {
            DeepLine::Monthly {
                count,
                days_ago: now.saturating_sub(deep.last) / 86_400,
                next,
            }
        })
    }

    /// Start a check that is due, and take what a running one has to say.
    /// Called by `pump`.
    pub(crate) fn deep_tick(&mut self) {
        if self.deep_run.is_some() {
            self.deep_pump();
            return;
        }
        let deep = self.view.deep();
        if self.view.released().is_empty() || deep.due == 0 || deep.due > self.watcher.now() {
            return;
        }
        let tip = self.watcher.chain().tip_height();
        // The check needs a view that has caught up, so that the target is a
        // height the live watch has read.
        if self.halted.is_some()
            || self.awaiting_rescan > 0
            || self.view.state() != ViewState::Ready
            || self.view.checked() < tip
        {
            return;
        }
        self.start_deep(tip);
    }

    /// Give up a running check without calling it a failure: it is due again
    /// as before.
    pub(crate) fn cancel_deep(&mut self, why: &str) {
        if let Some(mut run) = self.deep_run.take() {
            run.watcher.shutdown();
            self.note(format!("the check of old addresses was stopped: {why}"));
        }
    }

    fn start_deep(&mut self, target: u32) {
        let now = self.watcher.now();
        let from = self.view.deep().from;
        let all: Vec<u32> = self.view.released().iter().collect();
        if from >= target {
            // Nothing above the height the scripts were last checked to.
            self.deep_done(now, target);
            return;
        }
        let pass = self.pass_scripts(&all, 0);
        let pass_len = pass.len();
        let initial: Vec<ScriptBuf> = pass.keys().cloned().collect();
        let dir = self.dir.join(DEEP_DIR);
        let identity = match Identity::from_secret_bytes(&self.owner_secret) {
            Ok(identity) => identity,
            Err(e) => return self.deep_fail(&format!("the owner key: {e}")),
        };
        let mut config = OwnerConfig::new(&dir, identity, self.driver_config.clone());
        config.watch_from = Some(from);
        let spawn = &mut self.deep_spawn;
        let watcher =
            match OwnerWatcher::start_with(config, |driver, path| spawn(driver, path, initial)) {
                Ok(watcher) => watcher,
                Err(e) => return self.deep_fail(&e.to_string()),
            };
        watcher.port().set_fetch_transactions(false);
        self.note(format!(
            "checking {} old addresses for payments above block {from}, up to block {target}",
            all.len()
        ));
        self.deep_run = Some(DeepRun {
            watcher,
            all,
            pos: 0,
            pass,
            pass_len,
            from,
            target,
            awaiting_start: false,
            found: Vec::new(),
            progress_at: now,
        });
    }

    /// The scripts of the pass that starts at `pos`.
    fn pass_scripts(&self, all: &[u32], pos: usize) -> ScriptSet {
        let mut set = ScriptSet::new();
        for index in all.iter().skip(pos).take(self.limits.live) {
            if let Ok(script) = self.wallet.receive_script(*index) {
                set.insert(script, (Keychain::Receive, *index));
            }
        }
        set
    }

    /// Take the second driver's events.
    fn deep_pump(&mut self) {
        let Some(mut run) = self.deep_run.take() else {
            return;
        };
        let now = self.watcher.now();
        let mut finished_pass = false;
        for _ in 0..MAX_EVENTS_PER_PUMP {
            let Some(event) = run.watcher.next_event(Duration::ZERO) else {
                break;
            };
            match &event {
                AppEvent::RescanStarted(_) => {
                    run.awaiting_start = false;
                    run.progress_at = now;
                }
                AppEvent::Block { hash, bytes } if !run.awaiting_start => {
                    run.progress_at = now;
                    if let Err(why) = self.deep_block(&mut run, hash, bytes) {
                        run.watcher.shutdown();
                        return self.deep_fail(&why);
                    }
                }
                AppEvent::Checked(height) if !run.awaiting_start => {
                    run.progress_at = now;
                    if *height >= run.target {
                        finished_pass = true;
                    }
                }
                AppEvent::Chain { .. } => run.progress_at = now,
                _ => {}
            }
            run.watcher.apply(&event);
            if finished_pass {
                break;
            }
        }
        for line in run.watcher.take_notes() {
            self.note(format!("old-address check: {line}"));
        }
        if run.watcher.stopped() {
            run.watcher.shutdown();
            return self.deep_fail("its driver stopped");
        }
        if finished_pass {
            return self.deep_pass_done(run);
        }
        if now.saturating_sub(run.progress_at) > DEEP_STALL_SECS {
            run.watcher.shutdown();
            return self.deep_fail("its driver made no progress for an hour");
        }
        self.deep_run = Some(run);
    }

    /// Apply one block of the check. An error is the reason the check
    /// failed.
    fn deep_block(
        &mut self,
        run: &mut DeepRun<P>,
        hash: &bitcoin::BlockHash,
        bytes: &[u8],
    ) -> Result<(), String> {
        let block = match WellFormedBlock::parse(bytes) {
            Ok(b) if b.hash() == *hash => b,
            Ok(_) => return Err(format!("a block did not have the hash {hash} it came as")),
            Err(e) => return Err(format!("block {hash} was refused: {e}")),
        };
        let hits: Vec<u32> = block
            .output_scripts()
            .filter_map(|s| run.pass.get(s).map(|(_, index)| *index))
            .collect();
        let dir = self.dir.clone();
        let applied = self
            .view
            .apply_block(&block, self.watcher.chain(), &run.pass, |b| {
                write_file(&dir, crate::node::COINS_FILE, b)
            });
        match applied {
            Ok(effect) => {
                if !hits.is_empty() {
                    self.note(format!(
                        "a payment to an old address was found in block {hash}"
                    ));
                }
                for index in hits {
                    if !run.found.contains(&index) {
                        run.found.push(index);
                    }
                }
                for txid in effect.replaced {
                    // A held transaction cannot be replaced by an old block;
                    // if it is, the live path's handling is the right one.
                    self.note(format!("{txid} was replaced by a block of the check"));
                }
                Ok(())
            }
            // A block the node's own chain does not hold: above the target,
            // or after a change the node has not seen yet (which cancels the
            // check when it comes). Passed over.
            Err(ApplyError::Refused(Refusal::NotOnChain)) => {
                self.note(format!("block {hash} of the old-address check is not on the best chain; passed over"));
                Ok(())
            }
            Err(ApplyError::Store(e)) => {
                self.stop(format!("the coin view could not be stored: {e}"));
                Err(format!("the coin view could not be stored: {e}"))
            }
            Err(e) => Err(format!("block {hash} could not be applied: {e}")),
        }
    }

    /// The driver has read every block of the pass up to the target.
    fn deep_pass_done(&mut self, mut run: DeepRun<P>) {
        let now = self.watcher.now();
        let found = std::mem::take(&mut run.found);
        if !found.is_empty() {
            self.back_to_live(&found, run.target);
        }
        run.pos = run.pos.saturating_add(run.pass_len);
        if run.pos < run.all.len() {
            let next = self.pass_scripts(&run.all, run.pos);
            run.watcher
                .port()
                .unwatch_scripts(run.pass.keys().cloned().collect());
            for script in next.keys() {
                run.watcher.port().watch_script(script.clone());
            }
            run.pass_len = next.len();
            run.pass = next;
            run.awaiting_start = true;
            run.progress_at = now;
            run.watcher.port().rescan_from(run.from);
            self.deep_run = Some(run);
            return;
        }
        let (all, target) = (run.all.len(), run.target);
        run.watcher.shutdown();
        self.deep_done(now, target);
        self.note(format!("the check of {all} old addresses is done"));
    }

    /// Put scripts that were paid back in the live watch, and have the main
    /// driver read the blocks above `target` for them.
    fn back_to_live(&mut self, indices: &[u32], target: u32) {
        let dir = self.dir.clone();
        let through = self
            .view
            .checked()
            .max(self.watcher.chain().tip_height());
        match self
            .view
            .return_to_live(indices, target, through, |b| {
                write_file(&dir, crate::node::COINS_FILE, b)
            }) {
            Ok(true) => {
                for index in indices {
                    if let Ok(script) = self.wallet.receive_script(*index) {
                        if !self.scripts.contains_key(&script) {
                            self.watcher.port().watch_script(script.clone());
                            self.scripts.insert(script, (Keychain::Receive, *index));
                        }
                    }
                }
                if self.view.state() != ViewState::DeepReorg {
                    let from = self.view.checked();
                    self.awaiting_rescan = self.awaiting_rescan.saturating_add(1);
                    self.watcher.port().rescan_from(from);
                }
                self.note(format!(
                    "{} old addresses are watched all the time again",
                    indices.len()
                ));
            }
            Ok(false) => {}
            Err(e) => self.stop(format!("the coin view could not be stored: {e}")),
        }
    }

    /// The check read everything it set out to read.
    fn deep_done(&mut self, now: u64, target: u32) {
        let next = now.saturating_add(self.limits.deep_secs).max(2);
        let dir = self.dir.clone();
        if let Err(e) = self.view.deep_finished(now, next, target, |b| {
            write_file(&dir, crate::node::COINS_FILE, b)
        }) {
            self.stop(format!("the coin view could not be stored: {e}"));
        }
    }

    /// The check did not finish: it is tried again after a pause.
    fn deep_fail(&mut self, why: &str) {
        let now = self.watcher.now();
        self.note(format!("the check of old addresses failed: {why}"));
        let dir = self.dir.clone();
        let retry = now.saturating_add(DEEP_RETRY_SECS).max(2);
        if let Err(e) = self.view.deep_failed(retry, |b| {
            write_file(&dir, crate::node::COINS_FILE, b)
        }) {
            self.stop(format!("the coin view could not be stored: {e}"));
        }
    }
}
