//! SW-50 to SW-55: the state file.

use bitcoin::hashes::Hash;
use bitcoin::Txid;

use super::fixtures::*;
use crate::{Phase, Refused, SwapState, MAGIC_SWAP_STATE, STATE_VERSION};

fn state() -> SwapState {
    SwapState::new(&good_swap(), &secrets(), NET, &destination()).unwrap()
}

fn claimed() -> SwapState {
    state().claimed(Txid::from_byte_array([0x42; 32])).unwrap()
}

#[test]
fn sw_50_the_file_starts_with_its_magic_and_version_and_round_trips() {
    for s in [state(), claimed()] {
        let bytes = s.encode();
        assert_eq!(&bytes[..4], b"OBM%");
        assert_eq!(bytes[..4], MAGIC_SWAP_STATE);
        assert_eq!(bytes[4], STATE_VERSION);
        let back = SwapState::decode(&bytes).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.encode(), bytes);
        // What a claim needs after a restart is back.
        assert_eq!(back.secrets(), &secrets());
        assert_eq!(back.timeout(), TIMEOUT);
        assert_eq!(back.onchain_sats(), 99_350);
        assert_eq!(back.destination(), &destination());
        assert_eq!(back.lockup_address().unwrap(), good_swap().lockup_address);
        assert_eq!(back.id(), "aBc123xyz");
    }
}

#[test]
fn sw_50_the_magic_is_not_another_files() {
    assert_eq!(MAGIC_SWAP_STATE, *b"OBM%");
}

#[test]
fn sw_51_every_short_file_and_every_long_file_is_refused() {
    for s in [state(), claimed()] {
        let bytes = s.encode();
        for n in 0..bytes.len() {
            assert!(SwapState::decode(&bytes[..n]).is_err(), "{n}");
        }
        for extra in [1usize, 2, 32] {
            let mut long = bytes.clone();
            long.extend(vec![0u8; extra]);
            assert!(SwapState::decode(&long).is_err());
        }
    }
}

#[test]
fn sw_51_unknown_version_network_order_and_phase_are_refused() {
    let bytes = state().encode();
    for (at, values) in [
        (4usize, [0u8, 2, 255]),
        (5, [4, 9, 255]),
        (6, [2, 3, 255]),
        (7, [2, 3, 255]),
    ] {
        for v in values {
            let mut b = bytes.clone();
            b[at] = v;
            assert!(SwapState::decode(&b).is_err(), "byte {at} = {v}");
        }
    }
    let mut b = bytes;
    b[0] = b'X';
    assert_eq!(
        SwapState::decode(&b).unwrap_err(),
        Refused::BadState("magic")
    );
}

#[test]
fn sw_51_no_single_changed_byte_gives_a_state_that_encodes_differently() {
    for s in [state(), claimed()] {
        let bytes = s.encode();
        for i in 0..bytes.len() {
            let mut b = bytes.clone();
            b[i] ^= 0x01;
            if let Ok(got) = SwapState::decode(&b) {
                assert_eq!(got.encode(), b, "byte {i}");
            }
        }
    }
}

#[test]
fn sw_51_an_upper_case_address_or_invoice_is_not_canonical() {
    let bytes = state().encode();
    let dest = destination().to_string();
    let at = bytes
        .windows(dest.len())
        .position(|w| w == dest.as_bytes())
        .unwrap();
    let mut b = bytes.clone();
    for byte in &mut b[at..at + dest.len()] {
        *byte = byte.to_ascii_uppercase();
    }
    assert!(SwapState::decode(&b).is_err());
}

#[test]
fn sw_52_a_file_that_disagrees_with_itself_is_refused() {
    let s = state();
    let bytes = s.encode();
    // Offsets: magic 4, version, network, order, phase, preimage 32, claim
    // key 32, refund key 33, timeout 4, onchain 8, invoice sats 8, expiry 8.
    let preimage = 8;
    let claim_key = preimage + 32;
    let refund = claim_key + 32;
    let timeout = refund + 33;
    let onchain = timeout + 4;
    let invoice_sats = onchain + 8;
    let expires = invoice_sats + 8;
    let patch = |at: usize, with: &[u8]| {
        let mut b = bytes.clone();
        b[at..at + with.len()].copy_from_slice(with);
        b
    };
    // Another preimage: the invoice's hash is no longer its hash.
    assert_eq!(
        SwapState::decode(&patch(preimage, &[8; 32])).unwrap_err(),
        Refused::BadState("invoice")
    );
    // A claim key of zero.
    assert!(SwapState::decode(&patch(claim_key, &[0; 32])).is_err());
    // The claim key's own public key as the refund key.
    assert_eq!(
        SwapState::decode(&patch(refund, &secrets().claim_public().serialize())).unwrap_err(),
        Refused::BadState("refund key")
    );
    // A refund key that is no point.
    assert!(SwapState::decode(&patch(refund, &[5; 33])).is_err());
    // A timeout of zero and one that is a time.
    for t in [0u32, 500_000_000] {
        assert_eq!(
            SwapState::decode(&patch(timeout, &t.to_be_bytes())).unwrap_err(),
            Refused::BadState("timeout")
        );
    }
    // Amounts out of the band, and an invoice amount other than the invoice's.
    assert_eq!(
        SwapState::decode(&patch(onchain, &999u64.to_be_bytes())).unwrap_err(),
        Refused::BadState("amount")
    );
    assert_eq!(
        SwapState::decode(&patch(onchain, &(INVOICE_SATS + 1).to_be_bytes())).unwrap_err(),
        Refused::BadState("amount")
    );
    assert_eq!(
        SwapState::decode(&patch(invoice_sats, &(INVOICE_SATS + 1).to_be_bytes())).unwrap_err(),
        Refused::BadState("invoice")
    );
    // An expiry that is not the invoice's.
    assert_eq!(
        SwapState::decode(&patch(expires, &1u64.to_be_bytes())).unwrap_err(),
        Refused::BadState("invoice expiry")
    );
    // The other key order is no longer taken (SW-26): the file is refused.
    assert_eq!(
        SwapState::decode(&patch(6, &[1])).unwrap_err(),
        Refused::BadState("key order")
    );
}

#[test]
fn sw_52_an_id_that_breaks_sw_12_and_a_wrong_network_are_refused() {
    let bytes = state().encode();
    let text = bytes.clone();
    let at = text.windows(9).position(|w| w == b"aBc123xyz").unwrap();
    let mut b = bytes.clone();
    b[at] = b' ';
    assert_eq!(SwapState::decode(&b).unwrap_err(), Refused::BadState("id"));
    // The network byte says mainnet, the addresses and the invoice are signet.
    let mut b = bytes;
    b[5] = 0;
    assert!(SwapState::decode(&b).is_err());
}

#[test]
fn sw_53_the_phase_goes_forward_only_and_makes_a_new_value() {
    let waiting = state();
    let txid = Txid::from_byte_array([0x42; 32]);
    let done = waiting.claimed(txid).unwrap();
    assert_eq!(waiting.phase(), Phase::Waiting);
    assert_eq!(done.phase(), Phase::Claimed(txid));
    assert_eq!(done.claimed(txid).unwrap_err(), Refused::AlreadyClaimed);
    assert_eq!(
        done.claimed(Txid::from_byte_array([1; 32]))
            .unwrap_err()
            .rule(),
        "SW-53"
    );
    // The phase byte and the id of the claim go with the file.
    let back = SwapState::decode(&done.encode()).unwrap();
    assert_eq!(back.phase(), Phase::Claimed(txid));
    assert_eq!(back.claimed(txid).unwrap_err(), Refused::AlreadyClaimed);
}

#[test]
fn sw_54_the_state_does_not_print_its_secrets() {
    let s = state();
    let shown = format!("{s:?} {s:#?}");
    let preimage: String = s
        .secrets()
        .preimage()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let key: String = s
        .secrets()
        .claim_key()
        .secret_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!shown.contains(&preimage) && !shown.contains(&key));
    assert!(shown.contains("aBc123xyz"));
}

#[test]
fn sw_55_the_decoder_is_fuzzed() {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let target =
        std::path::Path::new(&manifest).join("../../fuzz/fuzz_targets/swap_state_decode.rs");
    let source = std::fs::read_to_string(target).unwrap();
    assert!(source.contains("SwapState::decode"));
    let toml =
        std::fs::read_to_string(std::path::Path::new(&manifest).join("../../fuzz/Cargo.toml"))
            .unwrap();
    assert!(toml.contains("swap_state_decode"));
}
