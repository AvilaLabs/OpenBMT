//! The pairing record of the demo counter: what `pair_finish` returns for a
//! shop whose owner and approver keys are fixed and whose wallet is the demo
//! wallet (`openbmt_demo::wallet`), so that a visitor's counter starts without
//! a pairing ceremony and a sale's address is a real regtest address of that
//! wallet. The keys are public and guard nothing.

use std::str::FromStr;

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpub};
use bitcoin::Network;
use openbmt_core::batch::{AddressBatch, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_counter_node::PairingRecord;

/// How many addresses the first batch holds.
pub const FIRST_BATCH: u32 = 1_000;

/// The three keys of the demo shop's wallet: the fingerprint and the account
/// key at `48h/1h/0h/2h` of three keys made once from random bytes that were
/// thrown away. Only these public keys are kept, so nothing can spend what is
/// paid to the wallet, and its addresses are regtest addresses (`bcrt1...`).
const KEYS: [(&str, &str); 3] = [
    ("04e2cbcf", "tpubDEEsFdrQcKpzQV7vCsKCuymi5XoLQ5k53YMGyH1d4wdp3wXZTaN8DaoBD4VftwFDhJgsqm37n9Zgfn54qvLDfvn1vhRZmqCbGRX22ubkgT1"),
    ("7f2dc2e3", "tpubDEmhcxEte8e1JWBZTPZs22Pk1BPSBxiozuB7XN97aanQY6v1HXTRt46TSuiPtkapHZVDhttAF79GTwgUcg3p2U6g7GGRi1kYSdu47E6q4rG"),
    ("4ed6df90", "tpubDFTXfAqAVw1qU3K9xyiM1q5R5F3dWZ4L1Yr1Hp5ALFWuuAFDjZK1mVg1V1KhHoDqpYYitihDYnPm5iQiUfNVQZTLyAPibWoZE7Lmd9LanZg"),
];

/// The settings' own rate, in satoshis for a dollar: the limits are in
/// satoshis at this rate, and it is the pretend price of the demo
/// (73,554.18 USD for a bitcoin), so a sale of 8.00 USD is under the first
/// instant limit of 10 USD.
pub const SATS_PER_USD: u64 = 1_360;

fn identity(n: u8) -> Option<Identity> {
    Identity::from_secret_bytes(&[n; 32]).ok()
}

/// The demo shop's wallet: the 2-of-3 wallet of the counter's pairing, whose
/// receive scripts the first batch holds. The counter takes a batch of
/// P2WSH scripts only, and a pairing with a single-key wallet (the demo
/// wallet of `openbmt_demo::wallet`) is for the main network only, so the
/// shop's wallet here is a 2-of-3 one on regtest.
pub fn demo_shop_wallet() -> Option<WalletDescription> {
    let path = DerivationPath::from(vec![
        ChildNumber::Hardened { index: 48 },
        ChildNumber::Hardened { index: 1 },
        ChildNumber::Hardened { index: 0 },
        ChildNumber::Hardened { index: 2 },
    ]);
    let slot = |i: usize| -> Option<KeySlot> {
        let (fingerprint, xpub) = KEYS.get(i)?;
        Some(KeySlot {
            index: u8::try_from(i + 1).ok()?,
            origin_fingerprint: Fingerprint::from_str(fingerprint).ok()?,
            origin_path: path.clone(),
            xpub: Xpub::from_str(xpub).ok()?,
        })
    };
    WalletDescription::new(Network::Regtest, [slot(0)?, slot(1)?, slot(2)?]).ok()
}

/// The pairing record of the demo counter on regtest, as its bytes, or
/// `None` when a key could not be made.
pub fn demo_record() -> Option<Vec<u8>> {
    let owner = identity(1)?;
    let approver = identity(2)?;
    let counter = identity(3)?;
    let wallet = demo_shop_wallet()?;
    let checksum = wallet.checksum32();
    let mut settings = Settings::defaults(SATS_PER_USD);
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
