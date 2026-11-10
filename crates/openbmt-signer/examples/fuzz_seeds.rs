//! Write valid PSBT files as a seed corpus for the `signer_review` fuzz
//! target.
//!
//! ```text
//! cargo run -p openbmt-signer --example fuzz_seeds -- fuzz/corpus/signer_review
//! ```
//!
//! The wallet is the one the fuzz target registers: three keys from fixed
//! seeds on regtest. Without these files the fuzzer rarely gets past
//! parsing; with them it starts from files that pass every rule.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::ecdsa;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{self, All, Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_signer::SignerState;

const NETWORK: Network = Network::Regtest;

struct Kit {
    secp: Secp256k1<All>,
    accounts: Vec<Xpriv>,
    wallet: WalletDescription,
}

/// The same three keys as `fuzz/fuzz_targets/signer_review.rs`.
fn kit() -> Result<Kit, String> {
    let secp = Secp256k1::new();
    let path: DerivationPath = "m/48'/1'/0'/2'".parse().map_err(|_| "path")?;
    let mut slots = Vec::new();
    let mut accounts = Vec::new();
    for n in 0..3u8 {
        let master = Xpriv::new_master(NETWORK, &[n + 1; 32]).map_err(|e| e.to_string())?;
        let account = master
            .derive_priv(&secp, &path)
            .map_err(|e| e.to_string())?;
        slots.push(KeySlot {
            index: n + 1,
            origin_fingerprint: master.fingerprint(&secp),
            origin_path: path.clone(),
            xpub: Xpub::from_priv(&secp, &account),
        });
        accounts.push(account);
    }
    let slots: [KeySlot; 3] = slots.try_into().map_err(|_| "three slots")?;
    let wallet = WalletDescription::new(NETWORK, slots).map_err(|e| e.to_string())?;
    Ok(Kit {
        secp,
        accounts,
        wallet,
    })
}

fn chain_step(chain: Keychain) -> ChildNumber {
    ChildNumber::Normal {
        index: match chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        },
    }
}

type Hints = BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>;

fn hints(k: &Kit, chain: Keychain, index: u32) -> Result<Hints, String> {
    let keys = k
        .wallet
        .derived_keys(chain, index)
        .map_err(|e| e.to_string())?;
    Ok(k.wallet
        .slots()
        .iter()
        .zip(keys)
        .map(|(slot, key)| {
            let path = slot
                .origin_path
                .child(chain_step(chain))
                .child(ChildNumber::Normal { index });
            (key.inner, (slot.origin_fingerprint, path))
        })
        .collect())
}

/// A wallet coin: a previous transaction with one output to `chain/index`.
struct Coin {
    prev: Transaction,
    chain: Keychain,
    index: u32,
}

fn coin(k: &Kit, tag: u8, chain: Keychain, index: u32, sats: u64) -> Result<Coin, String> {
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array([tag; 32]),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: k.wallet.script(chain, index).map_err(|e| e.to_string())?,
        }],
    };
    Ok(Coin { prev, chain, index })
}

/// An honest PSBT: `coins` in, `pay` satoshis to an outside address if not
/// zero, and change at `change` (index, satoshis) if given.
fn build(k: &Kit, coins: &[Coin], pay: u64, change: Option<(u32, u64)>) -> Result<Psbt, String> {
    let mut output = Vec::new();
    if pay > 0 {
        output.push(TxOut {
            value: Amount::from_sat(pay),
            script_pubkey: ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([7; 20])),
        });
    }
    if let Some((index, sats)) = change {
        output.push(TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: k
                .wallet
                .script(Keychain::Change, index)
                .map_err(|e| e.to_string())?,
        });
    }
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: coins
            .iter()
            .map(|c| TxIn {
                previous_output: OutPoint {
                    txid: c.prev.compute_txid(),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            })
            .collect(),
        output,
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).map_err(|e| e.to_string())?;
    for (input, c) in psbt.inputs.iter_mut().zip(coins) {
        input.non_witness_utxo = Some(c.prev.clone());
        input.witness_utxo = Some(c.prev.output[0].clone());
        input.witness_script = Some(
            k.wallet
                .witness_script(c.chain, c.index)
                .map_err(|e| e.to_string())?,
        );
        input.bip32_derivation = hints(k, c.chain, c.index)?;
    }
    if let (Some((index, _)), Some(last)) = (change, psbt.outputs.last_mut()) {
        last.bip32_derivation = hints(k, Keychain::Change, index)?;
    }
    Ok(psbt)
}

/// Add key 1's signature to every input, as the owner phone does.
fn sign_as_key1(k: &Kit, psbt: &mut Psbt, coins: &[Coin]) -> Result<(), String> {
    let tx = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&tx);
    for (i, c) in coins.iter().enumerate() {
        let script = k
            .wallet
            .witness_script(c.chain, c.index)
            .map_err(|e| e.to_string())?;
        let hash = cache
            .p2wsh_signature_hash(i, &script, c.prev.output[0].value, EcdsaSighashType::All)
            .map_err(|e| e.to_string())?;
        let child = k.accounts[0]
            .derive_priv(
                &k.secp,
                &[chain_step(c.chain), ChildNumber::Normal { index: c.index }],
            )
            .map_err(|e| e.to_string())?;
        let signature = k.secp.sign_ecdsa(
            &Message::from_digest(hash.to_byte_array()),
            &child.private_key,
        );
        let key = k
            .wallet
            .derived_keys(c.chain, c.index)
            .map_err(|e| e.to_string())?[0];
        psbt.inputs[i].partial_sigs.insert(
            key,
            ecdsa::Signature {
                signature,
                sighash_type: EcdsaSighashType::All,
            },
        );
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("usage: fuzz_seeds <directory>")?;
    let dir = Path::new(&dir);
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let k = kit()?;
    let mut seeds: Vec<(&str, Psbt)> = Vec::new();

    let one = [coin(&k, 1, Keychain::Receive, 0, 60_000)?];
    seeds.push(("payment", build(&k, &one, 59_000, None)?));

    let one = [coin(&k, 2, Keychain::Receive, 5, 500_000)?];
    seeds.push((
        "payment-with-change",
        build(&k, &one, 80_000, Some((0, 419_000)))?,
    ));

    let two = [
        coin(&k, 3, Keychain::Receive, 1, 40_000)?,
        coin(&k, 4, Keychain::Change, 3, 70_000)?,
    ];
    seeds.push(("consolidation", build(&k, &two, 0, Some((4, 109_000)))?));

    let two = [
        coin(&k, 5, Keychain::Receive, 2, 90_000)?,
        coin(&k, 6, Keychain::Receive, 9, 30_000)?,
    ];
    let mut signed = build(&k, &two, 50_000, Some((1, 69_000)))?;
    sign_as_key1(&k, &mut signed, &two)?;
    seeds.push(("payment-signed-by-key-1", signed));

    // Every seed must pass the review of the state the fuzz target uses.
    let own = Xpub::from_priv(&k.secp, &k.accounts[1]);
    let key3 = Xpub::from_priv(&k.secp, &k.accounts[2]);
    let state = SignerState::register(k.wallet.canonical_string(), NETWORK, &own, Some(&key3))
        .map_err(|e| e.to_string())?;
    for (name, psbt) in &seeds {
        let bytes = psbt.serialize();
        state
            .review(&bytes, 10_000_000)
            .map_err(|r| format!("seed {name} does not pass the review: {r}"))?;
        let path = dir.join(format!("{name}.psbt"));
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        println!("{}", path.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
