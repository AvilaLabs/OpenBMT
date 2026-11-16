//! The pairing record of the demo counter: what `pair_finish` returns for a
//! shop whose owner and approver keys are fixed and whose wallet is the demo
//! wallet (`openbmt_demo::wallet`), so that a visitor's counter starts without
//! a pairing ceremony and a sale's address is a real regtest address of that
//! wallet. The keys are public and guard nothing.

use bitcoin::Network;
use openbmt_core::batch::{AddressBatch, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::PairingRecord;
use openbmt_demo::wallet::demo_shop_wallet;

/// How many addresses the first batch holds.
pub const FIRST_BATCH: u32 = 1_000;

/// The settings' own rate, in satoshis for a dollar: the limits are in
/// satoshis at this rate, and it is the pretend price of the demo
/// (73,554.18 USD for a bitcoin), so a sale of 8.00 USD is under the first
/// instant limit of 10 USD.
pub const SATS_PER_USD: u64 = 1_360;

fn identity(n: u8) -> Option<Identity> {
    Identity::from_secret_bytes(&[n; 32]).ok()
}

/// The key of the shop's Box in the record that has one.
pub fn demo_box() -> Option<Identity> {
    identity(4)
}

/// The pairing record of the demo counter on regtest, as its bytes, or
/// `None` when a key could not be made.
pub fn demo_record() -> Option<Vec<u8>> {
    record(false)
}

/// The same record for a shop that has a Box (the Box is in the settings),
/// which is the one that sends the counter the requests of the shop's till.
pub fn demo_record_with_box() -> Option<Vec<u8>> {
    record(true)
}

fn record(with_box: bool) -> Option<Vec<u8>> {
    let owner = identity(1)?;
    let approver = identity(2)?;
    let counter = identity(3)?;
    let wallet = demo_shop_wallet()?;
    let checksum = wallet.checksum32();
    let mut settings = Settings::defaults(SATS_PER_USD);
    settings.version = 1;
    settings.customer_wifi = false;
    // The pretend Lightning node's key: sales ask it for an invoice.
    settings.lightning_node = Some(crate::lightning::node_key());
    settings.min_sale_sats = 0;
    settings.devices.insert(counter.device_id());
    settings.counters.insert(counter.device_id(), 10_000);
    if with_box {
        let shop_box = demo_box()?.device_id();
        settings.devices.insert(shop_box);
        settings.boxes.insert(shop_box);
    }
    let signed = SignedSettings {
        owner_sig: settings.sign(&owner),
        approver_sig: Some(settings.sign(&approver)),
        settings,
        wallet_sig: None,
    };
    let batch = AddressBatch {
        wallet_checksum: checksum,
        counter: counter.device_id(),
        start: 0,
        count: FIRST_BATCH,
        scripts: (0..FIRST_BATCH)
            .map(|index| wallet.receive_script(index).ok())
            .collect::<Option<Vec<_>>>()?,
    };
    let first = SignedBatch {
        owner_sig: batch.sign(&owner),
        approver_sig: batch.sign(&approver),
        batch,
    };
    let record = PairingRecord {
        identity_secret: [3; 32],
        roots: PinnedRoots::new(owner.device_id(), approver.device_id()).ok()?,
        wallet: checksum,
        network: Network::Regtest,
        settings_floor: 0,
        settings: signed,
        first_batch: Some(first),
        peers: Vec::new(),
        relay: None,
    };
    Some(record.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_reads_back_as_a_regtest_pairing() {
        let bytes = demo_record().expect("a record");
        let record = PairingRecord::from_bytes(&bytes).expect("it reads back");
        assert_eq!(record.network, Network::Regtest);
        let batch = record.first_batch.expect("a first batch").batch;
        assert_eq!(batch.count, FIRST_BATCH);
        let wallet = demo_shop_wallet().expect("the demo wallet");
        assert_eq!(record.wallet, wallet.checksum32());
        assert_eq!(
            batch.scripts.first(),
            Some(&wallet.receive_script(0).expect("a script"))
        );
    }

    #[test]
    fn the_record_with_a_box_names_the_box_in_its_settings() {
        let bytes = demo_record_with_box().expect("a record");
        let record = PairingRecord::from_bytes(&bytes).expect("it reads back");
        let shop_box = demo_box().expect("a key").device_id();
        assert!(record.settings.settings.boxes.contains(&shop_box));
        let plain = PairingRecord::from_bytes(&demo_record().expect("a record")).expect("reads");
        assert!(plain.settings.settings.boxes.is_empty());
    }

    #[test]
    fn the_sale_addresses_are_regtest_addresses_of_the_wallet() {
        use openbmt_core::wallet::Keychain;
        let wallet = demo_shop_wallet().expect("the demo wallet");
        let first = wallet.address(Keychain::Receive, 0).expect("an address");
        assert!(first.to_string().starts_with("bcrt1q"));
        assert!(first
            .as_unchecked()
            .clone()
            .require_network(Network::Bitcoin)
            .is_err());
    }
}
