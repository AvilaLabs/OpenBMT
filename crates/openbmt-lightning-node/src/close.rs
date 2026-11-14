//! Closing every channel to the shop's wallet (SL-36).

use crate::logger::Logger;
use crate::types::{Manager, Monitor};
use bitcoin::{Address, Script, Txid};
use lightning::chain::{Filter, WatchedOutput};
use lightning::ln::channel_state::ChannelDetails;
use lightning::ln::script::ShutdownScript;
use std::cell::RefCell;
use std::sync::Arc;

/// The refusal while a splice is not locked: the caller tries again later.
pub const SPLICE_NOT_LOCKED: &str = "a splice of a channel is not locked yet";

/// Closes each channel cooperatively, its balance paid to `to`; the number of
/// closes begun.
///
/// Nothing is begun while any channel has a splice that is not locked yet:
/// the closing transaction is signed on the funding the splice spends, so it
/// cannot confirm once the splice does (SL-36).
pub fn close_all(
    manager: &Manager,
    monitor: &Monitor,
    logger: &Arc<Logger>,
    to: &Address,
) -> Result<usize, String> {
    let script = ShutdownScript::try_from(to.script_pubkey()).map_err(|_| "script".to_string())?;
    let channels = manager.list_channels();
    if channels
        .iter()
        .any(|c| splice_not_locked(monitor, logger, c))
    {
        return Err(SPLICE_NOT_LOCKED.to_string());
    }
    let mut n = 0;
    for c in channels {
        manager
            .close_channel_with_feerate_and_script(
                &c.channel_id,
                &c.counterparty.node_id,
                None,
                Some(script.clone()),
            )
            .map_err(|e| format!("{e:?}"))?;
        n += 1;
    }
    Ok(n)
}

/// Whether the channel has a splice that is negotiated and not locked: its
/// monitor then watches a funding beside the one the channel has now. The
/// monitor is kept on disk, so this holds across a restart too.
pub fn splice_not_locked(
    monitor: &Monitor,
    logger: &Arc<Logger>,
    channel: &ChannelDetails,
) -> bool {
    let Some(current) = channel.funding_txo else {
        return false;
    };
    let Ok(m) = monitor.get_monitor(channel.channel_id) else {
        return false;
    };
    let seen = Fundings::default();
    m.load_outputs_to_watch(&&seen, logger);
    let found = seen.0.borrow().iter().any(|txid| *txid != current.txid);
    found
}

/// Records the funding transactions a monitor registers.
#[derive(Default)]
struct Fundings(RefCell<Vec<Txid>>);

impl Filter for Fundings {
    fn register_tx(&self, txid: &Txid, _script_pubkey: &Script) {
        self.0.borrow_mut().push(*txid);
    }

    fn register_output(&self, _output: WatchedOutput) {}
}
