//! Create a public, test-only Key app pairing for the native checkout proof.
//! The generic PairedRecord contains the counter identity secret, but no
//! wallet spending key or wallet descriptor.
use std::{env, fs, path::PathBuf};

use bitcoin::{
    bip32::{DerivationPath, Xpriv, Xpub},
    secp256k1::Secp256k1,
    Network,
};
use openbmt_core::{
    batch::{AddressBatch, PinnedRoots, SignedBatch},
    identity::Identity,
    pinned_wallet::PinnedWallet,
    settings::{Settings, SignedSettings},
    wallet::WalletDescription,
    wallet_single::ImportedWallet,
};
use openbmt_counter_node::{PairedRecord, PairingRecord};
use std::str::FromStr;

// Public deterministic test keys, matching the counter-node/FFI test kit.
// Never use these identities or records outside regtest.
const OWNER_SECRET: [u8; 32] = [1; 32];
const APPROVER_SECRET: [u8; 32] = [2; 32];
const COUNTER_SECRET: [u8; 32] = [3; 32];
const WALLET_SEEDS: [[u8; 32]; 3] = [[0xa1; 32], [0xa2; 32], [0xa3; 32]];
const FIRST_BATCH_COUNT: u32 = 1_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("openbmt-regtest-pairing.obmt"));

    let owner = Identity::from_secret_bytes(&OWNER_SECRET)?;
    let approver = Identity::from_secret_bytes(&APPROVER_SECRET)?;
    let counter = Identity::from_secret_bytes(&COUNTER_SECRET)?;
    let roots = PinnedRoots::new(owner.device_id(), approver.device_id())?;
    let secp = Secp256k1::new();

    // Derive a real test-only 2-of-3 BIP48 wallet and its public description.
    // The three deterministic wallet seeds stay in this generator; the
    // pairing record receives only the checksum and public receive scripts.
    let account_path = DerivationPath::from_str("m/48h/1h/0h/2h")?;
    let slots = WALLET_SEEDS
        .iter()
        .enumerate()
        .map(|(index, seed)| {
            let master = Xpriv::new_master(Network::Regtest, seed)?;
            let account = master.derive_priv(&secp, &account_path)?;
            Ok(openbmt_core::wallet::KeySlot {
                index: u8::try_from(index + 1)?,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: account_path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let slots: [openbmt_core::wallet::KeySlot; 3] = slots
        .try_into()
        .map_err(|_| "expected exactly three wallet keys")?;
    let wallet_description = WalletDescription::new(Network::Regtest, slots)?;
    let pinned_wallet = PinnedWallet::new(ImportedWallet::Multisig(wallet_description.clone()));

    let batch = AddressBatch {
        wallet_checksum: wallet_description.checksum32(),
        counter: counter.device_id(),
        start: 0,
        count: FIRST_BATCH_COUNT,
        scripts: (0..FIRST_BATCH_COUNT)
            .map(|index| pinned_wallet.receive_script(index))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let first_batch = SignedBatch {
        owner_sig: batch.sign(&owner),
        approver_sig: batch.sign(&approver),
        batch,
    };

    // Initial settings require both pinned roots and must name the counter.
    let mut settings = Settings::defaults(1_000);
    settings.customer_wifi = false;
    settings.min_sale_sats = 0;
    settings.devices.insert(counter.device_id());
    settings.counters.insert(counter.device_id(), 10_000);
    let signed_settings = SignedSettings {
        owner_sig: settings.sign(&owner),
        approver_sig: Some(settings.sign(&approver)),
        settings,
        wallet_sig: None,
    };

    // This follows the existing openbmt-counter-ffi Regtest test kit. The
    // record keeps only a wallet checksum and the signed public script batch.
    let record = PairingRecord {
        identity_secret: COUNTER_SECRET,
        roots,
        wallet: wallet_description.checksum32(),
        network: Network::Regtest,
        settings_floor: 0,
        settings: signed_settings,
        first_batch: Some(first_batch),
        peers: vec!["10.0.2.2:18444".to_string(), "127.0.0.1:18444".to_string()],
        relay: None,
    };
    let paired = PairedRecord::KeyApp(Box::new(record));
    paired.check(&openbmt_counter_node::batch_policy())?;
    let bytes = paired.to_bytes();
    let decoded = PairedRecord::from_bytes(&bytes)?;
    decoded.check(&openbmt_counter_node::batch_policy())?;

    fs::write(&path, bytes)?;
    println!(
        "wrote {} (Key app Regtest pairing, USD, test-only; counter identity secret included; no wallet spending key)",
        path.display()
    );
    Ok(())
}
