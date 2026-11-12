#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_requests::{address_in_groups, check_return_address};

fuzz_target!(|data: &[u8]| {
    // The text as a pasted or scanned reader gives it: valid UTF-8 as it is,
    // anything else with its bad bytes replaced. The first byte says whether
    // the wallet claims the address.
    let (own, rest) = data.split_first().map_or((false, data), |(b, r)| (b & 1 == 1, r));
    let text = String::from_utf8_lossy(rest);
    if let Ok(script) = check_return_address(&text, |_| own) {
        // It is not the wallet's own, and shows in groups of four that
        // read back as the same address.
        assert!(!own);
        let shown = address_in_groups(&script).expect("a standard kind shows");
        assert!(shown.split(' ').all(|g| g.len() <= 4));
        assert_eq!(check_return_address(&shown.replace(' ', ""), |_| false), Ok(script));
    }
});
