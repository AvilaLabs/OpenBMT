//! The owner phone of a shop with its own wallet, through `dispatch` (ops
//! 147 to 157, `docs/OWNER-APP-RULES.md` OA-105 to OA-113): the wallet's
//! file, the offer, the counter's request, the bundle and its words. The
//! counter is the core's `OwnCounterPairing`; the counter node itself is in
//! the counter FFI's end-to-end test.

use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::{OwnCounterPairing, OwnOffer};
use openbmt_core::pinned_wallet::PinnedWallet;

use super::import::descriptor;
use super::kit::Scratch;
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
