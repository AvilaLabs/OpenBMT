#![no_main]

use std::convert::Infallible;

use libfuzzer_sys::fuzz_target;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_link::{open, seal, Kind, Role};

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fuzz_target!(|data: &[u8]| {
    let owner = id(1);
    let counter = id(2);
    let fixture = seal(
        &owner,
        Role::Owner,
        &counter.device_id(),
        Role::Counter,
        1,
        Kind::Command,
        b"fixture",
        &mut |b: &mut [u8; 32]| -> Result<(), Infallible> {
            *b = [0x42; 32];
            Ok(())
        },
    )
    .unwrap();
    let owner_id: DeviceId = owner.device_id();
    let list = |d: &DeviceId| (*d == owner_id).then_some(Role::Owner);
    // Never panics; returns content only for the one valid message.
    if let Ok(opened) = open(&counter, Role::Counter, data, list, |_| 0) {
        assert_eq!(data, fixture.as_slice());
        assert_eq!(opened.content, b"fixture");
    }
});
