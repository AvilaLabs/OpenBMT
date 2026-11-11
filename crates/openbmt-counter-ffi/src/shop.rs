//! A look at the shop's signed settings before the node starts, so that the
//! app knows whether Tor is on (NR-26) and what the smallest sale is. The
//! reading is for display only: the settings are not verified here, and the
//! app must not rely on it. `start` checks the signatures
//! (`start-settings`) on the same bytes and itself refuses to start when the
//! verified settings have Tor on and the start has no proxy (CR-87).

use openbmt_core::settings::SignedSettings;

use crate::codec::wire_record;
use crate::error::{refused_by, CounterError};

/// What the app needs from the shop's settings before the node runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    /// The version of the settings.
    pub version: u32,
    /// Whether every connection goes through Tor.
    pub tor_on: bool,
    /// Whether customers can join the Wi-Fi the counter is on.
    pub customer_wifi: bool,
    /// The smallest bitcoin sale, in satoshis (0 means no floor).
    pub min_sale_sats: u64,
}

wire_record!(SettingsView {
    version,
    tor_on,
    customer_wifi,
    min_sale_sats
});

/// The settings in `bytes` (`SignedSettings::to_bytes`), read and not
/// verified.
pub(crate) fn settings_read(bytes: Vec<u8>) -> Result<SettingsView, CounterError> {
    let signed = SignedSettings::from_bytes(&bytes).map_err(|e| {
        refused_by(
            "settings-bytes",
            format!("the settings are not readable: {e}"),
        )
    })?;
    let s = signed.settings;
    Ok(SettingsView {
        version: s.version,
        tor_on: s.tor_on,
        customer_wifi: s.customer_wifi,
        min_sale_sats: s.min_sale_sats,
    })
}
