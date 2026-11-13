//! The ops of the owner's requests through `dispatch` (ops 216 to 226,
//! `docs/REQUEST-RULES.md`): a shop with its own wallet makes, lists, shares
//! and ends requests; a shop with a Key app has none (RQ-01).

use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::{OwnCounterPairing, OwnOffer};
use openbmt_core::pinned_wallet::PinnedWallet;

use super::import::descriptor;
use super::kit::{Rig, OWNER_SECRET};
use super::wire::{frame, Rd, W};
use crate::dispatch;

const T0: u64 = 1_000_000;

fn wallet_file(rig: &Rig) -> Vec<u8> {
    let response = rig.call(148, 0, &W::new().string(&descriptor()).0);
    let mut r = Rd::ok(&response);
    let (file, _, _) = (r.bytes(), r.string(), r.bool());
    r.done();
    file
}

fn shop(rig: &Rig, wallet: &[u8]) -> W {
    W::new()
        .string(rig.dir().to_str().expect("UTF-8"))
        .bytes(&OWNER_SECRET)
        .bytes(wallet)
}

/// The shop learns its settings by adding a counter (OA-110).
fn settle(rig: &Rig, file: &[u8]) {
    let code = Rd::ok(&dispatch(
        rig.registry(),
        &frame(150, 0, &shop(rig, file).u64(T0).0),
    ))
    .bytes();
    let offer = OwnOffer::from_bytes(&code).expect("an offer");
    let id = Identity::from_secret_bytes(&[2; 32]).expect("a key");
    let (_, request) = OwnCounterPairing::begin(offer, &id, "Front counter", [2u8; 32], T0 + 5)
        .expect("a request");
    let confirm = shop(rig, file)
        .bytes(&code)
        .u64(T0)
        .u64(T0 + 30)
        .bytes(&request.to_bytes())
        .u64(100_000)
        .opt_bytes(None);
    Rd::ok(&rig.call(152, 0, &confirm.0));
    // The wallet is the one the node starts with.
    assert!(PinnedWallet::from_bytes(file).is_ok());
}

/// What every request op is given: the time, the offset, no rate.
fn ask(now: u64) -> W {
    W::new().u64(now).u32(0).u8(0)
}

fn make_args(now: u64, note: &str, amount: u64, price_in: u8, due: (u32, u32, u32)) -> Vec<u8> {
    ask(now)
        .string(note)
        .u64(amount)
        .u8(price_in)
        .u32(due.0)
        .u32(due.1)
        .u32(due.2)
        .u8(0)
        .0
}

#[test]
fn rq_01_a_shop_with_a_key_app_has_no_requests() {
    let rig = Rig::new();
    let handle = rig.start();
    let response = rig.call(225, handle, &ask(T0).0);
    let mut r = Rd::ok(&response);
    let off = r.opt(|r| r.words()).expect("requests are off");
    assert_eq!(off.heading, "Requests need own wallet");
    assert!(!r.bool(), "no settings given");
    assert_eq!(r.u32(), 0);
    let fault = Rd::error(&rig.call(216, handle, &make_args(T0, "Rent", 5_000, 0, (2027, 1, 1))))
        .expect("refused");
    assert_eq!(fault.status, 3, "a state, not a rule");
    assert!(fault.text.starts_with("RQ-01"), "{}", fault.text);
    assert_eq!(fault.words.heading, "Requests need own wallet");
}

#[test]
fn rq_35_a_shop_that_has_no_settings_yet_gives_none_of_its_calls() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let fault = Rd::error(&rig.call(216, handle, &make_args(T0, "Rent", 5_000, 0, (2027, 1, 1))))
        .expect("refused");
    assert_eq!(fault.rule, "RQ-35");
    // The desk took no range for a call that was refused.
    let fault = Rd::error(&rig.call(223, handle, &ask(T0).0)).map_or(String::new(), |f| f.rule);
    assert_eq!(fault, "RQ-35");
}

#[test]
fn rq_03_rq_21_rq_13_a_request_is_made_listed_shared_and_cancelled() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    // A date that does not exist is refused before anything is taken.
    let bad = Rd::error(&rig.call(216, handle, &make_args(T0, "Rent", 5_000, 0, (2027, 2, 30))))
        .expect("refused");
    assert_eq!(bad.rule, "RQ-09");

    let response = rig.call(216, handle, &make_args(T0, "Rent", 5_000, 0, (1970, 1, 22)));
    let mut r = Rd::ok(&response);
    let (number, index, address) = (r.u64(), r.u32(), r.string());
    r.done();
    assert_eq!(number, 1);
    assert_ne!(index, 0, "the first index is the offer's");
    assert!(address.starts_with("bc1"), "{address}");

    // The list: one waiting, nothing finished.
    let response = rig.call(217, handle, &ask(T0).0);
    let mut r = Rd::ok(&response);
    let counts: Vec<usize> = (0..5)
        .map(|part| {
            r.list(|r| {
                let (n, note) = row(r);
                assert_eq!((n, note.as_str()), (1, "Rent"), "part {part}");
            })
            .len()
        })
        .collect();
    assert_eq!(counts, vec![0, 0, 0, 1, 0]);
    assert!(r.opt(|r| r.string()).is_none());
    // The words of the empty list come with the list (RQ-35).
    assert_eq!(r.string(), "No open requests");
    assert!(!r.string().is_empty());
    r.done();

    // Sharing writes the quote and gives the link.
    let share = ask(T0).u64(1).string("Corner Cafe").0;
    let response = rig.call(218, handle, &share);
    let mut r = Rd::ok(&response);
    let (n, owed, sats) = (r.u64(), r.u64(), r.u64());
    assert_eq!((n, owed, sats), (1, 5_000, 5_000));
    assert!(
        r.opt(|r| r.u64()).is_none(),
        "a request in bitcoin has no rate"
    );
    assert!(r.opt(|r| r.u64()).is_none());
    assert_eq!(r.string(), address);
    let link = r.string();
    assert!(link.starts_with("bitcoin:"), "{link}");
    assert!(r.string().starts_with("Corner Cafe asks for 5000 SAT."));
    r.done();

    // The state: one open request, no returns.
    let response = rig.call(225, handle, &ask(T0).0);
    let mut r = Rd::ok(&response);
    assert!(r.opt(|r| r.words()).is_none());
    assert!(r.bool());
    assert_eq!((r.u32(), r.u32(), r.u32()), (1, 0, 0));
    assert!(r.opt(|r| r.string()).is_none());
    assert_eq!((r.u32(), r.u32(), r.u64()), (0, 0, 0));
    assert!(r.opt(|r| r.words()).is_none());
    r.done();

    // Cancelling an unpaid request ends it; it moves to the finished.
    Rd::ok(&rig.call(219, handle, &ask(T0 + 5).u64(1).0)).done();
    let response = rig.call(217, handle, &ask(T0 + 5).0);
    let mut r = Rd::ok(&response);
    for _ in 0..4 {
        assert!(r.list(|r| row(r)).is_empty());
    }
    assert_eq!(r.list(|r| row(r)).len(), 1);
    // A second cancel is refused with the library's words.
    let fault = Rd::error(&rig.call(219, handle, &ask(T0 + 6).u64(1).0)).expect("refused");
    assert!(fault.rule.starts_with("RQ-"), "{}", fault.rule);

    // The books carry no sale for it and ask nothing of the shop's links.
    let response = rig.call(144, handle, &W::new().u32(0).0);
    assert!(Rd::error(&response).is_none());
}

/// One row of the list: the number and the note, the rest read and left.
fn row(r: &mut Rd<'_>) -> (u64, String) {
    let (number, note) = (r.u64(), r.string());
    let _owed = r.u64();
    let _due = (r.u32(), r.u32(), r.u32());
    let _lines = (r.string(), r.string());
    let _marks = r.list(Rd::string);
    let _repeats = r.bool();
    (number, note)
}

#[test]
fn rq_25_no_repeat_is_due_when_none_repeats() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let response = rig.call(223, handle, &ask(T0).0);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.u64()).is_empty());
    r.done();
}

fn return_args(now: u64, number: u64, amount: u64, address: &str) -> Vec<u8> {
    ask(now)
        .u64(number)
        .u8(0)
        .u64(amount)
        .string(address)
        .string("Corner Cafe")
        .0
}

#[test]
fn rq_27_rq_29_the_ask_line_is_written_and_a_return_of_nothing_received_is_refused() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    Rd::ok(&rig.call(216, handle, &make_args(T0, "Rent", 5_000, 0, (1970, 1, 22))));
    let line = Rd::ok(&rig.call(226, handle, &return_args(T0, 1, 500, ""))).string();
    assert!(
        line.starts_with("Corner Cafe would like to return 500 SAT for request No. 1."),
        "{line}"
    );
    // No request has that number.
    let fault = Rd::error(&rig.call(226, handle, &return_args(T0, 9, 500, ""))).expect("refused");
    assert!(fault.rule.starts_with("RQ-"), "{}", fault.rule);
    // A return needs money that arrived, and the address is read first.
    let address = super::kit::external_address(7);
    let fault =
        Rd::error(&rig.call(222, handle, &return_args(T0, 1, 500, &address))).expect("refused");
    assert!(fault.rule.starts_with("RQ-"), "{}", fault.rule);
    let fault = Rd::error(&rig.call(222, handle, &return_args(T0, 1, 500, "not an address")))
        .expect("refused");
    assert!(fault.rule.starts_with("RQ-"), "{}", fault.rule);
}

#[test]
fn rq_32_the_ledger_carries_a_line_for_each_request_and_the_days_stay_apart() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    Rd::ok(&rig.call(
        216,
        handle,
        &make_args(T0, "Rent, march", 5_000, 0, (1970, 1, 22)),
    ));
    let bytes = Rd::ok(&rig.call(146, handle, &[1])).bytes();
    let text = String::from_utf8(bytes).expect("UTF-8");
    let mut lines = text.split("\r\n");
    assert_eq!(lines.next(), Some(""));
    assert!(lines
        .next()
        .expect("header")
        .starts_with("Number,Note,Made,Due,Priced in,"));
    let line = lines.next().expect("a line");
    assert!(line.starts_with("1,\"Rent, march\",1970-01-12T13:46:40Z,1970-01-22,SAT,5000,"));
    assert_eq!(lines.next(), Some(""));
    assert_eq!(lines.next(), None);
    // The books give no day for a request that received nothing.
    let response = rig.call(144, handle, &W::new().u32(0).0);
    let mut r = Rd::ok(&response);
    let _currency = r.string();
    assert!(r
        .list(|r| (r.string(), r.u64(), r.u64(), r.string()))
        .is_empty());
    assert!(r.list(|r| (r.string(), r.u64())).is_empty());
}

/// The row a detail starts with: number and note read, the rest left.
fn detail_head(r: &mut Rd<'_>) -> (u64, String) {
    row(r)
}

#[test]
fn rq_35_rq_12_rq_19_one_request_is_read_with_its_quotes_and_the_words_of_its_questions() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    Rd::ok(&rig.call(216, handle, &make_args(T0, "Rent", 5_000, 0, (1970, 1, 22))));
    let one = ask(T0).u64(1).0;

    // Before any share: waiting, nothing paid, no quote, no payment, no return.
    let response = rig.call(227, handle, &one);
    let mut r = Rd::ok(&response);
    assert_eq!(detail_head(&mut r), (1, "Rent".to_string()));
    assert_eq!(r.u8(), 0, "waiting");
    assert!(r.opt(|r| r.u8()).is_none(), "not ended");
    assert!(r.opt(|r| r.u64()).is_none());
    assert_eq!(r.u8(), 0, "priced in bitcoin");
    assert_eq!(r.string(), "SAT");
    assert_eq!((r.u64(), r.u64(), r.u64(), r.u64()), (5_000, 0, 0, 0));
    assert_eq!(r.u64(), T0, "made at");
    assert_eq!((r.u32(), r.u32(), r.u32()), (1970, 1, 12));
    assert!(r.opt(|r| r.u8()).is_none(), "no repeat");
    assert!(r.list(|r| r.u64()).is_empty(), "no quote yet");
    assert!(r.list(|r| r.u64()).is_empty(), "no payment");
    assert!(r.list(|r| r.u64()).is_empty(), "no return");
    assert!(r.opt(|r| r.string()).is_none(), "nothing over-returned");
    for _ in 0..3 {
        let (title, text) = (r.string(), r.string());
        let (yes, no) = (r.string(), r.string());
        assert!(!title.is_empty() && !text.is_empty() && !yes.is_empty() && !no.is_empty());
    }
    r.done();

    // A share writes a quote, and the read shows it.
    Rd::ok(&rig.call(218, handle, &ask(T0 + 1).u64(1).string("Corner Cafe").0));
    let response = rig.call(227, handle, &one);
    let mut r = Rd::ok(&response);
    detail_head(&mut r);
    let _ = (
        r.u8(),
        r.opt(|r| r.u8()),
        r.opt(|r| r.u64()),
        r.u8(),
        r.string(),
    );
    let _ = (r.u64(), r.u64(), r.u64(), r.u64(), r.u64());
    let _ = (r.u32(), r.u32(), r.u32(), r.opt(|r| r.u8()));
    let quotes = r.list(|r| {
        (
            r.u64(),
            r.u64(),
            r.u64(),
            r.opt(|r| r.u64()),
            r.opt(|r| r.u64()),
        )
    });
    assert_eq!(quotes, vec![(T0 + 1, 5_000, 5_000, None, None)]);

    // Cancelling ends it; the read says how, and when.
    Rd::ok(&rig.call(219, handle, &ask(T0 + 5).u64(1).0));
    let response = rig.call(227, handle, &one);
    let mut r = Rd::ok(&response);
    detail_head(&mut r);
    assert_eq!(r.u8(), 0);
    assert_eq!(r.opt(|r| r.u8()), Some(0), "cancelled");
    assert_eq!(r.opt(|r| r.u64()), Some(T0 + 5));

    // A request that is not there is refused with the library's words.
    let fault = Rd::error(&rig.call(227, handle, &ask(T0).u64(9).0)).expect("refused");
    assert!(fault.rule.starts_with("RQ-"), "{}", fault.rule);
}

#[test]
fn rq_01_a_shop_with_a_key_app_reads_no_request() {
    let rig = Rig::new();
    let handle = rig.start();
    let fault = Rd::error(&rig.call(227, handle, &ask(T0).u64(1).0)).expect("refused");
    assert!(fault.rule.starts_with("RQ-"), "{}", fault.rule);
}
