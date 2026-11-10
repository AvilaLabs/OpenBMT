//! Fixed keys, coins and signers for the tests.
//!
//! The keys come from fixed seeds: test only, never use. The BIP 143 hash
//! used to check signatures is written out here again, so the checks do not
//! depend on the code under test.

#![allow(dead_code)]

use std::convert::Infallible;
use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv, Xpub};
use bitcoin::consensus::encode::{serialize, VarInt};
use bitcoin::ecdsa;
use bitcoin::hashes::{sha256d, Hash};
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{self, All, Message, Secp256k1};
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use openbmt_core::wallet::{KeySlot, Keychain, WalletDescription};
use openbmt_signer::{Ceilings, FeeCeilings, SignerState, MAX_MONEY_SATS};
use openbmt_spend::Coin;

pub const NET: Network = Network::Regtest;
pub const NOW: u64 = 10_000_000;

pub fn secp() -> &'static Secp256k1<All> {
    static CONTEXT: OnceLock<Secp256k1<All>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::new)
}

pub struct Kit {
    pub accounts: [Xpriv; 3],
    pub xpubs: [Xpub; 3],
    pub wallet: WalletDescription,
}

/// Three keys from fixed seeds, and the wallet they make. Test only.
pub fn kit() -> &'static Kit {
    static KIT: OnceLock<Kit> = OnceLock::new();
    KIT.get_or_init(|| {
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut accounts = Vec::new();
        let mut slots = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NET, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(secp(), &path).unwrap();
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(secp()),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(secp(), &account),
            });
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
        let xpubs = [0, 1, 2].map(|n| Xpub::from_priv(secp(), &accounts[n]));
        Kit {
            accounts: accounts.try_into().unwrap(),
            xpubs,
            wallet,
        }
    })
}

pub fn wallet() -> &'static WalletDescription {
    &kit().wallet
}

// ---- coins ---------------------------------------------------------------

/// A coin of `sats` at `chain/index` with `confirmations`. `tag` makes the
/// funding transaction (and so the outpoint) unique.
pub fn coin_tagged(tag: u32, chain: Keychain, index: u32, sats: u64, confirmations: u32) -> Coin {
    let mut funding = [0u8; 32];
    funding[..4].copy_from_slice(&tag.to_le_bytes());
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
        output: vec![
            TxOut {
                value: Amount::from_sat(1),
                script_pubkey: external(250),
            },
            TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: wallet().script(chain, index).unwrap(),
            },
        ],
    };
    Coin {
        outpoint: OutPoint {
            txid: prev.compute_txid(),
            vout: 1,
        },
        prev_tx: prev,
        vout: 1,
        chain,
        index,
        confirmations,
    }
}

/// A coin of `sats` on the receive keychain at `index`, 6 deep.
pub fn coin(tag: u32, index: u32, sats: u64) -> Coin {
    coin_tagged(tag, Keychain::Receive, index, sats, 6)
}

pub fn coin_conf(tag: u32, index: u32, sats: u64, confirmations: u32) -> Coin {
    coin_tagged(tag, Keychain::Receive, index, sats, confirmations)
}

/// A P2WPKH script outside the wallet.
pub fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

pub fn request(payments: Vec<(ScriptBuf, u64)>, rate: u64) -> openbmt_spend::SpendRequest {
    openbmt_spend::SpendRequest {
        payments,
        fee_rate: rate,
        change_index: 0,
        min_confirmations: 1,
        send_all: false,
        must_spend: Vec::new(),
    }
}

// ---- random numbers ------------------------------------------------------

/// A fixed-seed xorshift64*: the property tests need no dependency and run
/// the same every time.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `lo..=hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

// ---- signers -------------------------------------------------------------

pub fn ok(_: &[u8]) -> Result<(), Infallible> {
    Ok(())
}

/// A signer state for key `who` (0, 1 or 2), with every ceiling raised to
/// the highest the signer takes, so that the tests meet the builder's
/// rules and not the owner's ceilings.
pub fn signer(who: usize) -> SignerState {
    let k = kit();
    let key3 = (who != 2).then_some(&k.xpubs[2]);
    let mut s =
        SignerState::register(wallet().canonical_string(), NET, &k.xpubs[who], key3).unwrap();
    let c = Ceilings {
        per_tx: MAX_MONEY_SATS,
        per_day: MAX_MONEY_SATS,
        per_week: MAX_MONEY_SATS,
    };
    s.set_ceilings(c, c, ok).unwrap();
    s.set_fee_ceilings(
        FeeCeilings {
            sat_per_vb: 1000,
            absolute_sats: 2_000_000,
        },
        ok,
    )
    .unwrap();
    s
}

/// Sign with the real signer as key `who`: review, then sign. Returns the
/// signed file.
pub fn sign_with_signer(psbt: &Psbt, who: usize) -> Psbt {
    let mut s = signer(who);
    let review = s
        .review(&psbt.serialize(), NOW)
        .expect("the signer reviews the file");
    let signed = s
        .sign(review, &kit().accounts[who], NOW, ok)
        .expect("the signer signs");
    Psbt::deserialize(&signed.psbt).unwrap()
}

// ---- independent BIP 143 -------------------------------------------------

/// The BIP 143 signature hash of input `i` (SIGHASH_ALL), written out from
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

pub fn amount_of(psbt: &Psbt, i: usize) -> u64 {
    let vout = psbt.unsigned_tx.input[i].previous_output.vout as usize;
    psbt.inputs[i].non_witness_utxo.as_ref().unwrap().output[vout]
        .value
        .to_sat()
}

fn chain_and_index(psbt: &Psbt, i: usize) -> (Keychain, u32) {
    let (_, path) = psbt.inputs[i].bip32_derivation.values().next().unwrap();
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

/// A signature by key `who` on input `i`, over the independent hash.
pub fn signature_by(psbt: &Psbt, i: usize, who: usize) -> (bitcoin::PublicKey, ecdsa::Signature) {
    let (chain, index) = chain_and_index(psbt, i);
    let ws = wallet().witness_script(chain, index).unwrap();
    let hash = bip143(&psbt.unsigned_tx, i, &ws, amount_of(psbt, i));
    let c = match chain {
        Keychain::Receive => 0,
        Keychain::Change => 1,
    };
    let child = kit().accounts[who]
        .derive_priv(
            secp(),
            &[
                ChildNumber::Normal { index: c },
                ChildNumber::Normal { index },
            ],
        )
        .unwrap();
    let signature = secp().sign_ecdsa(&Message::from_digest(hash), &child.private_key);
    let public = wallet().derived_keys(chain, index).unwrap()[who];
    (
        public,
        ecdsa::Signature {
            signature,
            sighash_type: EcdsaSighashType::All,
        },
    )
}

/// Add a signature by key `who` to every input of `psbt`, without the
/// signer: as another device in the wallet would make it.
pub fn sign_as(psbt: &mut Psbt, who: usize) {
    for i in 0..psbt.inputs.len() {
        let (public, sig) = signature_by(psbt, i, who);
        psbt.inputs[i].partial_sigs.insert(public, sig);
    }
}

/// The same signature with its S replaced by the group order minus S: a
/// valid signature in high-S form, which consensus accepts and policy and
/// SR-14 do not.
pub fn high_s(sig: &ecdsa::Signature) -> ecdsa::Signature {
    // The group order n.
    const N: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xfe, 0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36,
        0x41, 0x41,
    ];
    let mut compact = sig.signature.serialize_compact();
    let mut borrow = 0i32;
    let mut s = [0u8; 32];
    for k in (0..32).rev() {
        let d = i32::from(N[k]) - i32::from(compact[32 + k]) - borrow;
        if d < 0 {
            s[k] = (d + 256) as u8;
            borrow = 1;
        } else {
            s[k] = d as u8;
            borrow = 0;
        }
    }
    compact[32..].copy_from_slice(&s);
    ecdsa::Signature {
        signature: secp256k1::ecdsa::Signature::from_compact(&compact).unwrap(),
        sighash_type: sig.sighash_type,
    }
}

/// A plan, its file and two signed copies (keys 1 and 2, by the signer), for
/// a single refund from `coins`.
pub struct Run {
    pub plan: openbmt_spend::Plan,
    pub base: Psbt,
    pub by1: Psbt,
    pub by2: Psbt,
}

pub fn run(coins: &[Coin], payments: Vec<(ScriptBuf, u64)>, rate: u64) -> Run {
    let plan = openbmt_spend::plan(wallet(), coins, &request(payments, rate)).unwrap();
    let base = plan.to_psbt(wallet()).unwrap();
    let by1 = sign_with_signer(&base, 0);
    let by2 = sign_with_signer(&base, 1);
    Run {
        plan,
        base,
        by1,
        by2,
    }
}
