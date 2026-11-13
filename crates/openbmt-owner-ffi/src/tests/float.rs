//! The ops of the refund float through `dispatch` (ops 165 to 170,
//! `docs/FLOAT-RULES.md`): the rules are in `openbmt-owner`, so these check
//! the wire, the words of a refusal and that nothing secret comes back.
//!
//! TEST ONLY, NEVER USE: the words are a published BIP-85 test vector.

use bitcoin::absolute::LockTime;
use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};
use openbmt_core::wallet::Keychain;
use openbmt_owner::float::{FloatWallet, FloatWords};

use super::import::descriptor;
use super::kit::Rig;
use super::wire::{frame, Rd, W};
use crate::{dispatch, Registry};

const WORDS: &str = "girl mad pet galaxy egg matter matrix prison refuse sense ordinary nose";
const OP_OPEN: u16 = 165;
const OP_STATUS: u16 = 166;
const OP_FEE: u16 = 167;
const OP_PAY: u16 = 168;
const OP_SWEEP: u16 = 169;
const OP_REFILL: u16 = 170;

fn call(op: u16, fields: &[u8]) -> Vec<u8> {
    dispatch(&Registry::new(), &frame(op, 0, fields))
}

fn float() -> FloatWallet {
    FloatWallet::from_words(&FloatWords::new(WORDS).unwrap()).unwrap()
}

fn opt_u64(w: W, v: Option<u64>) -> W {
    match v {
        None => w.u8(0),
        Some(v) => w.u8(1).u64(v),
    }
}

/// A coin of the float's receive chain: the fields of `FloatCoinView`.
fn coin(w: W, tag: u8, index: u32, sats: u64) -> W {
    let script = float().script(Keychain::Receive, index).unwrap();
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
            script_pubkey: script,
        }],
    };
    let txid = prev.compute_txid();
    w.bytes(&txid.to_byte_array())
        .u32(0)
        .bytes(&serialize(&prev))
        .u8(0)
        .u32(index)
        .u32(6)
}

#[test]
fn fl_02_the_words_open_the_float_and_only_its_public_description_comes_back() {
    let response = call(OP_OPEN, &W::new().string(WORDS).0);
    let mut r = Rd::ok(&response);
    let (description, first) = (r.string(), r.string());
    r.done();
    assert_eq!(description, float().description());
    assert_eq!(first, float().receive_address(0).unwrap().to_string());
    assert!(description.contains("xpub"));
    let text = String::from_utf8_lossy(&response).to_string();
    assert!(!text.contains("galaxy"));
    // Words that are not 12 English words are refused with the float's rule.
    let bad = Rd::error(&call(OP_OPEN, &W::new().string("girl mad pet").0)).expect("refused");
    assert_eq!(bad.rule, "FL-01");
    assert_eq!(bad.words.heading, "WORDS_NEEDED_FL_01_WORDS");
    assert_eq!(bad.words.what, bad.words.heading);
}

#[test]
fn fl_10_to_fl_12_the_cap_says_what_the_float_holds_whether_it_is_low_and_the_refill() {
    let ask = |cap: u64, confirmed: u64, unconfirmed: u64| {
        let response = call(
            OP_STATUS,
            &W::new().u64(cap).u64(confirmed).u64(unconfirmed).0,
        );
        let mut r = Rd::ok(&response);
        let out = (r.u64(), r.bool(), r.opt(|r| r.u64()));
        r.done();
        out
    };
    assert_eq!(
        ask(1_000_000, 100_000, 50_000),
        (150_000, true, Some(850_000))
    );
    assert_eq!(ask(1_000_000, 250_000, 0), (250_000, false, Some(750_000)));
    assert_eq!(ask(1_000_000, 1_000_000, 0), (1_000_000, false, None));
    assert_eq!(ask(0, 0, 0), (0, false, None));
}

#[test]
fn fl_22_the_refund_rate_is_asked_before_the_owner_confirms() {
    let ask = |recent: Option<u64>, typed: Option<u64>| {
        call(OP_FEE, &opt_u64(opt_u64(W::new(), recent), typed).0)
    };
    assert_eq!(Rd::ok(&ask(Some(300), None)).u64(), 1_000);
    assert_eq!(Rd::ok(&ask(None, Some(2_500))).u64(), 2_500);
    let fault = Rd::error(&ask(None, None)).expect("refused");
    assert_eq!(fault.rule, "FL-22");
}

fn pay_request(address: &str, sats: u64, coins: &[(u8, u32, u64)]) -> Vec<u8> {
    let mut w = W::new()
        .string(WORDS)
        .u8(0) // mainnet
        .string(address)
        .u64(sats)
        .u64(2_000)
        .u32(0)
        .u32(1)
        .u32(0)
        .u32(u32::try_from(coins.len()).unwrap());
    for (tag, index, value) in coins {
        w = coin(w, *tag, *index, *value);
    }
    w.0
}

const REFUND_TO: &str = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";

#[test]
fn fl_20_a_refund_is_paid_from_the_floats_coins_and_comes_back_finished() {
    let response = call(
        OP_PAY,
        &pay_request(REFUND_TO, 30_000, &[(1, 0, 60_000), (2, 1, 50_000)]),
    );
    let mut r = Rd::ok(&response);
    let (tx, txid, fee, vsize, paid) = (r.bytes(), r.string(), r.u64(), r.u64(), r.u64());
    let change_index = r.opt(|r| r.u32());
    let change_sats = r.opt(|r| r.u64());
    let destination = r.string();
    r.done();
    let tx: Transaction = deserialize(&tx).unwrap();
    assert_eq!(tx.compute_txid().to_string(), txid);
    assert_eq!(paid, 30_000);
    assert_eq!(destination, REFUND_TO);
    assert!(fee * 1000 >= 2_000 * vsize);
    assert_eq!(change_index, Some(0));
    let change = float().script(Keychain::Change, 0).unwrap();
    assert!(tx
        .output
        .iter()
        .any(|o| o.script_pubkey == change && Some(o.value.to_sat()) == change_sats));
    assert!(tx.input.iter().all(|i| i.witness.len() == 2));
    // Nothing secret is in what came back.
    let text = String::from_utf8_lossy(&response).to_string();
    assert!(!text.contains("galaxy"));
}

#[test]
fn fl_23_a_refund_larger_than_the_float_is_refused_with_its_words() {
    let response = call(OP_PAY, &pay_request(REFUND_TO, 59_950, &[(1, 0, 60_000)]));
    let fault = Rd::error(&response).expect("refused");
    assert_eq!(fault.rule, "FL-23");
    assert_eq!(fault.words.heading, "WORDS_NEEDED_FL_23_SHORT");
    // A bad address, and a coin that is not a transaction.
    let fault = Rd::error(&call(
        OP_PAY,
        &pay_request("nonsense", 30_000, &[(1, 0, 60_000)]),
    ))
    .expect("refused");
    assert_eq!(fault.rule, "address-invalid");
    let fault = Rd::error(&call(
        OP_PAY,
        &W::new()
            .string(WORDS)
            .u8(0)
            .string(REFUND_TO)
            .u64(1_000)
            .u64(2_000)
            .u32(0)
            .u32(1)
            .u32(0)
            .u32(1)
            .bytes(&[1; 32])
            .u32(0)
            .bytes(&[1, 2, 3])
            .u8(0)
            .u32(0)
            .u32(6)
            .0,
    ))
    .expect("refused");
    assert_eq!(fault.rule, "float-coin");
    assert!(fault.words.heading.starts_with("WORDS_NEEDED_FL_"));
}

#[test]
fn fl_35_turning_off_pays_the_address_of_the_shops_pinned_wallet() {
    let mut w = W::new()
        .string(WORDS)
        .string(&descriptor())
        .u32(2)
        .u64(2_000)
        .u32(1)
        .u32(0)
        .u32(2);
    w = coin(w, 1, 0, 60_000);
    w = coin(w, 2, 1, 50_000);
    let response = call(OP_SWEEP, &w.0);
    let mut r = Rd::ok(&response);
    let (tx, _txid, fee, _vsize, paid) = (r.bytes(), r.string(), r.u64(), r.u64(), r.u64());
    assert_eq!(r.opt(|r| r.u32()), None);
    assert_eq!(r.opt(|r| r.u64()), None);
    let destination = r.string();
    r.done();
    assert_eq!(paid + fee, 110_000);
    assert!(destination.starts_with("bc1q"));
    let tx: Transaction = deserialize(&tx).unwrap();
    assert_eq!(tx.output.len(), 1);
    assert_eq!(tx.input.len(), 2);
    // No coin to move.
    let empty = W::new()
        .string(WORDS)
        .string(&descriptor())
        .u32(2)
        .u64(2_000)
        .u32(1)
        .u32(0)
        .u32(0);
    let fault = Rd::error(&call(OP_SWEEP, &empty.0)).expect("refused");
    assert_eq!(fault.rule, "FL-35");
    // A shop description that does not read.
    let bad = W::new()
        .string(WORDS)
        .string("not a wallet")
        .u32(2)
        .u64(2_000)
        .u32(1)
        .u32(0)
        .u32(0);
    let fault = Rd::error(&call(OP_SWEEP, &bad.0)).expect("refused");
    assert_eq!(fault.rule, "float-shop-wallet");
}

fn wallet_file() -> Vec<u8> {
    let response = call(148, &W::new().string(&descriptor()).0);
    let mut r = Rd::ok(&response);
    let file = r.bytes();
    let _ = (r.string(), r.bool());
    r.done();
    file
}

#[test]
fn fl_30_a_refill_is_queued_through_the_node_and_refused_above_the_cap() {
    let rig = Rig::new();
    let file = wallet_file();
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let refill = |confirmed: u64, amount: Option<u64>| {
        let w = W::new()
            .string(float().description())
            .u32(3)
            .u64(1_000_000)
            .u64(confirmed)
            .u64(0);
        rig.call(OP_REFILL, handle, &opt_u64(w, amount).0)
    };
    let response = refill(300_000, None);
    let mut r = Rd::ok(&response);
    let (id, sats, address) = (r.u32(), r.u64(), r.string());
    r.done();
    assert!(id > 0);
    assert_eq!(sats, 700_000);
    assert_eq!(address, float().receive_address(3).unwrap().to_string());
    let fault = Rd::error(&refill(300_000, Some(700_001))).expect("refused");
    assert_eq!(fault.rule, "FL-11");
    let fault = Rd::error(&refill(1_000_000, None)).expect("refused");
    assert_eq!(fault.rule, "FL-11");
    // A description that is not the float's.
    let w = W::new().string("nope").u32(0).u64(10).u64(0).u64(0).u8(0);
    let fault = Rd::error(&rig.call(OP_REFILL, handle, &w.0)).expect("refused");
    assert_eq!(fault.rule, "FL-30");
}
