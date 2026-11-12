#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::bip321::PaymentRequest;

fuzz_target!(|data: &[u8]| {
    let Ok(uri) = std::str::from_utf8(data) else {
        return;
    };
    let parsed = PaymentRequest::parse(uri);
    assert_eq!(parsed, PaymentRequest::parse(uri));
    if let Ok(request) = parsed {
        if request.address.is_none() {
            assert!(request
                .payment_instructions
                .iter()
                .any(|instruction| !instruction.value.is_empty()));
        }
        assert!(request
            .payment_instructions
            .iter()
            .all(|instruction| !instruction.value.is_empty()));
    }
});
