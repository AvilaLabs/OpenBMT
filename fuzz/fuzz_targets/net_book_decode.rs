#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_net::AddressBook;

fuzz_target!(|data: &[u8]| {
    if let Ok(book) = AddressBook::from_bytes(data, 0) {
        assert_eq!(book.to_bytes(), data);
    }
});
