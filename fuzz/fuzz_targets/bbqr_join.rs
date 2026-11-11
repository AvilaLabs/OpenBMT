#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_bbqr::{encode, Joiner, Kind};

const ALL: &[Kind] = &[
    Kind::Psbt,
    Kind::Transaction,
    Kind::Json,
    Kind::Cbor,
    Kind::Text,
    Kind::Binary,
    Kind::Executable,
];

fuzz_target!(|data: &[u8]| {
    // Any text, one reader that keeps going across the lines of the input.
    let text = String::from_utf8_lossy(data);
    let mut j = Joiner::new(ALL);
    for line in text.split('\n') {
        match j.add(line) {
            Ok(p) => assert!(p.have <= p.total),
            Err(e) => assert!(e.rule().starts_with("BQ-")),
        }
    }
    match j.finish() {
        // A set that completes writes again to a set that reads back to the
        // same bytes and kind.
        Ok(done) => {
            if let Ok(parts) = encode(done.kind, &done.bytes, 400) {
                let mut again = Joiner::new(ALL);
                for p in parts.iter().rev() {
                    again.add(p).expect("a part this crate wrote");
                }
                let back = again.finish().expect("a whole set");
                assert_eq!(back.kind, done.kind);
                assert_eq!(back.bytes, done.bytes);
            }
        }
        Err(e) => assert!(e.rule().starts_with("BQ-")),
    }

    // The input's bytes as a payload, any kind chosen by the first byte.
    let Some((&k, payload)) = data.split_first() else {
        return;
    };
    let kind = ALL[usize::from(k) % ALL.len()];
    if let Ok(parts) = encode(kind, payload, 100 + usize::from(k)) {
        let mut j = Joiner::new(ALL);
        for p in parts.iter().rev() {
            j.add(p).expect("a part this crate wrote");
        }
        let done = j.finish().expect("a whole set");
        assert_eq!((done.kind, done.bytes.as_slice()), (kind, payload));
    }
});
