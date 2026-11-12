//! RQ-26 to RQ-29: returning money.

mod common;

use bitcoin::{Network, ScriptBuf};
use common::*;
use openbmt_requests::say::{ask_line, return_added};
use openbmt_requests::{
    address_in_groups, check_return_address, AddressRefused, RequestBook, ReturnAmount,
    ReturnRefused, ReturnState, MAX_QUEUED_RETURNS,
};

/// A book with request 1, asking 60,000 sats and paid 100,000.
fn overpaid() -> RequestBook {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 60_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    book
}

fn not_own(_: &bitcoin::Script) -> bool {
    false
}

#[test]
fn rq_26_money_can_be_returned_from_a_request_that_received_something() {
    let mut book = overpaid();
    let id = book
        .queue_return(
            1,
            ReturnAmount::Sats(40_000),
            script_of(P2WPKH),
            T0 + 1,
            None,
        )
        .unwrap();
    assert_eq!(id, 1);
    let r = &book.returns()[0];
    assert_eq!((r.request, r.sats, r.queued_at), (1, 40_000, T0 + 1));
    assert_eq!(r.state, ReturnState::Queued);
}

#[test]
fn rq_26_part_or_all_of_a_payment_the_owner_chooses_may_be_given_back() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 100_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 100_000)], T0, &shop(), None)
        .unwrap();
    // Paid in full and the owner still gives all of it back.
    book.queue_return(n, ReturnAmount::Sats(100_000), script_of(P2TR), T0, None)
        .unwrap();
    assert_eq!(book.returnable_sats(n), Some(0));
}

#[test]
fn rq_26_a_payment_after_a_cancel_or_a_close_is_offered_for_return() {
    let mut book = RequestBook::new();
    let a = make_btc(&mut book, 0, 100_000, 7);
    book.cancel(a, T0).unwrap();
    book.credit(a, &[arrival(1, 10, T0, 70_000)], T0, &shop(), None)
        .unwrap();
    book.queue_return(a, ReturnAmount::Sats(70_000), script_of(P2WPKH), T0, None)
        .unwrap();
    assert_eq!(book.returnable_sats(a), Some(0));
}

#[test]
fn rq_26_the_most_that_may_be_returned_is_what_arrived_less_what_was_returned() {
    let mut book = overpaid();
    book.queue_return(1, ReturnAmount::Sats(30_000), script_of(P2WPKH), T0, None)
        .unwrap();
    assert_eq!(book.returnable_sats(1), Some(70_000));
    let before = book.clone();
    assert_eq!(
        book.queue_return(1, ReturnAmount::Sats(70_001), script_of(P2WPKH), T0, None),
        Err(ReturnRefused::TooMuch { most_sats: 70_000 })
    );
    assert_eq!(book, before);
    book.queue_return(1, ReturnAmount::Sats(70_000), script_of(P2WPKH), T0, None)
        .unwrap();
    assert_eq!(book.returnable_sats(1), Some(0));
    assert_eq!(
        book.queue_return(1, ReturnAmount::Sats(1_000), script_of(P2WPKH), T0, None),
        Err(ReturnRefused::TooMuch { most_sats: 0 })
    );
    // A request that received nothing has nothing to return, and one that
    // does not exist has no request.
    let n = make_btc(&mut book, 1, 60_000, 7);
    assert_eq!(
        book.queue_return(n, ReturnAmount::Sats(1_000), script_of(P2WPKH), T0, None),
        Err(ReturnRefused::TooMuch { most_sats: 0 })
    );
    assert_eq!(
        book.queue_return(9, ReturnAmount::Sats(1_000), script_of(P2WPKH), T0, None),
        Err(ReturnRefused::NoSuchRequest)
    );
}

#[test]
fn rq_26_a_return_is_counted_against_the_request_whether_waiting_sent_or_done() {
    let mut book = overpaid();
    let id = book
        .queue_return(1, ReturnAmount::Sats(40_000), script_of(P2WPKH), T0, None)
        .unwrap();
    book.mark_returns_sent(&[id], [7; 32]);
    assert_eq!(book.returnable_sats(1), Some(60_000));
    book.mark_returns_done([7; 32], T0 + 5);
    assert_eq!(book.returnable_sats(1), Some(60_000));
}

#[test]
fn rq_26_the_amount_may_be_in_the_currency_at_the_rate_of_now_rounded_down() {
    let mut book = overpaid();
    // $5 is 500 cents; at $100,000 a bitcoin that is 5,000 sats.
    book.queue_return(
        1,
        ReturnAmount::Units(500),
        script_of(P2WPKH),
        T0,
        Some(&fresh(T0)),
    )
    .unwrap();
    assert_eq!(book.returns()[0].sats, 5_000);
    // $5.07 at $33,333 a bitcoin: 507 * 1e8 / 3.3333e6 = 15,210.15 -> 15,210.
    book.queue_return(
        1,
        ReturnAmount::Units(507),
        script_of(P2WPKH),
        T0,
        Some(&rate(3_333_300, T0)),
    )
    .unwrap();
    assert_eq!(book.returns()[1].sats, 15_210);
    let before = book.clone();
    assert_eq!(
        book.queue_return(1, ReturnAmount::Units(500), script_of(P2WPKH), T0, None),
        Err(ReturnRefused::StaleRate)
    );
    assert_eq!(
        book.queue_return(
            1,
            ReturnAmount::Units(500),
            script_of(P2WPKH),
            T0,
            Some(&fresh(T0 - 601))
        ),
        Err(ReturnRefused::StaleRate)
    );
    assert_eq!(book, before);
}

#[test]
fn rq_26_an_amount_under_what_the_address_kind_can_be_paid_is_refused() {
    let mut book = overpaid();
    for (address, min) in [
        (P2WPKH, 294),
        (P2WSH, 330),
        (P2TR, 330),
        (P2PKH, 546),
        (P2SH, 540),
    ] {
        let low = ReturnAmount::Sats(min - 1);
        assert_eq!(
            book.queue_return(1, low, script_of(address), T0, None),
            Err(ReturnRefused::TooSmall { min_sats: min }),
            "{address}"
        );
        assert!(book
            .queue_return(1, ReturnAmount::Sats(min), script_of(address), T0, None)
            .is_ok());
    }
}

#[test]
fn rq_27_the_address_a_payment_came_from_is_never_kept_or_used() {
    // The library is told the output's transaction, block and satoshis, and
    // never an address it came from: no field could hold one.
    let mut book = overpaid();
    let credit = format!("{:?}", book.request(1).unwrap().credits[0]);
    assert!(
        !credit.to_lowercase().contains("script") && !credit.to_lowercase().contains("address")
    );
    let arrival = format!("{:?}", arrival(1, 10, T0, 1));
    assert!(
        !arrival.to_lowercase().contains("script") && !arrival.to_lowercase().contains("address")
    );
    // A return takes its script from the customer's reply only.
    book.queue_return(1, ReturnAmount::Sats(5_000), script_of(P2TR), T0, None)
        .unwrap();
    assert_eq!(book.returns()[0].script, script_of(P2TR));
    // The line that asks the customer for an address.
    assert_eq!(
        ask_line("Dunmore Books", "$40", 12),
        "Dunmore Books would like to return $40 for request No. 12. Please reply with a bitcoin address of yours."
    );
}

#[test]
fn rq_28_a_mainnet_address_of_any_standard_kind_is_accepted() {
    for address in [P2WPKH, P2WSH, P2TR, P2PKH, P2SH] {
        let script = check_return_address(address, not_own).unwrap();
        assert_eq!(script, script_of(address), "{address}");
    }
    // Upper case bech32, as a QR code reads it; and spaces around.
    assert_eq!(
        check_return_address(P2WPKH_UPPER, not_own).unwrap(),
        script_of(P2WPKH)
    );
    assert_eq!(
        check_return_address(&format!("  {P2WPKH}\n"), not_own).unwrap(),
        script_of(P2WPKH)
    );
}

#[test]
fn rq_28_an_address_that_is_not_valid_is_refused() {
    let mut bad_checksum = P2WPKH.to_string();
    bad_checksum.pop();
    bad_checksum.push('5');
    let mut mixed = P2WPKH.to_string();
    mixed.replace_range(4..5, "W");
    for text in [
        "",
        " ",
        "hello",
        &bad_checksum,
        &mixed,
        "bc1q",
        "1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN3",
        WITNESS_V16,
        "bitcoin:",
    ] {
        assert_eq!(
            check_return_address(text, not_own),
            Err(AddressRefused::NotAnAddress),
            "{text:?}"
        );
    }
}

#[test]
fn rq_28_an_address_for_another_network_is_refused() {
    assert_eq!(
        check_return_address(TESTNET, not_own),
        Err(AddressRefused::TestNetwork)
    );
    // Testnet base58 and regtest.
    assert_eq!(
        check_return_address("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", not_own),
        Err(AddressRefused::TestNetwork)
    );
    assert_eq!(
        check_return_address("bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080", not_own),
        Err(AddressRefused::TestNetwork)
    );
    let _ = Network::Bitcoin;
}

#[test]
fn rq_28_an_address_of_the_shops_own_wallet_is_refused() {
    let own = script_of(P2WSH);
    assert_eq!(
        check_return_address(P2WSH, |s| *s == *own.as_script()),
        Err(AddressRefused::Own)
    );
    assert!(check_return_address(P2WPKH, |s| *s == *own.as_script()).is_ok());
    // Not a valid address is refused as such before it is asked whether it is the shop's.
    assert_eq!(
        check_return_address("nope", |_| true),
        Err(AddressRefused::NotAnAddress)
    );
}

#[test]
fn rq_28_a_bitcoin_link_uses_its_address_and_ignores_its_amount_for_the_return() {
    let no_amount = format!("bitcoin:{P2WPKH}");
    let one_btc = format!("bitcoin:{P2WPKH}?amount=1");
    let small_amount_and_details = format!(
        "bitcoin:{P2WPKH}?req-amount=0.001&req-label=return&req-message=thanks&lightning=lnbc1invoice&lno=lno1offer&sp=sp1silent"
    );
    assert_eq!(
        check_return_address(&no_amount, not_own),
        Ok(script_of(P2WPKH))
    );
    assert_eq!(
        check_return_address(&one_btc, not_own),
        Ok(script_of(P2WPKH))
    );
    assert_eq!(
        check_return_address(&small_amount_and_details, not_own),
        Ok(script_of(P2WPKH))
    );
    // The scheme check is ASCII case-insensitive, and all-uppercase bech32
    // remains accepted by the Bitcoin address parser.
    assert_eq!(
        check_return_address(&format!("BITCOIN:{P2WPKH_UPPER}?amount=2"), not_own),
        Ok(script_of(P2WPKH))
    );
}

#[test]
fn rq_28_addressless_payment_uris_do_not_provide_a_return_address() {
    for method in ["lightning=lnbc1invoice", "lno=lno1offer", "sp=sp1silent"] {
        let link = format!("bitcoin:?{method}");
        assert_eq!(
            check_return_address(&link, not_own),
            Err(AddressRefused::NotAnAddress),
            "{link}"
        );
    }
}

#[test]
fn rq_28_a_testnet_address_in_a_bitcoin_link_is_refused_as_testnet() {
    assert_eq!(
        check_return_address(&format!("bitcoin:{TESTNET}?amount=1"), not_own),
        Err(AddressRefused::TestNetwork)
    );
}

#[test]
fn rq_28_a_shops_own_address_in_a_bitcoin_link_is_refused() {
    let own = script_of(P2WSH);
    assert_eq!(
        check_return_address(&format!("bitcoin:{P2WSH}?amount=1"), |s| *s
            == *own.as_script()),
        Err(AddressRefused::Own)
    );
}

#[test]
fn rq_28_a_link_with_an_unknown_optional_parameter_uses_its_address() {
    let link = format!("bitcoin:{P2WPKH}?future-method=ignored");
    assert_eq!(check_return_address(&link, not_own), Ok(script_of(P2WPKH)));
}

#[test]
fn rq_28_a_link_with_an_unknown_required_parameter_is_refused() {
    let link = format!("bitcoin:{P2WPKH}?req-future-method=required");
    assert_eq!(
        check_return_address(&link, not_own),
        Err(AddressRefused::NotAnAddress)
    );
}

#[test]
fn rq_28_invalid_uri_amount_is_refused_by_the_strict_uri_parser() {
    let link = format!("bitcoin:{P2WPKH}?amount=not-a-number");
    assert_eq!(
        check_return_address(&link, not_own),
        Err(AddressRefused::NotAnAddress)
    );
}

#[test]
fn rq_28_plain_addresses_keep_their_size_limit_and_uris_use_their_own_limit() {
    let plain_128 = format!("{}{}", P2WPKH, "x".repeat(128 - P2WPKH.len()));
    let plain_129 = format!("{}{}", P2WPKH, "x".repeat(129 - P2WPKH.len()));
    assert_eq!(
        check_return_address(&plain_128, not_own),
        Err(AddressRefused::NotAnAddress)
    );
    assert_eq!(
        check_return_address(&plain_129, not_own),
        Err(AddressRefused::NotAnAddress)
    );

    let prefix = format!("bitcoin:{P2WPKH}?x=");
    let uri_2048 = format!("{prefix}{}", "a".repeat(2048 - prefix.len()));
    let uri_2049 = format!("{prefix}{}", "a".repeat(2049 - prefix.len()));
    assert_eq!(uri_2048.len(), 2048);
    assert_eq!(
        check_return_address(&uri_2048, not_own),
        Ok(script_of(P2WPKH))
    );
    assert_eq!(
        check_return_address(&uri_2049, not_own),
        Err(AddressRefused::NotAnAddress)
    );
}

#[test]
fn rq_28_the_accepted_address_is_shown_in_groups_of_four() {
    assert_eq!(
        address_in_groups(&script_of(P2WPKH)).unwrap(),
        "bc1q w508 d6qe jxtd g4y5 r3za rvar y0c5 xw7k v8f3 t4"
    );
    assert_eq!(
        address_in_groups(&script_of(P2PKH)).unwrap(),
        "1BvB MSEY stWe tqTF n5Au 4m4G Fg7x JaNV N2"
    );
    for address in [P2WSH, P2TR, P2SH] {
        let grouped = address_in_groups(&script_of(address)).unwrap();
        assert_eq!(grouped.replace(' ', ""), address);
        assert!(grouped.split(' ').all(|g| g.len() <= 4));
    }
    assert_eq!(address_in_groups(&ScriptBuf::new()), None);
}

#[test]
fn rq_28_a_script_of_no_standard_kind_is_not_queued() {
    let mut book = overpaid();
    assert_eq!(
        book.queue_return(1, ReturnAmount::Sats(5_000), ScriptBuf::new(), T0, None),
        Err(ReturnRefused::BadScript)
    );
    assert_eq!(
        book.queue_return(
            1,
            ReturnAmount::Sats(5_000),
            ScriptBuf::from_bytes(vec![0x6a, 1, 2]),
            T0,
            None
        ),
        Err(ReturnRefused::BadScript)
    );
}

#[test]
fn rq_29_each_accepted_return_is_queued_and_not_paid_at_once() {
    let mut book = overpaid();
    book.queue_return(1, ReturnAmount::Sats(5_000), script_of(P2WPKH), T0, None)
        .unwrap();
    assert_eq!(book.queued_return_ids(), [1]);
    assert_eq!(book.returns()[0].state, ReturnState::Queued);
    assert_eq!(
        return_added("5,000 sats", 1).what,
        "5,000 sats to request No. 1 is waiting with your other returns."
    );
}

#[test]
fn rq_29_the_queue_gives_every_return_as_a_payment_of_its_own_for_the_spend() {
    let mut book = overpaid();
    let second = {
        let n = make_btc(&mut book, 1, 60_000, 7);
        book.credit(n, &[arrival(2, 11, T0, 60_000)], T0, &shop(), None)
            .unwrap();
        n
    };
    book.queue_return(1, ReturnAmount::Sats(5_000), script_of(P2WPKH), T0, None)
        .unwrap();
    book.queue_return(second, ReturnAmount::Sats(7_000), script_of(P2TR), T0, None)
        .unwrap();
    // The same customer twice is two payments, not one.
    book.queue_return(1, ReturnAmount::Sats(6_000), script_of(P2WPKH), T0, None)
        .unwrap();
    let payments: Vec<(ScriptBuf, u64)> = book.returns_to_pay();
    assert_eq!(
        payments,
        [
            (script_of(P2WPKH), 5_000),
            (script_of(P2TR), 7_000),
            (script_of(P2WPKH), 6_000)
        ]
    );
    assert_eq!(book.queued_return_ids(), [1, 2, 3]);
}

#[test]
fn rq_29_a_return_is_done_when_its_spend_is_confirmed() {
    let mut book = overpaid();
    for _ in 0..3 {
        book.queue_return(1, ReturnAmount::Sats(5_000), script_of(P2WPKH), T0, None)
            .unwrap();
    }
    let ids = book.queued_return_ids();
    assert_eq!(book.mark_returns_sent(&ids[..2], [9; 32]), 2);
    // The two are in a spend and are not paid again by another.
    assert_eq!(book.returns_to_pay().len(), 1);
    assert_eq!(book.mark_returns_sent(&ids[..2], [8; 32]), 0);
    assert_eq!(book.mark_returns_done([9; 32], T0 + 700), 2);
    assert_eq!(
        book.returns()[0].state,
        ReturnState::Done {
            txid: [9; 32],
            at: T0 + 700
        }
    );
    assert_eq!(book.returns()[2].state, ReturnState::Queued);
    // A spend that confirmed is not one that can be replaced.
    assert_eq!(book.requeue_returns([9; 32]), 0);
}

#[test]
fn rq_29_a_return_goes_back_to_the_queue_if_the_spend_is_replaced_without_it() {
    let mut book = overpaid();
    for _ in 0..2 {
        book.queue_return(1, ReturnAmount::Sats(5_000), script_of(P2WPKH), T0, None)
            .unwrap();
    }
    let ids = book.queued_return_ids();
    book.mark_returns_sent(&ids, [9; 32]);
    assert!(book.returns_to_pay().is_empty());
    assert_eq!(book.requeue_returns([9; 32]), 2);
    assert_eq!(book.returns_to_pay().len(), 2);
    assert_eq!(book.mark_returns_done([9; 32], T0), 0);
}

#[test]
fn rq_29_no_more_returns_wait_than_one_spend_takes() {
    let mut book = RequestBook::new();
    let n = make_btc(&mut book, 0, 60_000, 7);
    book.credit(n, &[arrival(1, 10, T0, 5_000_000)], T0, &shop(), None)
        .unwrap();
    for _ in 0..MAX_QUEUED_RETURNS {
        book.queue_return(n, ReturnAmount::Sats(1_000), script_of(P2WPKH), T0, None)
            .unwrap();
    }
    assert_eq!(
        book.queue_return(n, ReturnAmount::Sats(1_000), script_of(P2WPKH), T0, None),
        Err(ReturnRefused::QueueFull)
    );
    // Under the 100 outputs a spend takes, with one left for the change.
    assert_eq!(MAX_QUEUED_RETURNS, 99);
    // Once some are in a spend the queue has room again.
    let ids = book.queued_return_ids();
    book.mark_returns_sent(&ids[..10], [1; 32]);
    assert!(book
        .queue_return(n, ReturnAmount::Sats(1_000), script_of(P2WPKH), T0, None)
        .is_ok());
}

#[test]
fn rq_29_the_books_record_each_return_against_its_request() {
    let mut book = overpaid();
    let id = book
        .queue_return(
            1,
            ReturnAmount::Sats(5_000),
            script_of(P2WPKH),
            T0 + 3,
            None,
        )
        .unwrap();
    book.mark_returns_sent(&[id], [4; 32]);
    book.mark_returns_done([4; 32], T0 + 900);
    let line = book.ledger_line(book.request(1).unwrap());
    assert_eq!(line.returns.len(), 1);
    assert_eq!(line.returns[0].sats, 5_000);
    assert_eq!(line.returns[0].queued_at, T0 + 3);
    assert_eq!(line.returns[0].done_at, Some(T0 + 900));
}
