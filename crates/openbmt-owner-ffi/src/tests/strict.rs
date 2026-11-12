//! Strict decoding: every cut, every extra byte and every flipped bit of a
//! set of valid requests is refused or handled without a panic, and every
//! error response carries words that pass `check` (SC-12).

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{frame, Rd, W};
use crate::dispatch;

/// A node that has heard from the network and has a batch queued.
fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    rig.queue_batch(handle);
    (rig, handle)
}

/// Valid requests for the node `handle`, one for each op that takes fields
/// and one for each that takes none but stop.
fn corpus(handle: u64) -> Vec<Vec<u8>> {
    let counter = counter_bytes();
    vec![
        frame(10, handle, &W::new().u64(T0).u32(0).0),
        frame(11, handle, &W::new().u64(T0).0),
        frame(12, handle, &W::new().u64(T0).0),
        frame(20, handle, &[]),
        frame(21, handle, &[]),
        frame(22, handle, &[]),
        frame(23, handle, &[]),
        frame(24, handle, &[]),
        frame(25, handle, &W::new().u64(T0).0),
        frame(26, handle, &[]),
        frame(27, handle, &[]),
        frame(30, handle, &W::new().u64(T0).u64(1_000).0),
        frame(31, handle, &W::new().u64(T0).0),
        frame(32, handle, &W::new().u64(T0).bytes(&[1, 2, 3]).0),
        frame(33, handle, &[]),
        frame(34, handle, &W::new().bytes(&[7; 32]).0),
        frame(35, handle, &W::new().bytes(&[7; 32]).0),
        frame(40, handle, &[]),
        frame(
            41,
            handle,
            &W::new()
                .refund(T0, &counter, 0, "Mia", &external_address(9), 10_000)
                .0,
        ),
        frame(42, handle, &W::new().u32(99).0),
        frame(43, handle, &W::new().u32(99).0),
        frame(
            44,
            handle,
            &W::new()
                .string(&external_address(8))
                .u8(0)
                .u64(5_000)
                .string("till")
                .u8(0)
                .0,
        ),
        frame(
            45,
            handle,
            &W::new().u64(T0).bytes(&counter).u32(3).u32(3).0,
        ),
        frame(46, handle, &W::new().bytes(&[1, 2, 3]).0),
        frame(50, handle, &[]),
        frame(51, handle, &W::new().u8(0).bytes(&counter).u64(T0).0),
        frame(52, handle, &[]),
        frame(53, handle, &W::new().u32(0).0),
        frame(60, handle, &[]),
        frame(62, handle, &[]),
        frame(63, handle, &[]),
    ]
}

/// No response is a fault of the library, and an error response has words
/// that pass the rules of SC-12.
fn assert_framed(response: &[u8], what: &str) -> bool {
    assert_eq!(response.get(..4), Some(b"OBMl".as_slice()), "{what}: magic");
    match Rd::error(response) {
        None => true,
        Some(e) => {
            assert_ne!(e.status, 4, "{what}: an internal error: {e:?}");
            check_wire_words(&e.words).unwrap_or_else(|why| panic!("{what}: {why}: {e:?}"));
            false
        }
    }
}

#[test]
fn every_cut_of_a_valid_request_is_refused() {
    let (rig, handle) = ready();
    let mut all = corpus(handle);
    all.push(start_request(&rig.dir()));
    for request in all {
        for n in 0..request.len() {
            let response = dispatch(rig.registry(), &request[..n]);
            let ok = assert_framed(&response, &format!("cut at {n} of {}", request.len()));
            assert!(
                !ok,
                "a request cut to {n} of {} bytes was accepted",
                request.len()
            );
        }
    }
    // Nothing the cuts did started a node.
    let response = rig.call(20, 2, &[]);
    assert_eq!(
        Rd::error(&response).expect("no node").rule,
        "handle-unknown"
    );
}

#[test]
fn bytes_left_over_after_the_last_field_are_refused() {
    let (rig, handle) = ready();
    for mut request in corpus(handle) {
        request.push(0);
        let response = dispatch(rig.registry(), &request);
        assert!(!assert_framed(&response, "trailing byte"));
        let e = Rd::error(&response).expect("an error");
        assert_eq!(e.rule, "request-malformed");
    }
}

#[test]
fn every_flipped_bit_is_refused_or_handled_without_a_panic() {
    let (rig, handle) = ready();
    for request in corpus(handle) {
        for i in 0..request.len() {
            for bit in 0..8 {
                let mut flipped = request.clone();
                flipped[i] ^= 1 << bit;
                let response = dispatch(rig.registry(), &flipped);
                assert_framed(&response, &format!("bit {bit} of byte {i}"));
            }
        }
    }
}

#[test]
fn unknown_ops_bad_handles_and_bad_frames_are_refused() {
    let (rig, handle) = ready();
    let rule = |request: &[u8]| {
        let response = dispatch(rig.registry(), request);
        assert!(!assert_framed(&response, "refusal"));
        Rd::error(&response).expect("an error").rule
    };
    assert_eq!(rule(&frame(0, 0, &[])), "op-unknown");
    assert_eq!(rule(&frame(999, handle, &[])), "op-unknown");
    assert_eq!(rule(&frame(20, 0, &[])), "handle-unknown");
    assert_eq!(rule(&frame(20, handle + 7, &[])), "handle-unknown");
    // `start` works on no node: its handle must be 0.
    let config = W::new().start(&start_config("x")).0;
    assert_eq!(rule(&frame(1, handle, &config)), "handle-unexpected");
    // The frame itself.
    assert_eq!(rule(&[]), "request-malformed");
    let mut bad = frame(20, handle, &[]);
    bad[3] = b'x';
    assert_eq!(rule(&bad), "request-magic");
    let mut bad = frame(20, handle, &[]);
    bad[4] = 2;
    assert_eq!(rule(&bad), "request-version");
    // The counter app's magic is not this one's.
    let mut other = frame(20, handle, &[]);
    other[..4].copy_from_slice(b"OBMm");
    assert_eq!(rule(&other), "request-magic");
    // Over 9 MiB, even when the first bytes are a valid frame.
    let mut big = frame(20, handle, &[]);
    big.resize((9 << 20) + 1, 0);
    assert_eq!(rule(&big), "request-too-long");
    // A length that says more than there is.
    let lie = frame(32, handle, &W::new().u64(T0).u32(u32::MAX).0);
    assert_eq!(rule(&lie), "request-malformed");
    // A tag that is not defined: a cash-out's amount, a network, a flag.
    let tag = frame(44, handle, &W::new().string("x").u8(9).u64(1).string("t").0);
    assert_eq!(rule(&tag), "request-malformed");
    let mut config = start_config("x");
    config.dir = String::new();
    let mut fields = W::new().start(&config).0;
    // The network byte follows the folder's empty text.
    fields[4] = 9;
    assert_eq!(rule(&frame(1, 0, &fields)), "request-malformed");
    let flag = frame(51, handle, &W::new().u8(7).bytes(&counter_bytes()).u64(1).0);
    assert_eq!(rule(&flag), "request-malformed");
    // Text that is not UTF-8.
    let utf = frame(
        41,
        handle,
        &W::new()
            .u64(T0)
            .bytes(&counter_bytes())
            .u32(0)
            .bytes(&[0xff, 0xfe, 0xfd])
            .string("x")
            .u64(1)
            .0,
    );
    assert_eq!(rule(&utf), "request-malformed");
}

#[test]
fn a_handle_is_refused_after_its_stop_and_is_never_given_again() {
    let (rig, handle) = ready();
    Rd::ok(&rig.call(2, handle, &[])).pump();
    for request in corpus(handle) {
        let response = dispatch(rig.registry(), &request);
        assert!(!assert_framed(&response, "after stop"));
        assert_eq!(Rd::error(&response).expect("error").rule, "handle-unknown");
    }
    rig.arm();
    assert_eq!(rig.start(), handle + 1);
}

#[test]
fn a_panic_in_an_op_is_caught_and_the_registry_goes_on() {
    let (rig, handle) = ready();
    let response = rig.call(65_000, 0, &[]);
    let e = Rd::error(&response).expect("an error");
    assert_eq!((e.status, e.rule.as_str()), (4, "internal"));
    assert_eq!(e.text, "caught a panic: [redacted]");
    assert!(!e.text.contains("test panic"), "the payload is not copied");
    check_wire_words(&e.words).expect("words");
    // The registry and the node are unharmed.
    assert!(Rd::error(&rig.call(20, handle, &[])).is_none());
}

#[test]
fn a_panic_inside_a_node_poisons_it_until_it_is_stopped() {
    let (rig, handle) = ready();
    let entry = rig.registry().entry_for_test(handle).expect("an entry");
    let result = entry.run_for_test(|| panic!("boom"));
    assert_eq!(result.expect_err("a panic").rule(), "internal");
    // Every later call on the node is the same error, whatever the op. (A
    // request whose own bytes are not settings is refused before the node is
    // asked.)
    for request in corpus(handle) {
        let response = dispatch(rig.registry(), &request);
        let e = Rd::error(&response).expect("an error");
        if e.rule == "settings-bytes" {
            continue;
        }
        assert_eq!((e.status, e.rule.as_str()), (4, "poisoned"));
        assert!(check_wire_words(&e.words).is_ok());
    }
    // Stop still works, and the folder can be used again.
    Rd::ok(&rig.call(2, handle, &[])).pump();
    rig.arm();
    assert_eq!(rig.start(), handle + 1);
}
