#![no_main]

use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_owner::hardware::{read_returned, FirstAddressRecord, Returned};

fn wallet() -> &'static WalletDescription {
    static WALLET: OnceLock<WalletDescription> = OnceLock::new();
    WALLET.get_or_init(|| {
        let secp = Secp256k1::new();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(Network::Regtest, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(&secp, &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            });
        }
        WalletDescription::new(Network::Regtest, slots.try_into().unwrap()).unwrap()
    })
}

/// What the hardware path holds of bytes it took: a record writes back to
/// the same bytes, and a binary PSBT that a signer returned writes back to
/// the same bytes when it is read as a PSBT.
fn check(data: &[u8]) {
    if let Ok(record) = FirstAddressRecord::from_bytes(wallet(), data) {
        assert_eq!(record.to_bytes(), data);
    }
    if data.starts_with(b"psbt\xff") {
        if let Ok(Returned::Psbt(p)) = read_returned(data) {
            assert_eq!(p.serialize(), data);
        }
    } else {
        let _ = read_returned(data);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the header of a record for this wallet and
    // behind the PSBT magic, so that the input gets to the parts.
    let mut record = FirstAddressRecord::new(
        wallet(),
        openbmt_owner::hardware::FirstAddressState::Checked,
        0,
    )
    .to_bytes();
    record.truncate(37);
    record.extend_from_slice(data);
    check(&record);
    let mut psbt = b"psbt\xff".to_vec();
    psbt.extend_from_slice(data);
    check(&psbt);
});
