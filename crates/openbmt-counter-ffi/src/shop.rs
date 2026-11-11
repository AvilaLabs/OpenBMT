//! A look at the pairing record before the node starts, so that the app
//! knows the network, whether Tor is on (NR-26) and what the smallest sale
//! is. The reading is for display only: the record's signatures are not
//! verified here, and the app must not rely on it. `start` checks them
//! (`CR-94` to `CR-97`) on the same bytes and itself refuses to start when
//! the verified settings have Tor on and the start has no proxy (CR-87).

use openbmt_core::pairing::network_code;
use openbmt_counter_node::{PairRefused, PairingRecord};

use crate::codec::wire_record;
use crate::error::{from_rule, CounterError};
use crate::views::{CounterNetwork, PairingView};

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

/// The pairing record in `bytes` (`PairingRecord::to_bytes`), read and not
/// verified.
pub(crate) fn pairing_read(bytes: Vec<u8>) -> Result<PairingView, CounterError> {
    let record = PairingRecord::from_bytes(&bytes).map_err(|e| {
        let e = PairRefused::Record(e);
        from_rule(e.rule(), &e, e.words())
    })?;
    let network = match network_code(record.network) {
        Some(0) => CounterNetwork::Mainnet,
        Some(1) => CounterNetwork::Testnet,
        Some(2) => CounterNetwork::Signet,
        _ => CounterNetwork::Regtest,
    };
    let s = record.settings.settings;
    Ok(PairingView {
        network,
        settings: SettingsView {
            version: s.version,
            tor_on: s.tor_on,
            customer_wifi: s.customer_wifi,
            min_sale_sats: s.min_sale_sats,
        },
    })
}
