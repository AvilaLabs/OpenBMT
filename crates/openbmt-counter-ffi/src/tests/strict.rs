//! Strict decoding: every cut, every extra byte and every flipped bit of a
//! set of valid requests is refused or handled without a panic, and every
//! error response carries words that pass `check` (SC-12).

use bitcoin::hashes::Hash;
use openbmt_core::heartbeat::WatcherHeartbeat;

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{frame, Rd, W};
use crate::dispatch;

/// A node that has heard from the network and has two sales open.
fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    for _ in 0..2 {
        rig.call(
            11,
            handle,
            &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
        );
    }
    (rig, handle)
}

/// Valid requests for the node `handle`, one for each op that takes fields.
fn corpus(handle: u64) -> Vec<Vec<u8>> {
    let hb = WatcherHeartbeat {
        counter: id(3).device_id(),
        seq: 5,
        time: T0,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
    .sign(&id(1))
    .to_bytes();
    vec![
        frame(10, handle, &W::new().u64(T0).u32(0).0),
        frame(
            11,
            handle,
            &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
        ),
        frame(12, handle, &W::new().u64(T0).u32(0).0),
        frame(
            13,
            handle,
            &W::new()
                .u64(T0)
                .u32(1)
                .u64(RATE)
                .string("test-fixed")
                .u64(T0)
                .0,
        ),
        frame(14, handle, &W::new().cancel(T0, 1).0),
        frame(15, handle, &W::new().u64(T0).u32(0).actor(true, "mia").0),
        frame(
            16,
            handle,
            &W::new()
                .u64(T0)
                .u32(0)
                .u8(0)
                .u64(10)
                .string("x")
                .actor(false, "ann")
                .string("why")
                .0,
        ),
        frame(
            17,
            handle,
            &W::new().u64(T0).u32(0).u64(1).actor(false, "ann").0,
        ),
        frame(
            18,
            handle,
            &W::new()
                .u64(T0)
                .u32(0)
                .actor(true, "mia")
                .u64(5)
                .string("r")
                .0,
        ),
        frame(19, handle, &W::new().u32(0).0),
        frame(20, handle, &W::new().u64(T0).bytes(&hb).0),
        frame(21, handle, &W::new().u64(T0).bytes(&[1, 2, 3]).0),
        frame(22, handle, &W::new().u64(T0).0),
        frame(23, handle, &W::new().u64(T0).bytes(&[1, 2, 3]).0),
        frame(30, handle, &[]),
        frame(31, handle, &[]),
    ]
}

/// Valid requests of the ops that work on no node.
fn price_corpus() -> Vec<Vec<u8>> {
    vec![
        frame(43, 0, &W::new().bytes(&signed_settings().to_bytes()).0),
        frame(40, 0, &[]),
        frame(
            41,
            0,
            &W::new()
                .string("kraken")
                .bytes(br#"{"error":[],"result":{"X":{"c":["73554.50"]}}}"#)
                .u64(T0)
                .0,
        ),
        frame(
            42,
            0,
            &W::new()
                .u32(2)
                .string("kraken")
                .u64(T0)
                .u64(7_355_450)
                .string("coinbase")
                .u64(T0)
                .u64(7_355_218)
                .u64(T0)
                .0,
        ),
    ]
}

/// No response is a fault of the library, and an error response has words
/// that pass the rules of SC-12.
fn assert_framed(response: &[u8], what: &str) -> bool {
    assert_eq!(response.get(..4), Some(b"OBMn".as_slice()), "{what}: magic");
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
    all.extend(price_corpus());
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
    let response = rig.call(30, 2, &[]);
    assert_eq!(
        Rd::error(&response).expect("no node").rule,
        "handle-unknown"
    );
}

#[test]
fn bytes_left_over_after_the_last_field_are_refused() {
    let (rig, handle) = ready();
    for mut request in corpus(handle).into_iter().chain(price_corpus()) {
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
    for request in corpus(handle).into_iter().chain(price_corpus()) {
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
    assert_eq!(rule(&frame(30, 0, &[])), "handle-unknown");
    assert_eq!(rule(&frame(30, handle + 7, &[])), "handle-unknown");
    // `start` works on no node: its handle must be 0.
    let config = W::new().start(&start_config("x")).0;
    assert_eq!(rule(&frame(1, handle, &config)), "handle-unexpected");
    // The frame itself.
    assert_eq!(rule(&[]), "request-malformed");
    let mut bad = frame(30, handle, &[]);
    bad[3] = b'x';
    assert_eq!(rule(&bad), "request-magic");
    let mut bad = frame(30, handle, &[]);
    bad[4] = 2;
    assert_eq!(rule(&bad), "request-version");
    // The Key app's magic is not this one's.
    let mut other = frame(30, handle, &[]);
    other[..4].copy_from_slice(b"OBM8");
    assert_eq!(rule(&other), "request-magic");
    // Over 1 MiB, even when the first bytes are a valid frame.
    let mut big = frame(30, handle, &[]);
    big.resize((1 << 20) + 1, 0);
    assert_eq!(rule(&big), "request-too-long");
    // A length that says more than there is.
    let lie = frame(23, handle, &W::new().u64(T0).u32(u32::MAX).0);
    assert_eq!(rule(&lie), "request-malformed");
    // A tag that is not defined.
    let tag = frame(
        16,
        handle,
        &W::new()
            .u64(T0)
            .u32(0)
            .u8(9)
            .u64(1)
            .string("x")
            .actor(false, "a")
            .string("r")
            .0,
    );
    assert_eq!(rule(&tag), "request-malformed");
    let role = frame(15, handle, &W::new().u64(T0).u32(0).u8(2).string("a").0);
    assert_eq!(rule(&role), "request-malformed");
    // Text that is not UTF-8.
    let utf = frame(
        11,
        handle,
        &W::new()
            .u64(T0)
            .u64(1)
            .bytes(&[0xff, 0xfe, 0xfd])
            .u64(1)
            .string("x")
            .u64(T0)
            .0,
    );
    assert_eq!(rule(&utf), "request-malformed");
}

#[test]
fn a_handle_is_refused_after_its_stop_and_is_never_given_again() {
    let (rig, handle) = ready();
    Rd::ok(&rig.call(2, handle, &[]));
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
    assert!(Rd::error(&rig.call(30, handle, &[])).is_none());
}

#[test]
fn a_panic_inside_a_node_poisons_it_until_it_is_stopped() {
    let (rig, handle) = ready();
    let entry = rig.registry().entry_for_test(handle).expect("an entry");
    let result = entry.run_for_test(|| panic!("boom"));
    assert_eq!(result.expect_err("a panic").rule(), "internal");
    // Every later call on the node is the same error, whatever the op. (A
    // request whose own bytes are not a command or a batch is refused
    // before the node is asked.)
    for request in corpus(handle) {
        let response = dispatch(rig.registry(), &request);
        let e = Rd::error(&response).expect("an error");
        if matches!(e.rule.as_str(), "command-bytes" | "batch-bytes") {
            continue;
        }
        assert_eq!((e.status, e.rule.as_str()), (4, "poisoned"));
        assert!(check_wire_words(&e.words).is_ok());
    }
    // Stop still works, and the folder can be used again.
    Rd::ok(&rig.call(2, handle, &[]));
    rig.arm();
    assert_eq!(rig.start(), handle + 1);
}
