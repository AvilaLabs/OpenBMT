#![no_main]

use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_approver::Approver;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_signer::SignerState;

const NETWORK: Network = Network::Regtest;
const OWNER_SECRET: [u8; 32] = [0x11; 32];
const APPROVER_SECRET: [u8; 32] = [0x22; 32];

/// The fixed trust anchors a caller supplies at start-up, and the bytes of
/// a valid snapshot with key 2 and without it, so that mutated bytes get
/// past the header.
struct Fixture {
    wallet: WalletDescription,
    key2: Xpriv,
    with_key2: Vec<u8>,
    without_key2: Vec<u8>,
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
        let mut accounts = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NETWORK, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(&secp, &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            });
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NETWORK, slots.try_into().unwrap()).unwrap();
        let owner = Identity::from_secret_bytes(&OWNER_SECRET).unwrap().device_id();
        let plain =
            Approver::new(&APPROVER_SECRET, wallet.clone(), owner, policy()).unwrap();
        let mut full =
            Approver::new(&APPROVER_SECRET, wallet.clone(), owner, policy()).unwrap();
        let state = SignerState::register(
            wallet.canonical_string(),
            NETWORK,
            &Xpub::from_priv(&secp, &accounts[1]),
            Some(&Xpub::from_priv(&secp, &accounts[2])),
        )
        .unwrap();
        full.attach_signer(state, accounts[1]).unwrap();
        Fixture {
            wallet,
            key2: accounts[1],
            with_key2: full.snapshot(),
            without_key2: plain.snapshot(),
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    let owner = Identity::from_secret_bytes(&OWNER_SECRET).unwrap().device_id();
    // The first byte picks whether this device holds key 2 and whether the
    // rest is taken as it is or after the header of a valid snapshot.
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let key2 = (mode & 1 == 1).then_some(f.key2);
    let valid = if key2.is_some() {
        &f.with_key2
    } else {
        &f.without_key2
    };
    let bytes: Vec<u8> = if mode & 2 == 2 {
        let keep = usize::from(mode >> 2).min(valid.len());
        let mut v = valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(a) = Approver::restore(
        &APPROVER_SECRET,
        f.wallet.clone(),
        owner,
        policy(),
        key2,
        &bytes,
    ) {
        assert_eq!(a.snapshot(), bytes);
    }
});
