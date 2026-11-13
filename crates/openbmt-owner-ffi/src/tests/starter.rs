//! The ops of the starter wallet through `dispatch` (ops 180 to 196,
//! `docs/START-SIMPLE-RULES.md`): the rules are in `openbmt-owner`, so these
//! check the wire, the words of a refusal, the one setup a process holds,
//! and that no word of the key comes back as text.
//!
//! TEST ONLY, NEVER USE: the kernel's bytes are fixed ones, so the words are
//! the ones of the independent Python vectors in
//! `crates/openbmt-owner/src/starter/tests.rs`.

use bitcoin::absolute::LockTime;
use bitcoin::consensus::encode::{deserialize, serialize};
use bitcoin::hashes::{sha256, Hash};
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid, Witness};
use openbmt_core::wallet::Keychain;
use openbmt_owner::starter::{KernelRandom, StarterWallet};

use super::import::descriptor;
use super::wire::{frame, Rd, W};
use crate::{dispatch, Registry};

const OP_MAKE: u16 = 180;
const OP_PAGE: u16 = 181;
const OP_CHECK: u16 = 182;
const OP_KEY: u16 = 183;
const OP_SEAL: u16 = 184;
const OP_RESTORE_START: u16 = 185;
const OP_RESTORE_FINISH: u16 = 186;
const OP_PAY: u16 = 187;
const OP_MOVE: u16 = 188;
const OP_LIMIT: u16 = 189;
const OP_HELD: u16 = 190;
const OP_OFFERS: u16 = 191;
const OP_MAY_GO: u16 = 192;
const OP_CANCEL: u16 = 193;
const OP_NOT_YET: u16 = 194;
const OP_FEE: u16 = 195;
const OP_OPEN: u16 = 196;
const OP_LINE_WORDS: u16 = 204;
const OP_STATE_WORDS: u16 = 205;
const OP_HELD_WORDS: u16 = 206;
const OP_DELETED_WORDS: u16 = 207;

/// The words of the first vector (kernel bytes 1 to 32, no rolls).
const WORDS: &str =
    "hamster smile exhibit tattoo marble danger excite exclude rice hover front nice";
const FINGERPRINT: &str = "ae8be9b4";
const FIRST: &str = "bc1q9lstj832v6pwwf88j9wt85zcdk2kgy0rcnm4gu";
/// The second vector: the same bytes and ten rolls.
const ROLLED_FINGERPRINT: &str = "ba7e95cd";
const ROLLED_FIRST: &str = "bc1q7xpkht8uju94z2s86h47tlg2wezftw3fftukaa";

/// Fixed first reads, then a chain of hashes.
struct Fixed {
    given: Vec<[u8; 32]>,
    next: u32,
}

impl Fixed {
    fn new(given: &[[u8; 32]]) -> Self {
        Self {
            given: given.iter().rev().copied().collect(),
            next: 0,
        }
    }
}

impl KernelRandom for Fixed {
    fn read32(&mut self) -> Option<[u8; 32]> {
        if let Some(g) = self.given.pop() {
            return Some(g);
        }
        self.next += 1;
        Some(sha256::Hash::hash(&self.next.to_le_bytes()).to_byte_array())
    }
}

pub(super) fn counting(start: u8) -> [u8; 32] {
    std::array::from_fn(|i| start.wrapping_add(i as u8))
}

pub(super) fn registry(given: &[[u8; 32]]) -> Registry {
    let reg = Registry::new();
    *reg.starter.rng.lock().unwrap() = Some(Box::new(Fixed::new(given)));
    reg
}

fn call(reg: &Registry, op: u16, fields: &[u8]) -> Vec<u8> {
    dispatch(reg, &frame(op, 0, fields))
}

fn make(reg: &Registry, rolls: &[u8]) -> Vec<u8> {
    call(reg, OP_MAKE, &W::new().bytes(rolls).0)
}

fn wallet_view(response: &[u8]) -> (String, String, String) {
    let mut r = Rd::ok(response);
    let out = (r.string(), r.string(), r.string());
    r.done();
    out
}

fn key(reg: &Registry, byte: u8) -> Vec<u8> {
    call(reg, OP_KEY, &W::new().u8(byte).0)
}

/// What a key said: typed, accepted, step, finished, failed.
fn typed(response: &[u8]) -> (u32, bool, u32, bool, bool) {
    let mut r = Rd::ok(response);
    let out = (r.u32(), r.bool(), r.u32(), r.bool(), r.bool());
    r.done();
    out
}

/// The positions the check-back asks, 1 to 12.
fn positions_of(response: &[u8]) -> Vec<u32> {
    let mut r = Rd::ok(response);
    let n = r.u32();
    let out = (0..n).map(|_| r.u32()).collect();
    r.done();
    out
}

fn type_word(reg: &Registry, word: &str, letters: usize) {
    for b in word.bytes().take(letters) {
        Rd::ok(&key(reg, b));
    }
}

fn words() -> Vec<&'static str> {
    WORDS.split(' ').collect()
}

fn no_words_in(response: &[u8]) {
    let text = String::from_utf8_lossy(response).to_string();
    for w in words() {
        assert!(!text.contains(w), "{w} is in a response");
    }
}

/// Makes the wallet, passes the check-back and seals: the sealed seed, the
/// data key and the wallet's description.
pub(super) fn sealed_wallet(reg: &Registry) -> (Vec<u8>, Vec<u8>, String) {
    let (description, _, _) = wallet_view(&make(reg, &[]));
    let positions = positions_of(&call(reg, OP_CHECK, &[]));
    assert_eq!(positions.len(), 3);
    for p in positions {
        type_word(reg, words()[p as usize - 1], 4);
        Rd::ok(&key(reg, 13));
    }
    let response = call(reg, OP_SEAL, &[]);
    let mut r = Rd::ok(&response);
    let (sealed, data_key) = (r.bytes(), r.bytes());
    r.done();
    (sealed, data_key, description)
}

// ---- making --------------------------------------------------------------------

#[test]
fn ss_05_the_words_are_made_from_the_kernel_and_only_the_public_wallet_comes_back() {
    let reg = registry(&[counting(1), counting(77)]);
    let response = make(&reg, &[]);
    let (description, fingerprint, first) = wallet_view(&response);
    assert_eq!(fingerprint, FINGERPRINT);
    assert_eq!(first, FIRST);
    assert!(description.contains(&format!("{FINGERPRINT}/84h/0h/0h")));
    assert!(description.contains("xpub"));
    no_words_in(&response);
    // The same bytes with die rolls are the second vector.
    let reg = registry(&[counting(1), counting(77)]);
    let (_, fingerprint, first) = wallet_view(&make(&reg, &[1, 2, 3, 4, 5, 6, 6, 5, 4, 3]));
    assert_eq!(fingerprint, ROLLED_FINGERPRINT);
    assert_eq!(first, ROLLED_FIRST);
}

#[test]
fn ss_02_a_weak_or_repeated_read_is_refused_and_makes_no_setup() {
    for given in [
        vec![[0u8; 32], counting(1)],
        vec![counting(1), [9u8; 32]],
        vec![counting(1), counting(1)],
    ] {
        let reg = registry(&given);
        let fault = Rd::error(&make(&reg, &[])).expect("refused");
        assert_eq!(fault.rule, "SS-02");
        assert!(fault.words.heading.starts_with("Random numbers "));
        // There is no setup to draw or to type on.
        let fault = Rd::error(&call(&reg, OP_PAGE, &W::new().u32(0).u32(2).0)).expect("refused");
        assert_eq!(fault.rule, "starter-no-setup");
        assert_eq!(fault.status, 3);
        assert!(Rd::error(&key(&reg, b'a')).is_some());
    }
}

#[test]
fn ss_03_a_roll_that_is_not_a_die_face_is_refused() {
    let reg = registry(&[counting(1), counting(77)]);
    let fault = Rd::error(&make(&reg, &[1, 7])).expect("refused");
    assert_eq!(fault.rule, "SS-03");
    assert_eq!(fault.words.heading, "Not a die roll");
}

// ---- the words as images -----------------------------------------------------------

fn page(reg: &Registry, n: u32, scale: u32) -> Vec<u8> {
    call(reg, OP_PAGE, &W::new().u32(n).u32(scale).0)
}

#[test]
fn ss_11_a_screen_of_words_is_an_image_and_never_text() {
    let reg = registry(&[counting(1), counting(77)]);
    make(&reg, &[]);
    let mut seen = Vec::new();
    for n in 0..3 {
        let response = page(&reg, n, 2);
        let mut r = Rd::ok(&response);
        let (width, height, pixels) = (r.u32(), r.u32(), r.bytes());
        r.done();
        assert_eq!(pixels.len(), (width * height) as usize);
        assert!(pixels.iter().all(|p| *p == 0 || *p == 255));
        assert!(pixels.contains(&0));
        no_words_in(&response);
        seen.push(pixels);
    }
    assert_ne!(seen[0], seen[1]);
    assert_ne!(seen[1], seen[2]);
    // Only three screens.
    let fault = Rd::error(&page(&reg, 3, 2)).expect("refused");
    assert_eq!(fault.rule, "SS-10");
}

#[test]
fn ss_10_the_keypad_answers_with_counts_and_a_yes_and_the_check_runs_through_the_ops() {
    let reg = registry(&[counting(1), counting(77)]);
    make(&reg, &[]);
    // The positions come from fresh bytes: 7, then 3 and 11 (the rest of
    // the bytes are above 251).
    let mut fresh = [255u8; 32];
    fresh[0] = 7;
    fresh[1] = 3;
    fresh[2] = 11;
    {
        let mut guard = reg.starter.rng.lock().unwrap();
        *guard = Some(Box::new(Fixed::new(&[fresh, counting(4)])));
    }
    let positions = positions_of(&call(&reg, OP_CHECK, &[]));
    assert_eq!(positions, vec![4, 8, 12]);
    let asked: Vec<&str> = positions.iter().map(|p| words()[*p as usize - 1]).collect();
    // Three letters are not yet a word; the fourth is accepted.
    for (i, word) in asked.iter().enumerate() {
        type_word(&reg, word, 3);
        let before = typed(&key(&reg, b'x'));
        assert_eq!(before.0, 4);
        assert!(!before.1);
        Rd::ok(&key(&reg, 8));
        let t = typed(&key(&reg, word.as_bytes()[3]));
        assert_eq!((t.0, t.1, t.2), (4, true, i as u32));
        let t = typed(&key(&reg, 13));
        assert_eq!((t.0, t.1, t.2), (0, false, i as u32 + 1));
        assert_eq!(t.3, i == 2);
        assert!(!t.4);
        no_words_in(&key(&reg, 8));
    }
    // The check is passed: the words are not shown again, and the keypad is
    // closed.
    let fault = Rd::error(&page(&reg, 0, 2)).expect("refused");
    assert_eq!(fault.rule, "SS-10");
    assert!(Rd::error(&key(&reg, b'a')).is_some());
    // A key that is not a letter, backspace or enter.
    let fault = Rd::error(&key(&reg, b'1')).expect("refused");
    assert_eq!(fault.rule, "SS-11");
}

#[test]
fn ss_10_a_wrong_answer_shows_the_words_again_before_a_new_check() {
    let reg = registry(&[counting(1), counting(77)]);
    make(&reg, &[]);
    Rd::ok(&call(&reg, OP_CHECK, &[]));
    type_word(&reg, "zoo", 3);
    let t = typed(&key(&reg, 13));
    assert!(t.4, "failed");
    // Not sealed, not checked again, until the words have been shown.
    assert_eq!(
        Rd::error(&call(&reg, OP_SEAL, &[])).expect("refused").rule,
        "SS-10"
    );
    assert_eq!(
        Rd::error(&call(&reg, OP_CHECK, &[])).expect("refused").rule,
        "SS-10"
    );
    Rd::ok(&page(&reg, 0, 1));
    Rd::ok(&call(&reg, OP_CHECK, &[]));
}

// ---- sealing -----------------------------------------------------------------------

#[test]
fn ss_20_the_seed_is_sealed_after_the_check_and_opens_with_the_data_key_alone() {
    let reg = registry(&[counting(1), counting(77)]);
    // Before the check: refused, and the setup is still there.
    make(&reg, &[]);
    let fault = Rd::error(&call(&reg, OP_SEAL, &[])).expect("refused");
    assert_eq!(fault.rule, "SS-10");
    Rd::ok(&page(&reg, 0, 1));
    let reg = registry(&[counting(1), counting(77)]);
    let (sealed, data_key, description) = sealed_wallet(&reg);
    assert_eq!(sealed.len(), 45);
    assert_eq!(data_key.len(), 32);
    assert_eq!(sealed[0], 1);
    let text = String::from_utf8_lossy(&sealed).to_string();
    for w in words() {
        assert!(!text.contains(w));
    }
    // The setup is used up: there is nothing more to draw.
    assert_eq!(
        Rd::error(&page(&reg, 0, 1)).expect("refused").rule,
        "starter-no-setup"
    );
    // The data key opens it.
    let open = |sealed: &[u8], data_key: &[u8]| {
        call(&reg, OP_OPEN, &W::new().bytes(sealed).bytes(data_key).0)
    };
    let (d, f, first) = wallet_view(&open(&sealed, &data_key));
    assert_eq!(
        (d, f.as_str(), first.as_str()),
        (description, FINGERPRINT, FIRST)
    );
    // A wrong key, a short key and a changed seed open nothing.
    assert_eq!(
        Rd::error(&open(&sealed, &[7u8; 32])).expect("refused").rule,
        "SS-20"
    );
    let fault = Rd::error(&open(&sealed, &data_key[..31])).expect("refused");
    assert_eq!(fault.rule, "SS-20");
    assert_eq!(fault.words.heading, "Cannot open the key");
    let mut bad = sealed.clone();
    bad[20] ^= 1;
    let fault = Rd::error(&open(&bad, &data_key)).expect("refused");
    assert_eq!(fault.words.heading, "The key will not open");
}

#[test]
fn ss_10_cancel_drops_the_setup() {
    let reg = registry(&[counting(1), counting(77)]);
    make(&reg, &[]);
    Rd::ok(&call(&reg, OP_CANCEL, &[]));
    assert_eq!(
        Rd::error(&page(&reg, 0, 1)).expect("refused").rule,
        "starter-no-setup"
    );
    // Cancelling nothing is fine.
    Rd::ok(&call(&reg, OP_CANCEL, &[]));
}

// ---- the restore ---------------------------------------------------------------------

fn restore_finish(reg: &Registry, description: &str) -> Vec<u8> {
    call(reg, OP_RESTORE_FINISH, &W::new().string(description).0)
}

#[test]
fn ss_23_the_typed_words_are_checked_against_the_wallet_and_sealed_again() {
    let reg = registry(&[counting(1), counting(77)]);
    let (old_sealed, old_key, description) = sealed_wallet(&reg);
    // The restore: only the owner starts it, and nothing finishes before it.
    assert_eq!(
        Rd::error(&restore_finish(&reg, &description))
            .expect("refused")
            .rule,
        "starter-no-setup"
    );
    Rd::ok(&call(&reg, OP_RESTORE_START, &[]));
    for (i, w) in words().iter().enumerate() {
        type_word(&reg, w, w.len());
        let t = typed(&key(&reg, 13));
        assert_eq!(t.2, i as u32 + 1);
        assert_eq!(t.3, i == 11);
    }
    // The wrong wallet is refused, and the words stay typed.
    let fault = Rd::error(&restore_finish(&reg, &descriptor())).expect("refused");
    assert_eq!(fault.rule, "SS-23");
    assert_eq!(fault.words.heading, "Not this wallet's words");
    let response = restore_finish(&reg, &description);
    let mut r = Rd::ok(&response);
    let (sealed, data_key) = (r.bytes(), r.bytes());
    r.done();
    assert_ne!(sealed, old_sealed);
    assert_ne!(data_key, old_key);
    let open = |sealed: &[u8], data_key: &[u8]| {
        call(&reg, OP_OPEN, &W::new().bytes(sealed).bytes(data_key).0)
    };
    assert_eq!(wallet_view(&open(&sealed, &data_key)).0, description);
    assert!(Rd::error(&open(&sealed, &old_key)).is_some());
    // The restore is used up.
    assert!(Rd::error(&restore_finish(&reg, &description)).is_some());
}

#[test]
fn ss_23_words_of_another_wallet_and_a_missing_word_are_refused() {
    let reg = registry(&[counting(1), counting(77)]);
    let (_, _, description) = sealed_wallet(&reg);
    Rd::ok(&call(&reg, OP_RESTORE_START, &[]));
    for _ in 0..12 {
        type_word(&reg, "abandon", 7);
        Rd::ok(&key(&reg, 13));
    }
    // Twelve of the list but not a wallet: the check word is wrong.
    assert_eq!(
        Rd::error(&restore_finish(&reg, &description))
            .expect("refused")
            .rule,
        "SS-04"
    );
    // Back one word: eleven are not enough.
    Rd::ok(&key(&reg, 8));
    assert_eq!(
        Rd::error(&restore_finish(&reg, &description))
            .expect("refused")
            .rule,
        "SS-23"
    );
}

// ---- paying -----------------------------------------------------------------------------

/// A coin of the starter wallet: the fields of `StarterCoinView`.
fn coin(w: W, wallet: &StarterWallet, tag: u8, index: u32, sats: u64, confirmations: u32) -> W {
    let script = wallet.script(Keychain::Receive, index).unwrap();
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
        .u32(confirmations)
}

const REFUND_TO: &str = "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu";

fn pay_request(
    sealed: &[u8],
    data_key: &[u8],
    wallet: &StarterWallet,
    address: &str,
    sats: u64,
    coins: &[(u8, u32, u64)],
) -> Vec<u8> {
    let mut w = W::new()
        .bytes(sealed)
        .bytes(data_key)
        .u8(0) // mainnet
        .string(address)
        .u64(sats)
        .u64(2_000)
        .u32(0)
        .u32(1)
        .u32(0)
        .u32(u32::try_from(coins.len()).unwrap());
    for (tag, index, value) in coins {
        w = coin(w, wallet, *tag, *index, *value, 6);
    }
    w.0
}

#[test]
fn ss_30_a_refund_is_signed_from_the_sealed_seed_and_comes_back_finished() {
    let reg = registry(&[counting(1), counting(77)]);
    let (sealed, data_key, description) = sealed_wallet(&reg);
    let wallet = StarterWallet::parse(&description).unwrap();
    let response = call(
        &reg,
        OP_PAY,
        &pay_request(
            &sealed,
            &data_key,
            &wallet,
            REFUND_TO,
            30_000,
            &[(1, 0, 60_000), (2, 1, 50_000)],
        ),
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
    // The address in full, for the owner to see (SS-31).
    assert_eq!(destination, REFUND_TO);
    assert!(fee * 1000 >= 2_000 * vsize);
    assert_eq!(change_index, Some(0));
    let change = wallet.script(Keychain::Change, 0).unwrap();
    assert!(tx
        .output
        .iter()
        .any(|o| o.script_pubkey == change && Some(o.value.to_sat()) == change_sats));
    assert!(tx.input.iter().all(|i| i.witness.len() == 2));
    no_words_in(&response);
    // The data key is not in the answer.
    assert!(!response.windows(32).any(|w| w == data_key.as_slice()));
}

#[test]
fn ss_30_a_wrong_data_key_a_bad_address_and_a_refund_too_large_are_refused() {
    let reg = registry(&[counting(1), counting(77)]);
    let (sealed, data_key, description) = sealed_wallet(&reg);
    let wallet = StarterWallet::parse(&description).unwrap();
    let coins = [(1u8, 0u32, 60_000u64)];
    let fault = Rd::error(&call(
        &reg,
        OP_PAY,
        &pay_request(&sealed, &[3u8; 32], &wallet, REFUND_TO, 30_000, &coins),
    ))
    .expect("refused");
    assert_eq!(fault.rule, "SS-20");
    let fault = Rd::error(&call(
        &reg,
        OP_PAY,
        &pay_request(&sealed, &data_key, &wallet, "nonsense", 30_000, &coins),
    ))
    .expect("refused");
    assert_eq!(fault.rule, "address-invalid");
    let fault = Rd::error(&call(
        &reg,
        OP_PAY,
        &pay_request(&sealed, &data_key, &wallet, REFUND_TO, 59_950, &coins),
    ))
    .expect("refused");
    assert_eq!(fault.rule, "SS-30");
    assert_eq!(fault.words.heading, "Not enough in the wallet");
    // A coin that is not a transaction.
    let fault = Rd::error(&call(
        &reg,
        OP_PAY,
        &W::new()
            .bytes(&sealed)
            .bytes(&data_key)
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
    assert_eq!(fault.rule, "starter-coin");
}

#[test]
fn ss_42_the_move_pays_the_new_wallets_address_from_every_coin() {
    let reg = registry(&[counting(1), counting(77)]);
    let (sealed, data_key, description) = sealed_wallet(&reg);
    let wallet = StarterWallet::parse(&description).unwrap();
    let new = descriptor();
    let request = |new: &str, confirmations: u32, coins: &[(u8, u32, u64)]| {
        let mut w = W::new()
            .bytes(&sealed)
            .bytes(&data_key)
            .string(new)
            .u32(2)
            .u64(2_000)
            .u32(1)
            .u32(0)
            .u32(coins.len() as u32);
        for (tag, index, value) in coins {
            w = coin(w, &wallet, *tag, *index, *value, confirmations);
        }
        w.0
    };
    let response = call(
        &reg,
        OP_MOVE,
        &request(&new, 6, &[(1, 0, 60_000), (2, 1, 50_000)]),
    );
    let mut r = Rd::ok(&response);
    let (tx, _txid, fee, _vsize, paid) = (r.bytes(), r.string(), r.u64(), r.u64(), r.u64());
    assert_eq!(r.opt(|r| r.u32()), None);
    assert_eq!(r.opt(|r| r.u64()), None);
    let destination = r.string();
    r.done();
    assert_eq!(paid + fee, 110_000);
    let tx: Transaction = deserialize(&tx).unwrap();
    assert_eq!(tx.output.len(), 1);
    assert_eq!(tx.input.len(), 2);
    let new_wallet = openbmt_core::wallet_single::ImportedWallet::read(&new).unwrap();
    assert_eq!(
        tx.output[0].script_pubkey,
        new_wallet.receive_script(2).unwrap()
    );
    assert_eq!(
        destination,
        bitcoin::Address::from_script(&tx.output[0].script_pubkey, new_wallet.network())
            .unwrap()
            .to_string()
    );
    // Nothing to move, an unconfirmed coin, a description that does not
    // read.
    assert_eq!(
        Rd::error(&call(&reg, OP_MOVE, &request(&new, 6, &[])))
            .expect("refused")
            .rule,
        "SS-42"
    );
    let fault =
        Rd::error(&call(&reg, OP_MOVE, &request(&new, 0, &[(1, 0, 60_000)]))).expect("refused");
    assert_eq!(fault.words.heading, "Wait for a payment");
    let fault = Rd::error(&call(
        &reg,
        OP_MOVE,
        &request("not a wallet", 6, &[(1, 0, 60_000)]),
    ))
    .expect("refused");
    assert_eq!(fault.rule, "starter-new-wallet");
    // To itself is not a move.
    let fault = Rd::error(&call(
        &reg,
        OP_MOVE,
        &request(&description, 6, &[(1, 0, 60_000)]),
    ))
    .expect("refused");
    assert_eq!(fault.rule, "SS-30");
}

// ---- the limit and the date ------------------------------------------------------------

#[test]
fn ss_35_to_ss_38_the_limit_the_date_and_the_counter_are_pure_ops() {
    let reg = Registry::new();
    let limit = |held: u64, limit: u64, now: u64, by: u64, not_yet: Option<u64>| {
        let mut w = W::new().u64(held).u64(limit).u64(now).u64(by);
        w = match not_yet {
            None => w.u8(0),
            Some(u) => w.u8(1).u64(u),
        };
        let response = call(&reg, OP_LIMIT, &w.0);
        let mut r = Rd::ok(&response);
        let out = (r.u8(), r.opt(|r| r.u64()));
        r.done();
        out
    };
    assert_eq!(limit(5, 100, 10, 1000, None), (0, None));
    assert_eq!(limit(100, 100, 10, 1000, None), (1, None));
    assert_eq!(limit(5, 100, 1000, 1000, None), (2, None));
    assert_eq!(limit(100, 100, 10, 1000, Some(500)), (3, Some(500)));
    // Further than 7 days is not honoured.
    assert_eq!(limit(100, 100, 10, 1000, Some(10 + 8 * 86_400)), (1, None));
    // SS-37: what waits.
    let held = |state: u8, change: u8| {
        Rd::ok(&call(&reg, OP_HELD, &W::new().u8(state).u8(change).0)).bool()
    };
    assert!(held(3, 0)); // not yet: raise the limit waits
    assert!(held(1, 2)); // at the limit: add a counter waits
    assert!(!held(3, 5)); // lowering the limit goes
    assert!(!held(0, 0)); // under: nothing waits
    let fault = Rd::error(&call(&reg, OP_HELD, &W::new().u8(9).u8(0).0)).expect("refused");
    assert_eq!(fault.status, 1);
    // SS-38.
    let offers = |stop: bool, held: u64, limit: u64| {
        Rd::ok(&call(
            &reg,
            OP_OFFERS,
            &W::new().u8(u8::from(stop)).u64(held).u64(limit).0,
        ))
        .bool()
    };
    assert!(offers(false, 500, 100));
    assert!(offers(true, 99, 100));
    assert!(!offers(true, 100, 100));
    // SS-43.
    assert!(!Rd::ok(&call(&reg, OP_MAY_GO, &W::new().u32(0).0)).bool());
    assert!(Rd::ok(&call(&reg, OP_MAY_GO, &W::new().u32(1).0)).bool());
    // SS-37: Not yet is at most 7 days.
    assert_eq!(
        Rd::ok(&call(
            &reg,
            OP_NOT_YET,
            &W::new().u64(1_000).u64(30 * 86_400).0
        ))
        .u64(),
        1_000 + 7 * 86_400
    );
    assert_eq!(
        Rd::ok(&call(&reg, OP_NOT_YET, &W::new().u64(1_000).u64(60).0)).u64(),
        1_060
    );
}

#[test]
fn ss_30_the_fee_rate_is_the_least_of_one_satoshi_per_byte() {
    let reg = Registry::new();
    let ask = |recent: Option<u64>, typed: Option<u64>| {
        let opt = |w: W, v: Option<u64>| match v {
            None => w.u8(0),
            Some(v) => w.u8(1).u64(v),
        };
        call(&reg, OP_FEE, &opt(opt(W::new(), recent), typed).0)
    };
    assert_eq!(Rd::ok(&ask(Some(300), None)).u64(), 1_000);
    assert_eq!(Rd::ok(&ask(None, Some(2_500))).u64(), 2_500);
    let fault = Rd::error(&ask(None, None)).expect("refused");
    assert_eq!(fault.rule, "SS-30");
}

#[test]
fn ss_11_the_ops_take_no_node_and_a_wrong_handle_is_refused() {
    let reg = Registry::new();
    let fault = Rd::error(&dispatch(&reg, &frame(OP_CANCEL, 7, &[]))).expect("refused");
    assert_eq!(fault.rule, "handle-unexpected");
}

#[test]
fn ss_36_ss_37_ss_43_the_core_words_cross_as_the_library_writes_them() {
    use openbmt_core::say::Words;
    use openbmt_owner::starter as lib;
    let reg = Registry::new();
    let same = |got: super::wire::Words, want: &Words, code: &str| {
        assert_eq!(got.heading, want.heading);
        assert_eq!(got.what, want.what);
        assert_eq!(got.next, want.next);
        assert_eq!(got.code.as_deref(), Some(code));
        assert!(!got.heading.starts_with("WORDS_NEEDED_"));
    };
    let response = call(&reg, OP_LINE_WORDS, &[]);
    let mut r = Rd::ok(&response);
    same(r.words(), &lib::line_words(), "SS-36");
    r.done();
    let response = call(&reg, OP_HELD_WORDS, &[]);
    let mut r = Rd::ok(&response);
    same(r.words(), &lib::held_words(), "SS-37");
    r.done();
    let response = call(&reg, OP_DELETED_WORDS, &[]);
    let mut r = Rd::ok(&response);
    same(r.words(), &lib::deleted_words(), "SS-43");
    r.done();
    // Each state of the limit has its words, and the wallet under its limit
    // has none.
    let state = |kind: u8, want: Option<lib::LimitState>| {
        let response = call(&reg, OP_STATE_WORDS, &W::new().u8(kind).0);
        let mut r = Rd::ok(&response);
        let got = r.opt(|r| r.words());
        r.done();
        match (got, want.and_then(lib::state_words)) {
            (None, None) => {}
            (Some(got), Some(want)) => same(got, &want, "SS-37"),
            other => panic!("kind {kind}: {other:?}"),
        }
    };
    state(0, Some(lib::LimitState::Under));
    state(1, Some(lib::LimitState::AtLimit));
    state(2, Some(lib::LimitState::PastDate));
    state(3, Some(lib::LimitState::NotYet { until: 0 }));
    let fault = Rd::error(&call(&reg, OP_STATE_WORDS, &W::new().u8(9).0)).expect("refused");
    assert_eq!(fault.status, 1);
}

// ---- a refund is signed only when it is ready (SS-30) -------------------------------

/// A node with a coin in its wallet that no sale paid, a sale paid at height
/// 2 and buried to the refund depth, and a refund of it that is ready. The
/// sealed seed and its data key come with it.
fn node_with_ready_refund() -> (super::kit::Rig, u64, Vec<u8>, Vec<u8>, u32) {
    use super::kit::{counter, receive, tx_from, Rig};
    let rig = Rig::new();
    *rig.registry().starter.rng.lock().unwrap() =
        Some(Box::new(Fixed::new(&[counting(1), counting(77)])));
    let (sealed, data_key, _) = sealed_wallet(rig.registry());
    let handle = rig.start();
    rig.queue_batch(handle);
    // The coin nothing owes, then the sale's payment.
    rig.mine(handle, &[tx_from(7, &[(&receive(0), 300_000)])]);
    rig.mine(handle, &[tx_from(8, &[(&receive(1), 50_000)])]);
    rig.mine_empty(handle, 5);
    let sale = openbmt_owner::SaleKey {
        counter: counter(),
        index: 1,
    };
    let refund = rig
        .registry()
        .entry_for_test(handle)
        .expect("an entry")
        .node_for_test(move |node| {
            let write = node.queue_writer();
            let owner = node.owner_mut().unwrap();
            let facts = openbmt_owner::SaleFacts {
                received_sats: 50_000,
                reached_refund_depth: true,
            };
            owner.set_sale_facts(sale, facts, &write).unwrap();
            owner
                .queue_refund(sale, "Mia", super::kit::external(9), 10_000, None, &write)
                .unwrap()
        });
    // The next block works the sale's facts out from the owner's own view.
    rig.mine_empty(handle, 1);
    (rig, handle, sealed, data_key, refund)
}

fn refund_ready(rig: &super::kit::Rig, handle: u64, refund: u32) -> bool {
    rig.registry()
        .entry_for_test(handle)
        .expect("an entry")
        .node_for_test(move |node| node.owner().refund_ready(refund))
}

fn node_pay(
    rig: &super::kit::Rig,
    handle: u64,
    sealed: &[u8],
    data_key: &[u8],
    refund: u32,
) -> Vec<u8> {
    rig.call(
        209,
        handle,
        &W::new()
            .bytes(sealed)
            .bytes(data_key)
            .u32(refund)
            .u64(2_000)
            .u32(1)
            .0,
    )
}

fn finished_tx(prev: u8) -> Vec<u8> {
    serialize(&super::kit::tx_from(
        prev,
        &[(&super::kit::external(9), 10_000)],
    ))
}

#[test]
fn ss_30_n13e_01_a_refund_whose_payment_a_reorganization_took_is_not_signed_from_other_coins() {
    let (rig, handle, sealed, data_key, refund) = node_with_ready_refund();
    assert!(refund_ready(&rig, handle, refund));
    // The payment's block leaves the chain; the refund stays queued.
    rig.reorg_from(handle, 2);
    assert!(!refund_ready(&rig, handle, refund));
    // The wallet still holds the unrelated coin, yet nothing is signed.
    let fault = Rd::error(&node_pay(&rig, handle, &sealed, &data_key, refund)).expect("refused");
    assert_eq!(fault.rule, "starter-refund-ready");
    assert_eq!(fault.words.code.as_deref(), Some("SS-30"));
    assert_eq!(fault.words.heading, "Refund not ready yet");
    // A transaction handed to the outbox is refused too, and writes nothing.
    let given = rig.call(
        210,
        handle,
        &W::new().bytes(&finished_tx(5)).u32(1).u32(refund).0,
    );
    let fault = Rd::error(&given).expect("refused");
    assert_eq!(fault.status, 1);
    let (stage_queued, outbox) = rig
        .registry()
        .entry_for_test(handle)
        .expect("an entry")
        .node_for_test(move |node| {
            let queued = node
                .owner()
                .refunds()
                .iter()
                .all(|r| r.stage == openbmt_owner::Stage::Queued);
            (queued, node.owner().outbox().len())
        });
    assert!(stage_queued);
    assert_eq!(outbox, 0);
}

#[test]
fn ss_30_n13e_01_a_ready_refund_is_still_signed_and_handed_to_the_outbox() {
    let (rig, handle, sealed, data_key, refund) = node_with_ready_refund();
    // The check lets it through: whatever the signing says is not the
    // refusal of readiness.
    let said = node_pay(&rig, handle, &sealed, &data_key, refund);
    if let Some(fault) = Rd::error(&said) {
        assert_ne!(fault.rule, "starter-refund-ready");
        assert_ne!(fault.rule, "starter-refund");
    }
    let tx = finished_tx(5);
    let given = rig.call(210, handle, &W::new().bytes(&tx).u32(1).u32(refund).0);
    assert!(Rd::error(&given).is_none());
    let signed = rig
        .registry()
        .entry_for_test(handle)
        .expect("an entry")
        .node_for_test(move |node| {
            matches!(
                node.owner().refunds()[0].stage,
                openbmt_owner::Stage::Signed { .. }
            )
        });
    assert!(signed);
}

#[test]
fn lk_79_ss_38_the_app_gives_the_node_the_limit_and_the_stop_and_nothing_else_is_taken() {
    let rig = super::kit::Rig::new();
    let handle = rig.start();
    let set = |limit: u64, stop: u8| rig.call(227, handle, &W::new().u64(limit).u8(stop).0);
    Rd::ok(&set(100_000, 1)).done();
    // Again, with other values: the app gives them when they change.
    Rd::ok(&set(0, 0)).done();
    // A stop byte that is not 0 or 1 is refused, and so is a short frame.
    assert_eq!(Rd::error(&set(5, 2)).expect("refused").status, 1);
    let short = rig.call(227, handle, &W::new().u64(5).0);
    assert!(Rd::error(&short).is_some());
}
