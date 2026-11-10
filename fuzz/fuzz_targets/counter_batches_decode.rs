#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::batch::{BatchPolicy, CounterBatches, PinnedRoots};
use openbmt_core::identity::{DeviceId, Identity};

fn id(n: u8) -> DeviceId {
    Identity::from_secret_bytes(&[n; 32]).unwrap().device_id()
}

fuzz_target!(|data: &[u8]| {
    // The roots and the policy are fixed, as a caller supplies them at
    // start-up. The counter and wallet checksum are read from the snapshot's
    // own header (bytes 5..37 and 37..69) so that the fuzzer reaches the
    // checks behind them; a header that names no valid key falls back to a
    // fixed counter.
    let counter = data
        .get(5..37)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .and_then(|b| DeviceId::from_bytes(&b).ok())
        .unwrap_or_else(|| id(1));
    let wallet: [u8; 32] = data
        .get(37..69)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .unwrap_or([0; 32]);
    let roots = PinnedRoots::new(id(2), id(3)).unwrap();
    let policy = BatchPolicy { min_count: 3 };
    if let Ok(decoded) = CounterBatches::from_bytes(data, counter, roots, wallet, policy) {
        assert_eq!(decoded.to_bytes(), data);
    }
});
