#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_lightning::bolt11::{check, decode, Expectation};
use openbmt_lightning::Ask;

fuzz_target!(|data: &[u8]| {
    // The invoice: read it, then check it against a sale.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let first = decode(text);
    // The same text always gives the same answer.
    assert_eq!(first, decode(text));
    let Ok(ask) = Ask::sale(17, 25_000_000, 300) else {
        return;
    };
    if let Ok(d) = first {
        // A text that is read is also read in lower case, to the same
        // invoice.
        assert_eq!(Ok(d.clone()), decode(&text.to_ascii_lowercase()));
        let expect = Expectation {
            network: bitcoin::Network::Regtest,
            node: d.signer,
            payment_hash: d.payment_hash,
            ask,
        };
        if let Ok(checked) = check(text, &expect) {
            // What passes the check names the amount, the hash and the
            // signer asked for.
            assert_eq!(checked.amount_msat, 25_000_000);
            assert_eq!(checked.payment_hash, d.payment_hash);
            assert_eq!(decode(&checked.bolt11), Ok(d));
        }
    }
});
