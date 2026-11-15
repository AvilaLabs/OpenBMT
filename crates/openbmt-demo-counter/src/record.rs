//! The pairing record of the demo counter: what `pair_finish` returns for a
//! shop whose owner and approver keys are fixed, so that a visitor's counter
//! starts without a pairing ceremony. The keys are public and guard nothing.

use bitcoin::Network;
use openbmt_core::batch::{AddressBatch, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::PairingRecord;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::{ScriptBuf, WScriptHash};

/// How many addresses the first batch holds.
pub const FIRST_BATCH: u32 = 1_000;

fn identity(n: u8) -> Option<Identity> {
    Identity::from_secret_bytes(&[n; 32]).ok()
}

/// The script of the sale with receive index `index`: the demo's chain does
/// not check spending, so a script made from the index stands in.
fn script_of(index: u32) -> ScriptBuf {
    let hash = sha256::Hash::hash(&index.to_le_bytes());
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array(hash.to_byte_array()))
}

/// The pairing record of the demo counter on regtest, as its bytes, or
/// `None` when a key could not be made.
pub fn demo_record() -> Option<Vec<u8>> {
    let owner = identity(1)?;
    let approver = identity(2)?;
    let counter = identity(3)?;
    let mut settings = Settings::defaults(1_000);
    settings.version = 1;
    settings.customer_wifi = false;
    settings.min_sale_sats = 0;
    settings.devices.insert(counter.device_id());
    settings.counters.insert(counter.device_id(), 10_000);
    let signed = SignedSettings {
        owner_sig: settings.sign(&owner),
        approver_sig: Some(settings.sign(&approver)),
        settings,
        wallet_sig: None,
    };
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: counter.device_id(),
        start: 0,
        count: FIRST_BATCH,
        scripts: (0..FIRST_BATCH).map(script_of).collect(),
    };
    let first = SignedBatch {
        owner_sig: batch.sign(&owner),
        approver_sig: batch.sign(&approver),
        batch,
    };
    let record = PairingRecord {
        identity_secret: [3; 32],
        roots: PinnedRoots::new(owner.device_id(), approver.device_id()).ok()?,
        wallet: [9; 32],
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
        assert_eq!(record.first_batch.map(|b| b.batch.count), Some(FIRST_BATCH));
    }
}
