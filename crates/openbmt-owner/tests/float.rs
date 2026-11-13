//! The refund float (`docs/FLOAT-RULES.md`, FL-01 to FL-40): the key from
//! 12 words, the cap, a refund paid from the float, a refill, and turning
//! the float off.
//!
//! TEST ONLY, NEVER USE: every key here comes from published or fixed test
//! words.

mod common;

use std::str::FromStr;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::{hmac, sha512, Hash, HashEngine};
use bitcoin::transaction::Version;
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    Witness,
};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_owner::float::*;
use openbmt_owner::own_shop::own_approver;
use openbmt_owner::{Options, Owner};
use openbmt_spend::Coin;

/// The BIP-85 specification's master key and its first 12-word child.
const BIP85_MASTER: &str = "xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb";
const BIP85_CHILD_0: &str =
    "girl mad pet galaxy egg matter matrix prison refuse sense ordinary nose";
const ABANDON: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

/// BIP-85 for 12 BIP39 words: `m/83696968'/39'/0'/12'/<index>'`, the first
/// 16 bytes of HMAC-SHA512 with the key "bip-entropy-from-k". Here only, to
/// check the vectors; the library never takes the shop's own words.
fn bip85_words(master: &str, index: u32) -> String {
    let master = Xpriv::from_str(master).unwrap();
    let path = DerivationPath::from_str(&format!("m/83696968'/39'/0'/12'/{index}'")).unwrap();
    let child = master.derive_priv(secp(), &path).unwrap();
    let mut engine = hmac::HmacEngine::<sha512::Hash>::new(b"bip-entropy-from-k");
    engine.input(&child.private_key.secret_bytes());
    let digest = hmac::Hmac::<sha512::Hash>::from_engine(engine).to_byte_array();
    let m = bip39::Mnemonic::from_entropy(&digest[..16]).unwrap();
    m.words().collect::<Vec<_>>().join(" ")
}

fn float_words() -> FloatWords {
    FloatWords::new(BIP85_CHILD_0).unwrap()
}

fn float() -> FloatWallet {
    FloatWallet::from_words(&float_words()).unwrap()
}

/// A coin of the float: `sats` at `chain/index`, `confirmations` deep.
fn float_coin(tag: u32, chain: Keychain, index: u32, sats: u64, confirmations: u32) -> Coin {
    let script = float().script(chain, index).unwrap();
    let mut id = [0u8; 32];
    id[..4].copy_from_slice(&tag.to_le_bytes());
    let prev = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array(id),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: script,
        }],
    };
    Coin {
        outpoint: OutPoint {
            txid: prev.compute_txid(),
            vout: 0,
        },
        prev_tx: prev,
        vout: 0,
        chain,
        index,
        confirmations,
    }
}

fn pay_to() -> ScriptBuf {
    external(7)
}

const RATE: u64 = 2_000;

// ---- FL-01, FL-02 ----------------------------------------------------------

#[test]
fn fl_01_the_bip85_spec_vector_gives_the_child_words() {
    assert_eq!(bip85_words(BIP85_MASTER, 0), BIP85_CHILD_0);
    // The library takes those words, and refuses anything that is not 12
    // English words with a right check word.
    assert!(FloatWords::new(BIP85_CHILD_0).is_ok());
    assert!(FloatWords::new(&BIP85_CHILD_0.to_uppercase()).is_ok());
    assert_eq!(
        FloatWords::new("girl mad pet galaxy egg matter matrix prison refuse sense ordinary")
            .unwrap_err(),
        FloatRefusal::Words
    );
    assert_eq!(
        FloatWords::new("girl mad pet galaxy egg matter matrix prison refuse sense ordinary girl")
            .unwrap_err(),
        FloatRefusal::Words
    );
    assert_eq!(
        FloatWords::new("girl mad pet galaxy egg matter matrix prison refuse sense ordinary xyzzy")
            .unwrap_err(),
        FloatRefusal::Words
    );
}

#[test]
fn fl_01_a_compact_seedqr_gives_the_words() {
    let entropy = bip39::Mnemonic::parse(BIP85_CHILD_0).unwrap().to_entropy();
    let words = FloatWords::from_compact_seedqr(&entropy).unwrap();
    assert_eq!(words.as_str(), BIP85_CHILD_0);
    assert_eq!(
        FloatWords::from_compact_seedqr(&entropy[..15]).unwrap_err(),
        FloatRefusal::Words
    );
    assert_eq!(
        FloatWords::from_compact_seedqr(&[0u8; 32]).unwrap_err(),
        FloatRefusal::Words
    );
}

#[test]
fn fl_02_the_first_address_of_known_words() {
    // The BIP84 specification's first receive address of the abandon words.
    let w = FloatWallet::from_words(&FloatWords::new(ABANDON).unwrap()).unwrap();
    assert_eq!(
        w.receive_address(0).unwrap().to_string(),
        "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
    );
    assert_eq!(
        w.receive_address(1).unwrap().to_string(),
        "bc1qnjg0jd8228aq7egyzacy8cys3knf9xvrerkf9g"
    );
}

#[test]
fn fl_02_the_child_words_are_a_plain_account_zero_wallet_with_no_passphrase() {
    // The same wallet from an independent derivation: seed with no
    // passphrase, then m/84'/0'/0'/0/<i>.
    let seed = bip39::Mnemonic::parse(BIP85_CHILD_0).unwrap().to_seed("");
    let master = Xpriv::new_master(Network::Bitcoin, &seed).unwrap();
    let account = master
        .derive_priv(secp(), &DerivationPath::from_str("m/84'/0'/0'").unwrap())
        .unwrap();
    let independent = SingleKeyWallet::new(
        master.fingerprint(secp()),
        0,
        Xpub::from_priv(secp(), &account),
    )
    .unwrap();
    let w = float();
    assert_eq!(w.description(), independent.canonical_string());
    for i in 0..3 {
        assert_eq!(
            w.receive_address(i).unwrap(),
            independent.address(Keychain::Receive, i).unwrap()
        );
    }
    // The description is public and reads back.
    assert_eq!(FloatWallet::parse(w.description()).unwrap(), w);
    // Another account is not the float.
    let other = SingleKeyWallet::new(
        master.fingerprint(secp()),
        1,
        Xpub::from_priv(
            secp(),
            &master
                .derive_priv(secp(), &DerivationPath::from_str("m/84'/0'/1'").unwrap())
                .unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(
        FloatWallet::parse(other.canonical_string()).unwrap_err(),
        FloatRefusal::Wallet
    );
}

// ---- FL-03 -------------------------------------------------------------------

#[test]
fn fl_03_the_words_are_hidden_and_not_in_what_comes_back() {
    let words = float_words();
    assert!(!format!("{words:?}").contains("girl"));
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(&words, &coins, &pay_to(), 30_000, RATE, 0, 1, 0).unwrap();
    let text = format!("{paid:?}");
    assert!(!text.contains("galaxy"));
    // The wallet the app keeps holds no secret: the description is an xpub.
    assert!(float().description().contains("xpub"));
    assert!(!float().description().contains("xprv"));
}

// ---- FL-10, FL-11, FL-12 ----------------------------------------------------

#[test]
fn fl_10_a_cap_of_zero_is_a_float_that_is_off() {
    assert_eq!(refill_amount(0, 0, 0).unwrap_err(), FloatRefusal::NoCap);
    assert!(!is_low(0, 0, 0));
}

#[test]
fn fl_11_the_refill_is_the_cap_less_what_is_held_counting_unconfirmed() {
    assert_eq!(refill_amount(1_000_000, 0, 0), Ok(1_000_000));
    assert_eq!(refill_amount(1_000_000, 300_000, 0), Ok(700_000));
    assert_eq!(refill_amount(1_000_000, 300_000, 200_000), Ok(500_000));
    assert_eq!(refill_amount(1_000_000, 0, 999_999), Ok(1));
}

#[test]
fn fl_11_at_the_cap_or_above_there_is_nothing_to_refill() {
    for (c, u) in [
        (1_000_000, 0),
        (600_000, 400_000),
        (1_500_000, 0),
        (0, 1_000_001),
    ] {
        assert_eq!(
            refill_amount(1_000_000, c, u).unwrap_err(),
            FloatRefusal::NothingToRefill
        );
    }
    // Sums that overflow are refused, not wrapped.
    assert_eq!(
        refill_amount(1_000_000, u64::MAX, 1).unwrap_err(),
        FloatRefusal::Amount
    );
}

#[test]
fn fl_11_a_refill_above_the_cap_is_refused_and_one_to_the_cap_is_not() {
    assert_eq!(check_refill_amount(1_000_000, 300_000, 0, 700_000), Ok(()));
    assert_eq!(check_refill_amount(1_000_000, 300_000, 0, 1), Ok(()));
    assert_eq!(
        check_refill_amount(1_000_000, 300_000, 0, 700_001).unwrap_err(),
        FloatRefusal::AboveCap {
            room: 700_000,
            asked: 700_001
        }
    );
    // What is on its way counts.
    assert_eq!(
        check_refill_amount(1_000_000, 300_000, 400_000, 300_001).unwrap_err(),
        FloatRefusal::AboveCap {
            room: 300_000,
            asked: 300_001
        }
    );
    assert_eq!(
        check_refill_amount(1_000_000, 300_000, 0, 0).unwrap_err(),
        FloatRefusal::Amount
    );
}

#[test]
fn fl_11_lowering_the_cap_leaves_the_float_and_the_next_refill_waits() {
    // The float holds 800,000; the cap goes to 500,000.
    assert_eq!(
        refill_amount(500_000, 800_000, 0).unwrap_err(),
        FloatRefusal::NothingToRefill
    );
    // After refunds take it to 300,000 the refill is the 200,000 of room.
    assert_eq!(refill_amount(500_000, 300_000, 0), Ok(200_000));
}

#[test]
fn fl_12_low_is_below_a_quarter_of_the_cap() {
    assert!(is_low(1_000_000, 249_999, 0));
    assert!(!is_low(1_000_000, 250_000, 0));
    assert!(!is_low(1_000_000, 1_000_000, 0));
    // Not rounded: a quarter of 10 sat is 2.5.
    assert!(is_low(10, 2, 0));
    assert!(!is_low(10, 3, 0));
    // A payment on its way counts.
    assert!(!is_low(1_000_000, 100_000, 150_000));
    assert!(is_low(1_000_000, 0, 0));
    // No wrap on the largest numbers.
    assert!(!is_low(u64::MAX, u64::MAX, u64::MAX));
    let w = low_words();
    assert_eq!(w.code, Some("FL-12"));
    assert_eq!(w.heading, "Refund float low");
}

// ---- FL-22 --------------------------------------------------------------------

#[test]
fn fl_22_the_refund_fee_rate_is_the_recent_rate_at_least_one_sat_per_vb() {
    assert_eq!(refund_fee_rate(Some(4_500), None), Ok(4_500));
    assert_eq!(refund_fee_rate(Some(300), Some(9_000)), Ok(1_000));
    assert_eq!(refund_fee_rate(None, Some(2_500)), Ok(2_500));
    assert_eq!(
        refund_fee_rate(None, None).unwrap_err(),
        FloatRefusal::NoFeeRate
    );
    assert_eq!(
        refund_fee_rate(None, Some(0)).unwrap_err(),
        FloatRefusal::NoFeeRate
    );
}

// ---- FL-20, FL-21, FL-23 -------------------------------------------------------

#[test]
fn fl_20_a_refund_is_paid_from_the_floats_coins_with_change_back_to_the_float() {
    let coins = [
        float_coin(1, Keychain::Receive, 0, 60_000, 6),
        float_coin(2, Keychain::Receive, 1, 90_000, 3),
    ];
    let paid = pay_refund(
        &float_words(),
        &coins,
        &pay_to(),
        40_000,
        RATE,
        5,
        1,
        800_000,
    )
    .unwrap();
    let tx = &paid.tx;
    assert!(tx.input.iter().all(|i| !i.witness.is_empty()));
    assert!(tx.input.iter().all(|i| i.witness.len() == 2));
    assert_eq!(tx.lock_time, LockTime::from_height(800_000).unwrap());
    assert_eq!(paid.txid, tx.compute_txid());
    // The refund is paid exactly; the change goes to the float's change
    // script at the index given.
    let refund = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == pay_to())
        .unwrap();
    assert_eq!(refund.value.to_sat(), 40_000);
    assert_eq!(paid.paid, 40_000);
    let change_script = float().script(Keychain::Change, 5).unwrap();
    let change = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == change_script)
        .unwrap();
    assert_eq!(paid.change, Some((5, change.value.to_sat())));
    assert_eq!(tx.output.len(), 2);
    // Nothing is lost: inputs = outputs + fee.
    let spent: u64 = tx
        .input
        .iter()
        .map(|i| {
            coins
                .iter()
                .find(|c| c.outpoint == i.previous_output)
                .unwrap()
                .output()
                .unwrap()
                .value
                .to_sat()
        })
        .sum();
    let out: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
    assert_eq!(spent, out + paid.fee);
    // FL-22: at least the asked rate.
    assert!(paid.fee * 1000 >= RATE * paid.vsize);
    assert_eq!(paid.spent.len(), tx.input.len());
}

#[test]
fn fl_20_a_coin_that_is_not_confirmed_enough_is_not_spent() {
    let coins = [
        float_coin(1, Keychain::Receive, 0, 500_000, 0),
        float_coin(2, Keychain::Change, 0, 60_000, 2),
    ];
    // With 1 confirmation required only the second can pay.
    let paid = pay_refund(&float_words(), &coins, &pay_to(), 30_000, RATE, 1, 1, 0).unwrap();
    assert_eq!(paid.spent, vec![coins[1].outpoint]);
    // With 0 the larger coin is also eligible, and a larger refund needs it.
    let paid = pay_refund(&float_words(), &coins, &pay_to(), 70_000, RATE, 1, 0, 0).unwrap();
    assert!(paid.spent.contains(&coins[0].outpoint));
}

#[test]
fn fl_20_the_signature_is_sighash_all_and_verifies() {
    let coins = [float_coin(1, Keychain::Receive, 3, 100_000, 6)];
    let paid = pay_refund(&float_words(), &coins, &pay_to(), 20_000, RATE, 0, 1, 0).unwrap();
    let witness = &paid.tx.input[0].witness;
    let sig = witness.nth(0).unwrap();
    assert_eq!(*sig.last().unwrap(), 0x01);
    let key = bitcoin::PublicKey::from_slice(witness.nth(1).unwrap()).unwrap();
    let signature = bitcoin::secp256k1::ecdsa::Signature::from_der(&sig[..sig.len() - 1]).unwrap();
    let mut cache = bitcoin::sighash::SighashCache::new(&paid.tx);
    let hash = cache
        .p2wpkh_signature_hash(
            0,
            &coins[0].output().unwrap().script_pubkey,
            coins[0].output().unwrap().value,
            bitcoin::sighash::EcdsaSighashType::All,
        )
        .unwrap();
    let msg = bitcoin::secp256k1::Message::from_digest(hash.to_byte_array());
    secp().verify_ecdsa(&msg, &signature, &key.inner).unwrap();
    // The key is the float's key at receive 3.
    assert_eq!(
        ScriptBuf::new_p2wpkh(&key.wpubkey_hash().unwrap()),
        float().script(Keychain::Receive, 3).unwrap()
    );
}

#[test]
fn fl_20_the_float_cannot_sign_for_coins_of_another_wallet() {
    // A coin of the shop's own wallet (not the float's) is refused.
    let foreign = coin(9, 0, 100_000);
    let e = pay_refund(&float_words(), &[foreign], &pay_to(), 20_000, RATE, 0, 1, 0).unwrap_err();
    assert!(matches!(e, FloatRefusal::NotFloatCoin(_)), "{e:?}");
    assert_eq!(e.rule(), "FL-21");
}

#[test]
fn fl_21_the_finished_transaction_pays_only_the_refund_and_the_floats_change() {
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(&float_words(), &coins, &pay_to(), 30_000, RATE, 2, 1, 0).unwrap();
    let w = float();
    let single = SingleKeyWallet::parse(w.description()).unwrap();
    let change = w.script(Keychain::Change, 2).unwrap();
    let refs: Vec<&Coin> = coins.iter().collect();
    let check = |tx: &Transaction, to: &ScriptBuf, sats: Option<u64>, sweep: bool| {
        check_float_only(&single, &refs, tx, to, sats, &change, sweep)
    };
    assert_eq!(check(&paid.tx, &pay_to(), Some(30_000), false), Ok(()));

    // An extra output to someone else.
    let mut extra = paid.tx.clone();
    extra.output.push(TxOut {
        value: Amount::from_sat(1_000),
        script_pubkey: external(8),
    });
    assert!(matches!(
        check(&extra, &pay_to(), Some(30_000), false),
        Err(FloatRefusal::NotOnlyFloat(_))
    ));
    // The change redirected to someone else.
    let mut stolen = paid.tx.clone();
    for o in &mut stolen.output {
        if o.script_pubkey == change {
            o.script_pubkey = external(8);
        }
    }
    assert!(matches!(
        check(&stolen, &pay_to(), Some(30_000), false),
        Err(FloatRefusal::NotOnlyFloat(_))
    ));
    // A refund of another amount than the queued one.
    assert!(matches!(
        check(&paid.tx, &pay_to(), Some(29_999), false),
        Err(FloatRefusal::NotOnlyFloat(_))
    ));
    // An input that is not a coin of the float.
    let mut other_input = paid.tx.clone();
    other_input.input[0].previous_output.vout = 7;
    assert!(matches!(
        check(&other_input, &pay_to(), Some(30_000), false),
        Err(FloatRefusal::NotOnlyFloat(_))
    ));
    // The destination paid twice, or not at all.
    let mut twice = paid.tx.clone();
    let again = twice
        .output
        .iter()
        .find(|o| o.script_pubkey == pay_to())
        .unwrap()
        .clone();
    twice.output.push(again);
    assert!(check(&twice, &pay_to(), Some(30_000), false).is_err());
    assert!(check(&paid.tx, &external(9), None, false).is_err());
    // Two change outputs.
    let mut two = paid.tx.clone();
    let c = two
        .output
        .iter()
        .find(|o| o.script_pubkey == change)
        .unwrap()
        .clone();
    two.output.push(c);
    assert!(check(&two, &pay_to(), Some(30_000), false).is_err());
    // A payment to the float itself is no refund.
    assert!(check(&paid.tx, &change, None, false).is_err());
}

#[test]
fn fl_23_a_refund_larger_than_the_float_is_refused_and_never_split() {
    let coins = [
        float_coin(1, Keychain::Receive, 0, 40_000, 6),
        float_coin(2, Keychain::Receive, 1, 50_000, 6),
    ];
    // 90,000 held; a refund of 89,900 leaves too little for the fee.
    let e = pay_refund(&float_words(), &coins, &pay_to(), 89_900, RATE, 0, 1, 0).unwrap_err();
    match e {
        FloatRefusal::NotEnough { available, needed } => {
            assert_eq!(available, 90_000);
            assert!(needed > available);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(e.rule(), "FL-23");
    // Far above.
    assert!(matches!(
        pay_refund(&float_words(), &coins, &pay_to(), 500_000, RATE, 0, 1, 0),
        Err(FloatRefusal::NotEnough { .. })
    ));
    // No coins at all, or none deep enough.
    assert!(matches!(
        pay_refund(&float_words(), &[], &pay_to(), 10_000, RATE, 0, 1, 0),
        Err(FloatRefusal::NotEnough { available: 0, .. })
    ));
    let shallow = [float_coin(3, Keychain::Receive, 0, 90_000, 0)];
    assert!(matches!(
        pay_refund(&float_words(), &shallow, &pay_to(), 10_000, RATE, 0, 1, 0),
        Err(FloatRefusal::NotEnough { .. })
    ));
    // A refund that fits is paid by one transaction.
    let ok = pay_refund(&float_words(), &coins, &pay_to(), 60_000, RATE, 0, 1, 0).unwrap();
    assert_eq!(
        ok.tx
            .output
            .iter()
            .filter(|o| o.script_pubkey == pay_to())
            .count(),
        1
    );
}

#[test]
fn fl_21_a_refund_to_the_floats_own_script_is_refused() {
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let own = float().script(Keychain::Receive, 4).unwrap();
    // The planner refuses a payment to one of the wallet's own scripts
    // only for scripts it knows; the float's change script is one.
    let change = float().script(Keychain::Change, 0).unwrap();
    assert!(pay_refund(&float_words(), &coins, &change, 30_000, RATE, 0, 1, 0).is_err());
    let _ = own;
}

// ---- FL-30 ---------------------------------------------------------------------

fn shop_single() -> (SingleKeyWallet, ImportedWallet) {
    let master = Xpriv::new_master(Network::Bitcoin, &[0x77; 32]).unwrap();
    let account = master
        .derive_priv(secp(), &DerivationPath::from_str("m/84'/0'/0'").unwrap())
        .unwrap();
    let w = SingleKeyWallet::new(
        master.fingerprint(secp()),
        0,
        Xpub::from_priv(secp(), &account),
    )
    .unwrap();
    (w.clone(), ImportedWallet::Single(w))
}

fn own_owner() -> Owner {
    Owner::new(
        &OWNER_SECRET,
        shop_single().1,
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        Options::default(),
    )
    .unwrap()
}

fn ok_store(_: &[u8]) -> Result<(), String> {
    Ok(())
}

#[test]
fn fl_30_a_refill_is_queued_as_a_payment_to_the_floats_next_receive_address() {
    let mut owner = own_owner();
    let queued = prepare_refill(
        &mut owner,
        &float(),
        3,
        1_000_000,
        300_000,
        0,
        None,
        ok_store,
    )
    .unwrap();
    assert_eq!(queued.sats, 700_000);
    assert_eq!(queued.address, float().receive_address(3).unwrap());
    let entry = owner
        .cash_outs()
        .iter()
        .find(|c| c.id == queued.id)
        .unwrap();
    assert_eq!(entry.script, queued.address.script_pubkey());
    assert_eq!(entry.label, REFILL_LABEL);
    assert_eq!(entry.amount, openbmt_owner::CashAmount::Sats(700_000));
    assert_eq!(entry.stage, openbmt_owner::Stage::Queued);
}

#[test]
fn fl_30_the_owner_may_refill_less_but_not_above_the_cap() {
    let mut owner = own_owner();
    let q = prepare_refill(
        &mut owner,
        &float(),
        0,
        1_000_000,
        300_000,
        0,
        Some(200_000),
        ok_store,
    )
    .unwrap();
    assert_eq!(q.sats, 200_000);
    // Above the cap, and at the cap: nothing is queued.
    let before = owner.cash_outs().len();
    for (confirmed, amount, expect_nothing) in
        [(300_000, Some(700_001), false), (1_000_000, None, true)]
    {
        let e = prepare_refill(
            &mut owner,
            &float(),
            0,
            1_000_000,
            confirmed,
            0,
            amount,
            ok_store,
        )
        .unwrap_err();
        match e {
            RefillError::Float(FloatRefusal::AboveCap { .. }) => assert!(!expect_nothing),
            RefillError::Float(FloatRefusal::NothingToRefill) => assert!(expect_nothing),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(owner.cash_outs().len(), before);
    // A failing store queues nothing and is passed on.
    let e = prepare_refill(
        &mut owner,
        &float(),
        0,
        1_000_000,
        0,
        0,
        None,
        |_: &[u8]| Err::<(), _>("disk full".to_string()),
    )
    .unwrap_err();
    assert!(matches!(e, RefillError::Owner(_)));
    assert_eq!(owner.cash_outs().len(), before);
}

// ---- FL-35 ---------------------------------------------------------------------

#[test]
fn fl_35_turning_off_pays_the_shops_own_address_from_every_coin() {
    let coins = [
        float_coin(1, Keychain::Receive, 0, 50_000, 6),
        float_coin(2, Keychain::Receive, 1, 70_000, 2),
        float_coin(3, Keychain::Change, 0, 30_000, 1),
    ];
    let (_, shop) = shop_single();
    let to = shop_receive_script(&shop, 4).unwrap();
    assert_eq!(to, shop.receive_script(4).unwrap());
    let paid = sweep(&float_words(), &coins, &to, RATE, 1, 0).unwrap();
    assert_eq!(paid.tx.output.len(), 1);
    assert_eq!(paid.tx.output[0].script_pubkey, to);
    assert_eq!(paid.tx.input.len(), 3);
    assert_eq!(paid.change, None);
    assert_eq!(paid.paid + paid.fee, 150_000);
    assert!(paid.fee * 1000 >= RATE * paid.vsize);
    assert!(paid.tx.input.iter().all(|i| i.witness.len() == 2));
}

#[test]
fn fl_35_the_shops_address_comes_from_either_kind_of_pinned_wallet() {
    let multi = ImportedWallet::Multisig(wallet().clone());
    let script = shop_receive_script(&multi, 2).unwrap();
    assert_eq!(script, multi.receive_script(2).unwrap());
    // A 2-of-3 shop receives the sweep too.
    let coins = [float_coin(1, Keychain::Receive, 0, 80_000, 6)];
    let paid = sweep(&float_words(), &coins, &script, RATE, 1, 0).unwrap();
    assert_eq!(paid.tx.output[0].script_pubkey, script);
}

#[test]
fn fl_35_a_float_with_nothing_has_nothing_to_move() {
    let to = external(5);
    assert_eq!(
        sweep(&float_words(), &[], &to, RATE, 1, 0).unwrap_err(),
        FloatRefusal::NothingToSweep
    );
    // Coins too small to pay their own fee.
    let dust = [float_coin(1, Keychain::Receive, 0, 200, 6)];
    assert!(sweep(&float_words(), &dust, &to, RATE, 1, 0).is_err());
}

#[test]
fn fl_35_a_sweep_that_leaves_a_coin_out_fails_the_check() {
    let coins = [
        float_coin(1, Keychain::Receive, 0, 50_000, 6),
        float_coin(2, Keychain::Receive, 1, 70_000, 6),
    ];
    let to = external(5);
    let paid = sweep(&float_words(), &coins, &to, RATE, 1, 0).unwrap();
    let single = SingleKeyWallet::parse(float().description()).unwrap();
    let change = float().script(Keychain::Change, 0).unwrap();
    let all: Vec<&Coin> = coins.iter().collect();
    assert_eq!(
        check_float_only(&single, &all, &paid.tx, &to, None, &change, true),
        Ok(())
    );
    let mut one = paid.tx.clone();
    one.input.pop();
    assert!(check_float_only(&single, &all, &one, &to, None, &change, true).is_err());
    // A change output has no place in a sweep.
    let mut with_change = paid.tx.clone();
    with_change.output.push(TxOut {
        value: Amount::from_sat(500),
        script_pubkey: change.clone(),
    });
    assert!(check_float_only(&single, &all, &with_change, &to, None, &change, true).is_err());
}

// ---- words ---------------------------------------------------------------------

#[test]
fn fl_words_every_state_has_its_own_words_and_names_its_rule() {
    let all = all_float_words();
    assert!(all.len() >= 19);
    // FL-06 is a placeholder until its words are written (OA-114).
    let placeholders: Vec<&str> = all
        .iter()
        .filter(|(_, w)| openbmt_owner::own_say::is_placeholder(w))
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(placeholders, ["FloatRefusal/NotThisFloat"]);
    let all: Vec<_> = all
        .into_iter()
        .filter(|(_, w)| !openbmt_owner::own_say::is_placeholder(w))
        .collect();
    for (name, w) in &all {
        assert!(w.heading.chars().count() < 28, "{name}");
        assert_ne!(w.heading, w.what, "{name}");
        assert!(w.code.is_some_and(|c| c.starts_with("FL-")), "{name}");
    }
    for (i, (a, x)) in all.iter().enumerate() {
        for (b, y) in all.iter().skip(i + 1) {
            assert_ne!(x.heading, y.heading, "{a} and {b}");
            assert_ne!(x.what, y.what, "{a} and {b}");
        }
    }
    for (name, r) in all_refusals() {
        assert!(r.to_string().starts_with(r.rule()), "{name}");
    }
    assert_eq!(
        refill_linked_words().heading,
        "A refill links your wallets",
        "FL-31"
    );
}

#[test]
fn fl_address_type_is_mainnet_native_segwit() {
    let a: Address = float().receive_address(0).unwrap();
    assert!(a.to_string().starts_with("bc1q"));
}

/// Kernel reads that are fixed, never weak and never repeated.
struct Steady(u8);

impl openbmt_owner::starter::KernelRandom for Steady {
    fn read32(&mut self) -> Option<[u8; 32]> {
        self.0 = self.0.wrapping_add(1);
        let step = self.0;
        Some(std::array::from_fn(|i| {
            step.wrapping_mul(31).wrapping_add(i as u8)
        }))
    }
}

#[test]
fn fl_03_a_scanned_seedqr_is_sealed_and_opens_to_the_floats_own_wallet() {
    let entropy = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, BIP85_CHILD_0)
        .unwrap()
        .to_entropy();
    let made = seal_compact_seedqr(&entropy, &mut Steady(0)).unwrap();
    let opened = wallet_of_sealed(&made.sealed.sealed, &made.sealed.data_key).unwrap();
    let float = FloatWallet::from_words(&float_words()).unwrap();
    assert_eq!(made.wallet, float);
    assert_eq!(opened, float);
    // A scan of another length is refused.
    assert_eq!(
        seal_compact_seedqr(&entropy[..15], &mut Steady(0)).unwrap_err(),
        FloatSealError::Float(FloatRefusal::Words)
    );
}

/// A starter wallet's sealed seed of the words `BIP85_CHILD_0`, made through the restore.
fn starter_blob() -> openbmt_owner::starter::Sealed {
    use openbmt_owner::starter::{Key, Restore, Seed};
    let m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, BIP85_CHILD_0).unwrap();
    let seed = Seed::from_mnemonic(&m).unwrap();
    let description = seed.wallet().unwrap().description().to_string();
    let mut restore = Restore::new();
    for word in BIP85_CHILD_0.split(' ') {
        for b in word.bytes() {
            restore.key(Key::Letter(b)).unwrap();
        }
        restore.key(Key::Enter).unwrap();
    }
    restore.finish(&description, &mut Steady(0)).unwrap()
}

#[test]
fn fl_03_the_float_path_refuses_a_sealed_starter_seed() {
    let starter = starter_blob();
    // The blob is good for the starter's own path, so the refusal is the label.
    assert!(openbmt_owner::starter::wallet_of_sealed(&starter.sealed, &starter.data_key).is_ok());
    assert!(wallet_of_sealed(&starter.sealed, &starter.data_key).is_err());
}

#[test]
fn ss_20_the_starter_path_refuses_a_sealed_float() {
    let entropy = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, BIP85_CHILD_0)
        .unwrap()
        .to_entropy();
    let made = seal_compact_seedqr(&entropy, &mut Steady(0)).unwrap();
    assert!(
        openbmt_owner::starter::wallet_of_sealed(&made.sealed.sealed, &made.sealed.data_key)
            .is_err()
    );
}

// ---- FL-06: the key brought back --------------------------------------------

fn entropy_of(words: &str) -> Vec<u8> {
    bip39::Mnemonic::parse_in_normalized(bip39::Language::English, words)
        .unwrap()
        .to_entropy()
}

#[test]
fn fl_06_the_right_words_for_the_recorded_wallet_are_taken_and_sealed_anew() {
    let recorded = float();
    let first = seal_compact_seedqr(&entropy_of(BIP85_CHILD_0), &mut Steady(0)).unwrap();
    // Scanned again, with another stream of random bytes: a new data key.
    let again =
        restore_compact_seedqr(&entropy_of(BIP85_CHILD_0), &recorded, &mut Steady(40)).unwrap();
    assert_eq!(again.wallet, recorded);
    assert_ne!(again.sealed.data_key.as_bytes(), first.sealed.data_key.as_bytes());
    assert_ne!(again.sealed.sealed, first.sealed.sealed);
    let opened = wallet_of_sealed(&again.sealed.sealed, &again.sealed.data_key).unwrap();
    assert_eq!(opened, recorded);
    // The old key does not open the new seal.
    assert!(wallet_of_sealed(&again.sealed.sealed, &first.sealed.data_key).is_err());
}

#[test]
fn fl_06_words_of_another_index_or_another_shop_are_refused() {
    let recorded = float();
    // The same shop, the next index.
    let next = bip85_words(BIP85_MASTER, 1);
    assert_ne!(next, BIP85_CHILD_0);
    let refused = restore_compact_seedqr(&entropy_of(&next), &recorded, &mut Steady(0));
    assert_eq!(
        refused.unwrap_err(),
        FloatSealError::Float(FloatRefusal::NotThisFloat)
    );
    // Another shop's words at the same index.
    let other = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    assert_eq!(
        restore_compact_seedqr(&entropy_of(other), &recorded, &mut Steady(0)).unwrap_err(),
        FloatSealError::Float(FloatRefusal::NotThisFloat)
    );
    assert_eq!(FloatRefusal::NotThisFloat.rule(), "FL-06");
    assert!(FloatRefusal::NotThisFloat.to_string().starts_with("FL-06"));
    // A scan that is not 12 words is the setup's refusal, as before.
    assert_eq!(
        restore_compact_seedqr(&entropy_of(BIP85_CHILD_0)[..15], &recorded, &mut Steady(0))
            .unwrap_err(),
        FloatSealError::Float(FloatRefusal::Words)
    );
    // A scan with a kernel that gives nothing seals nothing.
    struct Dry;
    impl openbmt_owner::starter::KernelRandom for Dry {
        fn read32(&mut self) -> Option<[u8; 32]> {
            None
        }
    }
    assert!(matches!(
        restore_compact_seedqr(&entropy_of(BIP85_CHILD_0), &recorded, &mut Dry),
        Err(FloatSealError::Seal(_))
    ));
}

#[test]
fn fl_06_the_words_of_the_refusal_are_the_placeholder_that_names_its_rule() {
    let w = FloatRefusal::NotThisFloat.words();
    assert!(openbmt_owner::own_say::is_placeholder(&w));
    assert_eq!(w.heading, "WORDS_NEEDED_FL06_HEADING");
    assert_eq!(w.what, "WORDS_NEEDED_FL06_WHAT");
    assert_eq!(w.next, "WORDS_NEEDED_FL06_NEXT");
    assert_eq!(w.code, Some("FL-06"));
}

#[test]
fn fl_06_the_restored_seal_signs_a_refund() {
    let recorded = float();
    let made =
        restore_compact_seedqr(&entropy_of(BIP85_CHILD_0), &recorded, &mut Steady(7)).unwrap();
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund_sealed(
        &made.sealed.sealed,
        &made.sealed.data_key,
        &coins,
        &external(1),
        40_000,
        RATE,
        0,
        1,
        0,
    )
    .unwrap();
    assert_eq!(paid.paid, 40_000);
    // The same payment as the one signed from the words themselves.
    let direct = pay_refund(&float_words(), &coins, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    assert_eq!(paid.txid, direct.txid);
    // And the move of everything.
    let moved = sweep_sealed(
        &made.sealed.sealed,
        &made.sealed.data_key,
        &coins,
        &external(5),
        RATE,
        1,
        0,
    )
    .unwrap();
    assert_eq!(moved.paid + moved.fee, 100_000);
}

// ---- FL-20, FL-21, FL-35: the outbox ----------------------------------------

fn float_points(coins: &[Coin]) -> Vec<OutPoint> {
    coins.iter().map(|c| c.outpoint).collect()
}

fn float_scripts() -> impl Fn(&ScriptBuf) -> bool {
    let all: Vec<ScriptBuf> = (0..4)
        .flat_map(|i| [Keychain::Receive, Keychain::Change].map(|c| float().script(c, i).unwrap()))
        .collect();
    move |s| all.contains(s)
}

#[test]
fn fl_20_the_outbox_takes_a_float_payment_of_a_ready_refund_and_marks_it_signed() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let r = refund(&mut o, 1, 100_000, 40_000);
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(&float_words(), &coins, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    let bytes = bitcoin::consensus::serialize(&paid.tx);
    let store = Store::default();
    let txid = o
        .take_float(
            &bytes,
            &[r],
            &float_points(&coins),
            &float_scripts(),
            None,
            |b| store.put(b),
        )
        .unwrap();
    assert_eq!(txid, paid.txid);
    assert!(matches!(
        o.refunds().iter().find(|x| x.id == r).unwrap().stage,
        openbmt_owner::Stage::Signed { .. }
    ));
    assert_eq!(o.outbox().len(), 1);
    // Giving it again changes nothing.
    let calls = store.calls();
    assert_eq!(
        o.take_float(
            &bytes,
            &[r],
            &float_points(&coins),
            &float_scripts(),
            None,
            |b| store.put(b)
        )
        .unwrap(),
        txid
    );
    assert_eq!(store.calls(), calls);
}

#[test]
fn fl_21_the_outbox_refuses_a_float_payment_that_spends_a_coin_outside_the_float() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let r = refund(&mut o, 1, 100_000, 40_000);
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(&float_words(), &coins, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    let bytes = bitcoin::consensus::serialize(&paid.tx);
    // The view holds another coin only: the spent one is not the float's.
    let other = [float_coin(2, Keychain::Receive, 1, 100_000, 6)];
    let e = o.take_float(
        &bytes,
        &[r],
        &float_points(&other),
        &float_scripts(),
        None,
        nop,
    );
    assert!(e.is_err());
    assert!(o.outbox().is_empty());
}

#[test]
fn fl_21_the_outbox_refuses_a_float_payment_that_pays_something_else() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let r = refund(&mut o, 1, 100_000, 40_000);
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(&float_words(), &coins, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    let mut tx = paid.tx.clone();
    tx.output.push(TxOut {
        value: Amount::from_sat(1_000),
        script_pubkey: external(99),
    });
    let e = o.take_float(
        &bitcoin::consensus::serialize(&tx),
        &[r],
        &float_points(&coins),
        &float_scripts(),
        None,
        nop,
    );
    assert!(e.is_err());
    // A refund of another amount than the one paid is not that refund.
    let mut o2 = new_owner(openbmt_owner::Options::default());
    let r2 = refund(&mut o2, 1, 100_000, 39_000);
    let e = o2.take_float(
        &bitcoin::consensus::serialize(&paid.tx),
        &[r2],
        &float_points(&coins),
        &float_scripts(),
        None,
        nop,
    );
    assert!(e.is_err());
    assert!(o.outbox().is_empty() && o2.outbox().is_empty());
}

#[test]
fn fl_20_the_outbox_refuses_a_float_payment_of_a_refund_that_is_not_ready_or_not_queued() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let r = refund(&mut o, 1, 100_000, 40_000);
    // The payment is no longer at the refund depth.
    set_facts(&mut o, sale(1), shallow(100_000));
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let paid = pay_refund(&float_words(), &coins, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    let bytes = bitcoin::consensus::serialize(&paid.tx);
    assert!(o
        .take_float(
            &bytes,
            &[r],
            &float_points(&coins),
            &float_scripts(),
            None,
            nop
        )
        .is_err());
    assert!(o
        .take_float(
            &bytes,
            &[r + 50],
            &float_points(&coins),
            &float_scripts(),
            None,
            nop
        )
        .is_err());
    assert!(o
        .take_float(
            &bytes,
            &[],
            &float_points(&coins),
            &float_scripts(),
            None,
            nop
        )
        .is_err());
    assert!(o.outbox().is_empty());
    // Ready again: taken; and once signed it is not queued, so a second
    // transaction for the same refund is refused.
    set_facts(&mut o, sale(1), deep(100_000));
    o.take_float(
        &bytes,
        &[r],
        &float_points(&coins),
        &float_scripts(),
        None,
        nop,
    )
    .unwrap();
    let coins2 = [float_coin(3, Keychain::Receive, 1, 100_000, 6)];
    let paid2 = pay_refund(&float_words(), &coins2, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    assert!(o
        .take_float(
            &bitcoin::consensus::serialize(&paid2.tx),
            &[r],
            &float_points(&coins2),
            &float_scripts(),
            None,
            nop
        )
        .is_err());
}

#[test]
fn fl_35_the_outbox_takes_the_move_only_to_the_one_address_and_with_no_refund() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let r = refund(&mut o, 1, 100_000, 40_000);
    let coins = [
        float_coin(1, Keychain::Receive, 0, 100_000, 6),
        float_coin(2, Keychain::Change, 0, 50_000, 6),
    ];
    let shop = external(200);
    let moved = sweep(&float_words(), &coins, &shop, RATE, 1, 0).unwrap();
    let bytes = bitcoin::consensus::serialize(&moved.tx);
    let points = float_points(&coins);
    // Another address, or a refund with it, is refused.
    assert!(o
        .take_float(
            &bytes,
            &[],
            &points,
            &float_scripts(),
            Some(&external(201)),
            nop
        )
        .is_err());
    assert!(o
        .take_float(&bytes, &[r], &points, &float_scripts(), Some(&shop), nop)
        .is_err());
    // A float payment is not a move: without `sweep_to` it pays a refund.
    assert!(o
        .take_float(&bytes, &[], &points, &float_scripts(), None, nop)
        .is_err());
    assert!(o.outbox().is_empty());
    let txid = o
        .take_float(&bytes, &[], &points, &float_scripts(), Some(&shop), nop)
        .unwrap();
    assert_eq!(txid, moved.txid);
    assert_eq!(o.outbox().len(), 1);
    // The refund stays queued.
    assert_eq!(
        o.refunds().iter().find(|x| x.id == r).unwrap().stage,
        openbmt_owner::Stage::Queued
    );
}

#[test]
fn fl_03_the_sealed_pay_and_sweep_open_only_the_floats_own_seed() {
    let entropy = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, BIP85_CHILD_0)
        .unwrap()
        .to_entropy();
    let made = seal_compact_seedqr(&entropy, &mut Steady(0)).unwrap();
    let coins = [float_coin(1, Keychain::Receive, 0, 100_000, 6)];
    let sealed_paid = pay_refund_sealed(
        &made.sealed.sealed,
        &made.sealed.data_key,
        &coins,
        &external(1),
        40_000,
        RATE,
        0,
        1,
        0,
    )
    .unwrap();
    let words_paid =
        pay_refund(&float_words(), &coins, &external(1), 40_000, RATE, 0, 1, 0).unwrap();
    assert_eq!(sealed_paid.txid, words_paid.txid);
    let swept = sweep_sealed(
        &made.sealed.sealed,
        &made.sealed.data_key,
        &coins,
        &external(2),
        RATE,
        1,
        0,
    )
    .unwrap();
    assert_eq!(swept.paid + swept.fee, 100_000);
    let starter = starter_blob();
    assert!(pay_refund_sealed(
        &starter.sealed,
        &starter.data_key,
        &coins,
        &external(1),
        40_000,
        RATE,
        0,
        1,
        0
    )
    .is_err());
    assert!(sweep_sealed(
        &starter.sealed,
        &starter.data_key,
        &coins,
        &external(2),
        RATE,
        1,
        0
    )
    .is_err());
}
