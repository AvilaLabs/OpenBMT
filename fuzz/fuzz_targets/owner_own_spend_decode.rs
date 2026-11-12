#![no_main]

use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use libfuzzer_sys::fuzz_target;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_owner::own_shop::own_approver;
use openbmt_owner::{CashAmount, Options, Owner};
use openbmt_spend::Coin;

const OWNER_SECRET: [u8; 32] = [0x11; 32];

/// The wallet of a shop with its own wallet, and the bytes of a valid
/// snapshot that holds an open session of an own-wallet spend (version 5),
/// so that mutated bytes get past the header and into the session.
struct Fixture {
    wallet: ImportedWallet,
    valid: Vec<u8>,
}

fn policy() -> BatchPolicy {
    BatchPolicy { min_count: 3 }
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).unwrap();
        let path: DerivationPath = "m/84'/0'/0'".parse().unwrap();
        let account = master.derive_priv(&secp, &path).unwrap();
        let wallet = ImportedWallet::Single(
            SingleKeyWallet::new(
                master.fingerprint(&secp),
                0,
                Xpub::from_priv(&secp, &account),
            )
            .unwrap(),
        );
        let mut owner = Owner::new(
            &OWNER_SECRET,
            wallet.clone(),
            own_approver(&OWNER_SECRET).unwrap(),
            policy(),
            Options::default(),
        )
        .unwrap();
        let nop = |_: &[u8]| Ok::<(), String>(());
        let to = ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([9; 20]));
        let cash = owner
            .queue_cash_out(to, CashAmount::Sats(100_000), "Savings", nop)
            .unwrap();
        let prev = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([1; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![
                TxOut {
                    value: Amount::from_sat(1),
                    script_pubkey: ScriptBuf::new(),
                },
                TxOut {
                    value: Amount::from_sat(500_000),
                    script_pubkey: wallet.receive_script(0).unwrap(),
                },
            ],
        };
        let coin = Coin {
            outpoint: OutPoint {
                txid: prev.compute_txid(),
                vout: 1,
            },
            prev_tx: prev,
            vout: 1,
            chain: Keychain::Receive,
            index: 0,
            confirmations: 6,
        };
        owner
            .build_own_spend(&[coin], &[cash], 1_000, 0, 850_000, 1_000, &[7; 32], nop)
            .unwrap();
        Fixture {
            wallet,
            valid: owner.snapshot().unwrap(),
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    // The first byte picks whether the rest follows the first bytes of a
    // valid snapshot or stands alone.
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let keep = (usize::from(mode >> 1) * f.valid.len() / 128).min(f.valid.len());
        let mut v = f.valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(o) = Owner::restore(
        &OWNER_SECRET,
        f.wallet.clone(),
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        &bytes,
    ) {
        assert_eq!(o.snapshot().unwrap(), bytes);
    }
});
