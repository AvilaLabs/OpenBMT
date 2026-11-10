//! Fixed keys, funding transactions and PSBTs for the tests.
//!
//! The keys come from fixed seeds. The PSBTs are built the way an honest
//! owner phone builds them: full previous transactions, witness scripts and
//! the key-path hints of all three keys on every input and on the change
//! output. The BIP143 hash used to check signatures is written out here
//! again, so it does not depend on the code under test.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::consensus::encode::{serialize, VarInt};
use bitcoin::ecdsa;
use bitcoin::hashes::{sha256d, Hash};
use bitcoin::psbt::{Input, Psbt};
use bitcoin::secp256k1::{self, Message};
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    WPubkeyHash, Witness,
};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};

use crate::crypto::secp;
use crate::{Refusal, Review, Signed, SignerState};

pub const NET: Network = Network::Regtest;
/// A time well after the start of the clock.
pub const NOW: u64 = 10_000_000;
/// The index (0-based) of the Key app's own key.
pub const OWN: usize = 1;

pub struct Kit {
    pub accounts: [Xpriv; 3],
    pub xpubs: [Xpub; 3],
    pub wallet: WalletDescription,
    pub description: String,
}

pub fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
        let secp = secp();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(secp, &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(secp, &account),
            });
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
        let xpubs = [0, 1, 2].map(|n| Xpub::from_priv(secp, &accounts[n]));
        Kit {
            accounts: accounts.try_into().unwrap(),
            xpubs,
            description: wallet.canonical_string().to_string(),
            wallet,
        }
    })
}

/// The Key app's account key (key 2).
pub fn key2() -> &'static Xpriv {
    &kit().accounts[OWN]
}

/// A registered state: this device holds slot 2, key 3 was given.
pub fn state() -> SignerState {
    let k = kit();
    SignerState::register(&k.description, NET, &k.xpubs[OWN], Some(&k.xpubs[2])).unwrap()
}

pub fn ok(_: &[u8]) -> Result<(), Infallible> {
    Ok(())
}

pub fn fail(_: &[u8]) -> Result<(), &'static str> {
    Err("disk full")
}

thread_local! {
    static NEXT_TAG: Cell<u32> = const { Cell::new(1) };
}

fn next_tag() -> u32 {
    NEXT_TAG.with(|c| {
        let t = c.get();
        c.set(t + 1);
        t
    })
}

/// A coin in the wallet: a previous transaction paying one wallet script.
#[derive(Clone)]
pub struct Coin {
    pub prev: Transaction,
    pub vout: u32,
    pub chain: Keychain,
    pub index: u32,
}

impl Coin {
    pub fn outpoint(&self) -> OutPoint {
        OutPoint {
            txid: self.prev.compute_txid(),
            vout: self.vout,
        }
    }
}

/// A coin of `sats` on the receive keychain, index 0.
pub fn coin(sats: u64) -> Coin {
    coin_at(Keychain::Receive, 0, sats)
}

/// A coin of `sats` at `chain/index`. Every call makes a new transaction.
pub fn coin_at(chain: Keychain, index: u32, sats: u64) -> Coin {
    let mut funding = [0u8; 32];
    funding[..4].copy_from_slice(&next_tag().to_le_bytes());
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(funding),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: kit().wallet.script(chain, index).unwrap(),
        }],
    };
    Coin {
        prev,
        vout: 0,
        chain,
        index,
    }
}

/// An output of the transaction under test.
#[derive(Clone)]
pub struct Out {
    pub script: ScriptBuf,
    pub sats: u64,
    /// The change index the owner phone claims, with honest hints.
    pub change: Option<u32>,
}

/// A P2WPKH script for a destination outside the wallet.
pub fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

pub fn pay(sats: u64) -> Out {
    pay_to(external(1), sats)
}

pub fn pay_to(script: ScriptBuf, sats: u64) -> Out {
    Out {
        script,
        sats,
        change: None,
    }
}

/// An honest change output at `index` on the change keychain.
pub fn change(index: u32, sats: u64) -> Out {
    Out {
        script: kit().wallet.change_script(index).unwrap(),
        sats,
        change: Some(index),
    }
}

/// The hints of all three keys at `chain/index`.
pub fn hints(
    chain: Keychain,
    index: u32,
) -> BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)> {
    let k = kit();
    let keys = k.wallet.derived_keys(chain, index).unwrap();
    let c = match chain {
        Keychain::Receive => 0,
        Keychain::Change => 1,
    };
    k.wallet
        .slots()
        .iter()
        .zip(keys)
        .map(|(slot, key)| {
            let path = slot
                .origin_path
                .child(ChildNumber::Normal { index: c })
                .child(ChildNumber::Normal { index });
            (key.inner, (slot.origin_fingerprint, path))
        })
        .collect()
}

/// An honest PSBT for `coins` and `outs`.
pub fn build(coins: &[Coin], outs: &[Out]) -> Psbt {
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: coins
            .iter()
            .map(|c| TxIn {
                previous_output: c.outpoint(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            })
            .collect(),
        output: outs
            .iter()
            .map(|o| TxOut {
                value: Amount::from_sat(o.sats),
                script_pubkey: o.script.clone(),
            })
            .collect(),
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
    for (input, c) in psbt.inputs.iter_mut().zip(coins) {
        input.non_witness_utxo = Some(c.prev.clone());
        input.witness_utxo = Some(c.prev.output[c.vout as usize].clone());
        input.witness_script = Some(kit().wallet.witness_script(c.chain, c.index).unwrap());
        input.bip32_derivation = hints(c.chain, c.index);
    }
    for (output, o) in psbt.outputs.iter_mut().zip(outs) {
        if let Some(index) = o.change {
            output.witness_script = Some(
                kit()
                    .wallet
                    .witness_script(Keychain::Change, index)
                    .unwrap(),
            );
            output.bip32_derivation = hints(Keychain::Change, index);
        }
    }
    psbt
}

/// One coin of `amount + fee` paying `amount` out; the fee is what is left.
pub fn simple(amount: u64, fee: u64) -> Psbt {
    build(&[coin(amount + fee)], &[pay(amount)])
}

/// A payment of `amount` to `script` with `fee`, from a fresh coin.
pub fn simple_to(script: ScriptBuf, amount: u64, fee: u64) -> Psbt {
    build(&[coin(amount + fee)], &[pay_to(script, amount)])
}

pub fn review_ok(state: &SignerState, psbt: &Psbt, now: u64) -> Review {
    state.review(&psbt.serialize(), now).unwrap()
}

/// Review and sign with key 2 and a store that works.
pub fn sign_ok(state: &mut SignerState, psbt: &Psbt, now: u64) -> Signed {
    let review = review_ok(state, psbt, now);
    state.sign(review, key2(), now, ok).unwrap()
}

/// The refusal that `review` gives. Also checks that the state did not
/// change.
pub fn refused(state: &SignerState, psbt: &Psbt, now: u64) -> Refusal {
    let before = state.to_bytes();
    let r = state.review(&psbt.serialize(), now).unwrap_err();
    assert_eq!(state.to_bytes(), before, "review changed the state");
    r
}

/// Refuse with `want`, and the rule it names.
#[track_caller]
pub fn assert_refused(state: &SignerState, psbt: &Psbt, now: u64, want: Refusal, rule: &str) {
    let got = refused(state, psbt, now);
    assert_eq!(got, want);
    assert_eq!(got.rule(), rule);
    assert!(got.to_string().starts_with(rule), "{got}");
}

// ---- independent BIP143 -------------------------------------------------

/// The BIP143 signature hash of input `i` (SIGHASH_ALL), written out from
/// the specification.
pub fn bip143(tx: &Transaction, i: usize, script_code: &bitcoin::Script, amount: u64) -> [u8; 32] {
    let dsha = |data: &[u8]| sha256d::Hash::hash(data).to_byte_array();
    let mut prevouts = Vec::new();
    let mut sequences = Vec::new();
    for input in &tx.input {
        prevouts.extend_from_slice(&serialize(&input.previous_output));
        sequences.extend_from_slice(&input.sequence.0.to_le_bytes());
    }
    let mut outputs = Vec::new();
    for output in &tx.output {
        outputs.extend_from_slice(&output.value.to_sat().to_le_bytes());
        outputs.extend_from_slice(&serialize(&VarInt(output.script_pubkey.len() as u64)));
        outputs.extend_from_slice(output.script_pubkey.as_bytes());
    }
    let mut pre = Vec::new();
    pre.extend_from_slice(&tx.version.0.to_le_bytes());
    pre.extend_from_slice(&dsha(&prevouts));
    pre.extend_from_slice(&dsha(&sequences));
    pre.extend_from_slice(&serialize(&tx.input[i].previous_output));
    pre.extend_from_slice(&serialize(&VarInt(script_code.len() as u64)));
    pre.extend_from_slice(script_code.as_bytes());
    pre.extend_from_slice(&amount.to_le_bytes());
    pre.extend_from_slice(&tx.input[i].sequence.0.to_le_bytes());
    pre.extend_from_slice(&dsha(&outputs));
    pre.extend_from_slice(&tx.lock_time.to_consensus_u32().to_le_bytes());
    pre.extend_from_slice(&1u32.to_le_bytes());
    dsha(&pre)
}

/// The keychain and index an input's hints name.
pub fn path_of(input: &Input) -> (Keychain, u32) {
    let (_, path) = input.bip32_derivation.values().next().unwrap();
    let steps: &[ChildNumber] = path.as_ref();
    let n = steps.len();
    let chain = match steps[n - 2] {
        ChildNumber::Normal { index: 0 } => Keychain::Receive,
        ChildNumber::Normal { index: 1 } => Keychain::Change,
        other => panic!("{other:?}"),
    };
    let ChildNumber::Normal { index } = steps[n - 1] else {
        panic!("hardened index")
    };
    (chain, index)
}

fn amount_of(psbt: &Psbt, i: usize) -> u64 {
    let vout = psbt.unsigned_tx.input[i].previous_output.vout as usize;
    psbt.inputs[i].non_witness_utxo.as_ref().unwrap().output[vout]
        .value
        .to_sat()
}

/// Add a signature by key `who` (0, 1 or 2) to every input, as another
/// device in the wallet would make it.
pub fn sign_as(psbt: &mut Psbt, who: usize) {
    for i in 0..psbt.inputs.len() {
        sign_input_as(psbt, i, who);
    }
}

/// Add a signature by key `who` to input `i` only.
pub fn sign_input_as(psbt: &mut Psbt, i: usize, who: usize) {
    let k = kit();
    let (chain, index) = path_of(&psbt.inputs[i]);
    let ws = k.wallet.witness_script(chain, index).unwrap();
    let hash = bip143(&psbt.unsigned_tx, i, &ws, amount_of(psbt, i));
    let c = match chain {
        Keychain::Receive => 0,
        Keychain::Change => 1,
    };
    let child = k.accounts[who]
        .derive_priv(
            secp(),
            &[
                ChildNumber::Normal { index: c },
                ChildNumber::Normal { index },
            ],
        )
        .unwrap();
    let signature = secp().sign_ecdsa(&Message::from_digest(hash), &child.private_key);
    let public = k.wallet.derived_keys(chain, index).unwrap()[who];
    psbt.inputs[i].partial_sigs.insert(
        public,
        ecdsa::Signature {
            signature,
            sighash_type: EcdsaSighashType::All,
        },
    );
}

/// Combine the signatures in `psbt_bytes` into a finalized transaction,
/// after checking each signature against the independent BIP143 hash and
/// the witness script against the script being spent.
pub fn finalize(psbt_bytes: &[u8]) -> Transaction {
    let psbt = Psbt::deserialize(psbt_bytes).unwrap();
    let mut tx = psbt.unsigned_tx.clone();
    for (i, input) in psbt.inputs.iter().enumerate() {
        let ws = input.witness_script.clone().unwrap();
        let spent = &input.witness_utxo.clone().unwrap();
        assert_eq!(
            ScriptBuf::new_p2wsh(&ws.wscript_hash()),
            spent.script_pubkey,
            "witness script does not hash to the spent script"
        );
        let hash = bip143(&psbt.unsigned_tx, i, &ws, spent.value.to_sat());
        let message = Message::from_digest(hash);
        let mut chosen: Vec<Vec<u8>> = Vec::new();
        for ins in ws.instructions() {
            let bitcoin::script::Instruction::PushBytes(bytes) = ins.unwrap() else {
                continue;
            };
            if bytes.len() != 33 {
                continue;
            }
            let key = PublicKey::from_slice(bytes.as_bytes()).unwrap();
            if let Some(sig) = input.partial_sigs.get(&key) {
                secp()
                    .verify_ecdsa(&message, &sig.signature, &key.inner)
                    .expect("signature verifies against the independent hash");
                assert_eq!(sig.sighash_type, EcdsaSighashType::All);
                assert!(sig.signature.serialize_compact()[32] < 0x80, "high S");
                if chosen.len() < 2 {
                    chosen.push(sig.to_vec());
                }
            }
        }
        assert_eq!(chosen.len(), 2, "input {i} needs two signatures");
        let mut witness = Witness::new();
        witness.push([] as [u8; 0]);
        witness.push(&chosen[0]);
        witness.push(&chosen[1]);
        witness.push(ws.as_bytes());
        tx.input[i].witness = witness;
    }
    tx
}
