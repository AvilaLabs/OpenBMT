//! Closing every channel to the shop's wallet (SL-36).

use crate::logger::Logger;
use crate::types::{Manager, Monitor};
use bitcoin::{Address, Script, Txid};
use lightning::chain::{Filter, WatchedOutput};
use lightning::ln::channel_state::{ChannelDetails, ChannelShutdownState};
use lightning::ln::script::ShutdownScript;
use lightning::ln::types::ChannelId;
use std::cell::RefCell;
use std::sync::Arc;

/// The refusal while a splice is not locked: the caller tries again later.
pub const SPLICE_NOT_LOCKED: &str = "a splice of a channel is not locked yet";

/// What a cooperative close of every channel did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Closing {
    /// Closes begun by this call.
    pub begun: usize,
    /// Channels that were already shutting down, left as they are.
    pub already: usize,
    /// The channels whose close was refused, and why. The others were still
    /// tried.
    pub failed: Vec<(ChannelId, String)>,
}

/// What a forced close of some channels did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Forced {
    /// Channels closed by force by this call.
    pub begun: usize,
    /// The channels the library refused to close, and why.
    pub failed: Vec<(ChannelId, String)>,
}

/// What the library is told when a channel is closed by force.
const FORCE_MESSAGE: &str = "closed by the shop";

/// Closes each channel cooperatively, its balance paid to `to`.
///
/// A channel that is already shutting down is left alone and counted; a
/// channel whose close is refused is recorded and the next is still tried, so
/// one stuck channel does not keep the others open (SL-36).
///
/// Nothing is begun while any channel has a splice that is not locked yet:
/// the closing transaction is signed on the funding the splice spends, so it
/// cannot confirm once the splice does (SL-36).
pub fn close_all(
    manager: &Manager,
    monitor: &Monitor,
    logger: &Arc<Logger>,
    to: &Address,
) -> Result<Closing, String> {
    let script = ShutdownScript::try_from(to.script_pubkey()).map_err(|_| "script".to_string())?;
    let channels = manager.list_channels();
    if channels
        .iter()
        .any(|c| splice_not_locked(monitor, logger, c))
    {
        return Err(SPLICE_NOT_LOCKED.to_string());
    }
    let mut out = Closing::default();
    for c in channels {
        if is_closing(&c) {
            out.already += 1;
            continue;
        }
        match manager.close_channel_with_feerate_and_script(
            &c.channel_id,
            &c.counterparty.node_id,
            None,
            Some(script.clone()),
        ) {
            Ok(()) => out.begun += 1,
            Err(e) => out.failed.push((c.channel_id, format!("{e:?}"))),
        }
    }
    Ok(out)
}

/// Closes by force, broadcasting the latest commitment, each channel of `ids`
/// that is still listed; one the library refuses is recorded and the rest are
/// still tried. Refused, like [`close_all`], while a splice is not locked.
pub fn force_close(
    manager: &Manager,
    monitor: &Monitor,
    logger: &Arc<Logger>,
    ids: &[ChannelId],
) -> Result<Forced, String> {
    let channels = manager.list_channels();
    if channels
        .iter()
        .any(|c| splice_not_locked(monitor, logger, c))
    {
        return Err(SPLICE_NOT_LOCKED.to_string());
    }
    let mut out = Forced::default();
    for c in channels.iter().filter(|c| ids.contains(&c.channel_id)) {
        match manager.force_close_broadcasting_latest_txn(
            &c.channel_id,
            &c.counterparty.node_id,
            FORCE_MESSAGE.to_string(),
        ) {
            Ok(()) => out.begun += 1,
            Err(e) => out.failed.push((c.channel_id, format!("{e:?}"))),
        }
    }
    Ok(out)
}

/// Whether the channel is already shutting down, by either side.
fn is_closing(c: &ChannelDetails) -> bool {
    c.channel_shutdown_state
        .is_some_and(|s| s != ChannelShutdownState::NotShuttingDown)
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
