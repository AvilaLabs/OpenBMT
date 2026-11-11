//! A look at the pairing record before the node starts, so that the app
//! knows the network, whether Tor is on (NR-26) and what the smallest sale
//! is and what currency the shop prices in. The reading is for display only: the record's signatures are not
//! verified here, and the app must not rely on it. `start` checks them
//! (`CR-94` to `CR-97`) on the same bytes and itself refuses to start when
//! the verified settings have Tor on and the start has no proxy (CR-87).

use bitcoin::Network;
use openbmt_core::pairing::network_code;
use openbmt_counter_node::{
    effective_settings, own_node_setting, DirStore, PairRefused, PairingRecord,
};

use crate::codec::wire_record;
use crate::error::{from_rule, CounterError};
use crate::views::{CounterNetwork, OwnNodeCheckArgs, PairingView, RelayKind};

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
    /// The currency the shop prices in (CR-193): three capital letters, or
    /// `SAT` for Bitcoin Standard, where an amount is satoshis and no price
    /// is asked (CR-197).
    pub currency: String,
    /// Whether the counters ask the customer for a tip (CR-187).
    pub tips_on: bool,
    /// The suggested tips, in whole percent, in the order the settings give
    /// them (CR-187).
    pub tip_percents: Vec<u8>,
}

wire_record!(SettingsView {
    version,
    tor_on,
    customer_wifi,
    min_sale_sats,
    currency,
    tips_on,
    tip_percents
});

/// The pairing record in `bytes` (`PairingRecord::to_bytes`), read and not
/// verified. With the counter's data folder `dir` (empty for none) the
/// settings shown are the ones a start there would use: the newer of the
/// record's and the file the owner's later settings were kept in (CR-163),
/// so that Tor turned on or off after pairing is known before the start. A
/// file that does not read leaves the record's settings, and the start
/// refuses it with words.
pub(crate) fn pairing_read(bytes: Vec<u8>, dir: &str) -> Result<PairingView, CounterError> {
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
    let relay = match record.relay.as_ref().map(|r| &r.route) {
        None => RelayKind::None,
        Some(openbmt_core::relay::RelayRoute::Direct(_)) => RelayKind::Direct,
        Some(openbmt_core::relay::RelayRoute::Onion { .. }) => RelayKind::Onion,
    };
    let kept = (!dir.is_empty() && std::path::Path::new(dir).join("settings.bin").exists())
        .then(|| DirStore::open(dir).ok())
        .flatten()
        .and_then(|store| effective_settings(&store, &record).ok());
    let s = kept.map_or_else(|| record.settings.settings.clone(), |e| e.signed.settings);
    Ok(PairingView {
        network,
        settings: SettingsView {
            version: s.version,
            tor_on: s.tor_on,
            customer_wifi: s.customer_wifi,
            min_sale_sats: s.min_sale_sats,
            currency: s.currency.code().to_string(),
            tips_on: s.tips_on,
            tip_percents: s.tip_percents.clone(),
        },
        relay,
    })
}

/// The network of a [`CounterNetwork`].
pub(crate) fn network_of(network: CounterNetwork) -> Network {
    match network {
        CounterNetwork::Mainnet => Network::Bitcoin,
        CounterNetwork::Testnet => Network::Testnet,
        CounterNetwork::Signet => Network::Signet,
        CounterNetwork::Regtest => Network::Regtest,
    }
}

/// CR-100 to CR-102: the own node's address as a manager typed it, read
/// and made canonical, or refused with words. Nothing is kept here: the app
/// keeps the text this returns and hands it back at every `start`.
pub(crate) fn own_node_check(a: OwnNodeCheckArgs) -> Result<String, CounterError> {
    own_node_setting(&a.text, network_of(a.network), a.tor)
        .map(|setting| setting.text)
        .map_err(|e| from_rule(e.rule(), e, e.words()))
}
