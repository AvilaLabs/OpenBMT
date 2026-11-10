#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_key_ffi::{parse_deposit_code, KeyError, KeyNetwork};

const MAX_SATS: u64 = 21_000_000 * 100_000_000;

fuzz_target!(|data: &[u8]| {
    let Some((first, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let network = match first % 4 {
        0 => KeyNetwork::Mainnet,
        1 => KeyNetwork::Testnet,
        2 => KeyNetwork::Signet,
        _ => KeyNetwork::Regtest,
    };
    match parse_deposit_code(text.to_string(), network) {
        Ok(code) => {
            assert!(text.len() <= 2_048);
            if let Some(sats) = code.amount_sats {
                assert!((1..=MAX_SATS).contains(&sats));
            }
            if let Some(label) = &code.label {
                assert!(!label.chars().any(char::is_control));
            }
            for name in &code.ignored {
                assert!(!name.starts_with("req-"));
                assert!(!matches!(name.as_str(), "amount" | "label" | "message"));
                assert!(name.bytes().all(|b| b.is_ascii_graphic()));
            }
            // The address is in its usual spelling: reading it again as a
            // bare address and as a bare URI gives the same code.
            let bare = parse_deposit_code(code.address.clone(), network).expect("bare address");
            assert_eq!(bare.address, code.address);
            assert!(bare.ignored.is_empty() && bare.amount_sats.is_none());
            let uri = parse_deposit_code(format!("bitcoin:{}", code.address), network)
                .expect("bare uri");
            assert_eq!(uri, bare);
        }
        Err(KeyError::Refused { rule, .. }) => {
            assert!(rule.starts_with("deposit-") || rule.starts_with("address"));
        }
        Err(other) => panic!("a refusal of another kind: {other:?}"),
    }
});
