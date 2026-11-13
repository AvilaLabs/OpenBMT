//! Closing every channel to the shop's wallet (SL-36).

use crate::types::Manager;
use bitcoin::Address;
use lightning::ln::script::ShutdownScript;

/// Closes each channel cooperatively, its balance paid to `to`; the number of
/// closes begun.
pub fn close_all(manager: &Manager, to: &Address) -> Result<usize, String> {
    let script = ShutdownScript::try_from(to.script_pubkey()).map_err(|_| "script".to_string())?;
    let mut n = 0;
    for c in manager.list_channels() {
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
