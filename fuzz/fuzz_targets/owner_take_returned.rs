#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_owner::hardware::{read_returned, HardwareRefusal, Returned};

fuzz_target!(|data: &[u8]| {
    // Anything a hardware signer could return: binary, base64 or hexadecimal
    // text, a PSBT or a finished transaction.
    match read_returned(data) {
        Ok(Returned::Psbt(p)) => {
            // A PSBT that is read writes again to one that reads the same.
            match read_returned(&p.serialize()) {
                Ok(Returned::Psbt(q)) => assert_eq!(p, q),
                // All inputs final reads as a transaction; that cannot come
                // from a file that was not.
                other => panic!("a PSBT did not read back: {other:?}"),
            }
        }
        Ok(Returned::Transaction(tx)) => {
            let raw = bitcoin::consensus::serialize(&tx);
            match read_returned(&raw) {
                Ok(Returned::Transaction(back)) => assert_eq!(tx, back),
                other => panic!("a transaction did not read back: {other:?}"),
            }
        }
        Err(e) => {
            assert!(matches!(e, HardwareRefusal::Unreadable(_)));
            assert!(e.to_string().starts_with("AS-93"));
        }
    }
});
