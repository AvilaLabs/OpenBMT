#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_ur::cbor::encode_bytes;
use openbmt_ur::{ur_to_wallet_text, WalletDecoder, MAX_WALLET_CBOR, MAX_WALLET_TEXT};

const TYPES: [&str; 6] = [
    "crypto-account",
    "crypto-output",
    "output",
    "output-descriptor",
    "bytes",
    "crypto-psbt",
];

fn check_payload(ur_type: &str, payload: &[u8]) {
    let first = ur_to_wallet_text(ur_type, payload);
    let second = ur_to_wallet_text(ur_type, payload);
    assert_eq!(first, second, "payload result must be stable");
    if let Ok(wallet) = first {
        let text = wallet.as_str();
        assert!(
            ImportedWallet::read(text).is_ok(),
            "accepted text must import"
        );
        assert_eq!(
            wallet,
            ur_to_wallet_text(ur_type, payload).expect("stable success")
        );
    }
}

fn check_scans(lines: &[&str]) {
    let mut first = WalletDecoder::new();
    let mut second = WalletDecoder::new();
    for text in lines.iter().take(8) {
        let a = first.receive(text);
        let b = second.receive(text);
        assert_eq!(a, b, "scan outcome and progress must be stable");
        match a {
            Ok(progress) => {
                assert!(progress.estimated_fraction.is_finite());
                assert!((0.0..=1.0).contains(&progress.estimated_fraction));
                if progress.complete {
                    let wallet = first.finish().expect("completed scan has a wallet");
                    assert!(ImportedWallet::read(wallet.as_str()).is_ok());
                    assert_eq!(first.result(), Some(&wallet));
                } else {
                    assert!(first.result().is_none());
                }
            }
            Err(error) => {
                assert!(!error.to_string().is_empty());
                assert!(error.rule().starts_with("UR-"));
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let payload = rest
        .get(..rest.len().min(MAX_WALLET_CBOR + 1))
        .unwrap_or(rest);
    check_payload(TYPES[usize::from(selector) % TYPES.len()], payload);
    check_payload("bytes", &encode_bytes(payload));
    for ur_type in TYPES {
        check_payload(ur_type, payload);
    }

    // Bound text input before converting to UTF-8; the wrapper itself must
    // receive bounded streams of up to eight lines in one scan.
    let text_bytes = rest.get(..rest.len().min(MAX_WALLET_TEXT)).unwrap_or(rest);
    if let Ok(text) = std::str::from_utf8(text_bytes) {
        let lines = text.lines().take(8).collect::<Vec<_>>();
        check_scans(&lines);
    }
});
