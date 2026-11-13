//! What the sign tests and `examples/payment.rs` share: the test words, a
//! payment as the owner phone makes it (the spend path's plan and file), and
//! mutations of it that the stick must refuse. Test keys only; never use
//! these words for money. The payments are for real bitcoin and are never
//! broadcast.
#![allow(dead_code)]

use std::str::FromStr;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::psbt::{Psbt, PsbtSighashType};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, WPubkeyHash,
    Witness,
};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{plan, ChangePosition, Coin, SpendRequest};
use openbmt_stick::app::{App, Chance, Facts, Outcome};
use openbmt_stick::frames::{texts_of, Form};
use openbmt_stick::keys::Key;
use openbmt_stick::outputs::master_of;

/// The test words of the stick's other tests (fingerprint 818bc9b6).
pub const WORDS: [&str; 12] = [
    "primary", "fetch", "primary", "fetch", "primary", "fetch", "primary", "fetch", "primary",
    "fetch", "primary", "fever",
];

pub fn master() -> Xpriv {
    master_of(&WORDS).unwrap()
}

/// The wallet of the test words: the account `m/84'/0'/0'`, one key.
pub fn wallet() -> ImportedWallet {
    let secp = Secp256k1::new();
    let m = master();
    let account = m
        .derive_priv(&secp, &DerivationPath::from_str("m/84'/0'/0'").unwrap())
        .unwrap();
    ImportedWallet::Single(
        SingleKeyWallet::new(m.fingerprint(&secp), 0, Xpub::from_priv(&secp, &account)).unwrap(),
    )
}

fn single(w: &ImportedWallet) -> &SingleKeyWallet {
    match w {
        ImportedWallet::Single(s) => s,
        ImportedWallet::Multisig(_) => unreachable!(),
    }
}

/// The wallet's script at `chain/index`.
pub fn script_at(chain: Keychain, index: u32) -> ScriptBuf {
    single(&wallet()).script(chain, index).unwrap()
}

/// A script that is not the wallet's.
pub fn external(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&WPubkeyHash::from_byte_array([n; 20]))
}

/// A coin of the wallet at `chain/index` of `sats`; `tag` makes its funding
/// transaction unique.
pub fn coin(tag: u32, chain: Keychain, index: u32, sats: u64) -> Coin {
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
                script_pubkey: script_at(chain, index),
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
        confirmations: 6,
    }
}

/// A payment as the owner phone's "Send from your wallet" writes it: the
/// spend path's plan over `coins`, as the file of SP-82 (no signature).
pub fn payment_of(coins: &[Coin], payments: Vec<(ScriptBuf, u64)>, change_index: u32) -> Psbt {
    let w = wallet();
    let request = SpendRequest {
        payments,
        fee_rate: 1000,
        change_index,
        min_confirmations: 1,
        send_all: false,
        must_spend: Vec::new(),
        lock_height: 0,
        change_position: ChangePosition::Last,
    };
    plan(&w, coins, &request)
        .expect("a plan")
        .to_psbt(&w)
        .expect("a file")
}

/// Two inputs (receive 2, change 4), 500,000 sats to an outside script and
/// change to change index 7.
pub fn payment() -> Psbt {
    payment_of(
        &[
            coin(1, Keychain::Receive, 2, 300_000),
            coin(2, Keychain::Change, 4, 400_000),
        ],
        vec![(external(1), 500_000)],
        7,
    )
}

/// A payment of `n` inputs, big enough to need several frames.
pub fn big_payment(n: u32) -> Psbt {
    let coins: Vec<Coin> = (0..n)
        .map(|i| coin(100 + i, Keychain::Receive, i, 100_000))
        .collect();
    payment_of(&coins, vec![(external(1), 50_000 * u64::from(n))], 9)
}

/// The refusals of SK-54 as `(name, mutated payment)`: each is the good
/// payment changed in one place.
pub fn refused_payments() -> Vec<(&'static str, Vec<u8>)> {
    let good = payment();
    let change = |f: &dyn Fn(&mut Psbt)| {
        let mut p = good.clone();
        f(&mut p);
        p.serialize()
    };
    let secp = Secp256k1::new();
    let test_master = Xpriv::new_master(Network::Testnet, &[7u8; 32]).unwrap();
    let test_xpub = Xpub::from_priv(&secp, &test_master);
    vec![
        ("NotAPsbt", b"psbt\xff not a payment".to_vec()),
        (
            "NotMainnet",
            change(&|p| {
                p.xpub.insert(
                    test_xpub,
                    (
                        test_master.fingerprint(&secp),
                        DerivationPath::from_str("m/84'/1'/0'").unwrap(),
                    ),
                );
            }),
        ),
        (
            "NoPrevout",
            change(&|p| {
                p.inputs[1].witness_utxo = None;
                p.inputs[1].non_witness_utxo = None;
            }),
        ),
        (
            "PrevoutMismatch",
            change(&|p| {
                let mut o = p.inputs[0].witness_utxo.clone().unwrap();
                o.value = Amount::from_sat(o.value.to_sat() + 1);
                p.inputs[0].witness_utxo = Some(o);
            }),
        ),
        (
            "NoPath",
            change(&|p| {
                p.inputs[0].bip32_derivation.clear();
            }),
        ),
        (
            "OtherWallet",
            change(&|p| {
                for (fp, _) in p.inputs[0].bip32_derivation.values_mut() {
                    *fp = bitcoin::bip32::Fingerprint::from([0u8; 4]);
                }
            }),
        ),
        (
            "PathNotWallet",
            change(&|p| {
                for (_, path) in p.inputs[0].bip32_derivation.values_mut() {
                    *path = DerivationPath::from_str("m/84'/0'/0'/2/0").unwrap();
                }
            }),
        ),
        (
            "KeyMismatch",
            change(&|p| {
                for (_, path) in p.inputs[0].bip32_derivation.values_mut() {
                    let mut steps: Vec<ChildNumber> = path.as_ref().to_vec();
                    let last = steps.len() - 1;
                    steps[last] = ChildNumber::Normal { index: 3 };
                    *path = DerivationPath::from(steps);
                }
            }),
        ),
        (
            "SighashNotAll",
            change(&|p| {
                p.inputs[1].sighash_type = Some(PsbtSighashType::from(
                    EcdsaSighashType::SinglePlusAnyoneCanPay,
                ));
            }),
        ),
        (
            "OutputsExceedInputs",
            change(&|p| {
                p.unsigned_tx.output[0].value = Amount::from_sat(900_000);
            }),
        ),
    ]
}

/// Six outputs (five outside, one change), so that the review has two pages.
pub fn many_outputs_payment() -> Psbt {
    payment_of(
        &[coin(1, Keychain::Receive, 2, 900_000)],
        (1..=5).map(|n| (external(n), 100_000)).collect(),
        7,
    )
}

/// The kernel's bytes: not used by the sign screens.
pub struct Zero;

impl Chance for Zero {
    fn kernel_bytes(&mut self) -> [u8; 32] {
        [0x5a; 32]
    }
}

pub fn app() -> App<Zero> {
    App::new(
        Facts {
            secure_boot: "1".into(),
            lockdown: "integrity".into(),
            interfaces: vec![],
            version: "0.1.0 test".into(),
            initrd_sha256: "ab".repeat(32),
        },
        Zero,
    )
}

pub fn press(a: &mut App<Zero>, s: &str) {
    for c in s.chars() {
        a.key(Key::Char(c));
    }
}

/// Types 12 words, each followed by Enter.
pub fn type_words(a: &mut App<Zero>, words: &[&str]) {
    for w in words {
        press(a, w);
        a.key(Key::Enter);
    }
}

/// The app after "Sign a payment" with every frame of `psbt` in `form`
/// read, standing at the typed-words screen.
pub fn scanned(psbt: &[u8], form: Form) -> App<Zero> {
    let mut a = app();
    assert_eq!(a.key(Key::Char('4')), Outcome::LoadCamera);
    a.camera_found();
    for t in texts_of(form, psbt).unwrap() {
        a.frame_text(&t);
    }
    assert_eq!(a.screen().id, "type");
    a
}

pub fn text(a: &App<Zero>) -> String {
    a.screen().lines.join("\n")
}

/// Every new screen of SK-32 to SK-58, for the test that every word of
/// `words.rs` is shown by some screen.
pub fn walk_new_screens() -> String {
    let mut seen = String::new();
    let mut note = |a: &App<Zero>| {
        seen.push_str(&text(a));
        seen.push('\n');
    };
    // SK-32, SK-33: the keys for the two wallets.
    let mut a = app();
    a.key(Key::Char('3'));
    type_words(&mut a, &WORDS);
    note(&a);
    a.key(Key::Enter);
    note(&a);
    a.key(Key::Char('f'));
    a.key(Key::Char('+'));
    note(&a);
    a.key(Key::Enter);
    a.key(Key::Char('l'));
    note(&a);
    // SK-52: reading, and no camera.
    let mut b = app();
    note(&b);
    b.key(Key::Char('4'));
    note(&b);
    b.camera_found();
    note(&b);
    let good = payment().serialize();
    let texts = texts_of(Form::Ur, &good).unwrap();
    b.frame_text(&texts[0]);
    note(&b);
    let mut c = app();
    c.key(Key::Char('4'));
    c.tick(15_000);
    note(&c);
    // SK-53 to SK-56.
    let mut d = scanned(&good, Form::Ur);
    note(&d);
    type_words(&mut d, &WORDS);
    note(&d);
    d.key(Key::Enter);
    note(&d);
    let mut e = scanned(&many_outputs_payment().serialize(), Form::Bbqr);
    type_words(&mut e, &WORDS);
    note(&e);
    e.key(Key::Down);
    note(&e);
    // SK-54: every refusal.
    for (_, bytes) in refused_payments() {
        let mut f = app();
        f.key(Key::Char('4'));
        f.camera_found();
        for t in texts_of(Form::Ur, &bytes).unwrap_or_default() {
            f.frame_text(&t);
        }
        if f.screen().id == "type" {
            type_words(&mut f, &WORDS);
        }
        note(&f);
    }
    seen
}
