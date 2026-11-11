#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::identity::Sig;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::{decode_settings_file, MAX_SETTINGS_FILE_BYTES};

/// What the reader of the counter's settings file must hold of a file it
/// took: it is under the size limit, the settings encode back to exactly
/// the bytes read (CR-160), and the same file with a byte added or taken
/// away is no longer read.
fn check(data: &[u8]) {
    if let Ok(signed) = decode_settings_file(data) {
        assert!(data.len() <= MAX_SETTINGS_FILE_BYTES);
        assert_eq!(signed.to_bytes(), data);
        let mut longer = data.to_vec();
        longer.push(0);
        assert!(decode_settings_file(&longer).is_err());
        assert!(decode_settings_file(&data[..data.len() - 1]).is_err());
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // A whole file with the input behind it, and a whole file cut where the
    // input says: neither may panic, and none that reads may differ from
    // what it was made of.
    let signed = SignedSettings {
        settings: Settings::defaults(1_000),
        owner_sig: Sig([0u8; 64]),
        approver_sig: None,
    };
    let whole = signed.to_bytes();
    let cut = data.first().map_or(0, |b| usize::from(*b)) % (whole.len() + 1);
    check(&whole[..cut]);
    let mut more = whole;
    more.extend_from_slice(data);
    check(&more);
});
