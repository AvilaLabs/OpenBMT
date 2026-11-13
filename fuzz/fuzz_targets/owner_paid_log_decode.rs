#![no_main]

use bitcoin::hashes::Hash;
use bitcoin::Txid;
use libfuzzer_sys::fuzz_target;
use openbmt_owner_node::{PaidEntry, PaidLog, PaidValue, PaidWhat};

// Any bytes are read strictly or refused, and what is read gives the same
// bytes back (OC-50). The first byte picks whether the rest follows the start
// of a valid log of two entries, so that mutated bytes get past the header
// and the entries' checks.
fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes: Vec<u8> = if mode & 1 == 1 {
        let entry = |n: u8, value: Option<PaidValue>| PaidEntry {
            what: PaidWhat::CashOut,
            number: u32::from(n),
            txid: Txid::from_byte_array([n; 32]),
            sats: 1_000 * u64::from(n),
            at: 1_700_000_000,
            value,
        };
        let undoing = entry(1, None).undoing(1_700_000_500);
        let valid = PaidLog::new()
            .with_added(&[
                entry(1, None),
                entry(
                    2,
                    Some(PaidValue {
                        currency: *b"USD",
                        minor: 77,
                    }),
                ),
            ])
            .ok()
            .flatten()
            .and_then(|l| l.with_added(undoing.as_slice()).ok().flatten())
            .map(|l| l.to_bytes())
            .unwrap_or_default();
        let keep = usize::from(mode >> 1).min(valid.len());
        let mut v = valid[..keep].to_vec();
        v.extend_from_slice(rest);
        v
    } else {
        rest.to_vec()
    };
    if let Ok(log) = PaidLog::from_bytes(&bytes) {
        assert_eq!(log.to_bytes(), bytes);
    }
});
