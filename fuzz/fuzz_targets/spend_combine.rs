#![no_main]

use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::transaction::Version;
use bitcoin::{Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{
    combine, combine_wallet, finalize, plan, take_wallet_signature, ChangePosition, Coin,
    SpendRequest,
};

const NETWORK: Network = Network::Regtest;

/// A wallet from fixed keys, and the file of one fixed plan over it.
/// Everything is made once.
fn fixed() -> &'static (WalletDescription, Psbt) {
    static FIXED: OnceLock<(WalletDescription, Psbt)> = OnceLock::new();
    FIXED.get_or_init(|| {
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
        let coins: Vec<Coin> = (0..2u32)
            .map(|n| {
                let prev = Transaction {
                    version: Version::TWO,
                    lock_time: LockTime::ZERO,
                    input: vec![TxIn {
                        previous_output: OutPoint::null(),
                        script_sig: ScriptBuf::from_bytes(vec![n as u8 + 1]),
                        sequence: Sequence::MAX,
                        witness: Witness::new(),
                    }],
                    output: vec![TxOut {
                        value: Amount::from_sat(300_000),
                        script_pubkey: wallet.receive_script(n).unwrap(),
                    }],
                };
                Coin {
                    outpoint: OutPoint {
                        txid: prev.compute_txid(),
                        vout: 0,
                    },
                    prev_tx: prev,
                    vout: 0,
                    chain: Keychain::Receive,
                    index: n,
                    confirmations: 6,
                }
            })
            .collect();
        let target: ScriptBuf =
            ScriptBuf::new_p2wsh(&bitcoin::WScriptHash::from_byte_array([7; 32]));
        let request = SpendRequest {
            payments: vec![(target, 400_000)],
            fee_rate: 2_000,
            change_index: 0,
            min_confirmations: 1,
            send_all: false,
            must_spend: Vec::new(),
            lock_height: 850_000,
            change_position: ChangePosition::Seed([5; 32]),
        };
        let psbt = plan(&wallet, &coins, &request)
            .unwrap()
            .to_psbt(&wallet)
            .unwrap();
        (wallet, psbt)
    })
}

/// SP-70, SP-87: a wallet with one key from a fixed key, and the file of one
/// fixed plan over it.
fn fixed_single() -> &'static (ImportedWallet, Psbt) {
    static FIXED: OnceLock<(ImportedWallet, Psbt)> = OnceLock::new();
    FIXED.get_or_init(|| {
        let secp = Secp256k1::new();
        let master = Xpriv::new_master(Network::Bitcoin, &[9; 32]).unwrap();
        let path: DerivationPath = "m/84'/0'/0'".parse().unwrap();
        let account = master.derive_priv(&secp, &path).unwrap();
        let single = SingleKeyWallet::new(
            master.fingerprint(&secp),
            0,
            Xpub::from_priv(&secp, &account),
        )
        .unwrap();
        let coins: Vec<Coin> = (0..2u32)
            .map(|n| {
                let prev = Transaction {
                    version: Version::TWO,
                    lock_time: LockTime::ZERO,
                    input: vec![TxIn {
                        previous_output: OutPoint::null(),
                        script_sig: ScriptBuf::from_bytes(vec![n as u8 + 1]),
                        sequence: Sequence::MAX,
                        witness: Witness::new(),
                    }],
                    output: vec![TxOut {
                        value: Amount::from_sat(300_000),
                        script_pubkey: single.receive_script(n).unwrap(),
                    }],
                };
                Coin {
                    outpoint: OutPoint {
                        txid: prev.compute_txid(),
                        vout: 0,
                    },
                    prev_tx: prev,
                    vout: 0,
                    chain: Keychain::Receive,
                    index: n,
                    confirmations: 6,
                }
            })
            .collect();
        let wallet = ImportedWallet::Single(single);
        let request = SpendRequest {
            payments: vec![(
                ScriptBuf::new_p2wsh(&bitcoin::WScriptHash::from_byte_array([7; 32])),
                400_000,
            )],
            fee_rate: 2_000,
            change_index: 0,
            min_confirmations: 1,
            send_all: false,
            must_spend: Vec::new(),
            lock_height: 850_000,
            change_position: ChangePosition::Seed([5; 32]),
        };
        let psbt = plan(&wallet, &coins, &request)
            .unwrap()
            .to_psbt(&wallet)
            .unwrap();
        (wallet, psbt)
    })
}

fuzz_target!(|data: &[u8]| {
    // The input is two "signed" files: the first byte says where the first
    // one ends. Neither is a signature by a key the fuzzer has, so no
    // transaction can come out of them.
    let (wallet, base) = fixed();
    let Some((cut, rest)) = data.split_first() else {
        return;
    };
    let cut = usize::from(*cut).min(rest.len());
    let (first, second) = rest.split_at(cut);
    let copies: Vec<Psbt> = [first, second]
        .into_iter()
        .filter_map(|bytes| Psbt::deserialize(bytes).ok())
        .collect();
    // Never a panic, and never a transaction.
    if let Ok(joined) = combine(base, &copies) {
        assert!(
            finalize(wallet, &joined).is_err(),
            "a transaction came out of bytes that hold no signature"
        );
    }
    // The same bytes in other places: errors, never panics.
    for copy in &copies {
        let _ = combine(copy, &[base.clone()]);
        let _ = combine(copy, &[]);
        assert!(finalize(wallet, copy).is_err());
    }

    // The path of a wallet with one key, over the same bytes: never a panic,
    // and never a transaction, since no signature here is by its key.
    let (single, single_base) = fixed_single();
    if let Ok(joined) = combine_wallet(single, single_base, &copies) {
        assert!(
            finalize(single, &joined).is_err(),
            "a transaction came out of bytes that hold no signature"
        );
    }
    for copy in &copies {
        let _ = combine_wallet(single, copy, &[single_base.clone()]);
        let _ = combine_wallet(single, copy, &[]);
        let _ = take_wallet_signature(single, single_base, copy);
        assert!(finalize(single, copy).is_err());
        // The other kind's wallet over the one-key file, and the reverse.
        let _ = combine_wallet(
            &ImportedWallet::Multisig(wallet.clone()),
            single_base,
            &[copy.clone()],
        );
        let _ = combine_wallet(single, base, &[copy.clone()]);
    }
});
