#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner::parse_lightning_node;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The same text always gives the same answer.
    let first = parse_lightning_node(text);
    assert_eq!(first, parse_lightning_node(text));
    if let Ok(key) = first {
        // The key read is the 66 digits, in lower or upper case, alone or
        // before an address.
        let digits: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(parse_lightning_node(&digits), Ok(key));
        assert_eq!(parse_lightning_node(&digits.to_uppercase()), Ok(key));
        assert_eq!(
            parse_lightning_node(&format!("{digits}@node.example:9735")),
            Ok(key)
        );
    }
});
