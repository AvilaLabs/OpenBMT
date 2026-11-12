#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::confirmed::ConfirmedSettings;
use openbmt_core::identity::Sig;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter_node::decode_settings_frame;

/// What the reader of `OBM<` must hold of a frame it took: it encodes back
/// to exactly the bytes read (CR-242), the same frame with a byte added or
/// taken away is no longer read, and the counter node's reader of either
/// frame reads it the same way.
fn check(data: &[u8]) {
    if let Ok(frame) = ConfirmedSettings::from_bytes(data) {
        assert_eq!(frame.to_bytes(), data);
        let mut longer = data.to_vec();
        longer.push(0);
        assert!(ConfirmedSettings::from_bytes(&longer).is_err());
        assert!(ConfirmedSettings::from_bytes(&data[..data.len() - 1]).is_err());
        let both = decode_settings_frame(data).unwrap();
        assert_eq!(both.to_bytes(), data);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // A whole frame cut where the input says, and a whole frame with the
    // input behind it: neither may panic.
    let signed = SignedSettings {
        settings: Settings::defaults(1_000),
        owner_sig: Sig([0u8; 64]),
        approver_sig: None,
    };
    let mut whole = b"OBM<\x01".to_vec();
    whole.extend_from_slice(&signed.to_bytes());
    whole.extend_from_slice(&[7u8; 96]);
    let cut = data.first().map_or(0, |b| usize::from(*b)) % (whole.len() + 1);
    check(&whole[..cut]);
    whole.extend_from_slice(data);
    check(&whole);
});
