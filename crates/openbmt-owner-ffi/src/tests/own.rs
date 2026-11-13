//! The owner phone of a shop with its own wallet, through `dispatch` (ops
//! 147 to 157, `docs/OWNER-APP-RULES.md` OA-105 to OA-113): the wallet's
//! file, the offer, the counter's request, the bundle and its words. The
//! counter is the core's `OwnCounterPairing`; the counter node itself is in
//! the counter FFI's end-to-end test.

use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::{OwnCounterPairing, OwnOffer};
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_owner::own_shop::own_approver;
use openbmt_owner::{Options, Owner};

use super::import::descriptor;
use super::kit::{tx_from, Scratch};
use super::wire::{frame, Fault, Rd, W};
use crate::{dispatch, Registry};

const T0: u64 = 1_000_000;

fn call(op: u16, fields: &[u8]) -> Vec<u8> {
    dispatch(&Registry::new(), &frame(op, 0, fields))
}

fn refusal(op: u16, fields: &[u8]) -> Fault {
    Rd::error(&call(op, fields)).expect("refused")
}

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// The wallet's file, made through op 148.
fn wallet_file() -> Vec<u8> {
    let response = call(148, &W::new().string(&descriptor()).0);
    let mut r = Rd::ok(&response);
    let (file, _, _) = (r.bytes(), r.string(), r.bool());
    r.done();
    file
}

fn shop(dir: &Scratch, wallet: &[u8]) -> W {
    W::new()
        .string(dir.path().to_str().unwrap())
        .bytes(&[1u8; 32])
        .bytes(wallet)
}

#[test]
fn oa_105_the_wallet_file_is_made_and_read_back_and_a_bad_one_gets_the_words() {
    let file = wallet_file();
    let response = call(149, &W::new().bytes(&file).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.bytes(), file);
    let first = r.string();
    assert!(!r.bool(), "single key");
    r.done();
    assert!(first.starts_with("bc1q"));
    // The same file, cut or grown, does not read, and the words say so.
    let mut longer = file.clone();
    longer.push(0);
    for bad in [&file[..file.len() - 1], &longer[..], &[][..]] {
        let fault = refusal(149, &W::new().bytes(bad).0);
        assert_eq!(fault.rule, "OA-105");
    }
    // The owner root is random and its secret is 32 bytes.
    let response = call(147, &[]);
    let mut r = Rd::ok(&response);
    let (secret, root) = (r.bytes(), r.bytes());
    r.done();
    assert_eq!((secret.len(), root.len()), (32, 32));
    assert_ne!(secret, root);
}

#[test]
fn oa_108_to_oa_111_the_offer_the_request_the_bundle_and_delivery_go_through_dispatch() {
    let dir = Scratch::new();
    let file = wallet_file();
    // OA-108: the offer.
    let response = call(150, &shop(&dir, &file).u64(T0).0);
    let mut r = Rd::ok(&response);
    let (code, made_at, ends_at, floor, first) = (r.bytes(), r.u64(), r.u64(), r.u32(), r.string());
    r.done();
    assert_eq!((made_at, ends_at, floor), (T0, T0 + 600, 0));
    let wallet = PinnedWallet::from_bytes(&file).unwrap();
    assert_eq!(first, wallet.receive_address(0).unwrap().to_string());

    // The counter reads the offer and makes its request (CR-228, CR-229).
    let offer = OwnOffer::from_bytes(&code).unwrap();
    let (counter, request) =
        OwnCounterPairing::begin(offer, &id(2), "Front counter", [2u8; 32], T0 + 5).unwrap();
    let request_bytes = request.to_bytes();

    // OA-109: the owner reads it and gets the counter's four words.
    let args = |now: u64| {
        shop(&dir, &file)
            .bytes(&code)
            .u64(T0)
            .u64(now)
            .bytes(&request_bytes)
    };
    let response = call(151, &args(T0 + 20).0);
    let mut r = Rd::ok(&response);
    let counter_key = r.bytes();
    let name = r.string();
    let words = r.list(Rd::string);
    let needs_rate = r.bool();
    let shares = r.list(|r| (r.bytes(), r.u32()));
    let left = r.u64();
    r.done();
    assert_eq!(counter_key, id(2).device_id().to_bytes());
    assert_eq!(name, "Front counter");
    assert_eq!(
        words,
        counter
            .session_words()
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
    );
    assert!(needs_rate, "the phone holds no settings yet");
    assert_eq!(shares.len(), 1);
    assert_eq!(left, 580);
    // A request is read at no cost: nothing waits.
    let response = call(154, &shop(&dir, &file).0);
    assert!(Rd::ok(&response)
        .list(|r| (r.bytes(), r.string()))
        .is_empty());

    // After the ten minutes the offer has ended (OA-108).
    assert_eq!(refusal(151, &args(T0 + 601).0).rule, "OA-108");

    // OA-110: "They match" makes the bundle.
    let confirm = |now: u64| {
        shop(&dir, &file)
            .bytes(&code)
            .u64(T0)
            .u64(now)
            .bytes(&request_bytes)
            .u64(100_000)
            .opt_bytes(None)
    };
    let response = call(152, &confirm(T0 + 30).0);
    let mut r = Rd::ok(&response);
    let (who, bundle, start, end, version) = (r.bytes(), r.bytes(), r.u32(), r.u32(), r.u32());
    let shares = r.list(|r| (r.bytes(), r.u32()));
    r.done();
    assert_eq!(who, counter_key);
    assert_eq!((start, end), (0, 1_000));
    assert!(version >= 1);
    assert_eq!(shares.len(), 1);
    // A counter added twice is refused.
    assert_eq!(refusal(152, &confirm(T0 + 31).0).rule, "OA-110");

    // OA-111: the bundle waits, comes again the same, and goes when delivered.
    let response = call(154, &shop(&dir, &file).0);
    let waiting = Rd::ok(&response).list(|r| (r.bytes(), r.string()));
    assert_eq!(waiting, vec![(counter_key.clone(), "Front counter".into())]);
    let named = |w: W| w.bytes(&counter_key);
    let response = call(155, &named(shop(&dir, &file)).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.bytes(), counter_key);
    assert_eq!(r.bytes(), bundle, "the same bundle again");
    let response = call(156, &named(shop(&dir, &file)).0);
    Rd::ok(&response).done();
    assert_eq!(refusal(155, &named(shop(&dir, &file)).0).rule, "OA-111");
    let response = call(154, &shop(&dir, &file).0);
    assert!(Rd::ok(&response)
        .list(|r| (r.bytes(), r.string()))
        .is_empty());
}

#[test]
fn oa_110_they_differ_gives_the_words_and_changes_nothing() {
    let response = call(153, &[]);
    let mut r = Rd::ok(&response);
    let words = r.words();
    r.done();
    assert_eq!(words.heading, "Pairing stopped");
}

#[test]
fn oa_113_the_gap_limit_is_worked_out_and_the_notice_shows_above_twenty() {
    let gap = |paid: &[u32], highest: Option<u32>| {
        let mut w = W::new().u32(paid.len() as u32);
        for p in paid {
            w = w.u32(*p);
        }
        let w = match highest {
            None => w.u8(0),
            Some(h) => w.u8(1).u32(h),
        };
        let response = call(157, &w.0);
        let mut r = Rd::ok(&response);
        let out = (r.u32(), r.bool());
        r.done();
        out
    };
    assert_eq!(gap(&[], None), (0, false));
    assert_eq!(gap(&[0], Some(0)), (1, false));
    assert_eq!(gap(&[0], Some(30)), (31, true));
    assert_eq!(gap(&[0, 5], Some(5)), (5, false));
}

#[test]
fn oa_105_a_secret_of_the_wrong_length_and_a_wallet_that_does_not_read_are_refused() {
    let dir = Scratch::new();
    let file = wallet_file();
    let short = W::new()
        .string(dir.path().to_str().unwrap())
        .bytes(&[1u8; 31])
        .bytes(&file);
    assert_eq!(refusal(150, &short.u64(T0).0).rule, "id-length");
    let bad = W::new()
        .string(dir.path().to_str().unwrap())
        .bytes(&[1u8; 32])
        .bytes(&file[..10]);
    assert_eq!(refusal(150, &bad.u64(T0).0).rule, "OA-105");
}

/// The own wallet's receive script `i`, from the wallet's file.
fn own_receive(file: &[u8], i: u32) -> bitcoin::ScriptBuf {
    PinnedWallet::from_bytes(file)
        .unwrap()
        .receive_script(i)
        .unwrap()
}

#[test]
fn oa_105_the_node_starts_for_a_shop_with_its_own_wallet_and_has_no_key_app() {
    let rig = super::kit::Rig::new();
    let file = wallet_file();
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    // A call that needs the Key app says there is none.
    let response = rig.call(
        45,
        handle,
        &W::new()
            .u64(T0)
            .bytes(&super::kit::counter_bytes())
            .u32(0)
            .u32(3)
            .0,
    );
    let fault = Rd::error(&response).expect("refused");
    assert_eq!(fault.rule, "no-key-app");
}

#[test]
fn oa_105_a_wallet_file_that_does_not_read_stops_the_start() {
    let rig = super::kit::Rig::new();
    let response = rig.start_own(&[1, 2, 3]);
    assert_eq!(Rd::error(&response).expect("refused").rule, "start-wallet");
}

#[test]
fn oa_113_the_gap_row_of_the_running_node_is_worked_out_from_the_payments_it_saw() {
    let rig = super::kit::Rig::new();
    let file = wallet_file();
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let gap_now = |rig: &super::kit::Rig| {
        let response = rig.call(158, handle, &[]);
        let mut r = Rd::ok(&response);
        let out = (r.u32(), r.bool());
        r.done();
        out
    };
    assert_eq!(gap_now(&rig), (0, false), "nothing paid yet");
    let pay0 = tx_from(7, &[(&own_receive(&file, 0), 50_000)]);
    let pay5 = tx_from(8, &[(&own_receive(&file, 5), 50_000)]);
    rig.mine(handle, &[pay0, pay5]);
    rig.mine_empty(handle, 5);
    // The same inputs through op 157 give the same row: paid 0 and 5, and
    // with no link the highest paid index stands for the highest handed out.
    let by_args = {
        let response = call(157, &W::new().u32(2).u32(0).u32(5).u8(1).u32(5).0);
        let mut r = Rd::ok(&response);
        let out = (r.u32(), r.bool());
        r.done();
        out
    };
    assert_eq!(gap_now(&rig), by_args);
    assert!(by_args.0 > 0);
}

#[test]
fn oa_113_a_shop_with_a_key_app_has_no_gap_row() {
    let rig = super::kit::Rig::new();
    let handle = rig.start();
    let fault = Rd::error(&rig.call(158, handle, &[])).expect("refused");
    assert_eq!(fault.rule, "no-own-wallet");
}

fn own_build(rig: &super::kit::Rig, handle: u64, picked: &[u32]) -> Vec<u8> {
    let mut w = W::new()
        .u64(T0)
        .u64(1_000)
        .u32(u32::try_from(picked.len()).unwrap());
    for p in picked {
        w = w.u32(*p);
    }
    rig.call(159, handle, &w.0)
}

#[test]
fn as_101_oa_115_the_views_of_the_own_spend_are_written_in_the_order_the_binding_reads() {
    use crate::codec::Put;
    use crate::own_spend_views::{
        OwnSpendBuiltView, OwnSpendFileView, OwnSpendItemView, OwnSpendSessionView,
        OwnSpendTakenView,
    };
    use crate::signer_views::SignerLineView;
    use crate::views::{SkippedView, WordsView};
    let item = OwnSpendItemView {
        number: 3,
        txid: vec![5; 32],
        refunds: vec![1, 2],
        cash_out: Some(9),
        filed: true,
        payments: vec![SignerLineView {
            address: "bc1qx".to_string(),
            sats: 40_000,
        }],
        change: None,
        fee: 300,
    };
    let mut bytes = Vec::new();
    item.put(&mut bytes);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.u32(), 3);
    assert_eq!(r.bytes(), vec![5; 32]);
    assert_eq!(r.list(|r| r.u32()), vec![1, 2]);
    assert_eq!(r.opt(|r| r.u32()), Some(9));
    assert!(r.bool());
    assert_eq!(
        r.list(|r| (r.string(), r.u64())),
        vec![("bc1qx".to_string(), 40_000)]
    );
    assert_eq!(r.opt(|r| (r.string(), r.u64())), None);
    assert_eq!(r.u64(), 300);
    r.done();
    // The built view: the session first, then the items, then what was left out.
    let words = openbmt_owner::own_spend::compare_words();
    let built = OwnSpendBuiltView {
        session: Some(vec![7; 32]),
        items: vec![item.clone()],
        skipped: vec![SkippedView {
            id: 4,
            reason: "why".to_string(),
            words: WordsView(words.clone()),
        }],
    };
    let mut bytes = Vec::new();
    built.put(&mut bytes);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.opt(|r| r.bytes()), Some(vec![7; 32]));
    assert_eq!(r.u32(), 1);
    let mut one = Vec::new();
    item.put(&mut one);
    assert_eq!(&bytes[r.pos()..r.pos() + one.len()], &one[..]);
    // The session, the file and what was taken.
    let mut bytes = Vec::new();
    OwnSpendSessionView {
        session: vec![7; 32],
        items: vec![],
    }
    .put(&mut bytes);
    assert_eq!(bytes.len(), 4 + 32 + 4);
    let mut bytes = Vec::new();
    OwnSpendFileView {
        psbt: vec![1, 2],
        txid: vec![5; 32],
        file_name: "ab.psbt".to_string(),
        payments: vec![],
        change: None,
        fee: 7,
        words: WordsView(words),
    }
    .put(&mut bytes);
    let mut r = Rd::new(&bytes);
    assert_eq!(r.bytes(), vec![1, 2]);
    assert_eq!(r.bytes(), vec![5; 32]);
    assert_eq!(r.string(), "ab.psbt");
    assert_eq!(r.u32(), 0);
    assert!(r.opt(|r| r.u8()).is_none());
    assert_eq!(r.u64(), 7);
    let mut bytes = Vec::new();
    OwnSpendTakenView {
        id: 1,
        txid: vec![5; 32],
        fee: 9,
        sends: vec![],
    }
    .put(&mut bytes);
    let mut r = Rd::new(&bytes);
    assert_eq!(
        (r.u32(), r.bytes(), r.u64(), r.u32()),
        (1, vec![5; 32], 9, 0)
    );
    r.done();
}

#[test]
fn as_101_a_node_that_is_not_ready_refuses_to_build_and_says_so() {
    let rig = super::kit::Rig::new();
    let file = wallet_file();
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let fault = Rd::error(&own_build(&rig, handle, &[1])).expect("refused");
    assert_eq!(fault.rule, "not-ready");
    let fault = Rd::error(&rig.call(164, handle, &W::new().u64(T0).0)).expect("refused");
    assert!(!fault.rule.is_empty());
}

#[test]
fn as_100_the_ops_of_the_own_wallet_are_refused_for_a_shop_with_a_key_app() {
    let rig = super::kit::Rig::new();
    let handle = rig.start();
    assert!(Rd::error(&own_build(&rig, handle, &[1])).is_some());
    assert!(Rd::error(&rig.call(164, handle, &W::new().u64(T0).0)).is_some());
    let response = rig.call(160, handle, &[]);
    let mut r = Rd::ok(&response);
    assert_eq!(r.u32(), 0);
    r.done();
}

#[test]
fn as_103_a_bad_session_number_is_refused_by_length_and_an_unknown_one_by_name() {
    let rig = super::kit::Rig::new();
    let file = wallet_file();
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let fault = Rd::error(&rig.call(161, handle, &W::new().bytes(&[1, 2]).u32(0).0)).unwrap();
    assert_eq!(fault.rule, "id-length");
    let fault = Rd::error(&rig.call(163, handle, &W::new().bytes(&[9u8; 32]).0)).expect("refused");
    assert_eq!(fault.rule, "session-unknown");
}

#[test]
fn cr_256_op_200_makes_the_offer_marked_as_a_starter_wallets_and_op_150_does_not() {
    let dir = Scratch::new();
    let file = wallet_file();
    let code_of = |op: u16| {
        let response = call(op, &shop(&dir, &file).u64(T0).0);
        let mut r = Rd::ok(&response);
        let (code, _, _, _, _) = (r.bytes(), r.u64(), r.u64(), r.u32(), r.string());
        r.done();
        OwnOffer::from_bytes(&code).unwrap()
    };
    let plain = code_of(150);
    let starter = code_of(200);
    assert!(!plain.starter);
    assert!(starter.starter);
    assert_eq!(plain.wallet, starter.wallet);
}

#[test]
fn ss_41_the_switch_ops_say_why_when_there_is_no_own_wallet_or_no_link() {
    let new = wallet_file();
    // A shop with a Key app has no starter wallet to switch from.
    let rig = super::kit::Rig::new();
    let handle = rig.start();
    for (op, fields) in [
        (201u16, W::new().bytes(&new).u64(T0).0),
        (202, W::new().bytes(&new).0),
    ] {
        let fault = Rd::error(&rig.call(op, handle, &fields)).expect("refused");
        assert_eq!(fault.rule, "no-own-wallet", "op {op}");
    }
    // A shop with its own wallet and no link has no counters to ask.
    let rig = super::kit::Rig::new();
    let handle = Rd::ok(&rig.start_own(&new)).u64();
    for (op, fields) in [
        (201u16, W::new().bytes(&new).u64(T0).0),
        (202, W::new().bytes(&new).0),
    ] {
        let fault = Rd::error(&rig.call(op, handle, &fields)).expect("refused");
        assert_eq!(fault.rule, "link-none", "op {op}");
    }
    // A new wallet file that does not read gets the words of OA-105.
    let fault = Rd::error(&rig.call(202, handle, &W::new().bytes(&[1, 2, 3]).0)).expect("refused");
    assert_eq!(fault.rule, "OA-105");
}

#[test]
fn ss_42_the_move_after_the_switch_is_refused_with_its_words_when_a_counter_waits() {
    use openbmt_owner::own_switch::MoveNotReady;
    let e = MoveNotReady {
        waiting: vec![id(2).device_id()],
        refused: Vec::new(),
    };
    let fault = crate::starter::not_ready(&e);
    let text = format!("{fault:?}");
    assert!(text.contains("starter-move-not-ready"), "{text}");
    assert!(text.contains("Not every counter switched"), "{text}");
}

/// A transaction that spends `from` and pays each of `outs`.
fn spend(
    from: &bitcoin::Transaction,
    vout: u32,
    outs: &[(&bitcoin::ScriptBuf, u64)],
) -> bitcoin::Transaction {
    let mut tx = tx_from(0, outs);
    tx.input[0].previous_output = bitcoin::OutPoint::new(from.compute_txid(), vout);
    tx
}

#[test]
fn ss_36_ss_43_the_starter_wallets_coins_and_confirmations_come_from_the_node_watch() {
    use openbmt_core::wallet::Keychain;
    // These ops read the node's watch only, so any node serves; the one of
    // the test kit is on the regtest chain, which the rig can mine.
    let rig = super::kit::Rig::new();
    let handle = rig.start();
    let held = || Rd::ok(&rig.call(208, handle, &[])).u64();
    let confirmations = |txid: &str| {
        let response = rig.call(211, handle, &W::new().string(txid).0);
        let mut r = Rd::ok(&response);
        let out = r.opt(|r| r.u32());
        r.done();
        out
    };
    rig.mine_empty(handle, 1);
    assert_eq!(held(), 0, "nothing is paid yet");
    // A payment into the wallet that a block holds is held.
    let pay = tx_from(7, &[(&super::kit::receive(0), 50_000)]);
    rig.mine(handle, std::slice::from_ref(&pay));
    rig.mine_empty(handle, 5);
    assert_eq!(held(), 50_000);
    // A transaction the owner phone signed goes to the node's outbox: it
    // pays 20,000 away and 29,000 back to the wallet's change. What it
    // spent no longer counts and its change counts once.
    let change = super::kit::kit()
        .wallet
        .script(Keychain::Change, 0)
        .unwrap();
    let away = super::kit::external(3);
    let out = spend(&pay, 0, &[(&away, 20_000), (&change, 29_000)]);
    let txid = out.compute_txid().to_string();
    assert_eq!(
        confirmations(&txid),
        None,
        "the node knows nothing of it yet"
    );
    let sent = |tx: &bitcoin::Transaction, refunds: &[u32]| {
        let mut w = W::new()
            .bytes(&bitcoin::consensus::encode::serialize(tx))
            .u32(u32::try_from(refunds.len()).unwrap());
        for r in refunds {
            w = w.u32(*r);
        }
        rig.call(210, handle, &w.0)
    };
    assert_eq!(Rd::ok(&sent(&out, &[])).string(), txid);
    // The change chain is watched from now on: the driver rechecks the
    // blocks for it, as the rig does.
    rig.feed(handle, Vec::new());
    assert_eq!(held(), 29_000);
    assert_eq!(confirmations(&txid), Some(0), "held, and in no block");
    // Giving it again changes nothing.
    assert_eq!(Rd::ok(&sent(&out, &[])).string(), txid);
    assert_eq!(held(), 29_000);
    // A block holds it: one confirmation, then the tip counts.
    rig.mine(handle, std::slice::from_ref(&out));
    assert_eq!(confirmations(&txid), Some(1));
    rig.mine_empty(handle, 2);
    assert_eq!(confirmations(&txid), Some(3));
    assert_eq!(held(), 29_000);
    // What does not read, and a refund that is not queued, are refused.
    let fault = Rd::error(&rig.call(211, handle, &W::new().string("zz").0)).expect("refused");
    assert_eq!(fault.rule, "id-length");
    let fault =
        Rd::error(&rig.call(210, handle, &W::new().bytes(&[1, 2, 3]).u32(0).0)).expect("refused");
    assert_eq!(fault.status, 1);
    let missing = rig.call(
        209,
        handle,
        &W::new()
            .bytes(&[9; 10])
            .bytes(&[7; 32])
            .u32(77)
            .u64(1_000)
            .u32(1)
            .0,
    );
    assert_eq!(Rd::error(&missing).expect("refused").rule, "starter-refund");
}

#[test]
fn ss_41_ss_42_the_move_and_the_repin_need_the_node_and_the_counters() {
    let rig = super::kit::Rig::new();
    let file = wallet_file();
    // A shop with a Key app has no starter wallet to move or re-pin.
    let key_app = rig.start();
    let move_args = || {
        W::new()
            .bytes(&[9; 10])
            .bytes(&[7; 32])
            .string(&descriptor())
            .u64(T0)
            .u64(1_000)
            .u32(1)
            .0
    };
    let fault = Rd::error(&rig.call(203, key_app, &move_args())).expect("refused");
    assert_eq!(fault.rule, "no-own-wallet");
    let fault =
        Rd::error(&rig.call(212, key_app, &W::new().string(&descriptor()).0)).expect("refused");
    assert_eq!(fault.rule, "no-own-wallet");
    // With no link there are no counters to ask.
    let rig = super::kit::Rig::new();
    let own = Rd::ok(&rig.start_own(&file)).u64();
    for (op, fields) in [
        (203u16, move_args()),
        (212, W::new().string(&descriptor()).0),
    ] {
        let fault = Rd::error(&rig.call(op, own, &fields)).expect("refused");
        assert_eq!(fault.rule, "link-none", "op {op}");
    }
    // A description that does not read, and a data key of the wrong size,
    // are refused first.
    let fault = Rd::error(&rig.call(212, own, &W::new().string("nope").0)).expect("refused");
    assert_eq!(fault.rule, "starter-new-wallet");
    let short = W::new()
        .bytes(&[9; 10])
        .bytes(&[7; 5])
        .string(&descriptor())
        .u64(T0)
        .u64(1_000)
        .u32(1)
        .0;
    let fault = Rd::error(&rig.call(203, own, &short)).expect("refused");
    assert_eq!(fault.rule, "SS-20");
}

/// A second single-key wallet, for the re-pin.
const OTHER: &str = "wpkh([37b5eed4/84h/0h/0h]xpub6BkU445MSEBXbPjD3g2c2ch6mn8yy1SXXQUM7EwjgYiq6Wt1NDwDZ45npqWcV8uQC5oi2gHuVukoCoZZyT4HKq8EpotPMqGqxdZRuapCQ23/<0;1>/*)#yhygl07f";

fn file_of(descriptor: &str) -> Vec<u8> {
    let response = call(148, &W::new().string(descriptor).0);
    let mut r = Rd::ok(&response);
    let (file, _, _) = (r.bytes(), r.string(), r.bool());
    r.done();
    file
}

#[test]
fn ss_41_a_start_between_the_repin_and_its_wallet_file_finishes_the_repin() {
    use openbmt_owner_node::OwnNode;
    let rig = super::kit::Rig::new();
    let (old_file, new_file) = (wallet_file(), file_of(OTHER));
    let (old, new) = (
        PinnedWallet::from_bytes(&old_file).unwrap(),
        PinnedWallet::from_bytes(&new_file).unwrap(),
    );
    // The shop's state was stored for the new wallet, with the re-pin
    // recorded, and the app's file is still the old one.
    let identity = Identity::from_secret_bytes(&super::kit::OWNER_SECRET).unwrap();
    let mut node = OwnNode::open(&rig.dir(), identity, old.clone()).unwrap();
    let (shop, store) = node.shop_and_store();
    shop.repin(&new, &[], store).unwrap();
    drop(node);
    // The owner's queue is still bound to the old wallet (N13F-03).
    let approver = own_approver(&super::kit::OWNER_SECRET).unwrap();
    let policy = BatchPolicy { min_count: 1 };
    let queue = rig.dir().join("owner-queue.bin");
    let queued = Owner::new(
        &super::kit::OWNER_SECRET,
        old.wallet().clone(),
        approver,
        policy,
        Options::default(),
    )
    .unwrap();
    std::fs::write(&queue, queued.snapshot().unwrap()).unwrap();
    // The old file finishes the re-pin: the pinned wallet is the new one, to
    // be written, and asking again gives the same.
    let handle = Rd::ok(&rig.start_own(&old_file)).u64();
    for _ in 0..2 {
        let response = rig.call(213, handle, &[]);
        let mut r = Rd::ok(&response);
        assert_eq!(r.bytes(), new_file);
        r.done();
    }
    // No late payment is held: the node has seen no block.
    let response = rig.call(214, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.bytes()).is_empty());
    r.done();
    // The start stored the queue for the new wallet, so the node starts
    // again with it.
    let bound = |wallet: &PinnedWallet| {
        Owner::restore(
            &super::kit::OWNER_SECRET,
            wallet.wallet().clone(),
            approver,
            policy,
            &std::fs::read(&queue).unwrap(),
        )
    };
    assert!(bound(&new).is_ok());
    assert!(bound(&old).is_err());
}

#[test]
fn ss_43_the_late_payment_ops_need_an_own_wallet_the_former_wallets_coins_and_a_key() {
    let rig = super::kit::Rig::new();
    let key_app = rig.start();
    let fault = Rd::error(&rig.call(213, key_app, &[])).expect("refused");
    assert_eq!(fault.rule, "no-own-wallet");
    let args = |wallet: &[u8], key: &[u8]| {
        W::new()
            .bytes(&[9; 10])
            .bytes(key)
            .bytes(wallet)
            .u64(T0)
            .u64(1_000)
            .u32(1)
            .0
    };
    // A checksum of the wrong size, and a key of the wrong size, are refused
    // first.
    let fault = Rd::error(&rig.call(215, key_app, &args(&[1; 5], &[7; 32]))).expect("refused");
    assert_eq!(fault.rule, "id-length");
    let fault = Rd::error(&rig.call(215, key_app, &args(&[1; 32], &[7; 5]))).expect("refused");
    assert_eq!(fault.rule, "SS-20");
    let fault = Rd::error(&rig.call(215, key_app, &args(&[1; 32], &[7; 32]))).expect("refused");
    assert_eq!(fault.rule, "no-own-wallet");
    // An own-wallet node has no former wallet and nothing to move.
    let rig = super::kit::Rig::new();
    let own = Rd::ok(&rig.start_own(&wallet_file())).u64();
    let fault = Rd::error(&rig.call(215, own, &args(&[1; 32], &[7; 32]))).expect("refused");
    assert_eq!(fault.rule, "starter-not-watching");
}

/// The node of a shop with its own wallet and a link to one counter.
fn start_own_linked(rig: &super::kit::Rig, wallet: &[u8]) -> u64 {
    use super::kit::{counter_bytes, peers_event, start_config};
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.network = crate::OwnerNetwork::Mainnet;
    config.own_wallet = Some(wallet.to_vec());
    config.approver_root = Vec::new();
    config.wallet = String::new();
    config.key1_account = Vec::new();
    config.key3_xpub = None;
    config.link = Some(crate::LinkConfig {
        route: crate::RouteView::Direct {
            addr: "127.0.0.1:9".to_string(),
        },
        relay_key: id(0x41).device_id().to_bytes().to_vec(),
        counters: vec![counter_bytes()],
        heartbeat_every_secs: 60,
        check_in_grace_secs: 3_600,
    });
    let handle = Rd::ok(&rig.call(1, 0, &W::new().start(&config).0)).u64();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    handle
}

/// The files of the data folder with what they hold, to see that a refused
/// re-pin wrote none.
fn folder(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            let bytes = std::fs::read(e.path()).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), bytes))
        })
        .collect();
    all.sort();
    all
}

#[test]
fn ss_41_a_repin_that_waits_comes_back_with_its_words_and_code_and_writes_nothing() {
    use super::kit::{external, OWNER_SECRET, T0};
    let rig = super::kit::Rig::new();
    let (old_file, new_text) = (wallet_file(), OTHER);
    let handle = start_own_linked(&rig, &old_file);
    let repin = |handle: u64| rig.call(212, handle, &W::new().string(new_text).0);
    let waits = |response: &[u8], heading: &str| {
        let fault = Rd::error(response).expect("refused");
        assert_eq!(fault.rule, "SS-41");
        assert_eq!(fault.words.code.as_deref(), Some("SS-41"));
        assert_eq!(fault.words.heading, heading);
    };
    // Another payment of the starter wallet is in no block: it is the
    // unsettled queue that waits, with the words written for it.
    let pay = super::kit::tx_from(7, &[(&external(3), 20_000)]);
    let send = W::new()
        .bytes(&bitcoin::consensus::encode::serialize(&pay))
        .u32(0)
        .0;
    Rd::ok(&rig.call(210, handle, &send)).string();
    let before = folder(&rig.dir());
    waits(&repin(handle), "Upgrade waits for a payment");
    assert_eq!(folder(&rig.dir()), before, "nothing is written");
    // An offer is shown: the pairing waits first, with the placeholder
    // words that name the rule until they are written.
    let shop_args = W::new()
        .string(rig.dir().to_str().unwrap())
        .bytes(&OWNER_SECRET)
        .bytes(&old_file);
    Rd::ok(&call(150, &shop_args.u64(T0).0)).bytes();
    let before = folder(&rig.dir());
    waits(&repin(handle), "Upgrade waits for a pairing");
    assert_eq!(folder(&rig.dir()), before, "nothing is written");
    // "They differ" gives the offer up: the unsettled queue waits again.
    Rd::ok(&call(153, &[]));
    waits(&repin(handle), "Upgrade waits for a payment");
}
