#![no_main]

use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_owner::{Options, Owner};

const NETWORK: Network = Network::Regtest;
const OWNER_SECRET: [u8; 32] = [0x11; 32];

/// The fixed trust anchors a caller supplies at start-up, and the bytes of
/// a valid empty snapshot, so that mutated bytes get past the header.
struct Fixture {
    wallet: WalletDescription,
    approver: DeviceId,
    valid: Vec<u8>,
}

fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let secp = Secp256k1::new();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NETWORK, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(&secp, &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            });
        }
        let wallet = WalletDescription::new(NETWORK, slots.try_into().unwrap()).unwrap();
        let approver = Identity::from_secret_bytes(&[0x22; 32]).unwrap().device_id();
        let owner = Owner::new(
            &OWNER_SECRET,
            wallet.clone(),
            approver,
            policy(),
            Options::default(),
        )
        .unwrap();
        Fixture {
            wallet,
            approver,
            valid: owner.snapshot().unwrap(),
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    // The first byte picks whether the rest follows the header of a valid
    // snapshot or stands alone.
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let keep = usize::from(mode >> 1).min(f.valid.len());
        let mut v = f.valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(o) = Owner::restore(
        &OWNER_SECRET,
        f.wallet.clone(),
        f.approver,
        policy(),
        &bytes,
    ) {
        assert_eq!(o.snapshot().unwrap(), bytes);
    }
});
